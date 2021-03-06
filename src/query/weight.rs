use super::Scorer;
use Result;
use core::SegmentReader;

/// A Weight is the specialization of a Query
/// for a given set of segments.
///
/// See [`Query`](./trait.Query.html).
pub trait Weight {
    /// Returns the scorer for the given segment.
    /// See [`Query`](./trait.Query.html).
    fn scorer(&self, reader: &SegmentReader) -> Result<Box<Scorer>>;

    /// Returns the number documents within the given `SegmentReader`.
    fn count(&self, reader: &SegmentReader) -> Result<u32> {
        Ok(self.scorer(reader)?.count())
    }
}
