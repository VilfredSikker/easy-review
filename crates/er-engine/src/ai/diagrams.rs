//! Mermaid diagram sidecars (`diagrams/*.json`) — AI-generated visual overviews
//! of a diff (mental model, subsystems, flows, or a custom prompt).
//!
//! Each diagram lives in its own file under `{er_dir}/diagrams/<id>.json`, so
//! regenerating one preset only overwrites that preset's file and custom
//! diagrams accumulate. Diagrams are per-view-bucket like triage/review: the
//! active view's `TabState::er_dir()` is both the write target
//! (`generate_diagram`) and the read source (`load_ai_state`).
//!
//! **Write confinement:** the agent runs with read-only tools and emits the
//! diagram JSON on stdout. Easy Review validates and atomically writes only
//! `diagrams/<id>.json` — so attacker-controlled diff content cannot steer
//! Write/Edit into the repo or other sidecars via prompt injection.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};

pub const DIAGRAM_KIND_MENTAL_MODEL: &str = "mental-model";
pub const DIAGRAM_KIND_SUBSYSTEMS: &str = "subsystems";
pub const DIAGRAM_KIND_FLOWS: &str = "flows";
pub const DIAGRAM_KIND_CUSTOM: &str = "custom";

/// Delimiters the agent must wrap its JSON payload with (host-owned write).
pub const DIAGRAM_JSON_BEGIN: &str = "---DIAGRAM_JSON---";
pub const DIAGRAM_JSON_END: &str = "---END_DIAGRAM_JSON---";

/// One AI-generated mermaid diagram of a diff.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ErDiagram {
    pub version: u32,
    pub diff_hash: String,
    #[serde(default)]
    pub created_at: String,
    /// `mental-model` | `subsystems` | `flows` | `custom`.
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub title: String,
    /// User-supplied prompt for `custom` diagrams (empty for presets). Kept so
    /// a custom diagram can be regenerated with the same intent.
    #[serde(default)]
    pub prompt: String,
    /// Mermaid diagram source (bare source — no ``` fences).
    pub mermaid: String,
    /// File-stem id (`diagrams/<id>.json`); assigned by the loader from the
    /// filename, not trusted from the file body.
    #[serde(default)]
    pub id: String,
    /// Runtime-only staleness flag, set by the loader when the diagram's
    /// `diff_hash` no longer matches the current diff.
    #[serde(skip)]
    pub stale: bool,
}

impl er_store::Artifact for ErDiagram {
    const KIND: &'static str = "diagram";
    const VERSION: u32 = 1;
    const MODE: er_store::Mode = er_store::Mode::Enveloped;
    const FILE: &'static str = "diagrams/{slot}.json";
    type Slot = er_store::DiagramId;

    fn diff_hash(&self) -> Option<&str> {
        Some(&self.diff_hash)
    }
}

/// A selectable diagram preset in the UI.
#[derive(Debug, Clone, Serialize)]
pub struct DiagramPresetInfo {
    pub kind: String,
    pub label: String,
    pub description: String,
}

/// Built-in diagram presets, in display order. `custom` is not listed — it is
/// the free-form prompt entry in the UI.
pub fn diagram_presets() -> Vec<DiagramPresetInfo> {
    vec![
        DiagramPresetInfo {
            kind: DIAGRAM_KIND_MENTAL_MODEL.to_string(),
            label: "Mental model".to_string(),
            description: "High-level map of the areas this diff touches and how they relate"
                .to_string(),
        },
        DiagramPresetInfo {
            kind: DIAGRAM_KIND_SUBSYSTEMS.to_string(),
            label: "Subsystems".to_string(),
            description: "Changed files grouped by subsystem, with interactions".to_string(),
        },
        DiagramPresetInfo {
            kind: DIAGRAM_KIND_FLOWS.to_string(),
            label: "Flows".to_string(),
            description: "Runtime flow through the changed code for the main scenarios".to_string(),
        },
    ]
}

pub fn diagram_kind_label(kind: &str) -> &'static str {
    match kind {
        DIAGRAM_KIND_MENTAL_MODEL => "Mental model",
        DIAGRAM_KIND_SUBSYSTEMS => "Subsystems",
        DIAGRAM_KIND_FLOWS => "Flows",
        DIAGRAM_KIND_CUSTOM => "Custom",
        _ => "Diagram",
    }
}

