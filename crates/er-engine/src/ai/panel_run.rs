//! `panel-run.json`: what one panel run did, and whether round two changed it.
//!
//! The panel is the reviewer stage of a Deep run. Its reviewers are ordinary
//! jobs, so the only durable trace of the stage is this record: the reviewers
//! that ran, the votes each cast, and the free round-two measurement — whether
//! the first cross round changed the set of claims two reviewers back. That
//! measurement costs nothing extra because the votes it reads were already
//! parsed to grade the findings; no second arbiter call is made.

use er_store::{Artifact, ArtifactKey, ArtifactStore, Scope};
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

/// One reviewer's part in the panel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PanelReviewer {
    /// The arena's reviewer id (`general::provider-model`).
    pub id: String,
    pub provider_id: String,
    pub model_id: String,
    /// Distinct findings the reviewer proposed in round 1.
    pub round1_findings: usize,
    /// Why round 1 failed, when it did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub round1_error: Option<String>,
    /// Ballots the reviewer cast in the first cross round.
    pub round2_votes: usize,
}

/// The durable result of one panel run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PanelRun {
    pub version: u32,
    /// The arena run whose reviewer stage this record describes.
    pub run_id: String,
    pub diff_hash: String,
    pub started_at: String,
    /// Rounds the panel actually ran.
    pub rounds: u8,
    pub reviewers: Vec<PanelReviewer>,
    /// `quorum()` over the round-1 proposals.
    pub round1_quorum: Agreement,
    /// `quorum()` over the first cross round's ballots; `None` when the panel
    /// ran a single round.
    pub round2_quorum: Option<Agreement>,
    /// Whether round two changed the quorum outcome. The field is required:
    /// a record that lost it cannot answer the question it exists for.
    pub quorum_changed: bool,
}

impl Artifact for PanelRun {
    const KIND: &'static str = "panel-run";
    const VERSION: u32 = 1;
    const FILE: &'static str = "panel-run.json";
    type Slot = ();

    fn diff_hash(&self) -> Option<&str> {
        Some(&self.diff_hash)
    }
}

/// Write the panel's record. The panel is the only writer.
pub fn save(store: &ArtifactStore, scope: &Scope, record: &PanelRun) -> anyhow::Result<()> {
    let key = ArtifactKey::<PanelRun>::new(scope.clone(), ())?;
    store.write_durable(&key, record)?;
    Ok(())
}

/// The panel record stored in this bucket, if it parses. An absent or
/// unreadable record reads as `None`, like every other sidecar.
pub fn load(store: &ArtifactStore, scope: &Scope) -> Option<PanelRun> {
    let key = ArtifactKey::<PanelRun>::new(scope.clone(), ()).ok()?;
    match store.read(&key).ok()? {
        er_store::Loaded::Found(stored) => Some(stored.value),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn record_json() -> serde_json::Value {
        json!({
            "version": 1,
            "run_id": "run-1",
            "diff_hash": "hash",
            "started_at": "t0",
            "rounds": 2,
            "reviewers": [{
                "id": "general::p-m",
                "provider_id": "p",
                "model_id": "m",
                "round1_findings": 1,
                "round2_votes": 1
            }],
            "round1_quorum": {"confirmed": []},
            "round2_quorum": {"confirmed": ["f"]},
            "quorum_changed": true
        })
    }

    /// The measurement is the reason the artifact exists; a record without it
    /// must not parse.
    #[test]
    fn panel_run_schema_requires_the_proxy() {
        let parsed: PanelRun = serde_json::from_value(record_json()).expect("the full record");
        assert!(parsed.quorum_changed);

        let mut missing = record_json();
        missing.as_object_mut().unwrap().remove("quorum_changed");
        assert!(
            serde_json::from_value::<PanelRun>(missing).is_err(),
            "a panel record without the proxy must not parse"
        );
    }

    #[test]
    fn panel_run_round_trips_through_the_store() {
        let dir = tempfile::tempdir().unwrap();
        crate::storage::with_test_storage_root(dir.path(), || {
            let store = crate::store_registry::store();
            let scope = Scope::dir(dir.path().to_str().unwrap()).unwrap();
            let record: PanelRun = serde_json::from_value(record_json()).unwrap();
            save(&store, &scope, &record).unwrap();

            let loaded = load(&store, &scope).expect("the saved panel record");
            assert_eq!(loaded, record);
        });
    }
}
