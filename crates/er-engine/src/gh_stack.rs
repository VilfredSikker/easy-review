//! GitHub stacked-PR support via the `gh stack` extension (`github/gh-stack`).
//!
//! A stack is an ordered chain of branches rooted on a trunk, where each layer
//! carries one PR based on the layer below it, so a reviewer only sees that
//! layer's diff. `gh stack view --json` describes the stack containing the
//! checked-out branch.
//!
//! This module is always-on (no `ui` feature) and holds no UI types: it parses
//! the extension's JSON into a [`Stack`] and turns it into plain
//! [`StackRow`]s that the TUI maps onto hub items. The `gh` invocation itself
//! lives in [`crate::github`] with the rest of the CLI wrappers.

use anyhow::{anyhow, Result};

/// One layer of a stack: a branch plus its PR, when it has one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StackEntry {
    pub branch: String,
    pub pr_number: Option<u64>,
    pub pr_url: Option<String>,
    /// `OPEN` | `MERGED` | `QUEUED`, straight from GitHub.
    pub pr_state: Option<String>,
    pub is_current: bool,
    pub is_merged: bool,
    pub is_queued: bool,
    pub needs_rebase: bool,
}

impl StackEntry {
    /// Whether this layer has a PR that can be opened.
    pub fn is_openable(&self) -> bool {
        self.pr_number.is_some() && self.pr_url.as_deref().is_some_and(|u| !u.is_empty())
    }

    /// `#42` for the PR number column, empty when the layer has no PR yet.
    pub fn pr_label(&self) -> String {
        match self.pr_number {
            Some(n) => format!("#{n}"),
            None => String::new(),
        }
    }

    /// Human description of the layer's state, e.g. `open · current`.
    pub fn state_label(&self) -> String {
        let status = self.status_label();
        if self.is_current {
            format!("{status} · current")
        } else {
            status
        }
    }

    /// State without the `current` marker: `open`, `merged · needs rebase`, …
    ///
    /// Used where the current branch is highlighted by other means — the desktop
    /// stack control marks the selected row instead of repeating it in the text.
    pub fn status_label(&self) -> String {
        if self.pr_number.is_none() {
            return "no PR yet".to_string();
        }

        let mut parts: Vec<String> = Vec::new();
        // The flags win over the reported state: `gh` reports MERGED/QUEUED on
        // the PR itself, but the per-branch booleans are what `gh stack`
        // itself displays icons for.
        if self.is_merged || self.pr_state.as_deref() == Some("MERGED") {
            parts.push("merged".into());
        } else if self.is_queued || self.pr_state.as_deref() == Some("QUEUED") {
            parts.push("queued".into());
        } else {
            parts.push(match self.pr_state.as_deref() {
                Some(state) => state.to_ascii_lowercase(),
                None => "open".into(),
            });
        }
        if self.needs_rebase {
            parts.push("needs rebase".into());
        }
        parts.join(" · ")
    }
}

/// A branch stack, ordered the way a stack viewer reads: top of the stack
/// (the newest PR, merging last) first, trunk at the bottom.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stack {
    pub trunk: String,
    pub current_branch: Option<String>,
    /// Top-of-stack first. `gh stack view --json` emits trunk-first, so this is
    /// the reverse of the wire order.
    pub entries: Vec<StackEntry>,
}

/// Outcome of a stack lookup.
///
/// `Unavailable` is an expected steady state, not an error: the branch may
/// simply not be in a stack, or the extension may not be installed. `Failed` is
/// an unexpected failure (a `gh` that isn't there, auth, network, malformed
/// output) — callers render it the same way but log it and may offer a retry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StackInfo {
    Stack(Stack),
    /// No stack to show; the string is a user-facing reason.
    Unavailable(String),
    /// The lookup failed unexpectedly; the string is a user-facing reason.
    Failed(String),
}

impl StackInfo {
    /// Rows for the hub's stack section, top-of-stack first.
    pub fn rows(&self) -> Vec<StackRow> {
        match self {
            Self::Unavailable(reason) | Self::Failed(reason) => vec![StackRow {
                label: "Stacked PRs".into(),
                hint: String::new(),
                description: reason.clone(),
                enabled: false,
                pr_number: None,
                pr_url: None,
            }],
            Self::Stack(stack) => stack.rows(),
        }
    }
}

