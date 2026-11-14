//! The arena's data spine and rules: the run model, the round schemas, voting,
//! merging, finding identity and provenance.

pub mod identity;
pub mod merge;
pub mod model;
pub mod provenance;
pub mod schema;
pub mod voting;

pub use model::*;
