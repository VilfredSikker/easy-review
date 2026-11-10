//! One final-text decoder for every provider shape.
//!
//! The card, arena and diagram paths all read a provider's final text out of
//! either plain stdout or Claude/Cursor `stream-json` NDJSON. They carried
//! three copies of this rule; the copies drifted at the edges (which event
//! wins, how assistant blocks join), so the rule lives here once.

use serde_json::Value;

/// The shape a provider's stdout is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputProtocol {
    /// The process's stdout is the final text.
    Plain,
    /// One JSON event per line; the final text is the `result` event's
    /// `result` string, else the concatenated assistant text blocks.
    ClaudeStreamJson,
}

/// The final text from `stdout` under `protocol`.
///
/// The fallback is the raw stdout, so a provider that printed nothing usable
/// still yields what it printed.
pub fn decode_final_text(stdout: &str, protocol: OutputProtocol) -> String {
    if protocol == OutputProtocol::Plain {
        return stdout.to_string();
    }

    let mut last_result = None;
    let mut assistant_text = Vec::new();
    for line in stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        match value.get("type").and_then(|kind| kind.as_str()) {
            Some("result") => {
                if let Some(result) = value.get("result").and_then(|result| result.as_str()) {
                    last_result = Some(result.to_string());
                }
            }
            Some("assistant") => {
                assistant_text.extend(assistant_text_blocks(&value));
            }
            _ => {}
        }
    }
    last_result
        .filter(|text| !text.trim().is_empty())
        .or_else(|| (!assistant_text.is_empty()).then(|| assistant_text.join("\n\n")))
        .unwrap_or_else(|| stdout.to_string())
}

/// The trimmed, non-empty text blocks of one `stream-json` assistant event, in
/// order.
fn assistant_text_blocks(event: &Value) -> impl Iterator<Item = String> + '_ {
    event
        .get("message")
        .and_then(|message| message.get("content"))
        .and_then(|content| content.as_array())
        .into_iter()
        .flatten()
        .filter(|item| item.get("type").and_then(|kind| kind.as_str()) == Some("text"))
        .filter_map(|item| item.get("text").and_then(|text| text.as_str()))
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The same raw shapes the card, arena and diagram paths see. The diagram
    /// case is compared byte-for-byte: the sentinel and its payload must reach
    /// the diagram parser unchanged.
    #[test]
    fn the_live_shapes_decode_the_same_way() {
        let assistant =
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"working"}]}}"#;
        let result_done = r#"{"type":"result","result":"done"}"#;
        let arena = r#"{"type":"result","result":"{\"findings\":[{\"title\":\"x\"}]}"}"#;
        let diagram = "---DIAGRAM_JSON---\n{\"version\":1,\"mermaid\":\"flowchart TD\"}\n---DIAGRAM_JSON_END---\n";
        let diagram_stdout = serde_json::json!({"type": "result", "result": diagram}).to_string();

        let cases = [
            (
                "card plain",
                "plain reply\n".to_string(),
                OutputProtocol::Plain,
                "plain reply\n",
            ),
            (
                "card result",
                format!("{assistant}\n{result_done}"),
                OutputProtocol::ClaudeStreamJson,
                "done",
            ),
            (
                "assistant only",
                assistant.to_string(),
                OutputProtocol::ClaudeStreamJson,
                "working",
            ),
            (
                "arena round",
                arena.to_string(),
                OutputProtocol::ClaudeStreamJson,
                r#"{"findings":[{"title":"x"}]}"#,
            ),
            (
                "diagram sentinel",
                diagram_stdout,
                OutputProtocol::ClaudeStreamJson,
                diagram,
            ),
            (
                "malformed falls back",
                "not json\n".to_string(),
                OutputProtocol::ClaudeStreamJson,
                "not json\n",
            ),
        ];
        for (name, stdout, protocol, expected) in cases {
            assert_eq!(decode_final_text(&stdout, protocol), expected, "{name}");
        }
    }

    #[test]
    fn assistant_blocks_join_with_a_blank_line() {
        let stdout = concat!(
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"a"}]}}"#,
            "\n",
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"b"}]}}"#,
        );
        assert_eq!(
            decode_final_text(stdout, OutputProtocol::ClaudeStreamJson),
            "a\n\nb"
        );
    }

    #[test]
    fn an_empty_result_falls_back_to_the_raw_stdout() {
        let stdout = "{\"type\":\"result\",\"result\":\"\"}\nplain";
        assert_eq!(
            decode_final_text(stdout, OutputProtocol::ClaudeStreamJson),
            stdout
        );
    }
}
