//! OS launch and reveal: a file in the editor, a URL in the browser, a folder
//! or a file in the file manager.

use std::path::Path;

use er_protocol::{ErError, OpenSource, OpenSourceKind};
use tauri::State;

use super::failed;
use crate::commands::AppState;

#[tauri::command]
pub fn open_in_editor(state: State<AppState>) -> Result<OpenSource, ErError> {
    open_source(state)
}

/// Open the selected file in VS Code (`code -g path:line`) when a local checkout exists.
/// No GitHub/browser fallback — desktop `e` key uses this exclusively.
#[tauri::command]
pub fn open_in_vscode(state: State<AppState>) -> Result<OpenSource, ErError> {
    let app = state.app.lock().map_err(failed)?;
    let tab = app.tab();
    let Some(file) = tab.selected_diff_file() else {
        return Ok(needs_checkout("No selected file"));
    };
    let line_num = file
        .hunks
        .get(tab.current_hunk)
        .map(|h| h.new_start)
        .unwrap_or(1);

    if let Some(local_root) = local_source_root(tab) {
        let file_path = Path::new(local_root).join(&file.path);
        if file_path.exists() {
            crate::os_launch::open_vscode_at(local_root, &file_path, line_num).map_err(failed)?;
            return Ok(opened(
                OpenSourceKind::OpenedLocal,
                &file_path.to_string_lossy(),
            ));
        }
    }

    Ok(needs_checkout(
        "No local checkout found for this file. Check out the branch in a worktree first.",
    ))
}

#[tauri::command]
pub fn open_source(state: State<AppState>) -> Result<OpenSource, ErError> {
    let app = state.app.lock().map_err(failed)?;
    let tab = app.tab();
    let Some(file) = tab.selected_diff_file() else {
        return Ok(needs_checkout("No selected file"));
    };
    let line_num = file
        .hunks
        .get(tab.current_hunk)
        .map(|h| h.new_start)
        .unwrap_or(1);

    // Local editable source is only valid when this tab represents a checked-out
    // local context (working tree, or local-branch view with checkout root).
    if let Some(local_root) = local_source_root(tab) {
        let file_path = Path::new(local_root).join(&file.path);
        if file_path.exists() {
            crate::os_launch::open_editor_at(local_root, &file_path, line_num).map_err(failed)?;
            return Ok(opened(
                OpenSourceKind::OpenedLocal,
                &file_path.to_string_lossy(),
            ));
        }
    }

    // Fallback: open GitHub URL for PR/tab-backed sources.
    if let Some(url) = github_file_url_for_tab(tab, &file.path, line_num) {
        drop(app);
        open_url_in_browser(url.clone())?;
        return Ok(opened(OpenSourceKind::OpenedUrl, &url));
    }

    Ok(needs_checkout(
        "No local checkout found for this source. Create editable worktree first.",
    ))
}

fn opened(kind: OpenSourceKind, target: &str) -> OpenSource {
    OpenSource {
        kind,
        target: target.to_owned(),
    }
}

fn needs_checkout(reason: &str) -> OpenSource {
    opened(OpenSourceKind::NeedsCheckout, reason)
}

fn local_source_root(tab: &er_engine::app::TabState) -> Option<&str> {
    tab.local_checkout_root()
}

