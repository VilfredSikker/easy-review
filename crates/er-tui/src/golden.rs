//! Golden frames: each fixture draws a whole screen to `TestBackend` and
//! compares it with a file under `goldens/`. A file holds the rows as text and
//! a digest of every cell's style, so a colour or highlight change fails as
//! surely as a text change. The fixtures are in `service::golden_tests`;
//! `ER_UPDATE_GOLDENS=1` rewrites the files.

use er_engine::config::PackageConfig;
use ratatui::buffer::Buffer;

fn golden_text(buf: &Buffer) -> String {
    let rows = buf.content.chunks(usize::from(buf.area.width));
    rows_text(rows, |line| line.to_string())
}

/// The rows as text, each passed through `line` with its trailing blanks
/// trimmed, then an FNV-1a digest of every cell's style in cell order. The
/// parity goldens share it, so the two formats cannot drift.
pub(crate) fn rows_text<'a>(
    rows: impl IntoIterator<Item = &'a [ratatui::buffer::Cell]>,
    line: impl Fn(&str) -> String,
) -> String {
    let mut out = String::new();
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for row in rows {
        let text: String = row.iter().map(ratatui::buffer::Cell::symbol).collect();
        out.push_str(&line(text.trim_end()));
        out.push('\n');
        for cell in row {
            for byte in format!("{:?}", cell.style()).bytes() {
                hash ^= u64::from(byte);
                hash = hash.wrapping_mul(0x0100_0000_01b3);
            }
        }
    }
    out.push_str(&format!("styles {hash:016x}\n"));
    out
}

/// How a frame differs from the golden file `name`, or `None` when it
/// matches. Under `ER_UPDATE_GOLDENS=1` it rewrites the file instead.
pub(crate) fn differs(name: &str, buffer: &Buffer) -> Option<String> {
    let path = format!("{}/goldens/{name}.txt", env!("CARGO_MANIFEST_DIR"));
    let actual = golden_text(buffer);
    if std::env::var_os("ER_UPDATE_GOLDENS").is_some() {
        assert!(std::env::var_os("CI").is_none(), "record goldens locally");
        std::fs::write(&path, &actual).unwrap_or_else(|e| panic!("{e}"));
        return None;
    }
    let expected = std::fs::read_to_string(&path).unwrap_or_default();
    (expected != actual).then(|| format!("{name}:\n--- golden\n{expected}--- drawn\n{actual}"))
}

pub(crate) const RAW_DIFF: &str = "\
diff --git a/src/auth.rs b/src/auth.rs
index 1111111..2222222 100644
--- a/src/auth.rs
+++ b/src/auth.rs
@@ -1,5 +1,6 @@
 fn check(token: &str) -> bool {
-    token.len() > 0
+    let audience = audience_of(token);
+    !token.is_empty() && audience == \"er\"
 }
 
 fn audience_of(token: &str) -> &str {
diff --git a/README.md b/README.md
index 3333333..4444444 100644
--- a/README.md
+++ b/README.md
@@ -1,2 +1,3 @@
 # er
+Review diffs as they land.
 A terminal diff reviewer.
";

/// A review of `src/auth.rs` with two findings and a question on line 3.
pub(crate) fn seed_review(ai: &mut er_engine::ai::AiState) {
    ai.review = Some(
        serde_json::from_value(serde_json::json!({
            "version": 1, "diff_hash": "hash",
            "files": {"src/auth.rs": {"risk": "high", "summary": "Token check", "findings": [
                {"id": "f1", "severity": "high", "title": "Audience read early",
                 "description": "Read before validation.", "hunk_index": 0, "line_start": 2},
                {"id": "f2", "severity": "low", "title": "Empty token", "hunk_index": 0}
            ]}}
        }))
        .unwrap(),
    );
    ai.questions = Some(
        serde_json::from_value(serde_json::json!({
            "version": 1, "diff_hash": "hash",
            "questions": [{"id": "q1", "file": "src/auth.rs", "hunk_index": 0,
                           "line_start": 3, "text": "Why compare to er?"}]
        }))
        .unwrap(),
    );
}

/// Configures one provider with two models, the first with effort levels.
pub(crate) fn providers_with_efforts(config: &mut er_engine::config::ErConfig) -> (String, String) {
    use er_engine::config::{AiModelConfig, AiProviderConfig};
    let model = |id: &str, efforts: &[&str]| AiModelConfig {
        id: id.into(),
        label: Some(id.to_uppercase()),
        effort_levels: efforts.iter().map(|e| (*e).to_string()).collect(),
        ..AiModelConfig::default()
    };
    config.ai_hub.providers.insert(
        "claude".into(),
        AiProviderConfig {
            label: Some("Claude".into()),
            command: "claude".into(),
            args: Vec::new(),
            family: None,
            models_command: Vec::new(),
            models: vec![model("opus", &["low", "high"]), model("sonnet", &[])],
            removed_catalog_models: Vec::new(),
        },
    );
    ("claude".into(), "opus".into())
}

/// A `web` package with test and lint commands.
pub(crate) fn package_web(config: &mut er_engine::config::ErConfig) {
    config.packages.items.insert(
        "web".into(),
        PackageConfig {
            label: Some("Web".into()),
            test: Some("pnpm test".into()),
            lint: Some("pnpm lint".into()),
            typecheck: None,
            security: None,
        },
    );
}
