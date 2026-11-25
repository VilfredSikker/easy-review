//! The flag-on findings: Ctrl+j/k across the files, the File panel's j/k and
//! Enter, the AI Summary panel's keys and its checklist toggle, and the reply
//! on a focused finding. All of it reads the published AI state; the
//! checklist toggle and the reply go to the actor of the tab they were
//! pressed on.
//!
//! Unlike the App, both finding steps skip a finding the Gate hides: Ctrl+j/k
//! keeps the findings that pass `Finding::passes`, and the panel's j/k steps
//! through the rows the File panel draws.

use std::collections::HashSet;

use er_engine::ai::{AiState, ReviewFocus};
use er_engine::git::DiffFile;
use er_service::{Command, TabContent};
use tui_textarea::TextArea;

use super::threads::Focus;
use super::tui_view::{is_stub, TabViewState};
use crate::input::AiSummaryAction;

/// Ctrl+j/k: moves the focus to the next or previous finding in the diff's
/// files, wrapping, and selects its file and line, as the App's
/// `jump_finding` does over the findings that pass the Gate. Answers the
/// file it newly selected.
pub fn jump(state: &mut TabViewState, content: &TabContent, forward: bool) -> Option<usize> {
    let ai = &content.ai.state;
    let files = &content.diff.files;
    let paths: HashSet<&str> = files.iter().map(|file| file.path.as_str()).collect();
    let all: Vec<_> = ai
        .all_findings_ordered()
        .into_iter()
        .filter(|(path, _, _, id)| paths.contains(path.as_str()) && passes(ai, state, path, id))
        .collect();
    let last = all.len().checked_sub(1)?;
    let current = files
        .get(state.selected_file)
        .map(|file| file.path.as_str());
    let hunk = state.current_hunk;
    let at = state
        .focused_finding_id
        .as_ref()
        .and_then(|id| all.iter().position(|finding| &finding.3 == id))
        .filter(|&at| current == Some(all[at].0.as_str()))
        .or_else(|| {
            let current = current?;
            if forward {
                all.iter().position(|(path, at, _, _)| {
                    path.as_str() > current || (path == current && at.unwrap_or(0) >= hunk)
                })
            } else {
                all.iter().rposition(|(path, at, _, _)| {
                    path.as_str() < current || (path == current && at.unwrap_or(0) <= hunk)
                })
            }
        });
    let next = match (at, forward) {
        (Some(at), true) if at < last => at + 1,
        (Some(_) | None, true) => 0,
        (Some(at), false) if at > 0 => at - 1,
        (Some(_) | None, false) => last,
    };
    let (path, hunk, _, id) = &all[next];
    state.focused_finding_id = Some(id.clone());
    state.focused_comment_id = None;
    let mut newly = None;
    if current != Some(path.as_str()) {
        let idx = files.iter().position(|file| &file.path == path)?;
        state.selected_file = idx;
        state.current_hunk = hunk.unwrap_or(0);
        state.current_line = None;
        state.diff_scroll = 0;
        state.h_scroll = 0;
        newly = Some(idx);
    } else if let Some(hunk) = hunk {
        state.current_hunk = *hunk;
        state.current_line = None;
    }
    let file = files.get(state.selected_file).map(|file| &**file);
    if file.is_some_and(is_stub) {
        state.place_on_parse = Some(state.selected_file);
    } else {
        place(state, ai, file);
    }
    newly
}

fn passes(ai: &AiState, state: &TabViewState, path: &str, id: &str) -> bool {
    ai.file_review(path)
        .and_then(|review| review.findings.iter().find(|finding| finding.id == id))
        .is_some_and(|finding| finding.passes(&state.layers))
}

/// Puts the cursor on the focused finding's line in `file`, or on its hunk's
/// last line for a hunk-level finding, and scrolls there.
fn place(state: &mut TabViewState, ai: &AiState, file: Option<&DiffFile>) {
    let finding = file
        .zip(state.focused_finding_id.as_deref())
        .and_then(|(file, id)| {
            let review = ai.file_review(&file.path)?;
            review.findings.iter().find(|finding| finding.id == id)
        });
    let hunk = file.and_then(|file| file.hunks.get(state.current_hunk));
    if let (Some(finding), Some(hunk)) = (finding, hunk) {
        match finding.line_start {
            Some(start) => {
                if let Some(at) = hunk.lines.iter().position(|l| l.new_num == Some(start)) {
                    state.current_line = Some(at);
                }
            }
            None => state.current_line = Some(hunk.lines.len().saturating_sub(1)),
        }
    }
    state.scroll_to_cursor(file);
}

/// A finding's file that was a stub has parsed: the cursor goes to the
/// finding.
pub fn place_if_parsed(state: &mut TabViewState, content: &TabContent) {
    let Some(at) = state.place_on_parse else {
        return;
    };
    // The selection moved on before the parse: the placement is dropped.
    if at != state.selected_file {
        state.place_on_parse = None;
        return;
    }
    let file = content.diff.files.get(at);
    if file.is_some_and(|file| !is_stub(file)) {
        state.place_on_parse = None;
        place(state, &content.ai.state, file.map(|file| &**file));
    }
}

