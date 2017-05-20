use compression::{NUM_DOCS_PER_BLOCK, BlockDecoder, VIntDecoder};
use DocId;
use postings::{Postings, FreqHandler, DocSet, HasLen, SkipResult};
use std::cmp;
use fastfield::DeleteBitSet;
use std::num::Wrapping;
use std::cmp::Ordering;

const EMPTY_DATA: [u8; 0] = [0u8; 0];


/// `SegmentPostings` represents the inverted list or postings associated to
/// a term in a `Segment`.
///
/// As we iterate through the `SegmentPostings`, the frequencies are optionally decoded.
/// Positions on the other hand, are optionally entirely decoded upfront.
pub struct SegmentPostings<'a> {
    len: usize,
    cur: Wrapping<usize>,
    block_cursor: BlockSegmentPostings<'a>,
    cur_block_len: usize,
    delete_bitset: DeleteBitSet,
}


fn binary_search(block: &[u32], target: u32) -> usize {
    let mut start = 0;
    let mut half: usize = NUM_DOCS_PER_BLOCK / 2;
    for _ in 0..7 {
        let middle = start + half;
        unsafe {
            let pivot: u32 = *block.get_unchecked(middle);
            asm!("cmpl $2, $1\ncmovge $3, $0"
                 : "+r"(start)
                 :  "r"(target),  "r"(pivot), "r"(middle))
                 ;
        }
        half /= 2;
    }
    start
}


// Returns the first `ord` such that
// block[ord] >= target.
//
// # ASSUMES
// that block is sorted, and that the last `doc` in 
// block is `>= target`.
fn search(block: &[DocId], target: DocId) -> usize {
    if block.len() == NUM_DOCS_PER_BLOCK {
        // Full block of 128 els.
        // 
        // We do a branchless, unrolled binary search.
        binary_search(block, target)
    }
    else {
        block.iter()
             .enumerate()
             .filter(|&(_, val)| *val >= target)
             .map(|(ord, _)| ord)
             .next()
             .unwrap()
    }
}

impl<'a> SegmentPostings<'a> {


    /// Reads a Segment postings from an &[u8]
    ///
    /// * `len` - number of document in the posting lists.
    /// * `data` - data array. The complete data is not necessarily used.
    /// * `freq_handler` - the freq handler is in charge of decoding
    ///   frequencies and/or positions
    pub fn from_block_postings(
            segment_block_postings: BlockSegmentPostings<'a>,
            delete_bitset: DeleteBitSet) -> SegmentPostings<'a> {
        SegmentPostings {
            len: segment_block_postings.len,
            block_cursor: segment_block_postings,
            cur: Wrapping(NUM_DOCS_PER_BLOCK),  // cursor within the block
            cur_block_len: 0,
            delete_bitset: delete_bitset,
        }
    }
    
    /// Returns an empty segment postings object
    pub fn empty() -> SegmentPostings<'static> {
        let empty_block_cursor = BlockSegmentPostings::empty();
        SegmentPostings {
            len: 0,
            block_cursor: empty_block_cursor,
            delete_bitset: DeleteBitSet::empty(),
            cur: Wrapping(NUM_DOCS_PER_BLOCK),
            cur_block_len: 0,
        }
    }
}


impl<'a> DocSet for SegmentPostings<'a> {
    // goes to the next element.
    // next needs to be called a first time to point to the correct element.
    #[inline]
    fn advance(&mut self) -> bool {
        loop {
            self.cur += Wrapping(1);
            if self.cur.0 >= self.cur_block_len {
                self.cur = Wrapping(0);
                if !self.block_cursor.advance() {
                    self.cur_block_len = 0;
                    self.cur = Wrapping(NUM_DOCS_PER_BLOCK);
                    return false;
                }
                self.cur_block_len = self.block_cursor.docs().len();
            }
            if !self.delete_bitset.is_deleted(self.doc()) {
                return true;
            }
        }
    }

    
    fn skip_next(&mut self, target: DocId) -> SkipResult {
        if !self.advance() {
            return SkipResult::End;
        }

        // skip blocks until one that might contain the target
        if self.block_cursor.skip_to_block_containing(target) {
            
        }
        loop {
            // check if we need to go to the next block
            let last_doc_in_block = {
                let block_docs = self.block_cursor.docs();
                block_docs[self.cur_block_len - 1]
            };
            if target > last_doc_in_block {
                if !self.block_cursor.advance() {
                    return SkipResult::End;
                }
                self.cur_block_len = self.block_cursor.docs().len();
                self.cur = Wrapping(0);
            } else {
                let block_docs = self.block_cursor.docs();
                if target < block_docs[self.cur.0] {
                    // We've overpassed the target after the first `advance` call
                    // or we're at the beginning of a block.
                    // Either way, we're on the first `DocId` greater than `target`
                    return SkipResult::OverStep;
                }
                break;
            }
        }
        {
            // search for the target within the block.
            // after the block, start should be the smallest value >= to
            // the target.
            let start = search(self.block_cursor.docs(), target);

            // `doc` is now the smallest number >= `target`
            let doc = self.block_cursor.docs()[start];
            self.cur = Wrapping(start);

            if !self.delete_bitset.is_deleted(doc) {
                if doc == target {
                    SkipResult::Reached
                } else {
                    SkipResult::OverStep
                }
            }
            else {
                if self.advance() {
                    SkipResult::OverStep
                } else {
                    SkipResult::End
                }
            }
        }
    }
    