pub fn is_valid_diagram_kind(kind: &str) -> bool {
    matches!(
        kind,
        DIAGRAM_KIND_MENTAL_MODEL
            | DIAGRAM_KIND_SUBSYSTEMS
            | DIAGRAM_KIND_FLOWS
            | DIAGRAM_KIND_CUSTOM
    )
}

/// Background task kind for a diagram run — `diagram:<diagram-kind>`, so two
/// different presets against the same view bucket don't dedupe against each
/// other (and are distinguishable in the UI).
pub fn diagram_task_kind(kind: &str) -> String {
    format!("diagram:{kind}")
}

/// Diagram id sanity check — ids become file names (`diagrams/<id>.json`), so
/// reject anything that could escape the directory.
pub fn is_safe_diagram_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 120
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Absolute path for a diagram sidecar under `er_dir`, or `None` if `file_name`
/// is not a safe `<id>.json`.
pub fn diagram_sidecar_path(er_dir: &str, file_name: &str) -> Option<PathBuf> {
    let id = file_name.strip_suffix(".json")?;
    if !is_safe_diagram_id(id) {
        return None;
    }
    Some(Path::new(er_dir).join("diagrams").join(file_name))
}

/// Output filename for a new diagram of `kind` — presets overwrite in place
///
/// (`<kind>.json`, one file per preset); `custom` gets a fresh timestamped id
/// so custom diagrams accumulate instead of overwriting each other.
pub fn diagram_output_file(kind: &str) -> String {
    if kind == DIAGRAM_KIND_CUSTOM {
        let ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        format!("custom-{ms}.json")
    } else {
        format!("{kind}.json")
    }
}

/// Parse agent stdout into a validated [`ErDiagram`] and atomically write it
/// to `output_path` (must already be a safe `…/diagrams/<id>.json`).
///
/// The agent must not have Write/Edit tools; this is the only write path.
pub fn persist_diagram_from_agent_stdout(
    stdout: &str,
    stream_json: bool,
    expected_kind: &str,
    expected_diff_hash: &str,
    custom_prompt: Option<&str>,
    output_path: &Path,
) -> Result<()> {
    let mut diagram = parse_diagram_from_agent_stdout(stdout, stream_json)?;
    validate_and_normalize_diagram(
        &mut diagram,
        expected_kind,
        expected_diff_hash,
        custom_prompt,
    )?;
    write_diagram_atomic(output_path, &diagram)
}

/// Validate + atomically write a diagram JSON body an MCP client already
///
/// authored (plain JSON, not wrapped agent stdout — see
/// [`persist_diagram_from_agent_stdout`] for the desktop host-owned-write
/// variant). The MCP caller is the reviewing agent itself, so there is no
/// stdout-extraction step, but `kind`/`diff_hash`/`prompt` are still pinned
/// from the harness rather than trusted from the body.
pub fn persist_diagram_json(
    json_text: &str,
    expected_kind: &str,
    expected_diff_hash: &str,
    custom_prompt: Option<&str>,
    output_path: &Path,
) -> Result<()> {
    let mut diagram: ErDiagram = serde_json::from_str(json_text).context("parse diagram JSON")?;
    validate_and_normalize_diagram(
        &mut diagram,
        expected_kind,
        expected_diff_hash,
        custom_prompt,
    )?;
    write_diagram_atomic(output_path, &diagram)
}

fn parse_diagram_from_agent_stdout(stdout: &str, stream_json: bool) -> Result<ErDiagram> {
    let text = if stream_json {
        er_agent::decode::decode_final_text(
            stdout,
            er_agent::decode::OutputProtocol::ClaudeStreamJson,
        )
    } else {
        stdout.to_string()
    };
    let json_text = extract_diagram_json_payload(&text)
        .ok_or_else(|| anyhow::anyhow!("agent did not emit a diagram JSON payload"))?;
    serde_json::from_str(&json_text).context("parse diagram JSON from agent output")
}

