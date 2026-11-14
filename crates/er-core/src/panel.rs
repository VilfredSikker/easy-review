//! The panel round's shared vocabulary.

use serde::{Deserialize, Serialize};

/// The claims a round backs with a quorum: two or more distinct reviewers
/// agreeing the claim is real.
///
/// The equality of two `Agreement`s is the whole of the free round-two
/// measurement: when the cross round leaves the same claims confirmed as
/// round one did, that round bought nothing and the panel can say so without
/// asking the arbiter again.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Agreement {
    /// Finding ids with at least two distinct backers, sorted.
    pub confirmed: Vec<String>,
}
