//! `change-facts.md` — what a diff adds versus what it edits, computed by the
//! engine and handed to triage and review agents beside `diff-tmp`.
//!
//! Reach (how much existing code a change touches) is judged by the agent, but
//! the inputs to that judgement are facts the engine already has: which files
//! are new, which existing files are edited or deleted, what kind each file is,
//! and the repo's declared importance. Computing them here means every agent
//! starts from the same numbers rather than estimating them from a skim of the
//! diff. Why the agent and not the engine makes the call:
//! `docs/adr/0039-reach-is-judged-from-engine-facts.md`.

use crate::config::{FileKindRepoConfig, ImportanceRepoConfig};
use crate::git::{DiffFileHeader, FileKind, FileStatus};
use std::collections::BTreeMap;
use std::fmt::Write as _;

pub const CHANGE_FACTS_FILE: &str = "change-facts.md";

/// Rows listed per section before the rest collapse into a count. Enough to
/// show the touch surface of a large feature; past it the agent has the diff.
const MAX_EXISTING_ROWS: usize = 40;
const MAX_NEW_DIRS: usize = 20;

/// The per-repo tables the facts resolve against.
#[derive(Clone, Copy)]
pub struct RepoRules<'a> {
    pub file_kinds: &'a FileKindRepoConfig,
    /// `None` when the repo declares no importance table at all — distinct
    /// from a table whose rules happen not to claim a path.
    pub importance: Option<&'a ImportanceRepoConfig>,
}

impl<'a> RepoRules<'a> {
    /// An empty importance table is no declaration at all: a repo with none
    /// gets an empty one, and the facts must say "undeclared", not "normal".
    pub fn new(file_kinds: &'a FileKindRepoConfig, importance: &'a ImportanceRepoConfig) -> Self {
        let declared = importance.default.is_some() || !importance.rules.is_empty();
        Self {
            file_kinds,
            importance: declared.then_some(importance),
        }
    }
}

/// A repo's tables held by value, for callers that cannot keep a borrow of a
/// tab or config across the diff preparation.
#[derive(Debug, Clone, Default)]
pub struct OwnedRepoRules {
    pub file_kinds: FileKindRepoConfig,
    pub importance: ImportanceRepoConfig,
}

impl OwnedRepoRules {
    /// The tables `config` declares under `key` (see `storage::rules_key`).
    pub fn from_config(config: &crate::config::ErConfig, key: &str) -> Self {
        Self {
            file_kinds: config.file_kinds.repo(key).cloned().unwrap_or_default(),
            importance: config.importance.repo(key).cloned().unwrap_or_default(),
        }
    }

    pub fn as_rules(&self) -> RepoRules<'_> {
        RepoRules::new(&self.file_kinds, &self.importance)
    }
}

/// Write `change-facts.md` under `er_dir` for `raw`.
///
/// Written on every call, unlike `diff-tmp`: the facts depend on the repo's
/// rule tables as well as the diff, so an unchanged diff after a
/// `[file_kinds]` or `[importance]` edit still needs a fresh file.
pub fn write_change_facts(er_dir: &str, raw: &str, rules: RepoRules<'_>) -> Result<(), String> {
    let facts = render_change_facts(&crate::git::parse_diff_headers(raw), rules);
    super::prepared_diff::atomic_write(std::path::Path::new(er_dir), CHANGE_FACTS_FILE, &facts)
}

struct Existing<'h> {
    header: &'h DiffFileHeader,
    importance: String,
    rank: u8,
}

