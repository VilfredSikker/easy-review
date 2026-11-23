//! OS launch and reveal: a file in the editor, a URL in the browser, a folder
//! or a file in the file manager. The page names the file and the line; with
//! the service, the tab its key names supplies the checkout and the GitHub
//! names, read from its published content.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use er_engine::git::Git;
use er_protocol::{ErError, OpenSource, OpenSourceKind};
use tauri::State;

use super::failed;
use crate::state::DesktopState;

/// What the open commands read of a tab: where a local edit can open, and
/// what names the file on GitHub.
pub(crate) struct SourceTab {
    /// The checkout a local edit opens in; none for a remote PR, or a branch
    /// view whose branch nobody checked out.
    pub local_root: Option<String>,
    /// The PR's head branch; none until the PR's overview has loaded.
    pub pr_head: Option<String>,
    pub pr_number: Option<u64>,
    pub local_branch_view: Option<String>,
    pub current_branch: String,
    pub remote_repo: Option<String>,
    pub repo_root: String,
    pub git: Arc<dyn Git>,
}

/// Where an open lands.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Target {
    Local {
        root: String,
        file: PathBuf,
        line: usize,
    },
    Url(String),
    NeedsCheckout(&'static str),
}

const NO_FILE: &str = "No selected file";

#[tauri::command]
pub fn open_in_editor(
    tab: er_protocol::TabKey,
    path: Option<String>,
    line: Option<usize>,
    desktop: State<DesktopState>,
) -> Result<OpenSource, ErError> {
    open_source(tab, path, line, desktop)
}

/// Open the file in VS Code (`code -g path:line`) when a local checkout exists.
/// No GitHub/browser fallback — desktop `e` key uses this exclusively.
#[tauri::command]
pub fn open_in_vscode(
    tab: er_protocol::TabKey,
    path: Option<String>,
    line: Option<usize>,
    desktop: State<DesktopState>,
) -> Result<OpenSource, ErError> {
    let source = content_source(&desktop, tab)?;
    launch(
        target(&source, path.as_deref(), line, Editor::VsCode),
        Editor::VsCode,
    )
}

/// Open the file in the editor when a local checkout holds it, else on GitHub.
#[tauri::command]
pub fn open_source(
    tab: er_protocol::TabKey,
    path: Option<String>,
    line: Option<usize>,
    desktop: State<DesktopState>,
) -> Result<OpenSource, ErError> {
    let source = content_source(&desktop, tab)?;
    launch(
        target(&source, path.as_deref(), line, Editor::Default),
        Editor::Default,
    )
}

/// The tab `wire` names, as its published content shows it.
pub(crate) fn content_source(
    desktop: &DesktopState,
    wire: er_protocol::TabKey,
) -> Result<SourceTab, ErError> {
    let content = super::content_of(desktop, wire)?;
    let tab = &content.tab;
    Ok(SourceTab {
        local_root: local_root(tab),
        pr_head: content
            .pr
            .overview
            .as_ref()
            .map(|pr| pr.head_branch.clone()),
        pr_number: tab.pr_number,
        local_branch_view: tab.local_branch_view.clone(),
        current_branch: tab.current_branch.clone(),
        remote_repo: tab.remote_repo.clone(),
        repo_root: tab.repo_root.clone(),
        git: Arc::clone(&desktop.ports.git),
    })
}

