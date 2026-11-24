//! The flag-on copy actions: the Copy hub (`y`), the AI hub's "Copy context"
//! and the config hub's two sidecar rows. Each reads the active tab's
//! content and view state at the key press; the text comes from the
//! engine's `copy_text`, the App's own builders.

use er_engine::ai::{annotation_store, ErQuestions, ErReview};
use er_engine::copy_text::{self, Built, Copied, Cursor};
use er_service::TabContent;
use er_store::{Artifact, Loaded};

use super::tui_view::{is_stub, TabViewState};
use crate::input::keys::OverlayAction;
use crate::screen::view::HubItem;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pick {
    FullFile,
    Path,
    Hunk,
    Line,
    Context,
    ReviewJson,
    QuestionsJson,
}

pub enum CopyStep {
    Stay,
    Close,
    Notify(String),
    Copy(Pick),
}

pub struct CopyHub {
    pub rows: Vec<HubItem>,
    picks: Vec<Pick>,
    pub selected: usize,
}

const ROWS: [(&str, &str, Pick); 4] = [
    (
        "Full file diff",
        "Copy all hunks for selected file",
        Pick::FullFile,
    ),
    ("File path", "Copy file path to clipboard", Pick::Path),
    ("Hunk", "Copy current hunk", Pick::Hunk),
    ("Line", "Copy current line content", Pick::Line),
];

impl CopyHub {
    /// The App's rows: each needs a file, and Line a cursor line.
    pub fn open(state: &TabViewState, content: &TabContent) -> Self {
        let has_file = !content.diff.files.is_empty();
        let has_line = has_file && state.current_line.is_some();
        let (rows, picks) = ROWS
            .iter()
            .map(|&(label, description, pick)| {
                let row = HubItem {
                    label: label.into(),
                    hint: String::new(),
                    description: description.into(),
                    is_header: false,
                    enabled: if pick == Pick::Line {
                        has_line
                    } else {
                        has_file
                    },
                    opens_stack_pr: false,
                };
                (row, pick)
            })
            .unzip();
        Self {
            rows,
            picks,
            selected: 0,
        }
    }

    pub fn act(&mut self, action: OverlayAction) -> CopyStep {
        match action {
            OverlayAction::Next if self.selected + 1 < self.rows.len() => self.selected += 1,
            OverlayAction::Prev => self.selected = self.selected.saturating_sub(1),
            OverlayAction::Close => return CopyStep::Close,
            OverlayAction::SelectRow => {
                let (Some(row), Some(&pick)) =
                    (self.rows.get(self.selected), self.picks.get(self.selected))
                else {
                    return CopyStep::Close;
                };
                return if row.enabled {
                    CopyStep::Copy(pick)
                } else {
                    CopyStep::Notify(row.description.clone())
                };
            }
            _ => {}
        }
        CopyStep::Stay
    }
}

/// The text `pick` copies from the tab as it stands. A file whose parse has
/// not landed has no hunks to copy yet.
pub fn build(pick: Pick, state: &TabViewState, content: &TabContent) -> Built {
    let files = &content.diff.files;
    let shown = files
        .get(state.selected_file)
        .filter(|_| state.watched.selected.is_none())
        .map(|file| &**file);
    // The App's hunk copy reads the selected index even on a watched file.
    let file = match pick {
        Pick::Hunk => files.get(state.selected_file).map(|file| &**file),
        _ => shown,
    };
    if pick != Pick::Path && file.is_some_and(is_stub) {
        return Err("File is still loading".into());
    }
    let at = Cursor {
        hunk: state.current_hunk,
        line: state.current_line,
        range: state.selected_range(),
    };
    let er_dir = &content.context.er_dir;
    match pick {
        Pick::FullFile => copy_text::full_file(file),
        Pick::Path => copy_text::path(file),
        Pick::Hunk => copy_text::hunk(file, state.current_hunk),
        Pick::Line => copy_text::line(file, &at),
        Pick::Context => {
            let branches = (&*content.tab.current_branch, &*content.tab.base_branch);
            copy_text::context(file, &at, branches, &content.ai.state, &state.layers)
        }
        Pick::ReviewJson => review_json(er_dir),
        Pick::QuestionsJson => questions_json(er_dir),
    }
}

/// The stored bytes of `review.json`, unparsed, as the App copies them.
fn review_json(er_dir: &str) -> Built {
    match er_engine::agent_prompt::stored_text(er_dir, ErReview::FILE) {
        Ok(Loaded::Found(stored)) => sized(stored.value, "review.json"),
        Ok(Loaded::Absent) => Err("No review.json found".into()),
        _ => Err("Failed to read review.json".into()),
    }
}

fn questions_json(er_dir: &str) -> Built {
    let store = er_engine::store_registry::store();
    let scope = er_store::Scope::dir(er_dir).map_err(|e| e.to_string())?;
    let path = annotation_store::path_of::<ErQuestions>(&store, &scope);
    let name = path
        .as_ref()
        .ok()
        .and_then(|path| path.file_name()?.to_str())
        .unwrap_or("annotation file")
        .to_string();
    match annotation_store::questions_text(&store, &scope) {
        Ok(Loaded::Found(stored)) => sized(stored.value, &name),
        Ok(Loaded::Absent) => Err(format!("No {name} found")),
        _ => Err(format!("Failed to read {name}")),
    }
}

fn sized(text: String, name: &str) -> Built {
    let notice = format!("Copied {name} ({} bytes)", text.len());
    Ok(Copied { text, notice })
}
