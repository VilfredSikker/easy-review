//! `diff(tab)`: one tab's file list and diff summary, built from the
//! content the workspace published for it. The builder lives beside the
//! service view, so another front end can reuse it.

use std::time::SystemTime;

use er_engine::config::{FileKindRepoConfig, RiskRuleSource, RiskRules};
use er_engine::file_rows::{
    active_finding_count, file_lines_key, mode_str, severity_str, status_str,
};
use er_engine::filter::{parse_filter_expr, FILTER_PRESETS};
use er_engine::git::{DiffFile, FileKind, FileStatus, ProdDiffStats};
use er_engine::preview_key;
use er_engine::tab_identity::DiffMode;
use er_engine::visible_files::{visible_files, FileListView};
use er_protocol as wire;

use crate::content::TabContent;

/// What the list reads beside the content: the repo's rule tables and the
/// tab's file-list view.
#[derive(Default)]
pub struct DiffInputs<'a> {
    pub file_kinds: FileKindRepoConfig,
    pub risk_rules: RiskRules,
    /// The filter expression; empty for none.
    pub filter: &'a str,
    /// The tab's recent filters, newest first.
    pub filter_history: &'a [String],
    /// Newest file first, by the mtimes the load stamped.
    pub sort_by_mtime: bool,
}

/// The version `diff(tab)` answers at: the newest revision of any section its
/// rows read. Revisions start at 1, so loaded content is always above the
/// version-0 answer of a tab that has not loaded.
pub fn diff_version(content: &TabContent) -> u64 {
    content
        .diff
        .revision
        .max(content.reviewed.revision)
        .max(content.ai.revision)
        .max(content.pr.revision)
        .max(content.mode_revision)
}

/// `content` as `diff(tab)` carries it.
pub fn diff_resource(content: &TabContent, inputs: &DiffInputs<'_>) -> wire::TabDiff {
    let tab = &content.tab;
    let meta = &content.diff.meta;
    let files: Vec<&DiffFile> = content.diff.files.iter().map(AsRef::as_ref).collect();
    let rules = parse_filter_expr(inputs.filter);
    let view = FileListView {
        rules: &rules,
        review: content.ai.state.review.as_ref(),
        risk_rules: &inputs.risk_rules,
        file_kinds: &inputs.file_kinds,
        search_lower: "",
        unreviewed_only: false,
        reviewed: &content.reviewed.marks,
    };
    let mut listed = visible_files(files.iter().copied(), &view);
    if inputs.sort_by_mtime {
        let mtime = |file: &DiffFile| {
            meta.stamps
                .mtimes
                .get(&file.path)
                .copied()
                .unwrap_or(SystemTime::UNIX_EPOCH)
        };
        listed.sort_by_key(|(_, file)| std::cmp::Reverse(mtime(file)));
    }
    let keys = content.preview_key_inputs();
    let rows = listed
        .into_iter()
        .map(|(index, file)| file_row(content, inputs, index, file, &keys))
        .collect();
    let stats = ProdDiffStats::summarize(
        files.iter().map(|f| (f.path.as_str(), f.adds, f.dels)),
        |path| inputs.file_kinds.classify(path),
    );
    let overview = content.pr.overview.as_ref();
    let branch = first_non_empty([
        tab.local_branch_view.as_deref().unwrap_or(""),
        &tab.current_branch,
        overview.map_or("", |o| o.head_branch.as_str()),
    ]);
    // A base pinned under `refs/er/` is a deleted branch's commit, no name to show.
    let tab_base = if er_engine::github::is_pinned_pr_base(&tab.base_branch) {
        ""
    } else {
        tab.base_branch.as_str()
    };
    let base = first_non_empty([tab_base, overview.map_or("", |o| o.base_branch.as_str())]);
    let shows_pr_head = tab.mode == DiffMode::PrDiff || tab.identity().is_remote();
    wire::TabDiff {
        mode: mode_str(tab.mode).to_string(),
        branch,
        base,
        files: rows,
        total_count: count(files.len()),
        diff_stats: wire::DiffLineStats {
            total: kind_stat(&stats.total),
            code: kind_stat(&stats.production),
        },
        filter: (!inputs.filter.is_empty()).then(|| inputs.filter.to_string()),
        filter_suggestions: suggestions(&files, inputs),
        preview_context_key: preview_key::preview_context_key(&keys),
        review_bucket_er_dir: content.context.er_dir.clone(),
        local_branch: tab.local_branch_view.clone(),
        local_branch_checked_out: tab.checkout_root.is_some(),
        unstaged_stat: scope_stat(meta.scope_stats.unstaged),
        staged_stat: scope_stat(meta.scope_stats.staged),
        built_from_head_oid: content
            .pr
            .head_oid
            .clone()
            .filter(|_| tab.pr_number.is_some()),
        pr_diff_head_oid: meta
            .preview_head_oid
            .clone()
            .filter(|oid| shows_pr_head && !oid.is_empty()),
        load_error: None,
    }
}