fn validate_and_normalize_diagram(
    diagram: &mut ErDiagram,
    expected_kind: &str,
    expected_diff_hash: &str,
    custom_prompt: Option<&str>,
) -> Result<()> {
    if !is_valid_diagram_kind(expected_kind) {
        bail!("invalid expected diagram kind: {expected_kind}");
    }
    // Never trust the agent for kind / hash / custom prompt — pin from the
    // harness so a prompt-injected payload cannot retarget the sidecar.
    diagram.kind = expected_kind.to_string();
    if diagram.diff_hash.trim().is_empty() || diagram.diff_hash != expected_diff_hash {
        diagram.diff_hash = expected_diff_hash.to_string();
    }
    if expected_kind == DIAGRAM_KIND_CUSTOM {
        diagram.prompt = custom_prompt.unwrap_or("").to_string();
    } else {
        diagram.prompt.clear();
    }
    if diagram.version == 0 {
        diagram.version = 1;
    }
    if diagram.created_at.trim().is_empty() {
        diagram.created_at = unix_now_secs_string();
    }
    let mermaid = diagram.mermaid.trim();
    if mermaid.is_empty() {
        bail!("diagram mermaid source is empty");
    }
    if mermaid.len() > 200_000 {
        bail!("diagram mermaid source is too large");
    }
    diagram.mermaid = mermaid.to_string();
    if diagram.title.trim().is_empty() {
        diagram.title = diagram_kind_label(expected_kind).to_string();
    }
    // id is assigned by the loader from the filename; clear any agent value.
    diagram.id.clear();
    Ok(())
}

fn write_diagram_atomic(path: &Path, diagram: &ErDiagram) -> Result<()> {
    let file_name = path
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| anyhow::anyhow!("diagram path has no file name"))?;
    let id = file_name
        .strip_suffix(".json")
        .ok_or_else(|| anyhow::anyhow!("diagram path must end in .json: {file_name}"))?;
    if !is_safe_diagram_id(id) {
        bail!("diagram id is not safe: {id}");
    }
    let diagrams_dir = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("diagram path has no parent"))?;
    if diagrams_dir.file_name().and_then(|s| s.to_str()) != Some("diagrams") {
        bail!("diagram path must be inside a diagrams directory");
    }
    let bucket = diagrams_dir
        .parent()
        .ok_or_else(|| anyhow::anyhow!("diagram directory has no parent"))?;
    let scope =
        er_store::Scope::dir(bucket).with_context(|| format!("bucket for {}", path.display()))?;
    let slot = er_store::DiagramId::new(id).map_err(|error| anyhow::anyhow!("{error}"))?;
    let key = er_store::ArtifactKey::<ErDiagram>::new(scope, slot)
        .map_err(|error| anyhow::anyhow!("{error}"))?;
    crate::store_registry::store()
        .write(&key, diagram)
        .with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

fn extract_diagram_json_payload(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if let Some((_, after)) = trimmed.split_once(DIAGRAM_JSON_BEGIN) {
        if let Some((body, _)) = after.split_once(DIAGRAM_JSON_END) {
            let body = body.trim();
            if !body.is_empty() {
                return Some(strip_optional_fence(body));
            }
        }
    }
    if let Some(block) = extract_fenced_json(trimmed) {
        return Some(block);
    }
    if trimmed.starts_with('{') && serde_json::from_str::<Value>(trimmed).is_ok() {
        return Some(trimmed.to_string());
    }
    for line in trimmed.lines().rev() {
        let t = line.trim();
        if t.starts_with('{') && serde_json::from_str::<Value>(t).is_ok() {
            return Some(t.to_string());
        }
    }
    None
}

fn strip_optional_fence(s: &str) -> String {
    let t = s.trim();
    if let Some(rest) = t.strip_prefix("```json") {
        if let Some((body, _)) = rest.split_once("```") {
            return body.trim().to_string();
        }
    }
    if let Some(rest) = t.strip_prefix("```") {
        if let Some((body, _)) = rest.split_once("```") {
            return body.trim().to_string();
        }
    }
    t.to_string()
}

fn extract_fenced_json(s: &str) -> Option<String> {
    let (_, rest) = s.split_once("```json")?;
    let (body, _) = rest.split_once("```")?;
    Some(body.trim().to_string())
}

