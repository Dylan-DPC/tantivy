/// The analyzer module contains all of the tools used to process
/// text in `tantivy`.

use std::borrow::{Borrow, BorrowMut};
use analyzer::TokenStreamChain;

/// Token 
pub struct Token {
    /// Offset (byte index) of the first character of the token.
    /// Offsets shall not be modified by token filters.
    pub offset_from: usize,
    /// Offset (byte index) of the last character of the token + 1.
    /// The text that generated the token should be obtained by 
    /// &text[token.offset_from..token.offset_to]
    pub offset_to: usize,
    /// Position, expressed in number of tokens.
    pub position: usize,
    /// Actual text content of the token.
    pub term: String,
}

impl Default for Token {
    fn default() -> Token {
        Token {
            offset_from: 0,
            offset_to: 0,
            position: usize::max_value(),
            term: String::new(),
        }
    }
}


// Warning! TODO may change once associated type constructor
// land in nightly.


/// `Analyzer`s are the text processing pipeline in tantivy.
pub trait Analyzer<'a>: Sized + Clone {

    /// Type of the token stream created by the analyzer.
    type TokenStreamImpl: TokenStream;

    /// Creates the `TokenStream` associated to a given text.
    fn token_stream(&mut self, text: &'a str) -> Self::TokenStreamImpl;

    /// Chains a given `TokenFilter` to the given analyzer, and
    /// returns the resulting analyzer.
    fn filter<NewFilter>(self, new_filter: NewFilter) -> ChainAnalyzer<NewFilter, Self>
        where NewFilter: TokenFilterFactory<<Self as Analyzer<'a>>::TokenStreamImpl>
    {
        ChainAnalyzer {
            head: new_filter,
            tail: self,
        }
    }
}


/// A `BoxedAnalyzer` is an analyzer that can produce boxed
/// `TokenStream` and be `boxed clone`.
pub trait BoxedAnalyzer: Send + Sync {
    ///  Returns a boxed `TokenStream` for the given text.
    fn token_stream<'a>(&mut self, text: &'a str) -> Box<TokenStream + 'a>;
    ///  Returns a boxed `TokenStream` for the given texts.
    fn token_stream_texts<'b>(&mut self, texts: &'b [&'b str]) -> Box<TokenStream + 'b>;
    /// Returns a boxed clone.
    fn boxed_clone(&self) -> Box<BoxedAnalyzer>;
}


#[derive(Clone)]
struct BoxableAnalyzer<A>(A) where A: for <'a> Analyzer<'a> + Send + Sync;

impl<A> BoxedAnalyzer for BoxableAnalyzer<A> where A: 'static + Send + Sync + for <'a> Analyzer<'a> {
    fn token_stream<'a>(&mut self, text: &'a str) -> Box<TokenStream + 'a> {
        box self.0.token_stream(text)
    }

    fn token_stream_texts<'b>(&mut self, texts: &'b [&'b str]) -> Box<TokenStream + 'b> {
        assert!(texts.len() > 0);
        if texts.len() == 1 {
            box self.0.token_stream(texts[0])
        }
        else {
            let mut offsets = vec!();
            let mut total_offset = 0;
            for text in texts {
                offsets.push(total_offset);
                total_offset += text.len();
            }
            let token_streams: Vec<_> = texts
                .iter()
                .map(|text| {
                    self.0.token_stream(text)
                })
                .collect();
            box TokenStreamChain::new(offsets, token_streams)
        }
    }

    fn boxed_clone(&self) -> Box<BoxedAnalyzer> {
        box self.clone()
    }
}

/// Boxes a given `Analyzer`.
pub fn box_analyzer<A>(a: A) -> Box<BoxedAnalyzer>
    where A: 'static + Send + Sync + for <'a> Analyzer<'a> {
    box BoxableAnalyzer(a)
}

impl<'b> TokenStream for Box<TokenStream + 'b> {
    fn advance(&mut self) -> bool {
        let token_stream: &mut TokenStream = self.borrow_mut();
        token_stream.advance()
    }

    fn token(&self) -> &Token {
        let token_stream: &TokenStream = self.borrow();
        token_stream.token()
    }

    fn token_mut(&mut self) -> &mut Token {
        let token_stream: &mut TokenStream = self.borrow_mut();
        token_stream.token_mut()
    }
}

/// A `TokenStream` is the equivalent of an iterator over `Token`s.
pub trait TokenStream {
    /// Advance to the next token.
    fn advance(&mut self) -> bool;

    /// Returns the given token.
    fn token(&self) -> &Token;

    /// Returns a mutable reference to the current token.
    /// This is useful for token filters which process
    /// tokens in place.
    fn token_mut(&mut self) -> &mut Token;

    /// Helper to iterate over the tokens.
    fn next(&mut self) -> Option<&Token> {
        if self.advance() {
            Some(self.token())
        } else {
            None
        }
    }

    /// Helper to iterate through the token stream
    /// and push each token to the `callback` function given
    /// in argument.
    fn process(&mut self, callback: &mut FnMut(&Token)) -> u32 {
        let mut num_tokens_pushed = 0u32;
        while self.advance() {
            callback(self.token());
            num_tokens_pushed += 1u32;
        }
        num_tokens_pushed
    }
}

#[derive(Clone)]
pub struct ChainAnalyzer<HeadTokenFilterFactory, TailAnalyzer> {
    head: HeadTokenFilterFactory,
    tail: TailAnalyzer,
}


impl<'a, HeadTokenFilterFactory, TailAnalyzer> Analyzer<'a>
    for ChainAnalyzer<HeadTokenFilterFactory, TailAnalyzer>
    where HeadTokenFilterFactory: TokenFilterFactory<TailAnalyzer::TokenStreamImpl>,
          TailAnalyzer: Analyzer<'a>
{
    type TokenStreamImpl = HeadTokenFilterFactory::ResultTokenStream;

    fn token_stream(&mut self, text: &'a str) -> Self::TokenStreamImpl {
        let tail_token_stream = self.tail.token_stream(text );
        self.head.transform(tail_token_stream)
    }
}


/// Modifies a `Tokenstream`, for instance by chaining a different
/// token filters.
pub trait TokenFilterFactory<TailTokenStream: TokenStream>: Clone {

    /// Type of the token stream resulting from modifying `TailTokenStream`.
    type ResultTokenStream: TokenStream;

    /// Transform the given `TailTokenStream`.
    fn transform(&self, token_stream: TailTokenStream) -> Self::ResultTokenStream;
}
