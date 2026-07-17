//! Classify diff file paths into review-relevant buckets.
//!
//! Used by MCP/review-queue tooling to separate production churn from tests,
//! Storybook, generated/lock files, and docs.

/// Coarse file category for production-vs-noise diff accounting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileKind {
    /// Application / library source a human should review carefully.
    Production,
    /// Unit/integration/e2e tests and fixtures.
    Test,
    /// Storybook / component gallery stories.
    Storybook,
    /// Lockfiles, minified assets, codegen, snapshots, protobuf stubs, etc.
    Generated,
    /// Markdown / docs / README-style paths.
    Docs,
}

impl FileKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Production => "production",
            Self::Test => "test",
            Self::Storybook => "storybook",
            Self::Generated => "generated",
            Self::Docs => "docs",
        }
    }
}

/// Classify a repo-relative path. Order matters: generated → storybook → test → docs → production.
pub fn classify_path(path: &str) -> FileKind {
    let path = path.trim_start_matches("./");
    let lower = path.to_ascii_lowercase();
    let filename = lower.rsplit('/').next().unwrap_or(lower.as_str());

    if is_generated(&lower, filename) {
        return FileKind::Generated;
    }
    if is_storybook(&lower, filename) {
        return FileKind::Storybook;
    }
    if is_test(&lower, filename) {
        return FileKind::Test;
    }
    if is_docs(&lower, filename) {
        return FileKind::Docs;
    }
    FileKind::Production
}

fn is_generated(path: &str, filename: &str) -> bool {
    // Lockfiles / package manager noise
    matches!(
        filename,
        "package-lock.json"
            | "yarn.lock"
            | "pnpm-lock.yaml"
            | "bun.lock"
            | "bun.lockb"
            | "cargo.lock"
            | "gemfile.lock"
            | "poetry.lock"
            | "composer.lock"
            | "go.sum"
            | "flake.lock"
    ) || filename.ends_with(".lock")
        // Minified / bundled
        || filename.ends_with(".min.js")
        || filename.ends_with(".min.css")
        || filename.ends_with(".min.mjs")
        // Codegen / snapshots
        || filename.contains(".generated.")
        || filename.ends_with(".pb.go")
        || filename.ends_with(".g.dart")
        || filename.ends_with(".snap")
        || filename.ends_with(".snap.json")
        || path_segment(path, "generated")
        || path_segment(path, "__generated__")
        || path_segment(path, ".turbo")
        || path_segment(path, "dist")
        || path.contains("/build/generated/")
        || path.starts_with("build/generated/")
}

/// True when `segment` is a path component (`seg/...` or `.../seg/...`).
fn path_segment(path: &str, segment: &str) -> bool {
    path.starts_with(&format!("{segment}/")) || path.contains(&format!("/{segment}/"))
}

fn is_storybook(path: &str, filename: &str) -> bool {
    filename.contains(".stories.")
        || filename.contains(".story.")
        || path.starts_with(".storybook/")
        || path.contains("/.storybook/")
        || path.contains("/storybook/")
}

fn is_test(path: &str, filename: &str) -> bool {
    // Directories
    path.contains("/__tests__/")
        || path.contains("/__mocks__/")
        || path.contains("/tests/")
        || path.contains("/test/")
        || path.contains("/spec/")
        || path.contains("/fixtures/")
        || path.contains("/testdata/")
        || path.contains("/__fixtures__/")
        || path.starts_with("tests/")
        || path.starts_with("test/")
        || path.starts_with("spec/")
        // Filenames
        || filename.ends_with("_test.rs")
        || filename.ends_with("_test.go")
        || filename.ends_with("_test.py")
        || filename.ends_with("_spec.rb")
        || filename.ends_with("_spec.ts")
        || filename.ends_with("_spec.js")
        || filename.ends_with(".test.ts")
        || filename.ends_with(".test.tsx")
        || filename.ends_with(".test.js")
        || filename.ends_with(".test.jsx")
        || filename.ends_with(".test.mjs")
        || filename.ends_with(".spec.ts")
        || filename.ends_with(".spec.tsx")
        || filename.ends_with(".spec.js")
        || filename.ends_with(".spec.jsx")
        // Python pytest convention only — avoid `test_helpers.rs` etc.
        || (filename.starts_with("test_") && filename.ends_with(".py"))
        || filename == "conftest.py"
}

fn is_docs(path: &str, filename: &str) -> bool {
    path.starts_with("docs/")
        || path.contains("/docs/")
        || filename == "readme.md"
        || filename == "changelog.md"
        || filename == "release_notes.md"
        || filename.ends_with(".md")
        || filename.ends_with(".mdx")
        || filename.ends_with(".rst")
        || filename.ends_with(".adoc")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_production_source() {
        assert_eq!(
            classify_path("crates/er-engine/src/lib.rs"),
            FileKind::Production
        );
        assert_eq!(
            classify_path("desktop-ui/src/App.svelte"),
            FileKind::Production
        );
    }

    #[test]
    fn classifies_tests() {
        assert_eq!(classify_path("src/foo.test.ts"), FileKind::Test);
        assert_eq!(classify_path("pkg/foo_test.go"), FileKind::Test);
        assert_eq!(
            classify_path("crates/er-engine/src/git/diff.rs"),
            FileKind::Production
        );
        assert_eq!(classify_path("tests/integration/foo.rs"), FileKind::Test);
        assert_eq!(classify_path("src/__tests__/Button.tsx"), FileKind::Test);
    }

    #[test]
    fn classifies_storybook() {
        assert_eq!(classify_path("src/Button.stories.tsx"), FileKind::Storybook);
        assert_eq!(classify_path(".storybook/main.ts"), FileKind::Storybook);
    }

    #[test]
    fn classifies_generated_and_locks() {
        assert_eq!(classify_path("Cargo.lock"), FileKind::Generated);
        assert_eq!(classify_path("package-lock.json"), FileKind::Generated);
        assert_eq!(classify_path("src/api.generated.ts"), FileKind::Generated);
        assert_eq!(classify_path("proto/foo.pb.go"), FileKind::Generated);
        assert_eq!(
            classify_path("src/__snapshots__/a.snap"),
            FileKind::Generated
        );
        assert_eq!(classify_path("dist/bundle.js"), FileKind::Generated);
        assert_eq!(classify_path("generated/api.ts"), FileKind::Generated);
        assert_eq!(classify_path("pkg/dist/out.js"), FileKind::Generated);
    }

    #[test]
    fn test_prefix_is_python_only() {
        assert_eq!(classify_path("tests/test_foo.py"), FileKind::Test);
        assert_eq!(classify_path("src/test_helpers.rs"), FileKind::Production);
    }

    #[test]
    fn classifies_docs() {
        assert_eq!(classify_path("README.md"), FileKind::Docs);
        assert_eq!(classify_path("docs/guide.md"), FileKind::Docs);
        assert_eq!(classify_path("notes.mdx"), FileKind::Docs);
    }

    #[test]
    fn generated_beats_test_when_both_match() {
        // Snapshot under a test dir is still generated noise for prod accounting.
        assert_eq!(
            classify_path("src/__tests__/__snapshots__/a.snap"),
            FileKind::Generated
        );
    }
}