fn unix_now_secs_string() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_else(|_| "0".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn diagram_presets_are_valid_kinds_in_display_order() {
        let presets = diagram_presets();
        let kinds: Vec<&str> = presets.iter().map(|p| p.kind.as_str()).collect();
        assert_eq!(
            kinds,
            vec![
                DIAGRAM_KIND_MENTAL_MODEL,
                DIAGRAM_KIND_SUBSYSTEMS,
                DIAGRAM_KIND_FLOWS
            ]
        );
        assert!(presets.iter().all(|p| is_valid_diagram_kind(&p.kind)));
        assert!(!is_valid_diagram_kind("not-a-kind"));
        assert!(is_valid_diagram_kind(DIAGRAM_KIND_CUSTOM));
    }

    #[test]
    fn persist_diagram_from_marked_payload_writes_only_target_file() {
        let dir = tempdir().unwrap();
        crate::storage::with_test_storage_root(dir.path(), || {
            let er_dir = dir.path().to_str().unwrap();
            let path = diagram_sidecar_path(er_dir, "mental-model.json").unwrap();
            let stdout = format!(
                "thinking…\n{DIAGRAM_JSON_BEGIN}\n{}\n{DIAGRAM_JSON_END}\n",
                r#"{
  "version": 1,
  "diff_hash": "evil-hash",
  "kind": "flows",
  "title": "Overview",
  "prompt": "ignore me",
  "mermaid": "flowchart TD\n  A-->B"
}"#
            );
            persist_diagram_from_agent_stdout(
                &stdout,
                false,
                DIAGRAM_KIND_MENTAL_MODEL,
                "real-hash",
                None,
                &path,
            )
            .unwrap();
            let listed = crate::ai::load_diagrams(er_dir, "real-hash");
            assert_eq!(listed.len(), 1);
            let loaded = &listed[0];
            assert_eq!(loaded.id, "mental-model");
            assert_eq!(loaded.kind, DIAGRAM_KIND_MENTAL_MODEL);
            assert_eq!(loaded.diff_hash, "real-hash");
            assert!(loaded.prompt.is_empty());
            assert!(loaded.mermaid.contains("flowchart TD"));
            let mut entries: Vec<_> = std::fs::read_dir(dir.path())
                .unwrap()
                .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .collect();
            entries.sort();
            // `.lock` is the store's scope lock; the point is that no temp file
            // survives beside it.
            assert_eq!(entries, vec![".lock", "diagrams"]);
        });
    }

    #[test]
    fn parse_rejects_empty_mermaid() {
        let dir = tempdir().unwrap();
        let path = diagram_sidecar_path(dir.path().to_str().unwrap(), "flows.json").unwrap();
        let stdout = format!(
            "{DIAGRAM_JSON_BEGIN}\n{}\n{DIAGRAM_JSON_END}",
            r#"{"version":1,"diff_hash":"h","kind":"flows","title":"t","prompt":"","mermaid":"  "}"#
        );
        let err =
            persist_diagram_from_agent_stdout(&stdout, false, DIAGRAM_KIND_FLOWS, "h", None, &path)
                .unwrap_err();
        assert!(err.to_string().contains("empty"));
        assert!(!path.exists());
    }

    #[test]
    fn a_diagram_is_written_as_an_envelope_and_listed_back() {
        let dir = tempdir().unwrap();
        crate::storage::with_test_storage_root(dir.path(), || {
            let er_dir = dir.path().to_str().unwrap();
            let path = diagram_sidecar_path(er_dir, "flows.json").unwrap();

            persist_diagram_json(
                r#"{"version":1,"diff_hash":"h","mermaid":"flowchart TD\n  A-->B"}"#,
                DIAGRAM_KIND_FLOWS,
                "h",
                None,
                &path,
            )
            .unwrap();

            let raw: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
            assert_eq!(raw["kind"], "diagram");
            assert_eq!(raw["version"], 1);
            assert_eq!(raw["payload"]["mermaid"], "flowchart TD\n  A-->B");

            let listed = crate::ai::load_diagrams(er_dir, "h");
            assert_eq!(listed.len(), 1);
            assert_eq!(listed[0].id, "flows");
            assert!(!listed[0].stale);
        });
    }

    #[test]
    fn persisting_to_a_path_outside_a_diagrams_directory_is_an_error() {
        let dir = tempdir().unwrap();
        let body = r#"{"version":1,"diff_hash":"h","mermaid":"flowchart TD\n  A-->B"}"#;
        for path in [
            dir.path().join("x/flows.json"),
            dir.path().join("diagrams/../flows.json"),
        ] {
            let error =
                persist_diagram_json(body, DIAGRAM_KIND_FLOWS, "h", None, &path).unwrap_err();
            assert!(error.to_string().contains("diagrams"), "{path:?}: {error}");
            assert!(!path.exists());
        }
    }

    #[test]
    fn diagram_output_file_presets_are_stable_customs_are_timestamped() {
        assert_eq!(
            diagram_output_file(DIAGRAM_KIND_MENTAL_MODEL),
            "mental-model.json"
        );
        assert_eq!(
            diagram_output_file(DIAGRAM_KIND_SUBSYSTEMS),
            "subsystems.json"
        );
        assert_eq!(diagram_output_file(DIAGRAM_KIND_FLOWS), "flows.json");
        let custom = diagram_output_file(DIAGRAM_KIND_CUSTOM);
        assert!(
            custom.starts_with("custom-") && custom.ends_with(".json"),
            "{custom}"
        );
    }

    #[test]
    fn persist_diagram_json_pins_kind_and_hash_like_stdout_variant() {
        let dir = tempdir().unwrap();
        crate::storage::with_test_storage_root(dir.path(), || {
            let er_dir = dir.path().to_str().unwrap();
            let path = diagram_sidecar_path(er_dir, "custom-1.json").unwrap();
            let body = r#"{
  "version": 1,
  "diff_hash": "evil-hash",
  "kind": "flows",
  "title": "Overview",
  "prompt": "ignore me",
  "mermaid": "flowchart TD\n  A-->B"
}"#;
            persist_diagram_json(
                body,
                DIAGRAM_KIND_CUSTOM,
                "real-hash",
                Some("what the user actually asked"),
                &path,
            )
            .unwrap();
            let listed = crate::ai::load_diagrams(er_dir, "real-hash");
            assert_eq!(listed.len(), 1);
            let loaded = &listed[0];
            assert_eq!(loaded.id, "custom-1");
            assert_eq!(loaded.kind, DIAGRAM_KIND_CUSTOM);
            assert_eq!(loaded.diff_hash, "real-hash");
            assert_eq!(loaded.prompt, "what the user actually asked");
            assert!(loaded.mermaid.contains("flowchart TD"));
        });
    }

    #[test]
    fn persist_diagram_json_rejects_invalid_json_without_writing() {
        let dir = tempdir().unwrap();
        let path = diagram_sidecar_path(dir.path().to_str().unwrap(), "flows.json").unwrap();
        let err =
            persist_diagram_json("not json", DIAGRAM_KIND_FLOWS, "h", None, &path).unwrap_err();
        assert!(err.to_string().contains("parse diagram JSON"));
        assert!(!path.exists());
    }

    #[test]
    fn extract_from_stream_json_result_event() {
        let stdout = concat!(
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"working"}]}}"#,
            "\n",
            r#"{"type":"result","subtype":"success","result":"---DIAGRAM_JSON---\n{\"version\":1,\"diff_hash\":\"h\",\"kind\":\"subsystems\",\"title\":\"S\",\"prompt\":\"\",\"mermaid\":\"flowchart LR\\n  A-->B\"}\n---END_DIAGRAM_JSON---"}"#,
            "\n",
        );
        let d = parse_diagram_from_agent_stdout(stdout, true).unwrap();
        assert!(d.mermaid.contains("flowchart LR"));
    }

    #[test]
    fn the_sentinel_and_payload_pass_through_the_shared_decoder() {
        let payload = r#"{"version":1,"diff_hash":"h","kind":"subsystems","title":"S","prompt":"","mermaid":"flowchart LR\n  A-->B"}"#;
        let expected = format!("{DIAGRAM_JSON_BEGIN}\n{payload}\n{DIAGRAM_JSON_END}");
        let stdout = serde_json::json!({"type": "result", "result": &expected}).to_string();
        let decoded = er_agent::decode::decode_final_text(
            &stdout,
            er_agent::decode::OutputProtocol::ClaudeStreamJson,
        );
        assert_eq!(
            decoded, expected,
            "the sentinel and payload must pass through byte-for-byte"
        );
        let diagram = parse_diagram_from_agent_stdout(&stdout, true).unwrap();
        assert_eq!(diagram.mermaid, "flowchart LR\n  A-->B");
    }
}
