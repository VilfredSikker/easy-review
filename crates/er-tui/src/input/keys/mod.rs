//! The key table: each key becomes a `TuiAction`, chosen from the `Screen`
//! alone, so the App path and the service path read one table. A step only
//! picks the action; the executor in `input/` runs it, so nothing here reads
//! the App or writes anything.

mod normal;
mod overlay;

use crate::screen::view::InputMode;
use crate::screen::Screen;
use crossterm::event::KeyEvent;
use er_engine::config::SettingsScope;
use er_engine::tab_identity::DiffMode;

#[cfg(test)]
pub use overlay::{review_flow_results_key_action, ReviewFlowResultsKeyAction};

/// What a key asks for. A key its mode binds to nothing, or whose guard
/// fails, is `Noop`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TuiAction {
    Noop,
    Normal(NormalAction),
    /// Normal mode with the AI summary panel focused: it takes every key the
    /// global keys leave.
    AiSummary(AiSummaryAction),
    History(HistoryAction),
    Tour(TourAction),
    Overlay(OverlayAction),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NormalAction {
    Quit,
    StartQuestion,
    SwitchMode(DiffMode),
    ToggleMtimeSort,
    Refresh,
    ToggleWatch,
    /// Edits the focused comment when it is the user's own, else opens the
    /// file in the editor.
    Edit,
    PrevHint,
    NextHint,
    NextFinding,
    PrevFinding,
    AskDeleteWatchedFile,
    AskDeleteComment,
    Reply,
    AskClearQuestions,
    AskClearReviews,
    CloseTab,
    NextTab,
    PrevTab,
    OpenOpenHub,
    ToggleWatchedFiles,
    ResizeFileTree(i16),
    ResizePanel(i16),
    NavigatePanelFinding(bool),
    JumpToFocusedFinding,
    PanelLineDown,
    PanelLineUp,
    UnfocusPanel,
    StartSearch,
    StartFilter,
    OpenFilterHistory,
    OpenConfigHub,
    ToggleAiFindings,
    AskPush,
    TogglePanel,
    TogglePanelReverse,
    ResumeComment,
    ToggleSplitFocus,
    TogglePanelFocus,
    StartCommit,
    StartGithubComment,
    ToggleComments,
    ToggleQuestions,
    ToggleHideResolved,
    CycleMinTrust,
    SyncGithubComments,
    OpenAiHub,
    OpenGitHub,
    OpenVerifyHub,
    OpenHelpHub,
    ToggleCompacted,
    ExpandContext,
    CollapseContext,
    ClearSearch,
    ClearFilter,
    UpdateWatchedSnapshot,
    ToggleStaged,
    ToggleUnreviewedOnly,
    ToggleReviewed,
    NextUnreviewedFile,
    OpenCopyHub,
    OpenExportPicker,
    PrevFile,
    NextFile,
    ExtendSelectionDown,
    ExtendSelectionUp,
    NextLine,
    PrevLine,
    NextHunk,
    PrevHunk,
    ScrollRight,
    ScrollLeft,
    ScrollHome,
    /// Scrolls the focused panel, or the diff when no panel has focus.
    ScrollDown(u16),
    ScrollUp(u16),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AiSummaryAction {
    Next,
    Prev,
    ToggleFocus,
    ToggleChecklistItem,
    JumpToFile,
    ScrollDown(u16),
    ScrollUp(u16),
    Unfocus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HistoryAction {
    NextCommit,
    PrevCommit,
    NextFile,
    PrevFile,
    NextLine,
    PrevLine,
    ScrollRight,
    ScrollLeft,
    ScrollHome,
    ScrollDown(u16),
    ScrollUp(u16),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TourAction {
    NextPillar,
    PrevPillar,
    NextFile,
    PrevFile,
    NextLine,
    PrevLine,
    ScrollRight,
    ScrollLeft,
    ScrollHome,
    ScrollDown(u16),
    ScrollUp(u16),
    ToggleReviewed,
    ReviewPillar,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OverlayAction {
    Next,
    Prev,
    Close,
    ScrollDetail(isize),
    RerunReviewFlow,
    ToggleFlowFinding,
    AcceptFlowFindings,
    ToggleExportKind,
    Export,
    EditSetting(Edit),
    ActivateSetting,
    ActivateSettingBack,
    DeleteSetting,
    SwitchSettingsTab(SettingsScope),
    CloseSettings,
    /// Runs the selected row: a hub row's action, a directory, a worktree.
    SelectRow,
    /// The selected row's second action; only a stacked-PR row has one.
    SecondaryRow,
    GoUp,
}

/// A key in a one-line text field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edit {
    Insert(char),
    Backspace,
    Left,
    Right,
    Submit,
    Cancel,
}

impl From<NormalAction> for TuiAction {
    fn from(action: NormalAction) -> Self {
        Self::Normal(action)
    }
}

/// The action for `key` on this screen: an open overlay takes the key first,
/// then the input mode.
pub fn key_action(key: KeyEvent, screen: &Screen) -> TuiAction {
    if let Some(overlay) = &screen.chrome.overlay {
        return overlay::action(key, overlay);
    }
    match screen.chrome.input_mode {
        InputMode::Normal => normal::action(key, screen),
        // The text inputs and the y/n prompt still read keys in their own
        // handlers.
        _ => TuiAction::Noop,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::screen::view::{self, ConfigEditState, OverlayData};
    use crossterm::event::{KeyCode, KeyModifiers};
    use std::collections::{BTreeMap, HashSet};

    /// Printable ASCII and the special keys, each bare, with Shift and with Ctrl.
    fn probe_keys() -> Vec<KeyEvent> {
        let specials = [
            KeyCode::Enter,
            KeyCode::Esc,
            KeyCode::Tab,
            KeyCode::BackTab,
            KeyCode::Backspace,
            KeyCode::Delete,
            KeyCode::Insert,
            KeyCode::Up,
            KeyCode::Down,
            KeyCode::Left,
            KeyCode::Right,
            KeyCode::Home,
            KeyCode::End,
            KeyCode::PageUp,
            KeyCode::PageDown,
            KeyCode::F(1),
        ];
        let codes = (' '..='~').map(KeyCode::Char).chain(specials);
        codes
            .flat_map(|code| {
                [
                    KeyModifiers::NONE,
                    KeyModifiers::SHIFT,
                    KeyModifiers::CONTROL,
                ]
                .map(|modifiers| KeyEvent::new(code, modifiers))
            })
            .collect()
    }

    /// `C-q`, `S-Enter`, `Space`: how a key is written in the tables below.
    fn key_name(key: KeyEvent) -> String {
        let code = match key.code {
            KeyCode::Char(' ') => "Space".to_owned(),
            KeyCode::Char(c) => c.to_string(),
            KeyCode::F(n) => format!("F{n}"),
            code => format!("{code:?}"),
        };
        match key.modifiers {
            KeyModifiers::SHIFT => format!("S-{code}"),
            KeyModifiers::CONTROL => format!("C-{code}"),
            _ => code,
        }
    }

    /// Asks the table for every probe key on `screen` and compares each
    /// action with `tables`. A table line is `keys => action`; `*k` stands
    /// for `k`, `S-k` and `C-k`. A key keeps the action of the first line
    /// that names it, as the step tries its groups in order, and a key no line
    /// names must be `Noop`.
    fn assert_table(context: &str, screen: &Screen, tables: &[&str]) {
        let mut expected = BTreeMap::new();
        for line in tables.iter().flat_map(|table| table.lines()) {
            let Some((keys, action)) = line.split_once(" => ") else {
                continue;
            };
            for key in keys.split_whitespace() {
                let names = match key.strip_prefix('*') {
                    Some(bare) => vec![bare.to_owned(), format!("S-{bare}"), format!("C-{bare}")],
                    None => vec![key.to_owned()],
                };
                for name in names {
                    expected
                        .entry(name)
                        .or_insert_with(|| action.trim().to_owned());
                }
            }
        }
        let mut wrong = Vec::new();
        let mut seen = HashSet::new();
        for key in probe_keys() {
            let name = key_name(key);
            let want = expected.get(&name).map_or("Noop", String::as_str);
            let got = format!("{:?}", key_action(key, screen));
            if got != want {
                wrong.push(format!("{name}: want {want}, got {got}"));
            }
            seen.insert(name);
        }
        for name in expected.keys().filter(|name| !seen.contains(*name)) {
            wrong.push(format!("{name}: not a probe key"));
        }
        assert!(wrong.is_empty(), "{context}:\n{}", wrong.join("\n"));
    }

    const GLOBAL: &str = "
        C-q => Normal(Quit)
        q S-q => Normal(StartQuestion)
        *1 => Normal(SwitchMode(Branch))
        *2 => Normal(SwitchMode(Unstaged))
        *3 => Normal(SwitchMode(Staged))
        *4 *5 *6 *7 *8 *9 => Noop
        *m => Normal(ToggleMtimeSort)
        *R => Normal(Refresh)
        *w => Normal(ToggleWatch)
        *e => Normal(Edit)
        *J => Normal(PrevHint)
        *K => Normal(NextHint)
        C-k => Normal(NextFinding)
        C-j => Normal(PrevFinding)
        *r => Normal(Reply)
        z => Normal(AskClearQuestions)
        *Z => Normal(AskClearReviews)
        *x => Normal(CloseTab)
        *] => Normal(NextTab)
        *[ => Normal(PrevTab)
        *o => Normal(OpenOpenHub)
        *W => Normal(ToggleWatchedFiles)
        *< => Normal(ResizeFileTree(-2))
        *> => Normal(ResizeFileTree(2))
        *{ => Normal(ResizePanel(-4))
        *} => Normal(ResizePanel(4))";

    /// The shared keys every diff mode has, History and Tour included.
    const SHARED: &str = "
        */ => Normal(StartSearch)
        *f => Normal(StartFilter)
        *F => Normal(OpenFilterHistory)
        *, => Normal(OpenConfigHub)
        *A => Normal(ToggleAiFindings)
        C-p => Normal(AskPush)
        p S-p => Normal(TogglePanel)
        *Tab => Normal(TogglePanelFocus)
        *c => Normal(StartGithubComment)
        *C => Normal(ToggleComments)
        *Q => Normal(ToggleQuestions)
        *X => Normal(ToggleHideResolved)
        *T => Normal(CycleMinTrust)
        *G => Normal(SyncGithubComments)
        *P => Normal(TogglePanelReverse)
        *a => Normal(OpenAiHub)
        *g => Normal(OpenGitHub)
        *v => Normal(OpenVerifyHub)
        *? => Normal(OpenHelpHub)
        *Enter *Esc => Noop
        *+ *= => Normal(ExpandContext)
        *- => Normal(CollapseContext)";

    /// The shared keys History and Tour leave out.
    const DIFF_ONLY: &str = "
        s => Normal(ToggleStaged)
        *! => Normal(ToggleUnreviewedOnly)
        *Space => Normal(ToggleReviewed)
        U => Normal(NextUnreviewedFile)
        y => Normal(OpenCopyHub)
        S-E => Normal(OpenExportPicker)";

    const NAVIGATION: &str = "
        *j => Normal(PrevFile)
        *k => Normal(NextFile)
        S-Down => Normal(ExtendSelectionDown)
        S-Up => Normal(ExtendSelectionUp)
        *Down => Normal(NextLine)
        *Up => Normal(PrevLine)
        *n => Normal(NextHunk)
        *N => Normal(PrevHunk)
        *l *Right => Normal(ScrollRight)
        *h *Left => Normal(ScrollLeft)
        *Home => Normal(ScrollHome)
        d C-d => Normal(ScrollDown(10))
        u C-u => Normal(ScrollUp(10))
        *PageDown => Normal(ScrollDown(20))
        *PageUp => Normal(ScrollUp(20))";

    const HISTORY: &str = "
        *k => History(NextCommit)
        *j => History(PrevCommit)
        *n => History(NextFile)
        *N => History(PrevFile)
        *Down => History(NextLine)
        *Up => History(PrevLine)
        *l *Right => History(ScrollRight)
        *h *Left => History(ScrollLeft)
        *Home => History(ScrollHome)
        d C-d => History(ScrollDown(10))
        u C-u => History(ScrollUp(10))
        *PageDown => History(ScrollDown(20))
        *PageUp => History(ScrollUp(20))";

    const TOUR: &str = "
        *k => Tour(NextPillar)
        *j => Tour(PrevPillar)
        *n => Tour(NextFile)
        *N => Tour(PrevFile)
        *Down => Tour(NextLine)
        *Up => Tour(PrevLine)
        *l *Right => Tour(ScrollRight)
        *h *Left => Tour(ScrollLeft)
        *Home => Tour(ScrollHome)
        d C-d => Tour(ScrollDown(10))
        u C-u => Tour(ScrollUp(10))
        *PageDown => Tour(ScrollDown(20))
        *PageUp => Tour(ScrollUp(20))
        *Space => Tour(ToggleReviewed)
        *b => Tour(ReviewPillar)";

    const AI_SUMMARY: &str = "
        *k *Down => AiSummary(Next)
        *j *Up => AiSummary(Prev)
        *Tab *BackTab *l *h *Right *Left => AiSummary(ToggleFocus)
        *Space => AiSummary(ToggleChecklistItem)
        *Enter => AiSummary(JumpToFile)
        d C-d => AiSummary(ScrollDown(10))
        u C-u => AiSummary(ScrollUp(10))
        *PageDown => AiSummary(ScrollDown(20))
        *PageUp => AiSummary(ScrollUp(20))
        *Esc => AiSummary(Unfocus)";

    const PANEL: &str = "
        *k *Down => Normal(PanelLineDown)
        *j *Up => Normal(PanelLineUp)
        *Esc => Normal(UnfocusPanel)";

    const LIST: &str = "
        *j *Down => Overlay(Next)
        *k *Up => Overlay(Prev)
        *Esc *q => Overlay(Close)";

    #[test]
    fn the_key_table_pins_every_key_in_normal_mode() {
        let app = crate::golden::base_app();
        let mut cache = crate::screen::ViewCache::default();
        let mut screen = crate::screen::from_app(&app, &mut cache);
        screen.tab.mode = DiffMode::Branch;
        screen.tab.modes = vec![DiffMode::Branch, DiffMode::Unstaged, DiffMode::Staged];
        screen.chrome.split_diff = false;
        assert_table("normal", &screen, &[GLOBAL, SHARED, DIFF_ONLY, NAVIGATION]);

        screen.tab.mode = DiffMode::History;
        assert_table("history", &screen, &[GLOBAL, SHARED, HISTORY]);
        screen.tab.mode = DiffMode::Tour;
        assert_table("tour", &screen, &[GLOBAL, SHARED, TOUR]);

        screen.tab.mode = DiffMode::Branch;
        screen.tab.panel_focus = true;
        screen.tab.panel = Some(er_engine::ai::PanelContent::AiSummary);
        assert_table("ai summary focused", &screen, &[GLOBAL, AI_SUMMARY]);
        screen.tab.panel = Some(er_engine::ai::PanelContent::FileDetail);
        assert_table(
            "panel focused",
            &screen,
            &[GLOBAL, PANEL, SHARED, DIFF_ONLY, NAVIGATION],
        );
    }

    /// The guards that send one key to another action.
    #[test]
    fn guards_pick_the_action_from_the_screen() {
        let app = crate::golden::base_app();
        let mut cache = crate::screen::ViewCache::default();
        let mut screen = crate::screen::from_app(&app, &mut cache);
        let key = |code| KeyEvent::new(code, KeyModifiers::NONE);
        let action = |screen: &Screen, code| key_action(key(code), screen);
        let check = |screen: &Screen, code, want: NormalAction| {
            assert_eq!(action(screen, code), want.into());
        };
        use NormalAction as A;
        let compacted = er_engine::git::DiffFile {
            compacted: true,
            ..screen.tab.selected_diff_file.unwrap().clone()
        };

        screen.tab.mode = DiffMode::Hidden;
        check(&screen, KeyCode::Char('x'), A::AskDeleteWatchedFile);
        screen.tab.mode = DiffMode::Staged;
        check(&screen, KeyCode::Char('c'), A::StartCommit);
        screen.tab.focused_comment_id = Some("c1");
        check(&screen, KeyCode::Char('x'), A::AskDeleteComment);

        screen.chrome.split_diff = true;
        check(&screen, KeyCode::Tab, A::ToggleSplitFocus);
        screen.tab.mode = DiffMode::History;
        check(&screen, KeyCode::Tab, A::TogglePanelFocus);
        screen.chrome.has_comment_draft = true;
        check(&screen, KeyCode::Tab, A::ResumeComment);

        screen.tab.mode = DiffMode::Branch;
        screen.tab.filter_expr = "+*.rs";
        check(&screen, KeyCode::Esc, A::ClearFilter);
        screen.tab.search_query = "auth";
        check(&screen, KeyCode::Esc, A::ClearSearch);
        screen.tab.selected_diff_file = Some(&compacted);
        check(&screen, KeyCode::Enter, A::ToggleCompacted);

        screen.tab.selected_watched_index = Some(0);
        check(&screen, KeyCode::Char('s'), A::UpdateWatchedSnapshot);
        screen.tab.is_remote = true;
        assert_eq!(action(&screen, KeyCode::Char('s')), TuiAction::Noop);

        screen.tab.modes = vec![DiffMode::PrDiff];
        check(&screen, KeyCode::Char('1'), A::SwitchMode(DiffMode::PrDiff));
        assert_eq!(action(&screen, KeyCode::Char('2')), TuiAction::Noop);

        screen.tab.panel_focus = true;
        screen.tab.panel = Some(er_engine::ai::PanelContent::FileDetail);
        screen.tab.focused_finding_id = Some("f1");
        check(&screen, KeyCode::Enter, A::JumpToFocusedFinding);

        // A file with findings: j and k step through them.
        let mut reviewed = crate::golden::base_app();
        let tab = reviewed.tab_mut();
        let path = tab.files[tab.selected_file].path.clone();
        tab.ai.review = Some(
            serde_json::from_value(serde_json::json!({
                "version": 1, "diff_hash": "hash",
                "files": {path: {"risk": "high", "summary": "s", "findings": [
                    {"id": "f1", "severity": "high", "title": "t", "hunk_index": 0}
                ]}}
            }))
            .unwrap(),
        );
        let mut cache = crate::screen::ViewCache::default();
        let mut screen = crate::screen::from_app(&reviewed, &mut cache);
        screen.tab.panel_focus = true;
        screen.tab.panel = Some(er_engine::ai::PanelContent::FileDetail);
        check(&screen, KeyCode::Char('j'), A::NavigatePanelFinding(true));
        check(&screen, KeyCode::Up, A::NavigatePanelFinding(false));
    }

    #[test]
    fn the_key_table_pins_every_key_in_each_overlay() {
        let app = crate::golden::base_app();
        let mut cache = crate::screen::ViewCache::default();
        let mut screen = crate::screen::from_app(&app, &mut cache);
        let flow = er_engine::ai::review_flow::ReviewFlow::new("hash");
        let results = er_engine::review_flow_driver::ReviewFlowResults {
            er_dir: String::new(),
            run_id: String::new(),
            diff_hash: String::new(),
            findings: Vec::new(),
        };
        let selected_ids = HashSet::new();
        let buffer = String::new();

        screen.chrome.overlay = Some(OverlayData::ReviewFlowProgress {
            er_dir: "",
            flow: &flow,
            selected: 0,
            detail_scroll: 0,
        });
        let progress = "
            *PageDown => Overlay(ScrollDetail(8))
            *PageUp => Overlay(ScrollDetail(-8))
            *r => Overlay(RerunReviewFlow)";
        assert_table("review progress", &screen, &[LIST, progress]);

        screen.chrome.overlay = Some(OverlayData::ReviewFlowResults {
            results: &results,
            selected: 0,
            selected_ids: &selected_ids,
            detail_scroll: 0,
        });
        let flow_results = "
            *PageDown => Overlay(ScrollDetail(8))
            *PageUp => Overlay(ScrollDetail(-8))
            *Space *Enter => Overlay(ToggleFlowFinding)
            *a => Overlay(AcceptFlowFindings)";
        assert_table("review results", &screen, &[LIST, flow_results]);

        screen.chrome.overlay = Some(OverlayData::ExportPicker {
            include_comments: true,
            include_findings: true,
            include_questions: true,
            include_notes: true,
            selected: 0,
        });
        let export = "
            *Space => Overlay(ToggleExportKind)
            *Enter => Overlay(Export)";
        assert_table("export picker", &screen, &[LIST, export]);

        screen.chrome.overlay = Some(OverlayData::ConfigHub {
            tab: SettingsScope::General,
            items: &[],
            selected: 0,
            editing: None,
        });
        let settings = "
            *j *Down => Overlay(Next)
            *k *Up => Overlay(Prev)
            *Enter *Space *Right => Overlay(ActivateSetting)
            *Left => Overlay(ActivateSettingBack)
            *d => Overlay(DeleteSetting)
            *1 => Overlay(SwitchSettingsTab(General))
            *2 *[ *] => Overlay(SwitchSettingsTab(Terminal))
            *s *S *q *Esc => Overlay(CloseSettings)";
        assert_table("settings", &screen, &[settings]);
        screen.chrome.overlay = Some(OverlayData::ConfigHub {
            tab: SettingsScope::Terminal,
            items: &[],
            selected: 0,
            editing: None,
        });
        let back = KeyEvent::new(KeyCode::Char('['), KeyModifiers::NONE);
        assert_eq!(
            key_action(back, &screen),
            TuiAction::Overlay(OverlayAction::SwitchSettingsTab(SettingsScope::General))
        );

        screen.chrome.overlay = Some(OverlayData::ConfigHub {
            tab: SettingsScope::General,
            items: &[],
            selected: 0,
            editing: Some(ConfigEditState {
                item_index: 0,
                buffer: &buffer,
            }),
        });
        let mut editing = String::from(
            "
            *Enter => Overlay(EditSetting(Submit))
            *Esc => Overlay(EditSetting(Cancel))
            *Backspace => Overlay(EditSetting(Backspace))
            *Left => Overlay(EditSetting(Left))
            *Right => Overlay(EditSetting(Right))",
        );
        for c in ' '..='~' {
            let name = if c == ' ' {
                "Space".to_owned()
            } else {
                c.to_string()
            };
            editing.push_str(&format!("\n*{name} => Overlay(EditSetting(Insert({c:?})))"));
        }
        assert_table("settings edit", &screen, &[&editing]);

        screen.chrome.overlay = Some(OverlayData::ModalHub {
            kind: view::HubKind::Git,
            title: None,
            items: &[],
            selected: 0,
        });
        let rows = "
            *b => Overlay(SecondaryRow)
            *Enter => Overlay(SelectRow)
            *Backspace => Overlay(GoUp)";
        assert_table("hub", &screen, &[LIST, rows]);
    }

    /// The key table reads only the Screen and writes nothing: no step names
    /// the App or a tab's state, and none calls a port, the GitHub CLI, a
    /// sidecar clear or a file write. Test modules and comments are skipped.
    #[test]
    fn the_key_table_names_no_app_and_makes_no_write() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/src/input/keys");
        let banned = |word: &str, code: &str| match word {
            "App" | "TabState" => code
                .split(|c: char| !(c.is_alphanumeric() || c == '_'))
                .any(|token| token == word),
            _ => code.contains(word),
        };
        let mut found = Vec::new();
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            let source = std::fs::read_to_string(&path).unwrap();
            let mut code = String::new();
            for (i, part) in source.split("#[cfg(test)]").enumerate() {
                if i > 0 && part.trim_start().starts_with("mod ") {
                    break;
                }
                code.push_str(part);
            }
            for (number, line) in code.lines().enumerate() {
                let line = line.split("//").next().unwrap_or_default();
                for word in [
                    "er_engine::app",
                    "App",
                    "TabState",
                    "ports.",
                    "std::fs",
                    "remove_file",
                    "gh_",
                    "cleanup_",
                ] {
                    if banned(word, line) {
                        found.push(format!("{}:{}: {word}", path.display(), number + 1));
                    }
                }
            }
        }
        assert!(
            found.is_empty(),
            "the key table reaches past the Screen:\n{}",
            found.join("\n")
        );
    }
}
