use schema::{Field, IndexRecordOption, Term};
use query::{Query, Scorer, Weight};
use termdict::{TermDictionary, TermStreamer, TermStreamerBuilder};
use core::SegmentReader;
use common::BitSet;
use Result;
use std::any::Any;
use core::Searcher;
use query::BitSetDocSet;
use query::ConstScorer;
use std::collections::Bound;
use std::collections::range::RangeArgument;


fn map_bound<TFrom, Transform: Fn(TFrom)->Vec<u8> >(bound: Bound<TFrom>, transform: &Transform) -> Bound<Vec<u8>> {
    use self::Bound::*;
    match bound {
        Excluded(from_val) => Excluded(transform(from_val)),
        Included(from_val) => Included(transform(from_val)),
        Unbounded => Unbounded
    }
}

#[derive(Debug)]
pub struct RangeQuery {
    field: Field,
    left_bound: Bound<Vec<u8>>,
    right_bound: Bound<Vec<u8>>,
}

impl RangeQuery {
    pub fn new_i64<TRangeArgument: RangeArgument<i64>>(field: Field, range: TRangeArgument) -> RangeQuery {
        let make_term_val = |val: &i64| {
            Term::from_field_i64(field, *val).value_bytes().to_owned()
        };
        RangeQuery {
            field,
            left_bound: map_bound(range.start(), &make_term_val),
            right_bound: map_bound(range.end(), &make_term_val)
        }
    }

    pub fn new_u64<TRangeArgument: RangeArgument<u64>>(field: Field, range: TRangeArgument) -> RangeQuery {
        let make_term_val = |val: &u64| {
            Term::from_field_u64(field, *val).value_bytes().to_owned()
        };
        RangeQuery {
            field,
            left_bound: map_bound(range.start(), &make_term_val),
            right_bound: map_bound(range.end(), &make_term_val)
        }
    }

    pub fn new_str<'b, TRangeArgument: RangeArgument<&'b str>>(field: Field, range: TRangeArgument) -> RangeQuery {
        let make_term_val = |val: &&str| {
            val.as_bytes().to_vec()
        };
        RangeQuery {
            field,
            left_bound: map_bound(range.start(), &make_term_val),
            right_bound: map_bound(range.end(), &make_term_val)
        }
    }
}

impl Query for RangeQuery {
    fn as_any(&self) -> &Any {
        self
    }

    fn weight(&self, _searcher: &Searcher) -> Result<Box<Weight>> {
        Ok(box RangeWeight {
            field: self.field,
            left_bound: self.left_bound.clone(),
            right_bound: self.right_bound.clone()
        })
    }
}

pub struct RangeWeight {
    field: Field,
    left_bound: Bound<Vec<u8>>,
    right_bound: Bound<Vec<u8>>,
}

impl RangeWeight {
    pub fn term_range<'a, T>(&self, term_dict: &'a T) -> T::Streamer
        where
            T: TermDictionary<'a> + 'a,
    {
        use std::collections::Bound::*;
        let mut term_stream_builder = term_dict.range();
        term_stream_builder = match &self.left_bound {
            &Included(ref term_val) => term_stream_builder.ge(term_val),
            &Excluded(ref term_val) => term_stream_builder.gt(term_val),
            &Unbounded => term_stream_builder,
        };
        term_stream_builder = match &self.right_bound {
            &Included(ref term_val) => term_stream_builder.le(term_val),
            &Excluded(ref term_val) => term_stream_builder.lt(term_val),
            &Unbounded => term_stream_builder,
        };
        term_stream_builder.into_stream()
    }
}

impl Weight for RangeWeight {
    fn scorer<'a>(&'a self, reader: &'a SegmentReader) -> Result<Box<Scorer + 'a>> {
        let max_doc = reader.max_doc();
        let mut doc_bitset = BitSet::with_max_value(max_doc);

        let inverted_index = reader.inverted_index(self.field);
        let term_dict = inverted_index.terms();
        let mut term_range = self.term_range(term_dict);
        while term_range.advance() {
            let term_info = term_range.value();
            let mut block_segment_postings = inverted_index
                .read_block_postings_from_terminfo(term_info, IndexRecordOption::Basic);
            while block_segment_postings.advance() {
                for &doc in block_segment_postings.docs() {
                    doc_bitset.insert(doc);
                }
            }
        }
        let doc_bitset = BitSetDocSet::from(doc_bitset);
        Ok(box ConstScorer::new(doc_bitset))
    }
}

#[cfg(test)]
mod tests {

    use Index;
    use schema::{Document, Field, SchemaBuilder, INT_INDEXED};
    use collector::CountCollector;
    use std::collections::Bound;
    use query::Query;
    use super::RangeQuery;

    #[test]
    fn test_range_query() {
        let int_field: Field;
        let schema = {
            let mut schema_builder = SchemaBuilder::new();
            int_field = schema_builder.add_i64_field("intfield", INT_INDEXED);
            schema_builder.build()
        };

        let index = Index::create_in_ram(schema);
        {
            let mut index_writer = index.writer_with_num_threads(2, 6_000_000).unwrap();

            for i in 1..100 {
                let mut doc = Document::new();
                for j in 1..100 {
                    if i % j == 0 {
                        doc.add_i64(int_field, j as i64);
                    }
                }
                index_writer.add_document(doc);
            }

            index_writer.commit().unwrap();
        }
        index.load_searchers().unwrap();
        let searcher = index.searcher();
        let count_multiples = |range_query: RangeQuery| {
            let mut count_collector = CountCollector::default();
            range_query
                .search(&*searcher, &mut count_collector)
                .unwrap();
            count_collector.count()
        };

        assert_eq!(
            count_multiples(RangeQuery::new_i64(int_field, 10..11)),
            9
        );
        assert_eq!(
            count_multiples(RangeQuery::new_i64(int_field, (Bound::Included(10), Bound::Included(11)) )),
            18
        );
        assert_eq!(
            count_multiples(RangeQuery::new_i64(int_field, (Bound::Excluded(9), Bound::Included(10)))),
            9
        );
        assert_eq!(
            count_multiples(RangeQuery::new_i64(int_field, 9..)),
            91
        );
    }

}