/// A local edit runs in the checkout of the branch the tab shows: its repo,
/// or a branch view's checkout when one exists.
fn local_root(tab: &er_engine::tab_identity::ResolvedTab) -> Option<String> {
    if tab.identity().is_remote() {
        None
    } else if tab.local_branch_view.is_some() {
        tab.checkout_root.clone()
    } else {
        Some(tab.repo_root.clone())
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Editor {
    Default,
    /// VS Code, with no GitHub fallback.
    VsCode,
}

/// Where `path` at `line` opens: the local checkout when it holds the file,
/// else GitHub (never for VS Code).
pub(crate) fn target(
    tab: &SourceTab,
    path: Option<&str>,
    line: Option<usize>,
    editor: Editor,
) -> Target {
    let Some(path) = path else {
        return Target::NeedsCheckout(NO_FILE);
    };
    let line = line.unwrap_or(1);
    // Local editable source is only valid when this tab represents a checked-out
    // local context (working tree, or local-branch view with checkout root).
    if let Some(root) = &tab.local_root {
        let file = Path::new(root).join(path);
        if file.exists() {
            return Target::Local {
                root: root.clone(),
                file,
                line,
            };
        }
    }
    if editor == Editor::VsCode {
        return Target::NeedsCheckout(
            "No local checkout found for this file. Check out the branch in a worktree first.",
        );
    }
    // Fallback: open GitHub URL for PR/tab-backed sources.
    match github_file_url(tab, path, line) {
        Some(url) => Target::Url(url),
        None => Target::NeedsCheckout(
            "No local checkout found for this source. Create editable worktree first.",
        ),
    }
}

fn launch(target: Target, editor: Editor) -> Result<OpenSource, ErError> {
    let (kind, target) = match target {
        Target::Local { root, file, line } => {
            match editor {
                Editor::Default => crate::os_launch::open_editor_at(&root, &file, line),
                Editor::VsCode => crate::os_launch::open_vscode_at(&root, &file, line),
            }
            .map_err(failed)?;
            (
                OpenSourceKind::OpenedLocal,
                file.to_string_lossy().into_owned(),
            )
        }
        Target::Url(url) => {
            open_url_in_browser(url.clone())?;
            (OpenSourceKind::OpenedUrl, url)
        }
        Target::NeedsCheckout(reason) => (OpenSourceKind::NeedsCheckout, reason.to_owned()),
    };
    Ok(OpenSource { kind, target })
}

fn github_file_url(tab: &SourceTab, file_path: &str, line_num: usize) -> Option<String> {
    let pr_head = tab
        .pr_head
        .as_ref()
        .map(|head| head.trim().to_string())
        .filter(|b| !b.is_empty());

    let mut branch_ref = pr_head
        .or_else(|| tab.local_branch_view.clone())
        .unwrap_or_else(|| tab.current_branch.clone())
        .trim()
        .to_string();

    // PR tab whose overview hasn't loaded yet: synchronously ask gh for the head branch.
    if tab.pr_head.is_none() {
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

    let remote = tab.git.remote_url(&tab.repo_root, "origin").ok()?;
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
pub fn reveal_er_folder(
    tab: er_protocol::TabKey,
    desktop: State<DesktopState>,
) -> Result<(), ErError> {
    reveal_folder(&view_bucket(&desktop, tab)?)
}

/// The view bucket of the tab `wire` names: its content's `context.er_dir`.
pub(crate) fn view_bucket(
    desktop: &DesktopState,
    wire: er_protocol::TabKey,
) -> Result<String, ErError> {
    Ok(super::content_of(desktop, wire)?.context.er_dir.clone())
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

    fn tab(local_root: Option<&str>) -> SourceTab {
        SourceTab {
            local_root: local_root.map(str::to_owned),
            pr_head: None,
            pr_number: None,
            local_branch_view: None,
            current_branch: "feat".into(),
            remote_repo: Some("o/r".into()),
            repo_root: "/nowhere".into(),
            git: er_engine::ports::Ports::for_test().git,
        }
    }

    #[test]
    fn open_source_policy_allows_only_checked_out_local_contexts() {
        let working = er_engine::tab_identity::ResolvedTab {
            repo_root: "/repo".into(),
            current_branch: "main".into(),
            base_branch: "main".into(),
            mode: er_engine::tab_identity::DiffMode::Branch,
            tour_is_pr: false,
            remote_repo: None,
            pr_number: None,
            local_branch_view: None,
            pr_head_ref: None,
            checkout_root: None,
        };
        assert_eq!(local_root(&working).as_deref(), Some("/repo"));

        let mut pr = working.clone();
        pr.remote_repo = Some("o/r".into());
        assert_eq!(local_root(&pr), None, "a remote PR has no checkout");
        pr.local_branch_view = Some("feat/pr-head".into());
        pr.current_branch = "feat/pr-head".into();
        assert_eq!(local_root(&pr), None);
        pr.checkout_root = Some("/wt".into());
        assert_eq!(local_root(&pr).as_deref(), Some("/wt"));
    }

    /// The page names the file and the line; a checkout that holds the file
    /// opens it there, VS Code never falls back to GitHub, and a remote tab
    /// opens the file at the line on GitHub.
    #[test]
    fn the_target_is_the_file_the_page_names() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let root = dir.path().to_string_lossy().into_owned();
        std::fs::write(dir.path().join("a.rs"), "").unwrap_or_else(|e| panic!("{e}"));

        let local = tab(Some(&root));
        assert_eq!(
            target(&local, Some("a.rs"), Some(7), Editor::Default),
            Target::Local {
                root: root.clone(),
                file: dir.path().join("a.rs"),
                line: 7,
            }
        );
        assert_eq!(
            target(&local, None, Some(7), Editor::Default),
            Target::NeedsCheckout(NO_FILE)
        );
        let remote = tab(None);
        assert!(matches!(
            target(&remote, Some("a.rs"), Some(7), Editor::VsCode),
            Target::NeedsCheckout(_)
        ));
        assert_eq!(
            target(&remote, Some("a.rs"), Some(7), Editor::Default),
            Target::Url("https://github.com/o/r/blob/feat/a.rs#L7".into())
        );
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
