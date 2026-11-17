//! Which diff files a view lists. The filter rules run first, then the
//! search, then the unreviewed-only toggle. The steps are pure, so a front end
//! that reads published tab content lists the same files as the App's tab.

use std::collections::HashMap;

use crate::ai::ErReview;
use crate::config::{FileKindRepoConfig, RiskRules};
use crate::filter::{apply_filter_with_kinds, FilterRule};
use crate::git::DiffFile;

/// What a file list is filtered by: the view's filter, search and toggle,
/// plus the rule data the filter reads beside the diff.
pub struct FileListView<'a> {
    pub rules: &'a [FilterRule],
    /// The loaded review, which `risk:` rules read.
    pub review: Option<&'a ErReview>,
    pub risk_rules: &'a RiskRules,
    pub file_kinds: &'a FileKindRepoConfig,
    /// The search query, already lowercased; empty for no search.
    pub search_lower: &'a str,
    pub unreviewed_only: bool,
    /// The tab's reviewed marks, by path.
    pub reviewed: &'a HashMap<String, String>,
}

impl FileListView<'_> {
    fn keeps(&self, file: &DiffFile) -> bool {
        apply_filter_with_kinds(
            self.rules,
            file,
            self.review,
            Some(self.risk_rules),
            Some(self.file_kinds),
        )
    }
}

/// The listed files with their index in `files`, in diff order.
pub fn visible_files<'f>(
    files: impl IntoIterator<Item = &'f DiffFile>,
    view: &FileListView<'_>,
) -> Vec<(usize, &'f DiffFile)> {
    let mut visible: Vec<(usize, &DiffFile)> = files.into_iter().enumerate().collect();
    if !view.rules.is_empty() {
        visible.retain(|(_, file)| view.keeps(file));
    }
    if !view.search_lower.is_empty() {
        visible.retain(|(_, file)| file.path.to_lowercase().contains(view.search_lower));
    }
    if view.unreviewed_only {
        visible.retain(|(_, file)| !view.reviewed.contains_key(&file.path));
    }
    visible
}

/// Reviewed and total counts among the files the filter rules keep, ignoring
/// the search and the toggle. `None` when no rule is set.
pub fn filtered_reviewed_count<'f>(
    files: impl IntoIterator<Item = &'f DiffFile>,
    view: &FileListView<'_>,
) -> Option<(usize, usize)> {
    if view.rules.is_empty() {
        return None;
    }
    let (mut total, mut reviewed) = (0, 0);
    for file in files.into_iter().filter(|file| view.keeps(file)) {
        total += 1;
        if view.reviewed.contains_key(&file.path) {
            reviewed += 1;
        }
    }
    Some((reviewed, total))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filter::parse_filter_expr;
    use crate::git::FileStatus;

    fn file(path: &str) -> DiffFile {
        DiffFile {
            path: path.to_string(),
            status: FileStatus::Modified,
            hunks: Vec::new(),
            adds: 1,
            dels: 1,
            compacted: false,
            raw_hunk_count: 0,
            similarity: None,
        }
    }

    fn view<'a>(
        rules: &'a [FilterRule],
        search_lower: &'a str,
        unreviewed_only: bool,
        reviewed: &'a HashMap<String, String>,
        rules_data: &'a (RiskRules, FileKindRepoConfig),
    ) -> FileListView<'a> {
        FileListView {
            rules,
            review: None,
            risk_rules: &rules_data.0,
            file_kinds: &rules_data.1,
            search_lower,
            unreviewed_only,
            reviewed,
        }
    }

    /// Each step narrows what the one before it kept, and the indices stay
    /// those of the full list.
    #[test]
    fn the_filter_then_the_search_then_the_toggle_narrow_the_list() {
        let files = [
            file("src/a.rs"),
            file("docs/a.md"),
            file("src/b.rs"),
            file("src/ab.rs"),
        ];
        let rules = parse_filter_expr("src/*");
        let reviewed = HashMap::from([("src/ab.rs".to_string(), "h".to_string())]);
        let data = (RiskRules::default(), FileKindRepoConfig::default());
        let listed = |rules: &[FilterRule], search: &str, toggle: bool| -> Vec<usize> {
            visible_files(&files, &view(rules, search, toggle, &reviewed, &data))
                .into_iter()
                .map(|(index, _)| index)
                .collect()
        };

        assert_eq!(listed(&[], "", false), vec![0, 1, 2, 3]);
        assert_eq!(listed(&rules, "", false), vec![0, 2, 3]);
        assert_eq!(listed(&rules, "a", false), vec![0, 3]);
        assert_eq!(listed(&rules, "a", true), vec![0]);
        assert_eq!(
            filtered_reviewed_count(&files, &view(&rules, "a", true, &reviewed, &data)),
            Some((1, 3)),
            "the count ignores the search and the toggle"
        );
        assert_eq!(
            filtered_reviewed_count(&files, &view(&[], "", false, &reviewed, &data)),
            None
        );
    }
}
