use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use analyzer::BoxedAnalyzer;
use analyzer::Analyzer;
use analyzer::box_analyzer;
use analyzer::RawTokenizer;
use analyzer::SimpleTokenizer;
use analyzer::JapaneseTokenizer;
use analyzer::RemoveLongFilter;
use analyzer::LowerCaser;
use analyzer::Stemmer;



/// The analyzer manager serves as a store for
/// all of the configured analyzers.
///
/// By default, it is populated with the following managers.
///
///  * raw : does not process nor tokenize the text.
///  * default : Tokenizes according to whitespace and punctuation, removes tokens that are too long, lowercases the
#[derive(Clone)]
pub struct AnalyzerManager {
    analyzers: Arc< RwLock<HashMap<String, Box<BoxedAnalyzer> >> >
}

impl AnalyzerManager {

    /// Registers an analyzer with a given name.
    pub fn register<A>(&self, analyzer_name: &str, analyzer: A) 
        where A: 'static + Send + Sync + for <'a> Analyzer<'a> {
        let boxed_analyzer = box_analyzer(analyzer);
        self.analyzers
            .write()
            .expect("Acquiring the lock should never fail")
            .insert(analyzer_name.to_string(), boxed_analyzer);
    }


    /// Gets the analyzer with associated to the given name.
    ///
    /// If no analyzer exists for the given name,
    pub fn get<Q>(&self, analyzer_name: &Q) -> Option<Box<BoxedAnalyzer>>
        where Q: AsRef<str> + ?Sized {
        self.analyzers
            .read()
            .expect("Acquiring the lock should never fail")
            .get(analyzer_name.as_ref())
            .map(|boxed_analyzer| {
              boxed_analyzer.boxed_clone()  
            })
    }
}

impl Default for AnalyzerManager {
    /// Creates an `AnalyzerManager` prepopulated with
    /// the default analyzers of `tantivy`.
    /// - simple
    /// - en_stem
    /// - jp
    fn default() -> AnalyzerManager {
        let manager = AnalyzerManager {
            analyzers: Arc::new(RwLock::new(HashMap::new()))
        };
        manager.register("raw",
            RawTokenizer
        );
        manager.register("default",
            SimpleTokenizer
                .filter(RemoveLongFilter::limit(40))
                .filter(LowerCaser)
        );
        manager.register("en_stem",
            SimpleTokenizer
                .filter(RemoveLongFilter::limit(40))
                .filter(LowerCaser)
                .filter(Stemmer::new())
        );
        manager.register("ja",
            JapaneseTokenizer
                .filter(RemoveLongFilter::limit(40))
        );
        manager
    }
}