/// The answer for a tab whose first load failed: no rows, and why.
pub fn diff_load_error(message: &str) -> wire::TabDiff {
    wire::TabDiff {
        load_error: Some(message.to_string()),
        ..wire::TabDiff::default()
    }
}

fn file_row(
    content: &TabContent,
    inputs: &DiffInputs<'_>,
    index: usize,
    file: &DiffFile,
    keys: &preview_key::PreviewKeyInputs<'_>,
) -> wire::FileRow {
    let ai = &content.ai.state;
    let declared = inputs.risk_rules.resolve(&file.path);
    wire::FileRow {
        path: file.path.clone(),
        status: status_str(&file.status),
        old_path: match &file.status {
            FileStatus::Renamed(old) | FileStatus::Copied(old) => Some(old.clone()),
            _ => None,
        },
        similarity: file.similarity,
        kind: inputs.file_kinds.classify(&file.path).as_str().to_string(),
        additions: count(file.adds),
        deletions: count(file.dels),
        reviewed: content.reviewed.marks.contains_key(&file.path),
        compacted: file.compacted,
        risk: ai
            .review
            .as_ref()
            .and_then(|review| review.files.get(&file.path))
            .map(|review| severity_str(review.risk).to_string()),
        risk_tier: declared.tier.as_str().to_string(),
        risk_rule: declared.rule.map(|rule| wire::MatchedRiskRule {
            key: rule.key,
            label: rule.label,
            source: match rule.source {
                RiskRuleSource::Project => wire::RiskRuleSource::Project,
                RiskRuleSource::User => wire::RiskRuleSource::User,
            },
        }),
        finding_count: count(active_finding_count(ai, Some(&file.path))),
        comment_count: count(ai.file_github_comment_count(&file.path)),
        question_count: count(ai.file_question_count(&file.path)),
        // A file with no +/- lines (a pure rename, a mode change) never parses
        // into hunks, so it is never a stub.
        is_lazy_stub: content.diff.meta.lazy
            && file.hunks.is_empty()
            && !file.compacted
            && file.adds + file.dels > 0,
        source_index: count(index),
        cache_key: format!("{:016x}", file_lines_key(file)),
        preview_key: preview_key::file_preview_key(keys, &file.path),
    }
}

/// One suggestion per file kind the diff has (code first), the presets, then
/// the tab's recent filters.
fn suggestions(files: &[&DiffFile], inputs: &DiffInputs<'_>) -> Vec<wire::FilterSuggestion> {
    const ORDER: [FileKind; 5] = [
        FileKind::Production,
        FileKind::Test,
        FileKind::Storybook,
        FileKind::Generated,
        FileKind::Docs,
    ];
    let mut counts = [0usize; ORDER.len()];
    for file in files {
        let kind = inputs.file_kinds.classify(&file.path);
        if let Some(i) = ORDER.iter().position(|k| *k == kind) {
            counts[i] += 1;
        }
    }
    let kinds = ORDER
        .into_iter()
        .zip(counts)
        .filter(|(_, n)| *n > 0)
        .map(|(kind, n)| {
            let name = match kind {
                FileKind::Production => "code",
                other => other.as_str(),
            };
            wire::FilterSuggestion {
                kind: "kind".into(),
                name: name.into(),
                expr: format!("kind:{name}"),
                files: Some(count(n)),
            }
        });
    let presets = FILTER_PRESETS.iter().map(|p| wire::FilterSuggestion {
        kind: "preset".into(),
        name: p.name.into(),
        expr: p.expr.into(),
        files: None,
    });
    let history = inputs
        .filter_history
        .iter()
        .map(|expr| wire::FilterSuggestion {
            kind: "history".into(),
            name: expr.clone(),
            expr: expr.clone(),
            files: None,
        });
    kinds.chain(presets).chain(history).collect()
}

fn first_non_empty<const N: usize>(values: [&str; N]) -> String {
    values
        .into_iter()
        .find(|value| !value.is_empty())
        .unwrap_or_default()
        .to_string()
}

fn kind_stat(stats: &er_engine::git::DiffKindStats) -> wire::ScopeStat {
    wire::ScopeStat {
        files: count(stats.files),
        additions: count(stats.additions),
        deletions: count(stats.deletions),
    }
}

fn scope_stat((files, additions, deletions): (usize, usize, usize)) -> wire::ScopeStat {
    wire::ScopeStat {
        files: count(files),
        additions: count(additions),
        deletions: count(deletions),
    }
}

fn count(n: usize) -> u64 {
    u64::try_from(n).unwrap_or(u64::MAX)
}
