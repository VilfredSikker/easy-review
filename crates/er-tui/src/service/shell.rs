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
use er_engine::github::BranchPrProbe;
use er_engine::tab_identity::DiffMode;
use er_engine::tabs_record::TabKind;
use er_engine::TabKey;
use er_service::{Command, Event, Reply, ReplyReceiver, ServiceHost, SettingsCommand, TabContent};
use ratatui::backend::{Backend, CrosstermBackend};
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use ratatui::Terminal;

use super::ai_picker;
use super::config_hub::{self, ConfigHub, Step};
use super::filter::{self, Done};
use super::github;
use super::layers::{self, Layer};
use super::pr_probe::Probe;
use super::reviewed::ReviewedCache;
use super::settings::{SettingsBridge, Side, Then};
use super::startup::{self, After, Args, Pending, Start, StartTab};
use super::threads::{self, Composed};
use super::tui_view::{self, Ask, Mode, Overlay, PrKnown, TuiView};
use super::watched::{Landed, Reader};
use super::worktree::{self, GitHub, GitPick, HubStep};
use crate::input::keys::{
    key_action, CommentAction, ConfirmKey, Edit, NormalAction, OverlayAction, TuiAction, Write,
};
use crate::screen::UNPORTED;
use crate::ui::highlight::Highlighter;

/// How long a frame waits for a key before the loop runs its duties.
const POLL_INTERVAL: Duration = Duration::from_millis(50);
/// How long a status-line message stays up.
const NOTIFICATION_DWELL: Duration = Duration::from_secs(3);
/// How long a message worth a longer read stays up, as the App's long notice.
const NOTIFICATION_DWELL_LONG: Duration = Duration::from_secs(5);
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
    let git = Arc::clone(&ports.git);
    let host = ServiceHost::new(ports, er_engine::config::managed_config_path()).with_events(tx);
    let mut shell = Shell::start(host, events, &start, git)?;
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
    dwell: Duration,
    /// The sidecar reloads in flight, whose failures pair with their events.
    reloads: Vec<ReplyReceiver>,
    reloaded_at: Option<Instant>,
    /// The tabs a key changed since their session was saved, and when the
    /// save is due.
    unsaved: HashSet<TabKey>,
    save_due: Option<Instant>,
    reviewed: ReviewedCache,
    /// The watched files' reader, a worker thread that only reads.
    watched: Reader,
    /// The start's PR probe, the other one, until it answers; then its
    /// answer, until the tab it probed has content.
    probe: Option<Probe>,
    probed: Option<(TabKey, BranchPrProbe)>,
    pub should_quit: bool,
}