/// Each new AI revision drops a finding focus whose finding is gone and
/// keeps the review cursor on a row, as the App's reload clamps it.
pub fn follow(state: &mut TabViewState, content: &TabContent) {
    let revision = content.ai.revision;
    if state.findings_followed == Some(revision) {
        return;
    }
    state.findings_followed = Some(revision);
    let ai = &content.ai.state;
    let present = |id: &String| {
        ai.review.as_ref().is_some_and(|review| {
            review
                .files
                .values()
                .any(|file| file.findings.iter().any(|finding| &finding.id == id))
        })
    };
    if !state.focused_finding_id.as_ref().is_some_and(present) {
        state.focused_finding_id = None;
    }
    let count = item_count(ai, state.review_focus);
    state.review_cursor = state.review_cursor.min(count.saturating_sub(1));
}

/// The File panel's j/k: the next or previous finding of the selected file,
/// wrapping, among the rows the panel draws.
pub fn step_panel(state: &mut TabViewState, content: &TabContent, forward: bool) {
    let Some(file) = content.diff.files.get(state.selected_file) else {
        return;
    };
    let drawn = crate::ui::panel::drawn_file_findings(&content.ai.state, &state.layers, &file.path);
    let Some(last) = drawn.len().checked_sub(1) else {
        return;
    };
    let at = state
        .focused_finding_id
        .as_ref()
        .and_then(|id| drawn.iter().position(|finding| &finding.id == id));
    let next = match (at, forward) {
        (Some(at), true) if at < last => at + 1,
        (Some(_) | None, true) => 0,
        (Some(at), false) => at.checked_sub(1).unwrap_or(last),
        (None, false) => last,
    };
    state.focused_finding_id = Some(drawn[next].id.clone());
}

/// The File panel's Enter: the diff goes to the focused finding's line in
/// the selected file and the panel lets go, as the App's
/// `jump_to_focused_finding` does. Answers the notice for a line the diff
/// does not hold.
pub fn jump_to_focused(state: &mut TabViewState, content: &TabContent) -> Option<String> {
    let id = state.focused_finding_id.as_deref()?;
    let file = content.diff.files.get(state.selected_file)?;
    let finding = content
        .ai
        .state
        .file_review(&file.path)?
        .findings
        .iter()
        .find(|finding| finding.id == id)?;
    let (hunk, start) = (finding.hunk_index, finding.line_start);
    let hunk_ok = hunk.is_none_or(|hunk| hunk < file.hunks.len());
    let line_ok = start.is_none_or(|start| {
        file.hunks
            .iter()
            .any(|hunk| hunk.lines.iter().any(|line| line.new_num == Some(start)))
    });
    state.panel_focus = false;
    if !hunk_ok || !line_ok {
        let line = start.map_or_else(|| "?".to_string(), |start| start.to_string());
        return Some(format!(
            "Line {line} is outside the diff — open in editor to view"
        ));
    }
    if let Some(hunk) = hunk {
        state.current_hunk = hunk;
        state.current_line = None;
    }
    if let Some(lines) = file.hunks.get(hunk.unwrap_or(0)).map(|hunk| &hunk.lines) {
        state.current_line = match start {
            Some(start) => lines.iter().position(|line| line.new_num == Some(start)),
            None => Some(lines.len().saturating_sub(1)),
        };
    }
    state.scroll_to_cursor(Some(file));
    None
}

/// What an AI Summary key asks of the loop.
pub enum Summary {
    Done,
    Notice(String),
    /// Enter selected a file; it may need a parse.
    Jumped {
        index: usize,
        notice: String,
    },
    /// Space on a checklist row: the toggle to send, and whether the row is
    /// checked after it, read from the row at the press.
    Toggle {
        command: Box<Command>,
        checked: bool,
    },
}

/// Runs an AI Summary key on the tab's view state.
pub fn summary_key(
    state: &mut TabViewState,
    content: &TabContent,
    action: AiSummaryAction,
) -> Summary {
    let ai = &content.ai.state;
    match action {
        AiSummaryAction::Next => {
            if state.review_cursor + 1 < item_count(ai, state.review_focus) {
                state.review_cursor += 1;
            }
        }
        AiSummaryAction::Prev => state.review_cursor = state.review_cursor.saturating_sub(1),
        AiSummaryAction::ToggleFocus => {
            state.review_focus = match state.review_focus {
                ReviewFocus::Files => ReviewFocus::Checklist,
                ReviewFocus::Checklist => ReviewFocus::Files,
            };
            state.review_cursor = 0;
            let (files, checklist) = section_offsets(ai);
            state.panel_scroll = match state.review_focus {
                ReviewFocus::Files => files,
                ReviewFocus::Checklist => checklist,
            };
        }
        AiSummaryAction::ToggleChecklistItem => {
            if state.review_focus != ReviewFocus::Checklist {
                return Summary::Done;
            }
            let index = state.review_cursor;
            let Some(item) = ai.checklist.as_ref().and_then(|list| list.items.get(index)) else {
                return Summary::Done;
            };
            let context = content.context.clone();
            return Summary::Toggle {
                command: Box::new(Command::ToggleChecklistItem { context, index }),
                checked: !item.checked,
            };
        }
        AiSummaryAction::JumpToFile => return jump_to_file(state, content),
        AiSummaryAction::ScrollDown(rows) => {
            state.panel_scroll = state.panel_scroll.saturating_add(rows).min(4096);
        }
        AiSummaryAction::ScrollUp(rows) => {
            state.panel_scroll = state.panel_scroll.saturating_sub(rows);
        }
        AiSummaryAction::Unfocus => state.panel_focus = false,
    }
    Summary::Done
}

