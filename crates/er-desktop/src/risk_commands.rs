//! The risk rules Settings and the Risks tab show, for the repo the caller
//! names.
//!
//! The caller passes the repo root, the remote of a remote-only tab, and its
//! changed paths, so neither path reads the active tab: the flag-on commands
//! never take the App lock, and both paths answer the same arguments the same
//! way. Paths are resolved here, on the caller's side of the service, until
//! the service holds the diff.

use er_engine::config::{RiskFilesView, RiskRulesView};
use tauri::State;

use crate::commands::{run_blocking, AppState};

/// One repo's declared rules and default.
#[tauri::command]
pub async fn get_risk_rules(
    repo_root: String,
    remote_repo: Option<String>,
    state: State<'_, AppState>,
) -> Result<RiskRulesView, String> {
    let state = state.inner().clone();
    run_blocking(move || risk_rules(&state, repo_root, remote_repo, &[])).await
}

/// The caller's paths resolved against the repo's rules, and the rules with
/// the paths they claim counted.
#[tauri::command]
pub async fn get_risk_files(
    repo_root: String,
    remote_repo: Option<String>,
    paths: Vec<String>,
    state: State<'_, AppState>,
) -> Result<RiskFilesView, String> {
    let state = state.inner().clone();
    run_blocking(move || risk_files(&state, repo_root, remote_repo, &paths)).await
}

pub(crate) fn risk_files(
    state: &AppState,
    repo_root: String,
    remote_repo: Option<String>,
    paths: &[String],
) -> Result<RiskFilesView, String> {
    Ok(risk_rules(state, repo_root, remote_repo, paths)?.resolve(paths))
}

/// Reads the repo's committed rules afresh on every call, so a table an agent
/// just wrote shows on the next open.
pub(crate) fn risk_rules(
    state: &AppState,
    repo_root: String,
    remote_repo: Option<String>,
    paths: &[String],
) -> Result<RiskRulesView, String> {
    {
        let settings = state.service.settings();
        settings
            .send_and_wait(er_service::SettingsCommand::LoadRiskRules {
                repo_root: repo_root.clone(),
                remote_repo: remote_repo.clone(),
                paths: paths.to_vec(),
            })
            .map_err(|error| error.to_string())?;
        settings
            .view()
            .risk_for(&repo_root, remote_repo.as_deref())
            .cloned()
            .ok_or_else(|| format!("No risk rules were loaded for {repo_root}"))
    }
}