impl Stack {
    /// Rows for the hub, top-of-stack first, with the trunk as a final
    /// non-selectable layer so the chain reads complete.
    pub fn rows(&self) -> Vec<StackRow> {
        let mut rows: Vec<StackRow> = self
            .entries
            .iter()
            .map(|entry| StackRow {
                label: entry.branch.clone(),
                hint: entry.pr_label(),
                description: entry.state_label(),
                enabled: entry.is_openable(),
                pr_number: entry.pr_number,
                pr_url: entry.pr_url.clone(),
            })
            .collect();

        // The trunk is usually not part of `branches[]`, but don't duplicate it
        // if the extension ever lists it as a layer.
        if !self.trunk.is_empty() && !self.entries.iter().any(|e| e.branch == self.trunk) {
            rows.push(StackRow {
                label: self.trunk.clone(),
                hint: String::new(),
                description: "trunk".into(),
                enabled: false,
                pr_number: None,
                pr_url: None,
            });
        }

        rows
    }
}

/// One row of the hub's stack section — plain data so the ordering and labels
/// stay testable without a terminal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StackRow {
    /// Branch name.
    pub label: String,
    /// `#42`, empty when the layer has no PR yet.
    pub hint: String,
    /// State description, e.g. `open · current`.
    pub description: String,
    /// Whether the row can be selected (it has a PR to open).
    pub enabled: bool,
    pub pr_number: Option<u64>,
    pub pr_url: Option<String>,
}

/// Look up the stack that contains the checked-out branch in `repo_root`.
///
/// Blocking: shells out to `gh stack view --json`, which is why callers run it
/// on a worker thread. Never fails — a missing extension or a branch outside
/// any stack becomes [`StackInfo::Unavailable`], and anything else
/// [`StackInfo::Failed`].
pub fn load(repo_root: &str) -> StackInfo {
    match crate::github::gh_stack_view_json(repo_root) {
        Ok(json) => match parse_view_json(&json) {
            Ok(stack) => StackInfo::Stack(stack),
            Err(e) => StackInfo::Failed(compact_reason(&format!("unexpected stack output: {e}"))),
        },
        // `gh` writes "not part of a stack" / "install the extension" to stderr,
        // and `gh_stack_view_json` surfaces that as the error message.
        Err(e) => {
            let raw = e.to_string();
            if is_expected_unavailable(&raw) {
                StackInfo::Unavailable(compact_reason(&raw))
            } else {
                StackInfo::Failed(compact_reason(&raw))
            }
        }
    }
}

/// Whether a raw `gh stack view` diagnostic is one of the documented steady
/// states rather than a failure: the branch isn't in a stack (the extension's
/// own wording), or the extension isn't installed (its install hint).
fn is_expected_unavailable(raw: &str) -> bool {
    raw.contains("is not part of a stack") || raw.contains("gh extension install")
}

/// Collapse a multi-line `gh` diagnostic into one short hub description.
fn compact_reason(raw: &str) -> String {
    // The extension prints a multi-line "run `gh extension install …`" hint; the
    // install command is the useful part and is far too long for a hub row.
    if raw.contains("gh extension install") {
        return "gh stack extension not installed".into();
    }

    let cleaned = raw
        .lines()
        .map(|l| l.trim().trim_start_matches('✗').trim())
        .find(|l| !l.is_empty())
        .unwrap_or("unavailable");

    if cleaned.chars().count() > 72 {
        let truncated: String = cleaned.chars().take(69).collect();
        format!("{}…", truncated)
    } else {
        cleaned.to_string()
    }
}

/// Parse `gh stack view --json`.
///
/// The wire shape (gh-stack `cmd/view.go`) is:
///
/// ```json
/// {
///   "trunk": "main",
///   "currentBranch": "feat/api",
///   "branches": [
///     { "name": "feat/auth", "head": "…", "base": "…", "isCurrent": false,
///       "isMerged": false, "isQueued": false, "needsRebase": false,
///       "pr": { "number": 42, "url": "https://…", "state": "OPEN" } }
///   ]
/// }
/// ```
///
/// `branches[]` is trunk-first (the trunk itself is `trunk`, not a branch);
/// the returned [`Stack`] is top-of-stack first. Unknown fields are ignored so a
/// newer extension can add some without breaking the parse.
pub fn parse_view_json(json: &str) -> Result<Stack> {
    let view: ViewJson =
        serde_json::from_str(json).map_err(|e| anyhow!("invalid stack JSON: {e}"))?;
    if view.branches.is_empty() {
        return Err(anyhow!("stack has no branches"));
    }

    let mut entries: Vec<StackEntry> = view
        .branches
        .into_iter()
        .map(|b| {
            let pr = b.pr;
            StackEntry {
                is_current: b.is_current,
                is_merged: b.is_merged,
                is_queued: b.is_queued,
                needs_rebase: b.needs_rebase,
                pr_number: pr.as_ref().map(|p| p.number),
                pr_url: pr.as_ref().map(|p| p.url.clone()).filter(|u| !u.is_empty()),
                pr_state: pr.and_then(|p| p.state),
                branch: b.name,
            }
        })
        .collect();

    // Wire order is trunk-first; a stack viewer reads top-down.
    entries.reverse();

    Ok(Stack {
        trunk: view.trunk,
        current_branch: view.current_branch,
        entries,
    })
}