fn github_file_url_for_tab(
    tab: &er_engine::app::TabState,
    file_path: &str,
    line_num: usize,
) -> Option<String> {
    let pr_head = tab
        .pr_data
        .as_ref()
        .map(|pr| pr.head_branch.trim().to_string())
        .filter(|b| !b.is_empty());

    let mut branch_ref = pr_head
        .or_else(|| tab.local_branch_view.clone())
        .unwrap_or_else(|| tab.current_branch.clone())
        .trim()
        .to_string();

    // PR tab whose overview hasn't loaded yet: synchronously ask gh for the head branch.
    if tab.pr_data.is_none() {
        if let Some(n) = tab.pr_number {
            if let Ok(name) = er_engine::github::gh_pr_head_branch_name(n, &tab.repo_root) {
                let name = name.trim().to_string();
                if !name.is_empty() {
                    branch_ref = name;
                }
            }
        }
    }

    if branch_ref.is_empty() {
        return None;
    }
    if let Some(slug) = tab.remote_repo.as_ref() {
        return Some(format!(
            "https://github.com/{slug}/blob/{branch_ref}/{file_path}#L{line_num}"
        ));
    }

    let remote = tab.ports.git.remote_url(&tab.repo_root, "origin").ok()?;
    let slug = parse_github_slug(&remote)?;
    Some(format!(
        "https://github.com/{slug}/blob/{branch_ref}/{file_path}#L{line_num}"
    ))
}

fn parse_github_slug(remote: &str) -> Option<String> {
    er_engine::github::RepoSlug::parse_github_com(remote).map(|slug| slug.to_string())
}

#[tauri::command]
pub fn open_url_in_browser(url: String) -> Result<(), ErError> {
    crate::os_launch::open_external_url(&url).map_err(failed)
}

#[tauri::command]
pub fn reveal_er_folder(state: State<AppState>) -> Result<(), ErError> {
    let er_dir = {
        let app = state.app.lock().map_err(failed)?;
        app.tab().er_dir()
    };
    reveal_folder(&er_dir)
}

/// Shows `dir` in the file manager, created first so reveal never fails on a
/// fresh or empty bucket.
fn reveal_folder(dir: &str) -> Result<(), ErError> {
    let target = Path::new(dir);
    let _ = std::fs::create_dir_all(target);
    crate::os_launch::reveal_folder(target).map_err(failed)
}

#[tauri::command]
pub fn reveal_path(path: String) -> Result<(), ErError> {
    crate::os_launch::reveal_path(Path::new(&path)).map_err(failed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_source_policy_allows_only_checked_out_local_contexts() {
        // The policy itself lives on `TabState::local_checkout_root` (see the
        // engine's tests); this pins the two roots it resolves to.
        let mut working = er_engine::app::TabState::new_for_test(vec![]);
        working.repo_root = "/repo".into();
        assert_eq!(local_source_root(&working), Some("/repo"));

        let mut pr = er_engine::app::TabState::new_for_test(vec![]);
        pr.repo_root = "/repo".into();
        pr.remote_repo = Some("o/r".into());
        pr.local_branch_view = Some("feat/pr-head".into());
        pr.current_branch = "feat/pr-head".into();
        assert_eq!(local_source_root(&pr), None);
        pr.local_branch_checkout_root = Some("/wt".into());
        assert_eq!(local_source_root(&pr), Some("/wt"));
    }

    #[test]
    fn parse_github_slug_accepts_ssh_and_https_remotes() {
        assert_eq!(
            parse_github_slug("git@github.com:owner/repo.git").as_deref(),
            Some("owner/repo")
        );
        assert_eq!(
            parse_github_slug("https://github.com/owner/repo.git").as_deref(),
            Some("owner/repo")
        );
        assert_eq!(
            parse_github_slug("https://user@github.com/owner/repo").as_deref(),
            Some("owner/repo")
        );
        assert_eq!(parse_github_slug("https://gitlab.com/owner/repo"), None);
    }

    #[test]
    fn git_at_github_com_colon_form_parses_the_same_everywhere() {
        let remote = "git@github.com:Acme/Widgets.git";
        assert_eq!(
            er_engine::github::parse_remote_url(remote),
            Some(("Acme".to_string(), "Widgets".to_string()))
        );
        assert_eq!(
            er_engine::projects_pins::normalize_remote_slug(remote),
            "acme/widgets"
        );
        assert_eq!(
            crate::projects::slug_from_remote_url(remote).as_deref(),
            Some("Acme/Widgets")
        );
        assert_eq!(parse_github_slug(remote).as_deref(), Some("Acme/Widgets"));
    }
}