impl Shell {
    /// Lists the start's tabs and loads the first. A flag with no service
    /// route yet goes on the status line. `git` lists the watched files.
    pub fn start(
        host: ServiceHost,
        events: Receiver<Event>,
        start: &Start,
        git: Arc<dyn er_git::Git>,
    ) -> anyhow::Result<Self> {
        let (keys, pending) = startup::open(&host, start)?;
        let mut view = TuiView::default();
        view.settings = Some(host.settings().view());
        view.start_filter = keys.first().copied().zip(start.filter.clone());
        // As the App's start: the first tab is probed unless a tab is a PR.
        let first = keys.first().copied().zip(start.tabs.first());
        let probe = first
            .filter(|_| start.tabs.iter().all(|t| t.tab.kind == TabKind::Working))
            .map(|(key, first)| {
                let github = Arc::clone(&host.ports().github);
                Probe::spawn(github, key, first.tab.repo_root.clone())
            });
        let starts = keys.into_iter().zip(start.tabs.iter().cloned()).collect();
        let mut shell = Self {
            host,
            events,
            view,
            starts,
            pending,
            settings: SettingsBridge::default(),
            parses_sent: HashSet::new(),
            load_errors: HashMap::new(),
            start_notice: None,
            redraw: true,
            notified_at: None,
            dwell: NOTIFICATION_DWELL,
            reloads: Vec::new(),
            reloaded_at: None,
            unsaved: HashSet::new(),
            save_due: None,
            reviewed: ReviewedCache::default(),
            watched: Reader::spawn(git),
            probe,
            probed: None,
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
                shell.dwell = NOTIFICATION_DWELL_LONG;
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
        self.settings
            .send(&self.host.settings(), command, Then::Nothing);
    }

    #[cfg(test)]
    pub fn view(&self) -> &TuiView {
        &self.view
    }

    #[cfg(test)]
    pub fn view_mut(&mut self) -> &mut TuiView {
        &mut self.view
    }

    #[cfg(test)]
    pub fn reviewed_builds(&self) -> usize {
        self.reviewed.builds
    }

    #[cfg(test)]
    pub fn in_flight(&self) -> usize {
        self.pending.len()
    }

    /// A write on `key` whose worker died before it answered.
    #[cfg(test)]
    pub fn lose_write(&mut self, key: TabKey, workspace: String, after: After) {
        let (_, replies) = std::sync::mpsc::channel();
        self.pending.push(Pending {
            key,
            workspace,
            replies,
            after,
        });
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
        self.dwell = NOTIFICATION_DWELL;
        self.redraw = true;
    }

    /// Runs a failed reply through the same path a landed one takes.
    #[cfg(test)]
    pub fn land_failed_reply(&mut self, message: String) {
        self.failure(Side::Reply, message);
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
        self.sync_probe();
        self.sync_watched(now);
        self.drain_reloads();
        let landed = self.settings.drain();
        for message in landed.failures {
            self.failure(Side::Reply, message);
        }
        let view = self.host.settings().view();
        self.view.settings = Some(Arc::clone(&view));
        let picked = landed.picked.then(|| {
            let label = self.settings.ai_label(&view);
            format!("AI target: {label}")
        });
        let still_failed = view.load_error.clone().filter(|_| landed.reloaded);
        if let Some(overlay) = self.view.overlay.as_mut() {
            overlay.sync(&view, &self.settings.ai_label(&view));
        }
        for message in landed.messages.into_iter().chain(picked) {
            self.settings.admit_message(&message);
            self.notify(message);
        }
        // A reload that still fails says why for longer, as the App's does.
        if let Some(error) = still_failed {
            self.settings.admit_message(&error);
            self.notify(error);
            self.dwell = NOTIFICATION_DWELL_LONG;
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
            .is_some_and(|at| now.duration_since(at) >= self.dwell)
        {
            self.notified_at = None;
            self.view.notification = None;
            self.redraw = true;
        }
    }

    /// Asks the actor of every loaded tab to read its sidecars again. A
    /// reload that read something publishes, and that event asks for the
    /// draw; only a failed reply is looked at.
    fn reload_sidecars(&mut self) {
        let tabs = self.host.registry().tabs_view();
        for entry in &tabs.tabs {
            let workspace = entry.descriptor.workspace_key();
            if self.host.content(&workspace, entry.key).is_some() {
                let replies = self
                    .host
                    .post(&workspace, Command::ReloadSidecars { key: entry.key });
                self.reloads.push(replies);
            }
        }
    }

    fn drain_reloads(&mut self) {
        let mut failures = Vec::new();
        self.reloads.retain(|replies| match replies.try_recv() {
            Ok(reply) => {
                if let Err(error) = reply {
                    failures.push(error.to_string());
                }
                false
            }
            Err(TryRecvError::Empty) => true,
            Err(TryRecvError::Disconnected) => false,
        });
        for message in failures {
            self.failure(Side::Reply, message);
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
                // A thread write whose worker died answers as a refusal, so
                // its composer takes edits again and a delete says so.
                Err(TryRecvError::Disconnected)
                    if matches!(
                        pending.after,
                        After::Composed | After::Deleted | After::Posted
                    ) =>
                {
                    let lost = er_service::WorkspaceError::Failed(
                        "The write stopped before it answered".into(),
                    );
                    landed.push((
                        pending.key,
                        pending.workspace.clone(),
                        pending.after,
                        Err(lost),
                    ));
                    false
                }
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
                Ok(Reply::ReviewedChanged { .. }) => {
                    if let After::Reviewed {
                        advance_from: Some(position),
                    } = after
                    {
                        self.advance(key, &workspace, position);
                    }
                }
                Ok(Reply::WorktreeWritten { .. }) if after == After::SnapshotSaved => {
                    if let Some(state) = self.view.tab_mut_if_open(key) {
                        state.watched.reread();
                    }
                }
                Ok(Reply::AnnotationWritten { .. } | Reply::ThreadChanged { .. })
                    if after == After::Composed =>
                {
                    self.composed(key);
                }
                Ok(Reply::AnnotationWritten { .. } | Reply::CommentPosted { .. })
                    if after == After::Posted =>
                {
                    self.posted(key);
                }
                Err(error) if after == After::Posted => {
                    let message = error.to_string();
                    if let Some(state) = self.view.tab_mut_if_open(key) {
                        github::refused(state, &message);
                    }
                    self.failure(Side::Reply, message);
                }
                Ok(Reply::CommentsSynced {
                    github_count,
                    local_count,
                }) if after == After::Synced => {
                    self.view.learn_pr(key, PrKnown::Present);
                    self.notify(github::synced(github_count, local_count));
                }
                Err(error) if after == After::Synced => {
                    let message = error.to_string();
                    if let Some(known) = github::known_after_refusal(&message) {
                        self.view.learn_pr(key, known);
                    }
                    self.failure(Side::Reply, message);
                }
                Ok(Reply::ReviewSubmitted { .. }) if after == After::Approved => {
                    self.notify("PR approved".into());
                }
                Err(error) if after == After::Approved => self.review_failure(error),
                Ok(Reply::ThreadChanged { .. }) if after == After::Deleted => {
                    self.notify("Comment deleted".into());
                }
                Err(error) if after == After::Composed => {
                    if let Some(state) = self.view.tab_mut_if_open(key) {
                        state.sent = None;
                    }
                    self.failure(Side::Reply, error.to_string());
                }
                Ok(Reply::WorktreeWritten { notice, .. }) => {
                    if after == After::Commit {
                        self.committed(key);
                    }
                    if after == After::Watched {
                        self.watched.rescan = true;
                    }
                    self.notify(notice);
                }
                Ok(_) => {}
                Err(error) if self.host.content(&workspace, key).is_none() => {
                    self.settings.absorb_reply(&error.to_string());
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
        self.view.settings = Some(Arc::clone(&settings));
        let ai_selection_label = self.settings.ai_label(&settings);
        if let Some(overlay) = self.view.overlay.as_mut() {
            overlay.sync(&settings, &ai_selection_label);
        }
        let (tab_names, active_tab, open) = self.tab_names();
        self.reviewed.keep_open(&open);
        let reviewed_count = self.reviewed.count(key, content);
        let state = self.view.tab_mut(key, content);
        layers::follow(state, content);
        threads::scroll_if_parsed(state, content);
        let state = self.view.tab(key)?;
        let frame = tui_view::Frame {
            tab_names,
            active_tab,
            content,
            config: &settings.config,
            config_load_error: settings.load_banner.as_deref(),
            ai_selection_label,
            reviewed_count,
        };
        Some(read(&tui_view::screen(&self.view, state, frame)))
    }

    fn execute(&mut self, action: TuiAction, key: TabKey, workspace: &str, content: &TabContent) {
        use NormalAction as A;
        let action = match action {
            TuiAction::Noop => return,
            TuiAction::Normal(action) => action,
            TuiAction::Overlay(action) if matches!(self.view.overlay, Some(Overlay::GitHub(_))) => {
                return self.git_hub_key(action, key, workspace, content)
            }
            TuiAction::Overlay(action)
                if matches!(self.view.overlay, Some(Overlay::FilterHistory { .. })) =>
            {
                let shown = self.watched_shown(key, content);
                let done = filter::history(&mut self.view, key, content, shown, action);
                return self.filtered(key, workspace, content, done);
            }
            TuiAction::Overlay(action) => return self.overlay_key(action, key, content),
            TuiAction::Search(edit) => {
                let shown = self.watched_shown(key, content);
                let done = filter::search(&mut self.view, key, content, shown, edit);
                return self.filtered(key, workspace, content, done);
            }
            TuiAction::Filter(edit) => {
                let shown = self.watched_shown(key, content);
                let done = filter::filter(&mut self.view, key, content, shown, edit);
                return self.filtered(key, workspace, content, done);
            }
            TuiAction::Commit(edit) => return self.commit_key(edit, key, workspace, content),
            TuiAction::Comment(action) => return self.comment_key(action, key, workspace, content),
            TuiAction::Write(Write::Push) => {
                self.view.input_mode = Mode::Normal;
                return self.post(key, workspace, Command::Push { key }, After::Nothing);
            }
            TuiAction::Write(Write::DeleteWatchedFile) => {
                self.view.input_mode = Mode::Normal;
                let state = self.view.tab_mut(key, content);
                if let Some(path) = state.watched.deleting.take() {
                    let command = Command::DeleteWatchedFile { key, path };
                    self.post(key, workspace, command, After::Watched);
                }
                return;
            }
            TuiAction::Write(Write::ApprovePr) => {
                self.view.input_mode = Mode::Normal;
                if !self.writing(key, After::Approved) {
                    let command = github::approve(content);
                    self.post(key, workspace, command, After::Approved);
                }
                return;
            }
            TuiAction::Write(Write::DeleteComment) => {
                self.view.input_mode = Mode::Normal;
                let deleting = self.writing(key, After::Deleted);
                let state = self.view.tab_mut(key, content);
                if let Some(id) = state.deleting_comment.take().filter(|_| !deleting) {
                    let command = github::delete(content, id);
                    self.post(key, workspace, command, After::Deleted);
                }
                return;
            }
            TuiAction::Confirm(ConfirmKey::Cancel) => {
                self.view.input_mode = Mode::Normal;
                return;
            }
            _ => return self.unported(),
        };
        let shown = self.watched_shown(key, content);
        let done = filter::normal(&mut self.view, key, content, shown, action.clone());
        if let Some(done) = done {
            return self.filtered(key, workspace, content, done);
        }
        let config = self.host.settings().view().config.watched.clone();
        let state = self.view.tab_mut(key, content);
        let file = content
            .diff
            .files
            .get(state.selected_file)
            .filter(|_| state.watched.selected.is_none())
            .map(|file| &**file);
        let shown = state.watched.is_shown(&config);
        let selected = match action {
            A::Quit => return self.quit(),
            A::NextFile => state.move_file(content, shown, true),
            A::PrevFile => state.move_file(content, shown, false),
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
            A::OpenConfigHub => return self.open_config_hub(),
            A::OpenAiHub => {
                let view = self.host.settings().view();
                let label = self.settings.ai_label(&view);
                let has_ai = content.ai.state.has_data();
                return self.run_step(ai_picker::open_ai_hub(&view, &label, has_ai));
            }
            A::ToggleStaged => return self.toggle_staged(key, workspace, content),
            A::ToggleReviewed => return self.toggle_reviewed(key, workspace, content),
            A::ToggleUnreviewedOnly => {
                state.show_unreviewed_only = !state.show_unreviewed_only;
                let showing = state.show_unreviewed_only;
                let selected = state.snap(content, shown);
                self.notify(if showing {
                    "Showing unreviewed only".into()
                } else {
                    "Showing all files".into()
                });
                selected
            }
            A::ToggleCompacted => {
                if let Some(file) = file.filter(|file| file.compacted) {
                    let paths = vec![file.path.clone()];
                    let command = Command::ParseFiles {
                        key,
                        paths,
                        expand: true,
                    };
                    self.post(key, workspace, command, After::Nothing);
                }
                return;
            }
            A::StartCommit => {
                state.commit_input.clear();
                self.view.input_mode = Mode::Commit;
                return;
            }
            A::AskPush => {
                if content.tab.mode == DiffMode::Staged && !content.tab.identity().is_remote() {
                    self.view.input_mode = Mode::Confirm(Ask::Push);
                }
                return;
            }
            A::OpenGitHub => {
                self.view.overlay = Some(Overlay::GitHub(GitHub::open(content)));
                return;
            }
            A::ToggleWatchedFiles => {
                let (message, scan) = state.watched.toggle(&config);
                self.watched.rescan |= scan;
                return self.notify(message.to_string());
            }
            A::UpdateWatchedSnapshot if config.diff_mode != "snapshot" => {
                return self.notify("Snapshot mode not enabled (diff_mode = \"content\")".into())
            }
            A::UpdateWatchedSnapshot => {
                if let Some(path) = state.watched.selected_file().map(|f| f.path.clone()) {
                    let command = Command::SaveWatchedSnapshot { key, path };
                    self.post(key, workspace, command, After::Watched);
                }
                return;
            }
            A::AskDeleteWatchedFile => {
                if let Some(file) = state.watched.selected_file() {
                    state.watched.deleting = Some(file.path.clone());
                    self.view.input_mode = Mode::Confirm(Ask::DeleteWatchedFile);
                }
                return;
            }
            A::NextTab => return self.step_tab(true),
            A::PrevTab => return self.step_tab(false),
            A::StartQuestion => {
                if threads::start(state, content, er_engine::ai::CommentType::Question) {
                    self.view.input_mode = Mode::Comment;
                }
                return;
            }
            A::StartGithubComment => {
                let known = self.view.pr_known(key, content);
                let state = self.view.tab_mut(key, content);
                let started = github::start_line(state, content, known);
                return self.compose(started);
            }
            A::SyncGithubComments => return self.pull_comments(key, workspace, content),
            A::ResumeComment => {
                self.view.input_mode = Mode::Comment;
                return;
            }
            A::Reply | A::Edit | A::AskDeleteComment => {
                let focus = match action {
                    A::Reply => threads::start_reply(state, content),
                    A::Edit => threads::start_edit(state, content),
                    _ => threads::ask_delete(state, content),
                };
                return self.focus(focus);
            }
            A::NextHint | A::PrevHint => threads::jump_hint(state, content, action == A::NextHint),
            A::TogglePanel | A::TogglePanelReverse => {
                layers::cycle_panel(state, content, action == A::TogglePanel);
                return;
            }
            A::TogglePanelFocus => {
                state.panel_focus = state.panel.is_some() && !state.panel_focus;
                return;
            }
            A::PanelLineDown => {
                state.panel_scroll = state.panel_scroll.saturating_add(1).min(4096);
                return;
            }
            A::PanelLineUp => {
                state.panel_scroll = state.panel_scroll.saturating_sub(1);
                return;
            }
            A::UnfocusPanel => {
                state.panel_focus = false;
                return;
            }
            A::ToggleComments
            | A::ToggleQuestions
            | A::ToggleHideResolved
            | A::ToggleAiFindings => {
                let layer = match action {
                    A::ToggleComments => Layer::Comments,
                    A::ToggleQuestions => Layer::Questions,
                    A::ToggleHideResolved => Layer::HideResolved,
                    _ => Layer::AiFindings,
                };
                let message = layers::toggle(state, layer);
                return self.notify(message.into());
            }
            A::CycleMinTrust => {
                let message = layers::cycle_gate(state, content);
                return self.notify(message);
            }
            _ => return self.unported(),
        };
        if let Some(index) = selected {
            self.parse_if_stub(key, workspace, content, index);
        }
    }

    /// Whether `key`'s tab shows its watched files.
    fn watched_shown(&mut self, key: TabKey, content: &TabContent) -> bool {
        let config = self.host.settings().view().config.watched.clone();
        self.view.tab_mut(key, content).watched.is_shown(&config)
    }

    /// Shows a filter or search key's message, and parses the file its snap
    /// selected when that is a stub.
    fn filtered(&mut self, key: TabKey, workspace: &str, content: &TabContent, done: Done) {
        if let Some(notice) = done.notice {
            self.notify(notice);
        }
        if let Some(index) = done.selected {
            self.parse_if_stub(key, workspace, content, index);
        }
    }

    /// Sends a command for `key`'s tab; its reply lands on a later tick.
    fn post(&mut self, key: TabKey, workspace: &str, command: Command, after: After) {
        let replies = self.host.post(workspace, command);
        self.pending.push(Pending {
            key,
            workspace: workspace.to_string(),
            replies,
            after,
        });
    }

    /// Runs a composer key on the tab's draft, and sends the write Submit
    /// builds; the draft keeps its text until the reply lands.
    fn comment_key(
        &mut self,
        action: CommentAction,
        key: TabKey,
        workspace: &str,
        content: &TabContent,
    ) {
        let mut mode = self.view.input_mode;
        let state = self.view.tab_mut(key, content);
        let composed = threads::key(state, &mut mode, action, content);
        self.view.input_mode = mode;
        match composed {
            Composed::Stay => {}
            Composed::Send(command) => self.post(key, workspace, *command, After::Composed),
            Composed::Posted(command) => self.post(key, workspace, *command, After::Posted),
            Composed::Unported => self.unported(),
        }
    }

    /// Runs what a thread key on the focused comment asks.
    fn focus(&mut self, focus: threads::Focus) {
        match focus {
            threads::Focus::Nothing => {}
            threads::Focus::Opened => self.view.input_mode = Mode::Comment,
            threads::Focus::AskDelete => {
                self.view.input_mode = Mode::Confirm(Ask::DeleteComment);
            }
            threads::Focus::Unported => self.unported(),
            threads::Focus::Refused(message) => self.notify(message.into()),
        }
    }

    /// A write landed on `key`'s tab: its draft clears, and the composer
    /// closes if that tab is still the one composing.
    fn composed(&mut self, key: TabKey) {
        let active = self.active().map(|(active, ..)| active);
        let Some(message) = self.view.tab_mut_if_open(key).and_then(threads::landed) else {
            return;
        };
        if active == Some(key) && self.view.input_mode == Mode::Comment {
            self.view.input_mode = Mode::Normal;
        }
        self.notify(message);
    }

    /// A GitHub post landed on `key`'s tab: its draft clears, and the
    /// composer closes if that tab is still the one composing.
    fn posted(&mut self, key: TabKey) {
        let active = self.active().map(|(active, ..)| active);
        let Some(message) = self.view.tab_mut_if_open(key).and_then(github::landed) else {
            return;
        };
        if active == Some(key) && self.view.input_mode == Mode::Comment {
            self.view.input_mode = Mode::Normal;
        }
        self.notify(message.into());
    }

    /// Opens the composer a GitHub start allowed, or says why it refused.
    fn compose(&mut self, started: Result<bool, String>) {
        match started {
            Ok(true) => self.view.input_mode = Mode::Comment,
            Ok(false) => {}
            Err(message) => self.notify(message),
        }
    }

    /// Whether `key`'s tab has a write of this kind in flight. The service
    /// makes every write it is sent, so a second one waits for the first.
    fn writing(&self, key: TabKey, after: After) -> bool {
        self.pending
            .iter()
            .any(|pending| pending.key == key && pending.after == after)
    }

    fn pull_comments(&mut self, key: TabKey, workspace: &str, content: &TabContent) {
        if !self.writing(key, After::Synced) {
            self.post(key, workspace, github::sync(content), After::Synced);
        }
    }

    /// A refused review write, in the review wording the desktop shows. The
    /// actor's event carries the plain text, so the reply takes its twin.
    fn review_failure(&mut self, error: er_service::WorkspaceError) {
        let plain = error.to_string();
        let worded = er_service::review_write_message(error);
        if worded == plain {
            return self.failure(Side::Reply, plain);
        }
        self.settings.absorb_reply(&plain);
        self.settings.admit_message(&worded);
        self.notify(worded);
    }

    /// The start's probe landed: once its tab has content, what it found
    /// goes in the view, and a PR on another base says how to open it, as
    /// the App's hint does.
    fn sync_probe(&mut self) {
        if let Some(landed) = self.probe.as_ref().and_then(Probe::try_landed) {
            self.probe = None;
            self.probed = Some(landed);
        }
        let Some(key) = self.probed.as_ref().map(|(key, _)| *key) else {
            return;
        };
        let workspace = self.workspace_of(key);
        let Some(content) = workspace.and_then(|workspace| self.host.content(&workspace, key))
        else {
            return;
        };
        let Some((_, probe)) = self.probed.take() else {
            return;
        };
        match probe {
            BranchPrProbe::Present { number, base } => {
                self.view.learn_pr(key, PrKnown::Present);
                if base != content.tab.base_branch {
                    self.notify(format!(
                        "PR #{number} targets {base} — run: er --pr {number}"
                    ));
                }
            }
            BranchPrProbe::Absent => self.view.learn_pr(key, PrKnown::Absent),
            BranchPrProbe::Unknown => {}
        }
    }

    fn toggle_staged(&mut self, key: TabKey, workspace: &str, content: &TabContent) {
        let selected = self.view.tab_mut(key, content).selected_file;
        match worktree::stage_command(key, content, selected) {
            Ok(Some(command)) => self.post(key, workspace, command, After::Nothing),
            Ok(None) => {}
            Err(message) => self.notify(message),
        }
    }

    /// Sends the selected file's reviewed toggle with the context of the
    /// content this key was pressed on, and says what it did as the App does.
    fn toggle_reviewed(&mut self, key: TabKey, workspace: &str, content: &TabContent) {
        let state = self.view.tab_mut(key, content);
        let Some(file) = content.diff.files.get(state.selected_file) else {
            return;
        };
        let marking = !content.reviewed.marks.contains_key(&file.path);
        let advance_from =
            (marking && state.show_unreviewed_only).then(|| state.visible_position(content));
        let command = Command::ToggleReviewed {
            context: content.context.clone(),
            path: file.path.clone(),
        };
        self.post(key, workspace, command, After::Reviewed { advance_from });
        let verb = if marking { "Reviewed" } else { "Unreviewed" };
        self.notify(format!("{verb}: {}", file.path));
    }

    /// A mark landed under unreviewed-only: the selection moves off the file
    /// the list now hides.
    fn advance(&mut self, key: TabKey, workspace: &str, position: usize) {
        let Some(content) = self.host.content(workspace, key) else {
            return;
        };
        let Some(state) = self.view.tab_mut_if_open(key) else {
            return;
        };
        if let Some(index) = state.advance_to(&content, position) {
            self.parse_if_stub(key, workspace, &content, index);
        }
    }

    fn commit_key(&mut self, edit: Edit, key: TabKey, workspace: &str, content: &TabContent) {
        let sending = self
            .pending
            .iter()
            .any(|pending| pending.key == key && pending.after == After::Commit);
        let mut mode = self.view.input_mode;
        let state = self.view.tab_mut(key, content);
        let command = worktree::commit_key(key, state, &mut mode, edit, sending);
        self.view.input_mode = mode;
        if let Some(command) = command {
            self.post(key, workspace, command, After::Commit);
        }
    }

    /// A commit landed: its input closes, as the App's does.
    fn committed(&mut self, key: TabKey) {
        if let Some(state) = self.view.tab_mut_if_open(key) {
            state.commit_input.clear();
        }
        if self.view.input_mode == Mode::Commit {
            self.view.input_mode = Mode::Normal;
        }
    }

    fn git_hub_key(
        &mut self,
        action: OverlayAction,
        key: TabKey,
        workspace: &str,
        content: &TabContent,
    ) {
        let Some(Overlay::GitHub(hub)) = self.view.overlay.as_mut() else {
            return;
        };
        let step = hub.act(action);
        if !matches!(step, HubStep::Stay) {
            self.view.overlay = None;
        }
        match step {
            HubStep::Stay | HubStep::Close => {}
            HubStep::Notify(message) => self.notify(message),
            HubStep::Unported => self.unported(),
            HubStep::Run(GitPick::Push) => {
                if content.tab.mode == DiffMode::Staged {
                    self.view.input_mode = Mode::Confirm(Ask::Push);
                }
            }
            HubStep::Run(GitPick::StageFile) => self.toggle_staged(key, workspace, content),
            HubStep::Run(GitPick::StageAll) => {
                self.post(key, workspace, Command::StageAll { key }, After::Nothing);
            }
            HubStep::Run(GitPick::PullComments) => self.pull_comments(key, workspace, content),
            HubStep::Run(GitPick::CommentOnPr) => {
                let known = self.view.pr_known(key, content);
                let state = self.view.tab_mut(key, content);
                let started = github::start_general(state, known);
                self.compose(started);
            }
            HubStep::Run(GitPick::ApprovePr) => {
                self.view.input_mode = Mode::Confirm(Ask::ApprovePr);
            }
            HubStep::Run(GitPick::Unported) => self.unported(),
        }
    }

    /// The active tab's watched files: landed reads are applied, the list is
    /// read when due, and the selected file's snapshot diff when the
    /// selection reached it or a write or rescan asked. A file with no
    /// snapshot yet gets one through the service.
    fn sync_watched(&mut self, now: Instant) {
        while let Some(landed) = self.watched.try_landed() {
            self.redraw = true;
            let key = match &landed {
                Landed::Scanned { key, .. } | Landed::Diff { key, .. } => *key,
            };
            let Some(state) = self.view.tab_mut_if_open(key) else {
                continue;
            };
            let save = match landed {
                Landed::Scanned {
                    files, not_ignored, ..
                } => {
                    state.watched.scanned(files, not_ignored);
                    None
                }
                Landed::Diff { path, diff, .. } => state.watched.read(path, diff),
            };
            let workspace = self.workspace_of(key);
            if let (Some(path), Some(workspace)) = (save, workspace) {
                let command = Command::SaveWatchedSnapshot { key, path };
                self.post(key, &workspace, command, After::SnapshotSaved);
            }
        }
        let Some((key, _, Some(content))) = self.active() else {
            return;
        };
        let tab = &content.tab;
        if tab.identity().is_remote() {
            return;
        }
        let settings = self.host.settings().view();
        let config = &settings.config.watched;
        let due = self.watched.scan_due(key, settings.revision, now);
        let state = self.view.tab_mut(key, &content);
        if due && state.watched.is_shown(config) && !config.paths.is_empty() {
            self.watched.scan(key, &tab.repo_root, config);
        } else if due {
            state.watched.clear();
        }
        // Hidden opens on its first watched file, as the App's switch does.
        let first = state.watched.selected.is_none() && !state.watched.files.is_empty();
        if tab.mode == DiffMode::Hidden && first {
            state.watched.selected = Some(0);
            state.diff_scroll = 0;
            state.h_scroll = 0;
        }
        if config.diff_mode == "snapshot" {
            if let Some(path) = state.watched.wants_read() {
                let er_dir = &content.context.er_dir;
                self.watched.diff(key, &tab.repo_root, path, er_dir);
            }
        }
    }

    fn workspace_of(&self, key: TabKey) -> Option<String> {
        let tabs = self.host.registry().tabs_view();
        let entry = tabs.tabs.iter().find(|entry| entry.key == key)?;
        Some(entry.descriptor.workspace_key())
    }

    /// Opens the config hub on the settings view, and asks the actor to
    /// adopt a config file repaired since a failed load.
    fn open_config_hub(&mut self) {
        let settings = self.host.settings();
        self.settings.send(
            &settings,
            SettingsCommand::ReloadIfFailed,
            Then::ReportReload,
        );
        let hub = ConfigHub::open(&settings.view());
        self.view.overlay = Some(Overlay::ConfigHub(hub));
    }

    /// Runs a key on the open overlay, against the latest settings view.
    fn overlay_key(&mut self, action: OverlayAction, key: TabKey, content: &TabContent) {
        let view = self.host.settings().view();
        let label = self.settings.ai_label(&view);
        let step = match self.view.overlay.as_mut() {
            Some(Overlay::ConfigHub(hub)) => {
                hub.sync(&view);
                config_hub::act(hub, action, &view)
            }
            Some(Overlay::Picker(picker)) => {
                picker.sync(&view, &label);
                ai_picker::act(picker, action, &view, &label)
            }
            _ => Step::Unported,
        };
        if let Step::Layer(row) = step {
            self.view.overlay = None;
            let state = self.view.tab_mut(key, content);
            let message = layers::hub_row(state, row);
            return self.notify(message);
        }
        self.run_step(step);
    }

    fn run_step(&mut self, step: Step) {
        let settings = self.host.settings();
        let (close, send) = match step {
            Step::Stay => (false, None),
            Step::Close | Step::Layer(_) => (true, None),
            Step::Send(command) => (false, Some((command, Then::Nothing))),
            Step::Leave(command) => (true, Some((command, Then::Nothing))),
            Step::Pick(command) => (true, Some((command, Then::AnnouncePick))),
            Step::Open(overlay, also) => {
                self.view.overlay = Some(*overlay);
                (false, also.map(|command| (command, Then::Nothing)))
            }
            Step::Notify(message) => {
                self.view.overlay = None;
                return self.notify(message);
            }
            Step::Unported => {
                self.view.overlay = None;
                return self.unported();
            }
        };
        if close {
            self.view.overlay = None;
        }
        if let Some((command, then)) = send {
            self.settings.send(&settings, command, then);
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

    /// The tab strip's names, the active tab's place in it, and the keys.
    fn tab_names(&self) -> (Vec<String>, usize, Vec<TabKey>) {
        let tabs = self.host.registry().tabs_view();
        let rows = er_service::tabs_resource(&tabs);
        let names = rows.tabs.into_iter().map(|row| row.label).collect();
        let active = tabs
            .active
            .and_then(|key| tabs.tabs.iter().position(|tab| tab.key == key))
            .unwrap_or(0);
        let keys = tabs.tabs.iter().map(|tab| tab.key).collect();
        (names, active, keys)
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
