//! Per-run operations both hosts share: the App on the flag-off path and the
//! workspace service on the flag path. Each takes the bucket and the live
//! controls it needs as arguments, so neither host's state leaks in.

use super::control::ArenaRunControls;
use super::storage::{list_run_ids, load_run, save_run, ArenaPaths};
use super::{
    accepted_finding_rows, mark_accepted, ArenaFinding, ArenaRun, ArenaRunKind, ArenaRunSnapshot,
    ArenaRunSummary, HumanOverride, PanelSummary, RunStatus, Verdict,
};
use crate::config::ErConfig;
use anyhow::{bail, Context, Result};
use er_store::StoreError;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Reject ordinary actions only while a workflow owns this exact bucket. The
/// probe does not wait: a held lease answers at once.
pub fn check_bucket_admission(er_dir: &str) -> Result<()> {
    let scope = er_store::Scope::dir(er_dir)?;
    match crate::store_registry::store().lease(&scope, Duration::ZERO, "admission") {
        Ok(guard) => {
            drop(guard);
            Ok(())
        }
        Err(StoreError::LeaseHeld { .. } | StoreError::AlreadyLocked { .. }) => {
            bail!("A review flow is already using this review bucket")
        }
        Err(error) => Err(error.into()),
    }
}

/// The bucket a run lives in, independent of the active tab: its live
/// control's bucket, else the first bucket holding `arena/<run_id>`, else
/// `active`. Supervisors run whichever tab is in view, so a per-run operation
/// must address the run by its own bucket.
pub fn bucket_for_run(
    controls: &ArenaRunControls,
    buckets: &[String],
    active: &str,
    run_id: &str,
) -> PathBuf {
    let live = controls.get(run_id);
    bucket_for_run_in(
        live.as_ref().map(|control| control.er_dir.as_str()),
        buckets,
        active,
        run_id,
    )
}

/// [`bucket_for_run`] for a reader that holds the live run's bucket, if any,
/// instead of the controls.
pub fn bucket_for_run_in(
    live: Option<&str>,
    buckets: &[String],
    active: &str,
    run_id: &str,
) -> PathBuf {
    if let Some(er_dir) = live {
        return PathBuf::from(er_dir);
    }
    buckets
        .iter()
        .find(|er| ArenaPaths::for_run(Path::new(er), run_id).root.is_dir())
        .map_or_else(|| PathBuf::from(active), PathBuf::from)
}

/// Accept a run's findings into the review's accepted-findings record.
/// `is_flow_run` names runs only the guarded flow acceptance may accept.
/// Returns how many rows were accepted.
pub fn accept_findings(
    er_dir: &Path,
    run_id: &str,
    finding_ids: Option<Vec<String>>,
    is_flow_run: impl FnOnce(&ArenaRun) -> bool,
) -> Result<usize> {
    let er_path = er_dir.to_string_lossy().into_owned();
    check_bucket_admission(&er_path)?;
    let paths = ArenaPaths::for_run(er_dir, run_id);
    let mut run = load_run(&paths)?;
    // A seeded run's verdicts reach the review through `arbiter.json`, which
    // merges at load; accepting them as rows would show each ruling twice.
    if run.config.run_kind == ArenaRunKind::Seeded {
        bail!(
            "a seeded run's verdicts are applied from arbiter.json on load; \
             there is nothing to accept as a finding"
        );
    }
    if is_flow_run(&run) {
        bail!(
            "Review flow findings require guarded flow acceptance. \
             Open the current flow result or start a new Review."
        );
    }
    let ids: Vec<String> = match finding_ids {
        Some(ids) => ids,
        None => run
            .findings
            .iter()
            .map(|finding| finding.id.clone())
            .collect(),
    };
    let now = crate::sync::chrono_now();
    let rows = accepted_finding_rows(&run, &ids, &now);
    if rows.is_empty() {
        return Ok(0);
    }
    let store = crate::store_registry::store();
    let scope = er_store::Scope::dir(&er_path)?;
    let accepted = crate::ai::accepted_findings::record_accepted(&store, &scope, &rows)
        .context("record the accepted findings")?;
    mark_accepted(&mut run, &rows, &now);
    if let Err(error) = save_run(&paths, &run) {
        return match accepted.rollback(&store) {
            Ok(()) => Err(error).context(
                "accepted IDs could not be saved to the run; accepted-findings.json was rolled back",
            ),
            Err(rollback) => Err(error).context(format!(
                "accepted IDs could not be saved to the run; accepted-findings.json rollback failed: {rollback}"
            )),
        };
    }
    Ok(rows.len())
}

