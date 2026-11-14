//! What the validate pass applied to a review.

/// What the validate pass applied on this load (diagnostics only in 1.0).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ValidationEffect {
    pub applied: usize,
    pub unmatched: usize,
    pub ignored: bool,
}