// ── Wire types ──

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ViewJson {
    #[serde(default)]
    trunk: String,
    #[serde(default)]
    current_branch: Option<String>,
    #[serde(default)]
    branches: Vec<ViewBranch>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ViewBranch {
    name: String,
    #[serde(default)]
    is_current: bool,
    #[serde(default)]
    is_merged: bool,
    #[serde(default)]
    is_queued: bool,
    #[serde(default)]
    needs_rebase: bool,
    #[serde(default)]
    pr: Option<ViewPr>,
}

#[derive(Debug, serde::Deserialize)]
struct ViewPr {
    number: u64,
    #[serde(default)]
    url: String,
    #[serde(default)]
    state: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> String {
        serde_json::json!({
            "trunk": "main",
            "currentBranch": "feat/api",
            "branches": [
                {
                    "name": "feat/auth",
                    "head": "aaa1111",
                    "base": "bbb2222",
                    "isCurrent": false,
                    "isMerged": true,
                    "isQueued": false,
                    "needsRebase": false,
                    "pr": { "number": 41, "url": "https://github.com/o/r/pull/41", "state": "MERGED" }
                },
                {
                    "name": "feat/api",
                    "head": "ccc3333",
                    "base": "ddd4444",
                    "isCurrent": true,
                    "isMerged": false,
                    "isQueued": false,
                    "needsRebase": true,
                    "pr": { "number": 42, "url": "https://github.com/o/r/pull/42", "state": "OPEN" }
                },
                {
                    "name": "feat/ui",
                    "head": "eee5555",
                    "base": "fff6666",
                    "isCurrent": false,
                    "isMerged": false,
                    "isQueued": true,
                    "needsRebase": false,
                    "pr": { "number": 43, "url": "https://github.com/o/r/pull/43", "state": "QUEUED" }
                }
            ]
        })
        .to_string()
    }

    #[test]
    fn parses_trunk_and_current_branch() {
        let stack = parse_view_json(&fixture()).unwrap();
        assert_eq!(stack.trunk, "main");
        assert_eq!(stack.current_branch.as_deref(), Some("feat/api"));
    }

    #[test]
    fn orders_top_of_stack_first() {
        let stack = parse_view_json(&fixture()).unwrap();
        let branches: Vec<&str> = stack.entries.iter().map(|e| e.branch.as_str()).collect();
        // Wire order is trunk-first (auth → api → ui); the stack reads ui-first.
        assert_eq!(branches, vec!["feat/ui", "feat/api", "feat/auth"]);
    }

    #[test]
    fn carries_pr_numbers_urls_and_flags() {
        let stack = parse_view_json(&fixture()).unwrap();
        let top = &stack.entries[0];
        assert_eq!(top.pr_number, Some(43));
        assert_eq!(
            top.pr_url.as_deref(),
            Some("https://github.com/o/r/pull/43")
        );
        assert!(top.is_queued);

        let middle = &stack.entries[1];
        assert!(middle.is_current);
        assert!(middle.needs_rebase);

        let bottom = &stack.entries[2];
        assert!(bottom.is_merged);
    }

    #[test]
    fn layer_without_a_pr_is_kept_but_not_openable() {
        let json = serde_json::json!({
            "trunk": "main",
            "currentBranch": "feat/auth",
            "branches": [
                { "name": "feat/auth", "isCurrent": true, "isMerged": false, "isQueued": false, "needsRebase": false }
            ]
        })
        .to_string();
        let stack = parse_view_json(&json).unwrap();
        let entry = &stack.entries[0];
        assert_eq!(entry.pr_number, None);
        assert_eq!(entry.pr_url, None);
        assert!(!entry.is_openable());

        let rows = stack.rows();
        assert_eq!(rows[0].hint, "");
        assert!(!rows[0].enabled);
        assert_eq!(rows[0].description, "no PR yet · current");
    }

    #[test]
    fn empty_pr_url_is_not_openable() {
        let json = serde_json::json!({
            "trunk": "main",
            "branches": [
                { "name": "feat/auth", "pr": { "number": 41, "url": "", "state": "OPEN" } }
            ]
        })
        .to_string();
        let stack = parse_view_json(&json).unwrap();
        assert_eq!(stack.entries[0].pr_number, Some(41));
        assert!(!stack.entries[0].is_openable());
    }

    #[test]
    fn rows_show_branch_and_pr_number_with_state() {
        let rows = parse_view_json(&fixture()).unwrap().rows();
        assert_eq!(rows[0].label, "feat/ui");
        assert_eq!(rows[0].hint, "#43");
        assert_eq!(rows[0].description, "queued");
        assert!(rows[0].enabled);

        assert_eq!(rows[1].label, "feat/api");
        assert_eq!(rows[1].hint, "#42");
        assert_eq!(rows[1].description, "open · needs rebase · current");

        assert_eq!(rows[2].description, "merged");
    }

    #[test]
    fn rows_append_the_trunk_last() {
        let rows = parse_view_json(&fixture()).unwrap().rows();
        assert_eq!(rows.len(), 4);
        let trunk = rows.last().unwrap();
        assert_eq!(trunk.label, "main");
        assert_eq!(trunk.description, "trunk");
        assert!(!trunk.enabled);
        assert_eq!(trunk.hint, "");
    }

    #[test]
    fn trunk_is_not_duplicated_when_the_extension_lists_it() {
        let json = serde_json::json!({
            "trunk": "main",
            "branches": [
                { "name": "main" },
                { "name": "feat/auth", "pr": { "number": 41, "url": "https://github.com/o/r/pull/41", "state": "OPEN" } }
            ]
        })
        .to_string();
        let rows = parse_view_json(&json).unwrap().rows();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].label, "feat/auth");
        assert_eq!(rows[1].label, "main");
    }

    #[test]
    fn tolerates_unknown_and_missing_fields() {
        let json = serde_json::json!({
            "trunk": "main",
            "somethingNew": true,
            "branches": [
                { "name": "feat/a", "pr": { "number": 7, "url": "https://github.com/o/r/pull/7", "state": "OPEN", "draft": false }, "extra": 1 }
            ]
        })
        .to_string();
        let stack = parse_view_json(&json).unwrap();
        assert_eq!(stack.entries[0].pr_number, Some(7));
        assert_eq!(stack.entries[0].pr_state.as_deref(), Some("OPEN"));
    }

    #[test]
    fn malformed_json_is_an_error() {
        assert!(parse_view_json("not json").is_err());
        assert!(parse_view_json("").is_err());
        assert!(parse_view_json("[]").is_err());
    }

    #[test]
    fn json_without_branches_is_an_error() {
        assert!(parse_view_json(r#"{"trunk":"main","branches":[]}"#).is_err());
    }

    #[test]
    fn unavailable_yields_one_disabled_row() {
        let info = StackInfo::Unavailable("current branch is not part of a stack".into());
        let rows = info.rows();
        assert_eq!(rows.len(), 1);
        assert!(!rows[0].enabled);
        assert_eq!(rows[0].description, "current branch is not part of a stack");
        assert!(matches!(info, StackInfo::Unavailable(_)));
    }

    #[test]
    fn failed_renders_like_unavailable() {
        let info = StackInfo::Failed("Failed to run `gh stack view`".into());
        let rows = info.rows();
        assert_eq!(rows.len(), 1);
        assert!(!rows[0].enabled);
        assert_eq!(rows[0].description, "Failed to run `gh stack view`");
    }

    #[test]
    fn only_the_documented_steady_states_are_expected() {
        assert!(is_expected_unavailable(
            "✗ current branch \"feat/x\" is not part of a stack"
        ));
        assert!(is_expected_unavailable(
            "To install it, run:\n  gh extension install github/gh-stack"
        ));
        // A missing `gh`, an auth failure, or a network blip is not.
        assert!(!is_expected_unavailable("Failed to run `gh stack view`"));
        assert!(!is_expected_unavailable(
            "gh: Not logged in to any GitHub hosts"
        ));
        assert!(!is_expected_unavailable("HTTP 502 from api.github.com"));
    }

    #[test]
    fn missing_extension_reason_is_concise() {
        let raw = "gh stack is available as an official extension.\nTo install it, run:\n  gh extension install github/gh-stack";
        assert_eq!(compact_reason(raw), "gh stack extension not installed");
    }

    #[test]
    fn not_in_a_stack_reason_drops_the_diagnostic_prefix() {
        let raw = "✗ current branch \"gh-stack-support\" is not part of a stack";
        assert_eq!(
            compact_reason(raw),
            "current branch \"gh-stack-support\" is not part of a stack"
        );
    }

    #[test]
    fn long_reasons_are_truncated() {
        let raw = "x".repeat(200);
        let reason = compact_reason(&raw);
        assert!(reason.chars().count() <= 72);
        assert!(reason.ends_with('…'));
    }

    #[test]
    fn empty_reason_falls_back() {
        assert_eq!(compact_reason("   \n  "), "unavailable");
    }
}