    #[inline]
    fn doc(&self) -> DocId {
        let docs = self.block_cursor.docs();
        assert!(self.cur.0 < docs.len(), "Have you forgotten to call `.advance()` at least once before calling .doc().");
        docs[self.cur.0]
    }
}

impl<'a> HasLen for SegmentPostings<'a> {
    fn len(&self) -> usize {
        self.len
    }
}

impl<'a> Postings for SegmentPostings<'a> {
    fn term_freq(&self) -> u32 {
        self.block_cursor.freq_handler().freq(self.cur.0)
    }

    fn positions(&self) -> &[u32] {
        self.block_cursor.freq_handler().positions(self.cur.0)
    }
}




pub struct BlockSegmentPostings<'a> {
    num_binpacked_blocks: usize,
    num_vint_docs: usize,
    block_decoder: BlockDecoder,
    freq_handler: FreqHandler,
    remaining_data: &'a [u8],
    doc_offset: DocId,
    len: usize,
}

impl<'a> BlockSegmentPostings<'a> {
    
    pub fn from_data(len: usize, data: &'a [u8], freq_handler: FreqHandler) -> BlockSegmentPostings<'a> {
        let num_binpacked_blocks: usize = (len as usize) / NUM_DOCS_PER_BLOCK;
        let num_vint_docs = (len as usize) - NUM_DOCS_PER_BLOCK * num_binpacked_blocks;
        BlockSegmentPostings {
            num_binpacked_blocks: num_binpacked_blocks,
            num_vint_docs: num_vint_docs,
            block_decoder: BlockDecoder::new(),
            freq_handler: freq_handler,
            remaining_data: data,
            doc_offset: 0,
            len: len,
        }
    }

    pub fn reset(&mut self, len: usize, data: &'a [u8]) {
        let num_binpacked_blocks: usize = (len as usize) / NUM_DOCS_PER_BLOCK;
        let num_vint_docs = (len as usize) - NUM_DOCS_PER_BLOCK * num_binpacked_blocks;
        self.num_binpacked_blocks = num_binpacked_blocks;
        self.num_vint_docs = num_vint_docs;
        self.remaining_data = data;
        self.doc_offset = 0;
        self.len = len;
    }


    /// Returns the array of docs in the current block.
    pub fn docs(&self) -> &[DocId] {
        self.block_decoder.output_array()
    }
    
    pub fn freq_handler(&self) -> &FreqHandler {
        &self.freq_handler
    }
    
    pub fn advance(&mut self) -> bool {
        if self.num_binpacked_blocks > 0 {
            self.remaining_data = self.block_decoder.uncompress_block_sorted(self.remaining_data, self.doc_offset);
            self.remaining_data = self.freq_handler.read_freq_block(self.remaining_data);
            self.doc_offset = self.block_decoder.output(NUM_DOCS_PER_BLOCK - 1);
            self.num_binpacked_blocks -= 1;
            true
        }
        else {
            if self.num_vint_docs > 0 {
                self.remaining_data = self.block_decoder.uncompress_vint_sorted(self.remaining_data, self.doc_offset, self.num_vint_docs);
                self.freq_handler.read_freq_vint(self.remaining_data, self.num_vint_docs);
                self.num_vint_docs = 0;
                true
            }
            else {
                false
            }
        }
    }
    
    /// Returns an empty segment postings object
    pub fn empty() -> BlockSegmentPostings<'static> {
        BlockSegmentPostings {
            num_binpacked_blocks: 0,
            num_vint_docs: 0,
            block_decoder: BlockDecoder::new(),
            freq_handler: FreqHandler::new_without_freq(),
            remaining_data:  &EMPTY_DATA,
            doc_offset: 0,
            len: 0,
        }
    }
    
}

#[cfg(test)]
mod tests {

    use DocSet;
    use super::{SegmentPostings, binary_search};
    use test::{self, Bencher};

    #[test]
    fn test_empty_segment_postings() {
        let mut postings = SegmentPostings::empty();
        assert!(!postings.advance());
        assert!(!postings.advance());
    }


    #[bench]
    fn bench_binary_search_optimized(b: &mut Bencher) {
        let mut arr = [0u32; 128];
        for i in 0..128 {  arr[i] = i as u32; }
        b.iter(|| {
            let n = test::black_box(10);
            for i in 0..(n * 10) {
                binary_search(&arr, i);
            }
        })
    }

    #[bench]
    fn bench_binary_search_standard(b: &mut Bencher) {
        let n = test::black_box(1000);
        let mut arr = [0u32; 128];
        for i in 0..128 {  arr[i] = i as u32; }
        b.iter(|| {
            let n = test::black_box(10);
            for i in 0..(n * 10) {
                arr.binary_search(&i);
            }
        })
    }
}