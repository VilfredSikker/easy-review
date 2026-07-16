//! Aggregate add/delete line counts by [`FileKind`](super::file_kind::FileKind).

use super::file_kind::{classify_path, FileKind};
use super::DiffFileHeader;
use std::collections::BTreeMap;

/// Per-kind and total line change stats for a diff.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DiffKindStats {
    pub files: usize,
    pub additions: usize,
    pub deletions: usize,
}

impl DiffKindStats {
    pub fn lines_changed(&self) -> usize {
        self.additions.saturating_add(self.deletions)
    }

    fn absorb(&mut self, adds: usize, dels: usize) {
        self.files += 1;
        self.additions += adds;
        self.deletions += dels;
    }
}

/// Full breakdown of a PR/diff into production vs noise buckets.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ProdDiffStats {
    pub total: DiffKindStats,
    pub production: DiffKindStats,
    pub test: DiffKindStats,
    pub storybook: DiffKindStats,
    pub generated: DiffKindStats,
    pub docs: DiffKindStats,
    /// Per-path breakdown (path → kind + adds/dels). Useful for MCP detail views.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<FileDiffStat>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FileDiffStat {
    pub path: String,
    pub kind: FileKind,
    pub additions: usize,
    pub deletions: usize,
}

impl ProdDiffStats {
    /// Build stats from header-only scan results (no hunk allocation).
    pub fn from_headers(headers: &[DiffFileHeader]) -> Self {
        let mut out = Self::default();
        for h in headers {
            out.push_file(&h.path, h.adds, h.dels);
        }
        out
    }

    /// Build stats from a raw unified diff string.
    pub fn from_raw_diff(raw: &str) -> Self {
        Self::from_headers(&super::parse_diff_headers(raw))
    }

    fn push_file(&mut self, path: &str, adds: usize, dels: usize) {
        let kind = classify_path(path);
        self.total.absorb(adds, dels);
        match kind {
            FileKind::Production => self.production.absorb(adds, dels),
            FileKind::Test => self.test.absorb(adds, dels),
            FileKind::Storybook => self.storybook.absorb(adds, dels),
            FileKind::Generated => self.generated.absorb(adds, dels),
            FileKind::Docs => self.docs.absorb(adds, dels),
        }
        self.files.push(FileDiffStat {
            path: path.to_string(),
            kind,
            additions: adds,
            deletions: dels,
        });
    }

    /// Compact summary without the per-file list (cheaper over MCP).
    pub fn summary_only(&self) -> Self {
        Self {
            total: self.total.clone(),
            production: self.production.clone(),
            test: self.test.clone(),
            storybook: self.storybook.clone(),
            generated: self.generated.clone(),
            docs: self.docs.clone(),
            files: Vec::new(),
        }
    }

    /// Counts keyed by kind name for JSON-friendly tooling.
    pub fn by_kind(&self) -> BTreeMap<&'static str, DiffKindStats> {
        BTreeMap::from([
            ("production", self.production.clone()),
            ("test", self.test.clone()),
            ("storybook", self.storybook.clone()),
            ("generated", self.generated.clone()),
            ("docs", self.docs.clone()),
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_production_from_noise() {
        let raw = "\
diff --git a/src/main.rs b/src/main.rs
--- a/src/main.rs
+++ b/src/main.rs
@@ -1,1 +1,2 @@
 fn main() {}
+println!(\"hi\");
diff --git a/src/main.test.ts b/src/main.test.ts
--- a/src/main.test.ts
+++ b/src/main.test.ts
@@ -1,0 +1,3 @@
+test('x', () => {});
+expect(1).toBe(1);
+// end
diff --git a/Button.stories.tsx b/Button.stories.tsx
--- a/Button.stories.tsx
+++ b/Button.stories.tsx
@@ -1,0 +1,1 @@
+export default {};
diff --git a/Cargo.lock b/Cargo.lock
--- a/Cargo.lock
+++ b/Cargo.lock
@@ -1,0 +1,2 @@
+a
+b
diff --git a/README.md b/README.md
--- a/README.md
+++ b/README.md
@@ -1,0 +1,1 @@
+# hi
";
        let stats = ProdDiffStats::from_raw_diff(raw);
        assert_eq!(stats.production.additions, 1);
        assert_eq!(stats.test.additions, 3);
        assert_eq!(stats.storybook.additions, 1);
        assert_eq!(stats.generated.additions, 2);
        assert_eq!(stats.docs.additions, 1);
        assert_eq!(stats.total.additions, 8);
        assert_eq!(stats.files.len(), 5);
    }
}