/// The App's notice for a checklist row's new state.
pub const fn checklist_notice(checked: bool) -> &'static str {
    if checked {
        "✓ Item checked"
    } else {
        "○ Item unchecked"
    }
}

fn item_count(ai: &AiState, focus: ReviewFocus) -> usize {
    match focus {
        ReviewFocus::Files => ai.review_file_count(),
        ReviewFocus::Checklist => ai.review_checklist_count(),
    }
}

/// Where the AI Summary panel's files and checklist sections start, as the
/// App's `ai_summary_section_offsets` estimates them from what the panel
/// draws.
fn section_offsets(ai: &AiState) -> (u16, u16) {
    // The tab bar and rule, then the summary's header and a blank.
    let mut line: usize = 4;
    match &ai.summary {
        Some(summary) => {
            for text in summary.lines() {
                line += if text.is_empty() || text.starts_with('#') {
                    1
                } else {
                    text.len() / 70 + 1
                };
            }
        }
        None => line += 1,
    }
    line += 1;
    let files = line;
    line += 2;
    match &ai.review {
        Some(review) => {
            line += review.files.len();
            if ai.total_findings() > 0 {
                line += 2;
            }
        }
        None => line += 1,
    }
    line += 1;
    let row = |line: usize| u16::try_from(line).unwrap_or(u16::MAX);
    (row(files), row(line))
}

/// Enter in the AI Summary panel: selects the file under the cursor and
/// focuses its first anchored finding, as the App's `review_jump_to_file`
/// does, among the findings the Gate shows.
fn jump_to_file(state: &mut TabViewState, content: &TabContent) -> Summary {
    let ai = &content.ai.state;
    let path = match state.review_focus {
        ReviewFocus::Files => ai.review_file_at(state.review_cursor),
        ReviewFocus::Checklist => ai.checklist_file_at(state.review_cursor),
    };
    let Some(path) = path else {
        return Summary::Notice("No file associated with this item".into());
    };
    let files = &content.diff.files;
    let Some(index) = files.iter().position(|file| file.path == path) else {
        return Summary::Notice(format!("File not in diff: {path}"));
    };
    let first = ai.file_review(&path).and_then(|review| {
        review
            .findings
            .iter()
            .filter(|finding| finding.passes(&state.layers))
            .filter_map(|finding| Some((finding.hunk_index?, finding)))
            .min_by_key(|(hunk, finding)| (*hunk, finding.line_start))
    });
    state.selected_file = index;
    state.current_hunk = first.map_or(0, |(hunk, _)| hunk);
    state.focused_finding_id = first.map(|(_, finding)| finding.id.clone());
    state.current_line = None;
    state.diff_scroll = 0;
    state.h_scroll = 0;
    if state.panel.is_none() {
        state.panel = Some(er_engine::ai::PanelContent::FileDetail);
    }
    let file = &files[index];
    if is_stub(file) {
        state.scroll_on_parse = true;
    } else {
        state.scroll_to_cursor(Some(file));
    }
    Summary::Jumped {
        index,
        notice: format!("Jumped to: {path}"),
    }
}

/// `r` with no comment focused: a reply draft on the focused finding, as
/// the App's `start_reply_finding` opens one. Submit sends it as the
/// service's finding reply.
pub fn start_reply(state: &mut TabViewState, content: &TabContent) -> Focus {
    let Some(id) = state.focused_finding_id.clone() else {
        return Focus::Nothing;
    };
    if state.sent.is_some() {
        return Focus::Nothing;
    }
    let Some(review) = content.ai.state.review.as_ref() else {
        return Focus::Refused("No AI review loaded — cannot reply to finding");
    };
    let found = review.files.iter().find_map(|(path, file)| {
        let finding = file.findings.iter().find(|finding| finding.id == id)?;
        Some((path, finding))
    });
    let Some((path, finding)) = found else {
        return Focus::Refused("Finding not found — review may be stale");
    };
    let draft = &mut state.draft;
    draft.textarea = TextArea::default();
    draft.file.clone_from(path);
    draft.hunk = finding.hunk_index.unwrap_or(0);
    draft.line = finding.line_start;
    draft.reply_to = None;
    draft.finding_ref = Some(id);
    draft.kind = er_engine::ai::CommentType::GitHubComment;
    draft.edit_id = None;
    draft.client_id = None;
    Focus::Opened
}
