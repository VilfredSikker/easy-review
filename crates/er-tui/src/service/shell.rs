//! The flag-on loop. It draws when something changed: a key, a reply, or an
//! event from an actor or the registry. No key waits on an actor: commands
//! go out with `post`, and their replies land here on the next tick.

use std::collections::HashMap;
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
use ratatui::widgets::Paragraph;
use ratatui::Terminal;

use super::startup::{self, After, Args, Pending, Start, StartTab};
use super::tui_view::{self, TuiView};
use crate::input::keys::{key_action, NormalAction, TuiAction};
use crate::screen::UNPORTED;
use crate::ui::highlight::Highlighter;

/// How long a frame waits for a key before the loop runs its duties.
const POLL_INTERVAL: Duration = Duration::from_millis(50);
/// How long a status-line message stays up.
const NOTIFICATION_DWELL: Duration = Duration::from_secs(3);

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
    redraw: bool,
    notified_at: Option<Instant>,
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
            redraw: true,
            notified_at: None,
            should_quit: false,
        };
        if !start.notices.is_empty() {
            shell.notify(start.notices.join("; "));
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
    pub fn view(&self) -> &TuiView {
        &self.view
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

    fn unported(&mut self) {
        self.notify(format!("That action is {UNPORTED}"));
    }

    /// The loop's duties, none of which waits: an event from an actor or the
    /// registry asks for a draw, landed replies are applied, and an old
    /// status-line message is cleared.
    pub fn tick(&mut self, now: Instant) {
        loop {
            match self.events.try_recv() {
                Ok(Event::CommandFailed { message }) => self.notify(message),
                Ok(_) => self.redraw = true,
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => break,
            }
        }
        self.drain_replies();
        if self
            .notified_at
            .is_some_and(|at| now.duration_since(at) >= NOTIFICATION_DWELL)
        {
            self.notified_at = None;
            self.view.notification = None;
            self.redraw = true;
        }
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
                Err(error) => self.notify(error.to_string()),
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

    /// Picks the key's action from this frame's screen and runs it.
    pub fn handle_key(&mut self, key: KeyEvent) {
        self.redraw = true;
        let Some((tab, workspace, Some(content))) = self.active() else {
            if key.code == KeyCode::Char('q') && key.modifiers == KeyModifiers::CONTROL {
                self.should_quit = true;
            }
            return;
        };
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
        let config = self.host.settings().view().config.clone();
        let (tab_names, active_tab) = self.tab_names();
        self.view.tab_mut(key, content);
        let state = self.view.tab(key)?;
        let frame = tui_view::Frame {
            tab_names,
            active_tab,
            content,
            config: &config,
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
            A::Quit => {
                self.should_quit = true;
                return;
            }
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

    /// A file the selection reached that is still a stub parses on the
    /// service; the frame shows it once the parse publishes.
    fn parse_if_stub(&mut self, key: TabKey, workspace: &str, content: &TabContent, index: usize) {
        let Some(file) = content.diff.files.get(index) else {
            return;
        };
        if !tui_view::is_stub(file) {
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
    pub fn draw(&mut self, f: &mut ratatui::Frame, highlighter: &mut Highlighter) {
        let Some((key, _, content)) = self.active() else {
            return f.render_widget(Paragraph::new("No tab is open"), f.area());
        };
        let Some(content) = content else {
            return f.render_widget(Paragraph::new("Loading…"), f.area());
        };
        self.with_screen(key, &content, |screen| {
            crate::ui::draw(f, screen, highlighter)
        });
    }
}
