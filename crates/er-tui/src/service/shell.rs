//! The flag-on loop. It draws when something changed: a key, a reply, or an
//! event from an actor or the registry. No key waits on an actor: commands
//! go out with `post`, and their replies land here on the next tick.

use std::collections::{HashMap, HashSet};
use std::io;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossterm::event::{self, KeyCode, KeyEvent, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use er_engine::TabKey;
use er_service::{Command, Event, Reply, ServiceHost, TabContent};
use ratatui::backend::{Backend, CrosstermBackend};
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use ratatui::Terminal;

use super::settings::{SettingsBridge, Side};
use super::startup::{self, After, Args, Pending, Start, StartTab};
use super::tui_view::{self, TuiView};
use crate::input::keys::{key_action, NormalAction, TuiAction};
use crate::screen::UNPORTED;
use crate::ui::highlight::Highlighter;

/// How long a frame waits for a key before the loop runs its duties.
const POLL_INTERVAL: Duration = Duration::from_millis(50);
/// How long a status-line message stays up.
const NOTIFICATION_DWELL: Duration = Duration::from_secs(3);
/// How often each loaded tab's sidecars are read again, for writes made
/// outside this process.
const RELOAD_INTERVAL: Duration = Duration::from_secs(1);
/// How long after the last key a tab's session is saved.
const SESSION_SAVE_DELAY: Duration = Duration::from_secs(2);

/// Opens the arguments' tabs on an in-process service and runs the loop
/// until the user quits.
pub fn run(args: &Args) -> anyhow::Result<()> {
    let ports = er_engine::ports::Ports::cli();
    let start = startup::plan(args, &*ports.git)?;
    let (tx, events) = std::sync::mpsc::channel();
    let host = ServiceHost::new(ports, er_engine::config::managed_config_path()).with_events(tx);
    let mut shell = Shell::start(host, events, &start)?;
    let mut highlighter = Highlighter::new();

    enable_raw_mode()?;
    execute!(io::stdout(), EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    let result = shell.run_loop(&mut terminal, &mut highlighter);
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    result
}

/// The loop's state: the host, the view state, and the commands in flight.
pub struct Shell {
    host: ServiceHost,
    events: Receiver<Event>,
    view: TuiView,
    /// How each tab was opened, so a tab first shown later loads the same way.
    starts: HashMap<TabKey, StartTab>,
    pending: Vec<Pending>,
    settings: SettingsBridge,
    /// The stubs already sent to parse, per tab and diff revision, so a
    /// parse that fails is not sent again every tick.
    parses_sent: HashSet<(TabKey, u64, String)>,
    /// Why a tab that has no content yet did not load.
    load_errors: HashMap<TabKey, String>,
    /// What the start said about its flags, shown until a tab has content.
    start_notice: Option<String>,
    redraw: bool,
    notified_at: Option<Instant>,
    reloaded_at: Option<Instant>,
    /// The tabs a key changed since their session was saved, and when the
    /// save is due.
    unsaved: HashSet<TabKey>,
    save_due: Option<Instant>,
    pub should_quit: bool,
}

impl Shell {
    /// Lists the start's tabs and loads the first. A flag with no service
    /// route yet goes on the status line.
    pub fn start(
        host: ServiceHost,
        events: Receiver<Event>,
        start: &Start,
    ) -> anyhow::Result<Self> {
        let (keys, pending) = startup::open(&host, start)?;
        let starts = keys.into_iter().zip(start.tabs.iter().cloned()).collect();
        let mut shell = Self {
            host,
            events,
            view: TuiView::default(),
            starts,
            pending,
            settings: SettingsBridge::default(),
            parses_sent: HashSet::new(),
            load_errors: HashMap::new(),
            start_notice: None,
            redraw: true,
            notified_at: None,
            reloaded_at: None,
            unsaved: HashSet::new(),
            save_due: None,
            should_quit: false,
        };
        if !start.notices.is_empty() {
            let notice = start.notices.join("; ");
            shell.start_notice = Some(notice.clone());
            shell.notify(notice);
        }
        if let Some(error) = shell.host.settings().view().load_error.clone() {
            let revision = shell.host.settings().view().revision;
            if shell.settings.admit_once(revision, &error) {
                shell.notify(error);
            }
        }
        Ok(shell)
    }

    fn run_loop<B: Backend<Error: Send + Sync + 'static>>(
        &mut self,
        terminal: &mut Terminal<B>,
        highlighter: &mut Highlighter,
    ) -> anyhow::Result<()> {
        loop {
            if self.take_redraw() {
                terminal.draw(|f| self.draw(f, highlighter))?;
            }
            if event::poll(POLL_INTERVAL)? {
                match event::read()? {
                    event::Event::Key(key) => self.handle_key(key),
                    event::Event::Resize(..) => self.redraw = true,
                    _ => {}
                }
            }
            self.tick(Instant::now());
            if self.should_quit {
                return Ok(());
            }
        }
    }

    #[cfg(test)]
    pub fn host(&self) -> &ServiceHost {
        &self.host
    }

    #[cfg(test)]
    pub fn settings(&self) -> &SettingsBridge {
        &self.settings
    }

    #[cfg(test)]
    pub fn send_settings(&mut self, command: er_service::SettingsCommand) {
        self.settings.send(&self.host.settings(), command);
    }

    #[cfg(test)]
    pub fn view(&self) -> &TuiView {
        &self.view
    }

    #[cfg(test)]
    pub fn view_mut(&mut self) -> &mut TuiView {
        &mut self.view
    }

    /// The active tab's content, once its load has published.
    #[cfg(test)]
    pub fn active_content(&self) -> Option<Arc<TabContent>> {
        self.active()?.2
    }

    /// Whether a draw is due; asking clears it.
    pub fn take_redraw(&mut self) -> bool {
        std::mem::take(&mut self.redraw)
    }

    fn notify(&mut self, message: String) {
        self.view.notification = Some(message);
        self.notified_at = Some(Instant::now());
        self.redraw = true;
    }

    /// Shows an actor's failure, once for its event and its reply together.
    fn failure(&mut self, side: Side, message: String) {
        if self.settings.admit_failure(side, &message) {
            self.notify(message);
        }
    }

    fn unported(&mut self) {
        self.notify(format!("That action is {UNPORTED}"));
    }

    /// The loop's duties, none of which waits: an event from an actor or the
    /// registry asks for a draw, landed replies are applied, a selected stub
    /// is sent to parse, the sidecars are read again each second, a session
    /// is saved once its keys stop, and an old status-line message is
    /// cleared. A message holds while the active tab loads, so its dwell runs
    /// on frames that show the tab.
    pub fn tick(&mut self, now: Instant) {
        loop {
            match self.events.try_recv() {
                Ok(Event::CommandFailed { message }) => self.failure(Side::Event, message),
                Ok(_) => self.redraw = true,
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => break,
            }
        }
        self.drain_replies();
        let landed = self.settings.drain();
        for message in landed.failures {
            self.failure(Side::Reply, message);
        }
        for message in landed.messages {
            self.settings.admit_message(&message);
            self.notify(message);
        }
        if self
            .reloaded_at
            .is_none_or(|at| now.duration_since(at) >= RELOAD_INTERVAL)
        {
            self.reloaded_at = Some(now);
            self.reload_sidecars();
        }
        if self.save_due.is_some_and(|due| now >= due) {
            self.save_sessions();
        }
        let loaded = self.parse_selected_stub();
        if !loaded && self.notified_at.is_some() {
            self.notified_at = Some(now);
        } else if self
            .notified_at
            .is_some_and(|at| now.duration_since(at) >= NOTIFICATION_DWELL)
        {
            self.notified_at = None;
            self.view.notification = None;
            self.redraw = true;
        }
    }

    /// Asks the actor of every loaded tab to read its sidecars again. The
    /// reply is dropped: a reload that read something publishes, and that
    /// event asks for the draw.
    fn reload_sidecars(&self) {
        let tabs = self.host.registry().tabs_view();
        for entry in &tabs.tabs {
            let workspace = entry.descriptor.workspace_key();
            if self.host.content(&workspace, entry.key).is_some() {
                drop(
                    self.host
                        .post(&workspace, Command::ReloadSidecars { key: entry.key }),
                );
            }
        }
    }

    /// Saves the session of every tab a key changed. A failed save says so
    /// on the status line.
    fn save_sessions(&mut self) {
        self.save_due = None;
        let tabs = self.host.registry().tabs_view();
        let mut failed = None;
        for key in std::mem::take(&mut self.unsaved) {
            let Some(entry) = tabs.tabs.iter().find(|entry| entry.key == key) else {
                continue;
            };
            let content = self.host.content(&entry.descriptor.workspace_key(), key);
            let (Some(content), Some(state)) = (content, self.view.tab(key)) else {
                continue;
            };
            if let Err(error) = super::session::save(&content, state) {
                failed = Some(format!("Session not saved: {error}"));
            }
        }
        if let Some(message) = failed {
            self.notify(message);
        }
    }

    /// Saves the sessions before the loop ends, the active tab's included.
    fn quit(&mut self) {
        if let Some((key, _, Some(_))) = self.active() {
            self.unsaved.insert(key);
        }
        self.save_sessions();
        self.should_quit = true;
    }

    fn drain_replies(&mut self) {
        let mut landed = Vec::new();
        self.pending
            .retain(|pending| match pending.replies.try_recv() {
                Ok(reply) => {
                    landed.push((pending.key, pending.workspace.clone(), pending.after, reply));
                    false
                }
                Err(TryRecvError::Empty) => true,
                Err(TryRecvError::Disconnected) => false,
            });
        for (key, workspace, after, reply) in landed {
            self.redraw = true;
            match reply {
                Ok(Reply::ContentLoaded { .. }) if after == After::FetchPrRefs => {
                    let replies = self.host.post(
                        &workspace,
                        Command::RefreshDiff {
                            key,
                            force: true,
                            adopt_head_oid: None,
                        },
                    );
                    self.pending.push(Pending {
                        key,
                        workspace,
                        replies,
                        after: After::Nothing,
                    });
                }
                Ok(_) => {}
                Err(error) if self.host.content(&workspace, key).is_none() => {
                    self.load_errors.insert(key, error.to_string());
                }
                Err(error) => self.failure(Side::Reply, error.to_string()),
            }
        }
    }

    /// The active tab's key, its workspace, and its content once loaded.
    fn active(&self) -> Option<(TabKey, String, Option<Arc<TabContent>>)> {
        let tabs = self.host.registry().tabs_view();
        let entry = tabs.active_entry()?;
        let workspace = entry.descriptor.workspace_key();
        let content = self.host.content(&workspace, entry.key);
        Some((entry.key, workspace, content))
    }

    /// Picks the key's action from this frame's screen and runs it. A tab
    /// with no content has no screen, so there only the tab keys and Ctrl+Q
    /// run, the same keys the table binds: a tab that is loading or failed
    /// never traps the user.
    pub fn handle_key(&mut self, key: KeyEvent) {
        self.redraw = true;
        let Some((tab, workspace, Some(content))) = self.active() else {
            match key.code {
                KeyCode::Char('q') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.quit();
                }
                KeyCode::Char(']') => self.step_tab(true),
                KeyCode::Char('[') => self.step_tab(false),
                _ => {}
            }
            return;
        };
        self.unsaved.insert(tab);
        self.save_due = Some(Instant::now() + SESSION_SAVE_DELAY);
        if let Some(action) = self.with_screen(tab, &content, |screen| key_action(key, screen)) {
            self.execute(action, tab, &workspace, &content);
        }
    }

    /// Runs `read` on this frame's screen for `key`'s tab.
    fn with_screen<R>(
        &mut self,
        key: TabKey,
        content: &TabContent,
        read: impl FnOnce(&crate::screen::Screen) -> R,
    ) -> Option<R> {
        let settings = self.host.settings().view();
        let ai_selection_label = self.settings.ai_label(&settings);
        let (tab_names, active_tab) = self.tab_names();
        self.view.tab_mut(key, content);
        let state = self.view.tab(key)?;
        let frame = tui_view::Frame {
            tab_names,
            active_tab,
            content,
            config: &settings.config,
            config_load_error: settings.load_banner.as_deref(),
            ai_selection_label,
        };
        Some(read(&tui_view::screen(&self.view, state, frame)))
    }

    fn execute(&mut self, action: TuiAction, key: TabKey, workspace: &str, content: &TabContent) {
        use NormalAction as A;
        let action = match action {
            TuiAction::Noop => return,
            TuiAction::Normal(action) => action,
            _ => return self.unported(),
        };
        let state = self.view.tab_mut(key, content);
        let file = content
            .diff
            .files
            .get(state.selected_file)
            .map(|file| &**file);
        let selected = match action {
            A::Quit => return self.quit(),
            A::NextFile => state.move_file(content, true),
            A::PrevFile => state.move_file(content, false),
            A::NextUnreviewedFile => match state.next_unreviewed(content) {
                tui_view::Jump::NoFiles => None,
                tui_view::Jump::AllReviewed => {
                    return self.notify("All visible files reviewed".into())
                }
                tui_view::Jump::To(index) => {
                    if let Some(file) = content.diff.files.get(index) {
                        self.notify(format!("Jumped to: {}", file.path));
                    }
                    Some(index)
                }
            },
            A::NextHunk | A::PrevHunk => {
                state.move_hunk(file, action == A::NextHunk);
                None
            }
            A::NextLine | A::PrevLine => {
                state.move_line(file, action == A::NextLine);
                None
            }
            A::ScrollDown(rows) if state.panel_focus && state.panel.is_some() => {
                state.panel_scroll = state.panel_scroll.saturating_add(rows).min(4096);
                None
            }
            A::ScrollUp(rows) if state.panel_focus && state.panel.is_some() => {
                state.panel_scroll = state.panel_scroll.saturating_sub(rows);
                None
            }
            A::ScrollDown(rows) => {
                state.scroll(file, i32::from(rows));
                None
            }
            A::ScrollUp(rows) => {
                state.scroll(file, -i32::from(rows));
                None
            }
            A::ScrollRight => {
                state.h_scroll = state.h_scroll.saturating_add(8);
                None
            }
            A::ScrollLeft => {
                state.h_scroll = state.h_scroll.saturating_sub(8);
                None
            }
            A::ScrollHome => {
                state.h_scroll = 0;
                None
            }
            A::NextTab => return self.step_tab(true),
            A::PrevTab => return self.step_tab(false),
            _ => return self.unported(),
        };
        if let Some(index) = selected {
            self.parse_if_stub(key, workspace, content, index);
        }
    }

    /// Sends the active tab's selected file to parse when it is a stub: a
    /// load, a reload or a tab switch can each leave one selected. Answers
    /// whether the active tab has content.
    fn parse_selected_stub(&mut self) -> bool {
        let Some((key, workspace, Some(content))) = self.active() else {
            return false;
        };
        // The view state is made here, as the load lands and before the parse,
        // so the start scrolls where the App's lazy load leaves it.
        let index = self.view.tab_mut(key, &content).selected_file;
        self.parse_if_stub(key, &workspace, &content, index);
        true
    }

    /// A file the selection reached that is still a stub parses on the
    /// service; the frame shows it once the parse publishes.
    fn parse_if_stub(&mut self, key: TabKey, workspace: &str, content: &TabContent, index: usize) {
        let Some(file) = content.diff.files.get(index) else {
            return;
        };
        let sent = (key, content.diff.revision, file.path.clone());
        if !tui_view::is_stub(file) || !self.parses_sent.insert(sent) {
            return;
        }
        let replies = self.host.post(
            workspace,
            Command::ParseFiles {
                key,
                paths: vec![file.path.clone()],
                expand: false,
            },
        );
        self.pending.push(Pending {
            key,
            workspace: workspace.to_string(),
            replies,
            after: After::Nothing,
        });
    }

    /// Selects the next or previous tab, wrapping, and loads it the first
    /// time it is shown.
    fn step_tab(&mut self, forward: bool) {
        let tabs = self.host.registry().tabs_view();
        let count = tabs.tabs.len();
        let Some(at) = tabs
            .active
            .and_then(|key| tabs.tabs.iter().position(|tab| tab.key == key))
        else {
            return;
        };
        let to = if forward {
            (at + 1) % count
        } else {
            (at + count - 1) % count
        };
        let Some(entry) = tabs.tabs.get(to) else {
            return;
        };
        let key = entry.key;
        if let Err(error) = self
            .host
            .registry()
            .send(er_service::AppCommand::SelectTab { key })
        {
            return self.notify(error.to_string());
        }
        let loaded = self
            .host
            .content(&entry.descriptor.workspace_key(), key)
            .is_some();
        let loading = self.pending.iter().any(|pending| pending.key == key);
        if let (false, false, Some(start_tab)) = (loaded, loading, self.starts.get(&key)) {
            self.load_errors.remove(&key);
            let pending = startup::load(&self.host, key, start_tab);
            self.pending.push(pending);
        }
    }

    /// The tab strip's names, and the active tab's place in it.
    fn tab_names(&self) -> (Vec<String>, usize) {
        let tabs = self.host.registry().tabs_view();
        let rows = er_service::tabs_resource(&tabs);
        let names = rows.tabs.into_iter().map(|row| row.label).collect();
        let active = tabs
            .active
            .and_then(|key| tabs.tabs.iter().position(|tab| tab.key == key))
            .unwrap_or(0);
        (names, active)
    }

    /// Draws the active tab from its published content and its view state.
    /// Until the content lands, the frame says why it is missing, then the
    /// start's notice and the status line, each once.
    pub fn draw(&mut self, f: &mut ratatui::Frame, highlighter: &mut Highlighter) {
        let Some((key, _, content)) = self.active() else {
            return f.render_widget(Paragraph::new("No tab is open"), f.area());
        };
        let Some(content) = content else {
            let status = match self.load_errors.get(&key) {
                Some(error) => {
                    format!("This tab did not load: {error}. [ and ] switch tabs, Ctrl+Q quits.")
                }
                None => "Loading…".to_string(),
            };
            let mut lines = vec![status];
            let notices = [self.start_notice.clone(), self.view.notification.clone()];
            for notice in notices.into_iter().flatten() {
                if !lines.iter().any(|line| line.contains(&notice)) {
                    lines.push(notice);
                }
            }
            let lines: Vec<Line> = lines.into_iter().map(Line::from).collect();
            return f.render_widget(Paragraph::new(lines), f.area());
        };
        self.with_screen(key, &content, |screen| {
            crate::ui::draw(f, screen, highlighter);
        });
    }
}
