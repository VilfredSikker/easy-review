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

/// Whether `line` is a `stream-json` result event that carries a usable
/// answer. An error result is not a reply: that run settles on its exit.
pub fn is_final_result_line(line: &str) -> bool {
    let Ok(value) = serde_json::from_str::<Value>(line.trim()) else {
        return false;
    };
    value.get("type").and_then(|kind| kind.as_str()) == Some("result")
        && value.get("is_error").and_then(|error| error.as_bool()) != Some(true)
        && value
            .get("result")
            .and_then(|result| result.as_str())
            .is_some_and(|result| !result.trim().is_empty())
}

/// One line of a provider's `stream-json` log, reduced to what a log shows.
#[derive(Debug, Clone, PartialEq)]
pub enum LogEvent {
    /// An assistant event's content, in order.
    Assistant(Vec<LogPart>),
    /// A system event worth showing (cost, usage, result), with its text.
    SystemNote(String),
}

/// One piece of an assistant event's content.
#[derive(Debug, Clone, PartialEq)]
pub enum LogPart {
    Text(String),
    ToolUse { name: String, input: Option<Value> },
}

/// What one log line carries, or `None` when it shows nothing: a non-JSON
/// line, a tool result, the final result event, a hook or an unknown event.
///
/// The display form belongs to the front end; this only decides what each
/// event says.
pub fn decode_log_line(line: &str) -> Option<LogEvent> {
    let value = serde_json::from_str::<Value>(line).ok()?;
    let object = value.as_object()?;

    match object.get("type")?.as_str()? {
        "assistant" => {
            let message = object.get("message")?.as_object()?;
            let content = message.get("content")?.as_array()?;
            let mut parts = Vec::new();
            for item in content {
                let Some(item) = item.as_object() else {
                    continue;
                };
                match item.get("type").and_then(|kind| kind.as_str()) {
                    Some("text") => {
                        let Some(text) = item.get("text").and_then(|text| text.as_str()) else {
                            continue;
                        };
                        let trimmed = text.trim();
                        if !trimmed.is_empty() {
                            parts.push(LogPart::Text(trimmed.to_string()));
                        }
                    }
                    Some("tool_use") => {
                        let name = item
                            .get("name")
                            .and_then(|name| name.as_str())
                            .unwrap_or("tool");
                        parts.push(LogPart::ToolUse {
                            name: name.to_string(),
                            input: item.get("input").cloned(),
                        });
                    }
                    _ => {}
                }
            }
            (!parts.is_empty()).then_some(LogEvent::Assistant(parts))
        }
        "system" => {
            let subtype = object
                .get("subtype")
                .and_then(|subtype| subtype.as_str())
                .unwrap_or("");
            if !(subtype.contains("cost")
                || subtype.contains("usage")
                || subtype.contains("result"))
            {
                return None;
            }
            let text = object
                .get("message")
                .and_then(|message| message.as_str())
                .unwrap_or(subtype);
            Some(LogEvent::SystemNote(text.to_string()))
        }
        _ => None,
    }
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

    #[test]
    fn a_result_line_with_text_is_final() {
        assert!(is_final_result_line(
            r#"{"type":"result","subtype":"success","result":"done"}"#
        ));
    }

    #[test]
    fn an_error_result_is_not_final() {
        assert!(!is_final_result_line(
            r#"{"type":"result","is_error":true,"result":"boom"}"#
        ));
    }

    #[test]
    fn an_empty_or_missing_result_is_not_final() {
        assert!(!is_final_result_line(r#"{"type":"result","result":""}"#));
        assert!(!is_final_result_line(r#"{"type":"result"}"#));
    }

    #[test]
    fn assistant_and_non_json_lines_are_not_final() {
        assert!(!is_final_result_line(
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"working"}]}}"#
        ));
        assert!(!is_final_result_line("not json"));
    }

    /// One assistant event holds text and tool calls interleaved; the order is
    /// the order they arrived in, since the log reprints it verbatim.
    #[test]
    fn a_log_line_decodes_assistant_parts_in_order() {
        let line = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"  Reading file  "},{"type":"tool_use","name":"Read","input":{"file_path":"/src/lib.rs"}},{"type":"text","text":"done"}]}}"#;
        assert_eq!(
            decode_log_line(line),
            Some(LogEvent::Assistant(vec![
                LogPart::Text("Reading file".into()),
                LogPart::ToolUse {
                    name: "Read".into(),
                    input: Some(serde_json::json!({"file_path": "/src/lib.rs"})),
                },
                LogPart::Text("done".into()),
            ]))
        );
    }

    #[test]
    fn a_log_line_suppresses_events_with_nothing_to_show() {
        for line in [
            r#"{"type":"tool_result","content":"some output"}"#,
            r#"{"type":"result","result":"done"}"#,
            r#"{"type":"system","subtype":"hook"}"#,
            r#"{"type":"unknown_event","data":"something"}"#,
            r#"{"type":"assistant","message":{"content":[]}}"#,
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"   "}]}}"#,
            "not json at all",
            "{invalid}",
            "",
        ] {
            assert_eq!(decode_log_line(line), None, "{line}");
        }
    }

    #[test]
    fn a_system_note_uses_its_message_or_its_subtype() {
        assert_eq!(
            decode_log_line(r#"{"type":"system","subtype":"cost","message":"$0.05 used"}"#),
            Some(LogEvent::SystemNote("$0.05 used".into()))
        );
        assert_eq!(
            decode_log_line(r#"{"type":"system","subtype":"usage","message":"1000 tokens"}"#),
            Some(LogEvent::SystemNote("1000 tokens".into()))
        );
        assert_eq!(
            decode_log_line(r#"{"type":"system","subtype":"usage"}"#),
            Some(LogEvent::SystemNote("usage".into()))
        );
    }
}