/// Record a human verdict on one finding and save the run.
pub fn override_finding(
    er_dir: &Path,
    run_id: &str,
    finding_id: &str,
    verdict: Verdict,
    note: String,
) -> Result<ArenaFinding> {
    let paths = ArenaPaths::for_run(er_dir, run_id);
    let mut run = load_run(&paths)?;
    let finding = run
        .findings
        .iter_mut()
        .find(|finding| finding.id == finding_id)
        .ok_or_else(|| anyhow::anyhow!("finding not found"))?;
    finding.override_ = Some(HumanOverride {
        verdict: verdict.clone(),
        note,
        at: crate::sync::chrono_now(),
    });
    finding.verdict = verdict;
    let updated = finding.clone();
    save_run(&paths, &run)?;
    Ok(updated)
}

/// The bucket's run summaries on `branch_ref` (every branch when `None`),
/// newest first.
pub fn list_summaries(er_dir: &Path, branch_ref: Option<&str>) -> Result<Vec<ArenaRunSummary>> {
    let mut out = Vec::new();
    for id in list_run_ids(er_dir)? {
        let paths = ArenaPaths::for_run(er_dir, &id);
        if let Ok(run) = load_run(&paths) {
            if branch_ref.is_some_and(|branch| run.branch_ref != branch) {
                continue;
            }
            out.push(run.summary());
        }
    }
    out.sort_by_key(|run| std::cmp::Reverse(run.created_at.clone()));
    Ok(out)
}

/// One run's snapshot with its live status and panel summary. `is_flow_run`
/// marks a legacy Deep run as flow-owned, so its history stays read only
/// unless the captured flow supplies its guarded acceptance path.
pub fn run_snapshot(
    config: &ErConfig,
    controls: &ArenaRunControls,
    er_dir: &Path,
    run_id: &str,
    is_flow_run: impl FnOnce(&ArenaRun) -> bool,
) -> Result<ArenaRunSnapshot> {
    let live = controls.get(run_id).map(|control| control.status());
    run_snapshot_with_status(config, live, er_dir, run_id, is_flow_run)
}

/// [`run_snapshot`] for a reader that holds the live status, if any, instead
/// of the controls.
pub fn run_snapshot_with_status(
    config: &ErConfig,
    live: Option<RunStatus>,
    er_dir: &Path,
    run_id: &str,
    is_flow_run: impl FnOnce(&ArenaRun) -> bool,
) -> Result<ArenaRunSnapshot> {
    let paths = ArenaPaths::for_run(er_dir, run_id);
    let mut run = load_run(&paths)?;
    if is_flow_run(&run) {
        run.config.run_kind = ArenaRunKind::ReviewFlow;
    }
    if let Some(status) = live {
        run.status = status;
    }
    // A bucket-rooted store reaches the same file the panel wrote, without
    // resolving the process storage root for a bucket the caller named.
    let panel = crate::ai::panel_run::load(
        &er_store::ArtifactStore::new(er_dir),
        &er_store::Scope::dir(er_dir)?,
    )
    .and_then(|record| PanelSummary::for_run(&record, &run.id));
    Ok(super::build_snapshot_with_panel(config, run, panel))
}
