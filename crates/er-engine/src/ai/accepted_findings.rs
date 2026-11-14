//! Accepted arena findings are their own artifact.
//!
//! Acceptance used to merge a run's kept findings into `review.json`, which
//! made the general review a target for arena writes and re-stamped its diff
//! hash with the run's (ADR 0035, ADR 0065). An accepted row is host output:
//! it keeps the run it came from, the providers where the run recorded them,
//! and the review-row id the import path produced.

use anyhow::{bail, Context, Result};
use er_store::{
    Artifact, ArtifactKey, ArtifactSnapshot, ArtifactStore, Loaded, Scope, DEFAULT_LOCK_TIMEOUT,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::Finding;

/// One finding the user accepted from an arena run.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AcceptedFinding {
    /// The review file the row belongs to; `Finding` carries no path.
    pub file: String,
    /// The run finding id (`kept`), for the run's own accept bookkeeping.
    pub run_finding_id: String,
    pub run_id: String,
    pub run_diff_hash: String,
    pub accepted_at: String,
    /// Host-derived provider ids. `Finding.providers` never serializes, so the
    /// row keeps them and the projection restores them after the provenance
    /// clear.
    #[serde(default)]
    pub providers: Vec<String>,
    pub finding: Finding,
}

/// The bucket's accepted findings, in acceptance order.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AcceptedFindings {
    pub version: u32,
    #[serde(default)]
    pub findings: Vec<AcceptedFinding>,
}

impl AcceptedFindings {
    pub fn empty() -> Self {
        Self {
            version: Self::VERSION,
            findings: Vec::new(),
        }
    }

    /// Add rows in order, replacing a row with the same review id.
    pub fn extend(&mut self, rows: impl IntoIterator<Item = AcceptedFinding>) {
        for row in rows {
            match self
                .findings
                .iter()
                .position(|existing| existing.finding.id == row.finding.id)
            {
                Some(index) => self.findings[index] = row,
                None => self.findings.push(row),
            }
        }
    }
}

impl Artifact for AcceptedFindings {
    const KIND: &'static str = "accepted-findings";
    const VERSION: u32 = 1;
    const FILE: &'static str = "accepted-findings.json";
    type Slot = ();
}

/// The write acceptance performed, kept so a failed run save can undo it.
pub struct AcceptedWrite {
    key: ArtifactKey<AcceptedFindings>,
    original: Option<ArtifactSnapshot<AcceptedFindings>>,
    written: ArtifactSnapshot<AcceptedFindings>,
}

impl AcceptedWrite {
    /// Put the artifact back, refusing to overwrite a file that changed since
    /// the acceptance wrote it.
    pub fn rollback(&self, store: &ArtifactStore) -> Result<()> {
        match &self.original {
            Some(original) => {
                if store.restore_if_unchanged(original, &self.written)? {
                    Ok(())
                } else {
                    bail!(
                        "accepted-findings.json changed during acceptance; refusing to overwrite the newer file"
                    )
                }
            }
            None => {
                let guard = store
                    .lock_scope(self.key.scope(), DEFAULT_LOCK_TIMEOUT)
                    .context("lock the bucket to remove the new accepted-findings.json")?;
                match store.snapshot(&self.key)? {
                    Loaded::Found(current) if current.value.bytes() == self.written.bytes() => {
                        store.delete_locked(&guard, &self.key)?;
                        Ok(())
                    }
                    _ => bail!(
                        "accepted-findings.json changed during acceptance; refusing to remove the newer file"
                    ),
                }
            }
        }
    }
}

/// Append the selected rows under one scope lock, returning the write so the
/// caller can roll it back when saving the run's own record fails.
pub fn record_accepted(
    store: &ArtifactStore,
    scope: &Scope,
    rows: &[AcceptedFinding],
) -> Result<AcceptedWrite> {
    let key = ArtifactKey::<AcceptedFindings>::new(scope.clone(), ())?;
    let guard = store
        .lock_scope(scope, DEFAULT_LOCK_TIMEOUT)
        .context("lock the bucket to record accepted findings")?;
    let original = match store.snapshot(&key)? {
        Loaded::Absent => None,
        Loaded::Found(stored) => {
            match stored.value.parse() {
                Loaded::Found(_) => {}
                other => bail!(
                    "accepted-findings.json is unreadable: {}",
                    unreadable_reason(&other)
                ),
            }
            Some(stored.value)
        }
        Loaded::Unreadable(reason) => {
            bail!("accepted-findings.json is unreadable: {reason:?}")
        }
    };
    let mut merged = match &original {
        None => AcceptedFindings::empty(),
        Some(snapshot) => match snapshot.parse() {
            Loaded::Found(stored) => stored.value,
            other => bail!(
                "accepted-findings.json is unreadable: {}",
                unreadable_reason(&other)
            ),
        },
    };
    merged.extend(rows.iter().cloned());
    store.write_locked(&guard, &key, &merged)?;
    let written = match store.snapshot(&key)? {
        Loaded::Found(stored) => stored.value,
        other => bail!(
            "read back the accepted-findings.json just written: {}",
            unreadable_reason(&other)
        ),
    };
    Ok(AcceptedWrite {
        key,
        original,
        written,
    })
}

fn unreadable_reason<T>(loaded: &Loaded<T>) -> String {
    match loaded {
        Loaded::Unreadable(reason) => format!("{reason:?}"),
        Loaded::Found(_) => "readable".to_string(),
        Loaded::Absent => "absent".to_string(),
    }
}