pub fn render_change_facts(headers: &[DiffFileHeader], rules: RepoRules<'_>) -> String {
    let mut new_code: Vec<&DiffFileHeader> = Vec::new();
    let mut existing: Vec<Existing<'_>> = Vec::new();
    let mut other: BTreeMap<&'static str, (usize, usize, usize)> = BTreeMap::new();

    for h in headers {
        let kind = rules.file_kinds.classify(&h.path);
        if kind != FileKind::Production {
            let e = other.entry(kind.as_str()).or_default();
            e.0 += 1;
            e.1 += h.adds;
            e.2 += h.dels;
            continue;
        }
        if is_new(&h.status) {
            new_code.push(h);
        } else {
            let (importance, rank) = importance_of(rules.importance, &h.path);
            existing.push(Existing {
                header: h,
                importance,
                rank,
            });
        }
    }

    let mut out = String::from(
        "# Change facts\n\nComputed by Easy Review from the diff and this repo's config — not an estimate.\n\n",
    );
    let sum = |files: &mut dyn Iterator<Item = &DiffFileHeader>| {
        files.fold((0usize, 0usize, 0usize), |(n, a, d), h| {
            (n + 1, a + h.adds, d + h.dels)
        })
    };
    let (new_n, new_a, _) = sum(&mut new_code.iter().copied());
    let (ex_n, ex_a, ex_d) = sum(&mut existing.iter().map(|e| e.header));
    let deleted = existing
        .iter()
        .filter(|e| e.header.status == FileStatus::Deleted)
        .count();

    let _ = writeln!(out, "## Code (production files only)\n");
    // Whether anything reaches the new files is the agent's call: file-based
    // routing, autoload and plugin directories make a new file live with no
    // edit to existing code, which the diff alone cannot show.
    let _ = writeln!(out, "- New files: {new_n} (+{new_a}).");
    let _ = writeln!(
        out,
        "- Existing files edited, renamed or deleted: {ex_n} (+{ex_a} −{ex_d}), {deleted} deleted."
    );
    if other.is_empty() {
        let _ = writeln!(out, "- Non-code files: none.");
    } else {
        let parts: Vec<String> = other
            .iter()
            .map(|(kind, (n, a, d))| format!("{kind} {n} (+{a} −{d})"))
            .collect();
        let _ = writeln!(
            out,
            "- Non-code files, left out above: {}.",
            parts.join(", ")
        );
    }
    let _ = writeln!(
        out,
        "- Importance: {}\n",
        if rules.importance.is_some() {
            "declared for this repo (`[importance]`); tiers below are the declared ones."
        } else {
            "not declared for this repo. Tiers are unknown — do not read `undeclared` as `normal`."
        }
    );

    let _ = writeln!(out, "## Existing code this diff touches\n");
    if existing.is_empty() {
        let _ = writeln!(out, "None — every code change is in a new file.\n");
    } else {
        existing.sort_by(|a, b| {
            let churn = |e: &Existing<'_>| e.header.adds + e.header.dels;
            a.rank
                .cmp(&b.rank)
                .then_with(|| churn(b).cmp(&churn(a)))
                .then_with(|| a.header.path.cmp(&b.header.path))
        });
        let _ = writeln!(
            out,
            "| path | change | lines | importance |\n|---|---|---|---|"
        );
        for e in existing.iter().take(MAX_EXISTING_ROWS) {
            let _ = writeln!(
                out,
                "| `{}` | {} | +{} −{} | {} |",
                e.header.path,
                status_word(&e.header.status),
                e.header.adds,
                e.header.dels,
                e.importance
            );
        }
        if existing.len() > MAX_EXISTING_ROWS {
            let _ = writeln!(out, "\n…and {} more.", existing.len() - MAX_EXISTING_ROWS);
        }
        out.push('\n');
    }

    let _ = writeln!(out, "## New code, by directory\n");
    if new_code.is_empty() {
        let _ = writeln!(out, "None.");
    } else {
        let mut dirs: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
        for h in &new_code {
            let dir = h.path.rsplit_once('/').map_or(".", |(d, _)| d);
            let e = dirs.entry(dir).or_default();
            e.0 += 1;
            e.1 += h.adds;
        }
        let mut dirs: Vec<_> = dirs.into_iter().collect();
        dirs.sort_by(|a, b| b.1 .1.cmp(&a.1 .1).then_with(|| a.0.cmp(b.0)));
        for (dir, (n, a)) in dirs.iter().take(MAX_NEW_DIRS) {
            let _ = writeln!(out, "- `{dir}/` — {n} files, +{a}");
        }
        if dirs.len() > MAX_NEW_DIRS {
            let _ = writeln!(
                out,
                "- …and {} more directories.",
                dirs.len() - MAX_NEW_DIRS
            );
        }
    }
    out
}

/// A copy is a new file too: nothing depended on the copy before this diff.
const fn is_new(status: &FileStatus) -> bool {
    matches!(status, FileStatus::Added | FileStatus::Copied(_))
}

const fn status_word(status: &FileStatus) -> &'static str {
    match status {
        FileStatus::Deleted => "deleted",
        FileStatus::Renamed(_) => "renamed",
        FileStatus::Unmerged => "unmerged",
        _ => "edited",
    }
}

/// Label and sort rank: foundational first, undeclared last.
fn importance_of(table: Option<&ImportanceRepoConfig>, path: &str) -> (String, u8) {
    use crate::config::ImportanceTier;
    let Some(table) = table else {
        return ("undeclared".to_string(), 3);
    };
    let (tier, label) = match table.matching_rule(path) {
        Some((_, tier)) => (tier, tier.as_str().to_string()),
        None => {
            let tier = table.default_tier();
            (tier, format!("{} (default)", tier.as_str()))
        }
    };
    let rank = match tier {
        ImportanceTier::Foundational => 0,
        ImportanceTier::Normal => 1,
        ImportanceTier::Isolated => 2,
    };
    (label, rank)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn header(path: &str, status: FileStatus, adds: usize, dels: usize) -> DiffFileHeader {
        DiffFileHeader {
            path: path.to_string(),
            status,
            adds,
            dels,
            hunk_count: 1,
            byte_offset: 0,
            byte_length: 0,
        }
    }

    fn no_rules() -> FileKindRepoConfig {
        FileKindRepoConfig::default()
    }

    #[test]
    fn isolated_feature_lists_no_existing_code() {
        let headers = [
            header("src/plates/service.ts", FileStatus::Added, 300, 0),
            header("src/plates/routes.ts", FileStatus::Added, 80, 0),
            header("src/plates/service.test.ts", FileStatus::Added, 200, 0),
        ];
        let kinds = no_rules();
        let facts = render_change_facts(
            &headers,
            RepoRules {
                file_kinds: &kinds,
                importance: None,
            },
        );
        assert!(facts.contains("- New files: 2 (+380)."), "{facts}");
        // Reach is the agent's call; the facts must not assert it.
        assert!(!facts.contains("Nothing existing calls into"), "{facts}");
        assert!(
            facts.contains("None — every code change is in a new file."),
            "{facts}"
        );
        assert!(facts.contains("test 1 (+200 −0)"), "{facts}");
        assert!(facts.contains("- `src/plates/` — 2 files, +380"), "{facts}");
    }

    #[test]
    fn edits_and_deletes_are_the_touch_surface() {
        let headers = [
            header("src/plates/service.ts", FileStatus::Added, 300, 0),
            header("src/router.ts", FileStatus::Modified, 2, 0),
            header("src/legacy.ts", FileStatus::Deleted, 0, 40),
        ];
        let kinds = no_rules();
        let facts = render_change_facts(
            &headers,
            RepoRules {
                file_kinds: &kinds,
                importance: None,
            },
        );
        assert!(
            facts.contains("Existing files edited, renamed or deleted: 2 (+2 −40), 1 deleted."),
            "{facts}"
        );
        assert!(
            facts.contains("| `src/router.ts` | edited | +2 −0 | undeclared |"),
            "{facts}"
        );
        assert!(
            facts.contains("| `src/legacy.ts` | deleted | +0 −40 | undeclared |"),
            "{facts}"
        );
        assert!(
            facts.contains("do not read `undeclared` as `normal`"),
            "{facts}"
        );
    }

    /// No table means undeclared; a table whose rules miss a path means its
    /// default tier. The agent needs to tell the two apart.
    #[test]
    fn undeclared_differs_from_default_tier() {
        let headers = [
            header("src/router.ts", FileStatus::Modified, 2, 0),
            header("src/util.ts", FileStatus::Modified, 9, 0),
        ];
        let importance = ImportanceRepoConfig {
            default: None,
            rules: BTreeMap::from([("src/router.ts".to_string(), "foundational".to_string())]),
        };
        let kinds = no_rules();
        let facts = render_change_facts(
            &headers,
            RepoRules {
                file_kinds: &kinds,
                importance: Some(&importance),
            },
        );
        assert!(
            facts.contains("| `src/router.ts` | edited | +2 −0 | foundational |"),
            "{facts}"
        );
        assert!(
            facts.contains("| `src/util.ts` | edited | +9 −0 | normal (default) |"),
            "{facts}"
        );
        // Foundational sorts first even with less churn.
        let router = facts.find("src/router.ts").unwrap();
        let util = facts.find("src/util.ts").unwrap();
        assert!(router < util);
    }

    #[test]
    fn file_kind_overrides_move_lines_out_of_code() {
        let headers = [header("src/api/schema.ts", FileStatus::Modified, 900, 10)];
        let kinds = FileKindRepoConfig::new(BTreeMap::from([(
            "src/api/schema.ts".to_string(),
            "generated".to_string(),
        )]));
        let facts = render_change_facts(
            &headers,
            RepoRules {
                file_kinds: &kinds,
                importance: None,
            },
        );
        assert!(facts.contains("generated 1 (+900 −10)"), "{facts}");
        assert!(
            facts.contains("None — every code change is in a new file."),
            "{facts}"
        );
    }

    /// The tab path and the config path must agree on what "declared" means:
    /// an empty `[importance.<repo>]` table declares nothing.
    #[test]
    fn empty_importance_table_is_undeclared_on_every_path() {
        let kinds = no_rules();
        let empty = ImportanceRepoConfig::default();
        assert!(RepoRules::new(&kinds, &empty).importance.is_none());

        let mut config = crate::config::ErConfig::default();
        config
            .importance
            .items
            .insert("svc".to_string(), ImportanceRepoConfig::default());
        let owned = OwnedRepoRules::from_config(&config, "svc");
        assert!(owned.as_rules().importance.is_none());

        config.importance.items.insert(
            "svc".to_string(),
            ImportanceRepoConfig {
                default: Some("isolated".to_string()),
                rules: BTreeMap::new(),
            },
        );
        let owned = OwnedRepoRules::from_config(&config, "svc");
        assert!(owned.as_rules().importance.is_some());
    }

    #[test]
    fn write_change_facts_rewrites_on_every_call() {
        let dir = tempfile::tempdir().unwrap();
        let er_dir = dir.path().to_str().unwrap();
        let raw = "diff --git a/src/a.rs b/src/a.rs\n--- a/src/a.rs\n+++ b/src/a.rs\n@@ -1,1 +1,2 @@\n a\n+b\n";
        let kinds = no_rules();
        write_change_facts(
            er_dir,
            raw,
            RepoRules {
                file_kinds: &kinds,
                importance: None,
            },
        )
        .unwrap();
        let first = std::fs::read_to_string(dir.path().join(CHANGE_FACTS_FILE)).unwrap();
        assert!(first.contains("undeclared"));

        let importance = ImportanceRepoConfig::default();
        write_change_facts(
            er_dir,
            raw,
            RepoRules {
                file_kinds: &kinds,
                importance: Some(&importance),
            },
        )
        .unwrap();
        let second = std::fs::read_to_string(dir.path().join(CHANGE_FACTS_FILE)).unwrap();
        assert!(second.contains("normal (default)"), "{second}");
    }
}
