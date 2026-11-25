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
use er_engine::review_flow_driver::ReviewFlowResults;
use er_engine::tab_identity::DiffMode;
use er_engine::tabs_record::TabKind;
use er_engine::TabKey;
use er_service::{
    Command, Event, Reply, ReplyReceiver, RunReceiver, RunRequest, ServiceHost, SettingsCommand,
    TabContent, WorkspaceError,
};
use ratatui::backend::{Backend, CrosstermBackend};
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use ratatui::Terminal;

use super::agent_log::{self, Jobs};
use super::ai_picker;
use super::ai_runs::{self, Next, Rerun};
use super::chrome::{self, HelpHub};
use super::clears::{self, Clear};
use super::config_hub::{self, ConfigHub, Step};
use super::context;
use super::copy::{self, CopyHub, CopyStep};
use super::export::{self, ExportStep};
use super::filter::{self, Done};
use super::findings::{self, Summary};
use super::flows::{self, Act, Then as FlowThen};
use super::github;
use super::handoff::{self, Launch};
use super::history;
use super::layers::{self, Layer};
use super::modes::{self, BaseStep, Follow};
use super::open;
use super::open_hub::{OpenHub, OpenStep};
use super::open_pr::{self, Opened};
use super::pr_probe::Probe;
use super::review_hub::{self, ReviewHub};
use super::reviewed::ReviewedCache;
use super::settings::{SettingsBridge, Side, Then};
use super::startup::{self, After, Args, Pending, Refresh, Start, StartTab};
use super::threads::{self, Composed};
use super::tour;
use super::tui_view::{self, Ask, Mode, Overlay, PrKnown, TuiView};
use super::watched::{Landed, Reader};
use super::watcher::Watch;
use super::worktree::{self, GitHub, GitPick, HubStep};
use crate::input::keys::{
    key_action, AiSummaryAction, CommentAction, ConfirmKey, Edit, HistoryAction, NormalAction,
    OverlayAction, TourAction, TuiAction, Write,
};
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
/// Resends of a reload refused as moved, as the service's worktree writes.
const RELOAD_TRIES: u8 = 3;

/// Opens the arguments' tabs on an in-process service and runs the loop
/// until the user quits. `updates` answers a newer release's version once
/// the update check finds one; the loop never waits for it.
pub fn run(args: &Args, updates: Receiver<String>) -> anyhow::Result<()> {
    let ports = er_engine::ports::Ports::cli();
    let start = startup::plan(args, &ports)?;
    let (tx, events) = std::sync::mpsc::channel();
    let git = Arc::clone(&ports.git);
    let host = ServiceHost::new(ports, er_engine::config::managed_config_path()).with_events(tx);
    let mut shell = Shell::start(host, events, &start, git)?;
    shell.listen_for_update(updates);
    shell.first_launch(er_engine::first_launch::notice());
    let mut highlighter = Highlighter::new();

    enable_raw_mode()?;
    execute!(io::stdout(), EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    let result = shell.run_loop(&mut terminal, &mut highlighter);
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    // As the App's exit: the error, then the line that reopens the tabs.
    if let Err(err) = result {
        eprintln!("Error: {err:?}");
    }
    if let Some(hint) = shell.resume_hint() {
        eprintln!("\x1b[2m{hint}\x1b[0m");
    }
    Ok(())
}

struct Landing {
    key: TabKey,
    workspace: String,
    after: After,
    retry: Option<(Command, u8)>,
}

impl Landing {
    fn of(pending: &Pending) -> Self {
        Self {
            key: pending.key,
            workspace: pending.workspace.clone(),
            after: pending.after,
            retry: pending.retry.clone(),
        }
    }
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
    watch: Watch,
    /// The start's PR probe, the other one, until it answers; then its
    /// answer, until the tab it probed has content.
    probe: Option<Probe>,
    probed: Option<(TabKey, BranchPrProbe)>,
    /// The PR the probe found for a working tab's branch, and that branch.
    branch_prs: HashMap<TabKey, (u64, String)>,
    /// The tabs with a stack lookup on the way.
    stacks_reading: HashSet<TabKey>,
    /// Each tab's worktrees, from the list its Open hub's last opening asked
    /// for; none until that list lands.
    worktrees: HashMap<TabKey, Vec<er_engine::git::Worktree>>,
    /// The update check, until it answers.
    updates: Option<Receiver<String>>,
    /// The clipboard and the editor.
    launch: Box<dyn Launch>,
    /// What a test's copies and editor opens asked for.
    #[cfg(test)]
    pub recorder: handoff::Recorder,
    /// The runs the host is planning.
    runs: Vec<RunReceiver>,
    /// The bucket each tab that ran something showed, so a closed tab's log
    /// goes.
    run_buckets: HashMap<TabKey, String>,
    /// The terminal's width at the last draw, which the width keys hold
    /// the diff's minimum against, as the App's `last_terminal_width`.
    terminal_width: u16,
    /// The flow requests the host is building or the actor is running.
    flow_replies: Vec<(flows::Ask, ReplyReceiver)>,
    /// The flow results being read, which open the results overlay.
    flow_reads: Vec<Receiver<Result<ReviewFlowResults, WorkspaceError>>>,
    /// The buckets whose dead owners' flows were asked for.
    reconciled: HashSet<String>,
    pub should_quit: bool,
    /// The refusal every reload gets in a test, and the reloads it refused.
    #[cfg(test)]
    pub refuse_reloads: Option<String>,
    #[cfg(test)]
    pub reload_sends: usize,
    /// Every status-line message, in order, so a test sees one that a
    /// later message replaced within the same tick.
    #[cfg(test)]
    pub told: Vec<String>,
    /// The runs asked of the host in a test.
    #[cfg(test)]
    pub runs_started: usize,
    /// The automatic context pick's threshold in a test, over the settings'.
    #[cfg(test)]
    pub auto_context: Option<usize>,
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
        #[cfg(test)]
        let recorder = handoff::Recorder::default();
        #[cfg(test)]
        let launch: Box<dyn Launch> = Box::new(recorder.clone());
        #[cfg(not(test))]
        let launch = handoff::launcher();
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
            watch: Watch::default(),
            probe,
            probed: None,
            branch_prs: HashMap::new(),
            stacks_reading: HashSet::new(),
            worktrees: HashMap::new(),
            updates: None,
            launch,
            #[cfg(test)]
            recorder,
            runs: Vec::new(),
            run_buckets: HashMap::new(),
            terminal_width: 0,
            flow_replies: Vec::new(),
            flow_reads: Vec::new(),
            reconciled: HashSet::new(),
            should_quit: false,
            #[cfg(test)]
            refuse_reloads: None,
            #[cfg(test)]
            reload_sends: 0,
            #[cfg(test)]
            told: Vec::new(),
            #[cfg(test)]
            runs_started: 0,
            #[cfg(test)]
            auto_context: None,
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

    /// Takes the update check's answer on a later tick.
    pub fn listen_for_update(&mut self, updates: Receiver<String>) {
        self.updates = Some(updates);
    }

    /// Shows the first-launch notice, for longer, as the App's start does.
    pub fn first_launch(&mut self, notice: Option<String>) {
        if let Some(notice) = notice {
            self.notify(notice);
            self.dwell = NOTIFICATION_DWELL_LONG;
        }
    }

    /// The `er <args>` line that reopens the open tabs, if the App would
    /// print one.
    pub fn resume_hint(&self) -> Option<String> {
        let tabs = self.host.registry().tabs_view();
        let descriptors: Vec<_> = tabs
            .tabs
            .iter()
            .map(|entry| entry.descriptor.clone())
            .collect();
        startup::resume_hint(&descriptors)
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
    pub fn watch_mut(&mut self) -> &mut Watch {
        &mut self.watch
    }

    #[cfg(test)]
    pub fn in_flight(&self) -> usize {
        self.pending.len()
    }

    /// The sidecar reloads whose replies have not landed.
    #[cfg(test)]
    pub fn reloads_in_flight(&self) -> usize {
        self.reloads.len()
    }

    /// Takes the status-line message down, as the App's `clear_notification`.
    #[cfg(test)]
    pub fn clear_notification(&mut self) {
        self.view.notification = None;
        self.notified_at = None;
    }

    #[cfg(test)]
    pub fn runs_in_flight(&self) -> usize {
        self.runs.len()
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
            retry: None,
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
        #[cfg(test)]
        self.told.push(message.clone());
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

    /// The automatic context pick's threshold; zero turns it off.
    fn auto_context(&self) -> usize {
        #[cfg(test)]
        if let Some(threshold) = self.auto_context {
            return threshold;
        }
        let view = self.host.settings().view();
        view.config.display.auto_context_threshold
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
                // A moved load is resent or its reply says so.
                Ok(Event::CommandFailed { message }) if er_service::refused_as_moved(&message) => {}
                Ok(Event::CommandFailed { message }) => self.failure(Side::Event, message),
                Ok(Event::PendingReviewChanged { context }) => {
                    self.redraw = true;
                    self.review_changed(&context);
                }
                Ok(Event::TaskNotice { message, long }) => {
                    self.notify(message);
                    if long {
                        self.dwell = NOTIFICATION_DWELL_LONG;
                    }
                }
                Ok(Event::TabsChanged { .. }) => {
                    self.redraw = true;
                    let tabs = self.host.registry().tabs_view();
                    let open = tabs.tabs.iter().map(|entry| entry.key).collect();
                    let shown = (tabs.tabs.iter())
                        .filter_map(|entry| {
                            let workspace = entry.descriptor.workspace_key();
                            self.host.content(&workspace, entry.key)
                        })
                        .map(|content| content.context.er_dir.clone())
                        .collect();
                    let tasks = self.host.tasks();
                    agent_log::forget_closed(tasks, &mut self.run_buckets, &open, &shown);
                }
                Ok(_) => self.redraw = true,
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => break,
            }
        }
        self.drain_replies();
        self.drain_runs();
        self.drain_flows();
        flows::reconcile(&self.host, &mut self.reconciled);
        self.refresh_progress();
        self.sync_tasks();
        self.sync_watch(now);
        self.sync_probe();
        self.sync_watched(now);
        self.drain_reloads();
        if let Some(version) = self.updates.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.view.available_update = Some(version);
            self.updates = None;
            self.redraw = true;
        }
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
        self.send_restore();
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

    /// Asks the host to plan `request` on `key`'s tab. The reply lands on a
    /// later tick; nothing here waits on it.
    fn start_run(&mut self, key: TabKey, request: RunRequest) {
        if let Some(content) = self
            .workspace_of(key)
            .and_then(|workspace| self.host.content(&workspace, key))
        {
            self.run_buckets.insert(key, content.context.er_dir.clone());
        }
        // Said as asked, as the App does; any later notice replaces it.
        if let Some(started) = ai_runs::started(&request) {
            self.notify(started);
        }
        #[cfg(test)]
        {
            self.runs_started += 1;
        }
        self.runs.push(self.host.run(key, request));
    }

    /// A run the planner refused says why.
    fn drain_runs(&mut self) {
        let mut said = Vec::new();
        self.runs.retain(|replies| match replies.try_recv() {
            Ok(Ok(())) => false,
            Ok(Err(error)) => {
                said.push(error.message);
                false
            }
            Err(TryRecvError::Empty) => true,
            Err(TryRecvError::Disconnected) => {
                said.push("The run stopped before it answered".into());
                false
            }
        });
        for message in said {
            self.notify(message);
        }
    }

    /// Asks the host for a Review row's request, or opens its overlay.
    fn flow_act(&mut self, key: TabKey, act: Act) {
        match act {
            Act::Progress(er_dir) => {
                let live = self.live_flow(&er_dir);
                if let Some(flow) = live {
                    let progress = flows::Progress::new(er_dir, flow);
                    self.view.overlay = Some(Overlay::FlowProgress(Box::new(progress)));
                }
            }
            Act::Results { er_dir, run_id } => {
                self.flow_reads
                    .push(self.host.flow_results(&er_dir, &run_id));
            }
            act => {
                if let Some((request, ask)) = act.request() {
                    self.flow_replies.push((ask, self.host.flow(key, request)));
                }
            }
        }
    }

    /// The active tab's actor's flow on `er_dir`.
    fn live_flow(&self, er_dir: &str) -> Option<er_engine::ai::review_flow::ReviewFlow> {
        let (_, workspace, _) = self.active()?;
        let view = self.host.published_view(&workspace)?;
        view.review_flow(er_dir).map(|live| live.flow.clone())
    }

    /// The progress overlay follows the published flow.
    fn refresh_progress(&mut self) {
        let Some(Overlay::FlowProgress(progress)) = &self.view.overlay else {
            return;
        };
        if let Some(latest) = self.live_flow(&progress.er_dir) {
            if let Some(Overlay::FlowProgress(progress)) = self.view.overlay.as_mut() {
                progress.refresh(&latest);
            }
        }
    }

    /// Says what each landed flow answer says, and opens the results read.
    fn drain_flows(&mut self) {
        let mut landed = Vec::new();
        self.flow_replies
            .retain(|(ask, replies)| match replies.try_recv() {
                Ok(reply) => {
                    landed.push(ask.landed(reply));
                    false
                }
                Err(TryRecvError::Empty) => true,
                Err(TryRecvError::Disconnected) => {
                    landed.push(ask.landed(Err(WorkspaceError::Disconnected)));
                    false
                }
            });
        for (said, reread) in landed {
            if let Some(message) = said {
                self.notify(message);
            }
            if let Some((er_dir, run_id)) = reread {
                self.flow_reads
                    .push(self.host.flow_results(&er_dir, &run_id));
            }
        }
        let mut read = Vec::new();
        self.flow_reads.retain(|reads| match reads.try_recv() {
            Ok(results) => {
                read.push(results);
                false
            }
            Err(TryRecvError::Empty) => true,
            Err(TryRecvError::Disconnected) => false,
        });
        for results in read {
            let free = matches!(self.view.overlay, None | Some(Overlay::FlowResults(_)));
            match results {
                Ok(results) if free => {
                    let results = flows::Results::new(results);
                    self.view.overlay = Some(Overlay::FlowResults(Box::new(results)));
                    self.redraw = true;
                }
                Ok(_) => {}
                Err(error) => self.notify(format!("Review results: {error}")),
            }
        }
    }

    /// Retires finished tasks and starts what the queue can, as the
    /// desktop's tick does, and draws each tick while a command runs on the
    /// tab whose Log panel is open, since an output line sends no event.
    fn sync_tasks(&mut self) {
        let tasks = self.host.tasks();
        if tasks.rows().is_empty() {
            return;
        }
        er_service::tasks::poll(tasks, &self.host.task_hooks());
        let Some((key, _, Some(content))) = self.active() else {
            return;
        };
        let shows_log = self
            .view
            .tab(key)
            .is_some_and(|state| state.panel == Some(er_engine::ai::PanelContent::AgentLog));
        if shows_log && agent_log::any_running(self.host.tasks(), &content.context.er_dir) {
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

    /// A reload that read an outside write on the active tab says so, as the
    /// App's sidecar poll does.
    fn drain_reloads(&mut self) {
        let mut failures = Vec::new();
        let mut refreshed = Vec::new();
        self.reloads.retain(|replies| match replies.try_recv() {
            Ok(reply) => {
                match reply {
                    Ok(Reply::SidecarsReloaded {
                        key,
                        reloaded: true,
                    }) => refreshed.push(key),
                    Ok(_) => {}
                    Err(error) => failures.push(error.to_string()),
                }
                false
            }
            Err(TryRecvError::Empty) => true,
            Err(TryRecvError::Disconnected) => false,
        });
        for message in failures {
            self.failure(Side::Reply, message);
        }
        if refreshed.is_empty() {
            return;
        }
        let active = self.host.registry().tabs_view().active;
        if active.is_some_and(|key| refreshed.contains(&key)) {
            self.notify("✓ AI data refreshed".into());
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
                    landed.push((Landing::of(pending), reply));
                    false
                }
                Err(TryRecvError::Empty) => true,
                // A dead write or reload answers as a refusal.
                Err(TryRecvError::Disconnected)
                    if pending.after.follows()
                        || matches!(
                            pending.after,
                            After::Composed
                                | After::Deleted
                                | After::Posted
                                | After::ReviewRead
                                | After::ReviewEdit
                                | After::ReviewSubmit
                                | After::Checklist { .. }
                                | After::Cleared(_)
                                | After::StackRead
                                | After::PrRefreshed { .. }
                                | After::Rerun(_)
                        ) =>
                {
                    let lost = er_service::WorkspaceError::Failed(
                        "The write stopped before it answered".into(),
                    );
                    landed.push((Landing::of(pending), Err(lost)));
                    false
                }
                Err(TryRecvError::Disconnected) => false,
            });
        for (landing, reply) in landed {
            let Landing {
                key,
                workspace,
                after,
                retry,
            } = landing;
            self.redraw = true;
            match reply {
                reply if matches!(after, After::Context { .. }) => {
                    let content = self.host.content(&workspace, key);
                    let state = self.view.tab_mut_if_open(key);
                    if let (Some(state), Some(content)) = (state, content) {
                        let sent = retry.as_ref().map(|(command, _)| command);
                        context::landed(state, &content, reply.is_ok(), sent);
                    }
                    if let (Err(error), After::Context { auto: false }) = (reply, after) {
                        self.failure(Side::Reply, error.to_string());
                    }
                }
                reply if after == After::Worktrees => {
                    let listed = match reply {
                        Ok(Reply::Worktrees { worktrees, .. }) => worktrees,
                        _ => Vec::new(),
                    };
                    self.worktrees.insert(key, listed);
                }
                reply if after == After::StackRead => {
                    self.stacks_reading.remove(&key);
                    if let Err(error) = reply {
                        self.failure(Side::Reply, error.to_string());
                    }
                }
                reply if matches!(after, After::PrRefreshed { .. }) => {
                    if let After::PrRefreshed {
                        github_count,
                        local_count,
                    } = after
                    {
                        // A branch PR dropped because the tab moved branch
                        // meanwhile refreshed nothing.
                        let content = self.host.content(&workspace, key);
                        let held = content.is_some_and(|content| content.pr.pr_number.is_some());
                        let refreshed = reply.is_ok() && held;
                        self.notify(github::synced(github_count, local_count, refreshed));
                    }
                }
                _ if matches!(after, After::CommitSelected | After::MoreHistory) => {
                    self.history_landed(key, &workspace, after, reply);
                }
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
                        retry: None,
                    });
                }
                Ok(Reply::ContentLoaded {
                    reviewed_cleared, ..
                }) if after.follows() => self.reloaded(key, &workspace, after, reviewed_cleared),
                Err(error) if after.follows() => match retry {
                    Some((command, tries))
                        if tries < RELOAD_TRIES
                            && er_service::refused_as_moved(&error.to_string()) =>
                    {
                        let replies = self.send_reload(&workspace, command.clone(), after);
                        self.pending.push(Pending {
                            key,
                            workspace,
                            replies,
                            after,
                            retry: Some((command, tries + 1)),
                        });
                    }
                    _ => self.reload_refused(key, after, &error),
                },
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
                Ok(
                    Reply::AnnotationWritten { .. }
                    | Reply::ThreadChanged { .. }
                    | Reply::FindingReplied { .. },
                ) if after == After::Composed => {
                    self.composed(key);
                }
                Ok(
                    Reply::AnnotationWritten { .. }
                    | Reply::CommentPosted { .. }
                    | Reply::PendingReview { .. },
                ) if after == After::Posted => {
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
                    self.refresh_pr(key, &workspace, github_count, local_count);
                }
                Err(error) if after == After::Synced => {
                    let message = error.to_string();
                    if let Some(known) = github::known_after_refusal(&message) {
                        self.view.learn_pr(key, known);
                    }
                    self.failure(Side::Reply, message);
                }
                Ok(Reply::AiWritten) if matches!(after, After::Checklist { .. }) => {
                    if let After::Checklist { checked } = after {
                        self.notify(findings::checklist_notice(checked).into());
                    }
                }
                Ok(Reply::AiWritten) if matches!(after, After::Cleared(_)) => {
                    if let After::Cleared(clear) = after {
                        self.notify(clears::landed(clear).into());
                    }
                }
                Ok(_) if matches!(after, After::Rerun(_)) => {
                    if let After::Rerun(rerun) = after {
                        self.start_run(key, rerun.request());
                    }
                }
                Err(error) if matches!(after, After::Rerun(_)) => {
                    if let After::Rerun(rerun) = after {
                        self.settings.claim_reply(&error.to_string());
                        self.notify(rerun.refused(&error.to_string()));
                    }
                }
                Err(error) if matches!(after, After::Cleared(_)) => {
                    if let After::Cleared(clear) = after {
                        self.settings.claim_reply(&error.to_string());
                        self.notify(clears::refused(clear, &error.to_string()));
                    }
                }
                Ok(Reply::ReviewSubmitted { .. }) if after == After::Approved => {
                    self.notify("PR approved".into());
                }
                Err(error) if after == After::Approved => self.review_failure(error),
                Ok(Reply::PendingReview { review, .. })
                    if matches!(after, After::ReviewRead | After::ReviewEdit) =>
                {
                    if let Some(hub) = self.view.review.as_mut().filter(|hub| hub.key == key) {
                        hub.load(review);
                    }
                    if after == After::ReviewEdit {
                        self.notify("Comment removed from your pending review".into());
                    }
                }
                Err(error) if after == After::ReviewRead => {
                    // A hub whose first read failed has nothing to show.
                    if self.view.review.as_ref().is_some_and(|hub| !hub.loaded()) {
                        self.close_review();
                    }
                    self.failure(Side::Reply, error.to_string());
                }
                Ok(Reply::ReviewSubmitted { .. }) if after == After::ReviewSubmit => {
                    self.close_review();
                    self.notify("Review submitted".into());
                }
                Err(error) if after == After::ReviewSubmit => self.review_failure(error),
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
        let Some(action) = self.with_screen(tab, &content, |screen| key_action(key, screen)) else {
            return;
        };
        let before = self.view.tab(tab).map(|state| state.selected_file);
        self.execute(action, tab, &workspace, &content);
        // The App clears the selection on every move to another file.
        if let Some(state) = self.view.tab_mut_if_open(tab) {
            if Some(state.selected_file) != before {
                state.selection_anchor = None;
            }
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
        state.history.follow(content);
        tour::follow(&mut state.tour, content);
        layers::follow(state, content);
        threads::scroll_if_parsed(state, content);
        findings::follow(state, content);
        findings::place_if_parsed(state, content);
        let reading = self.stacks_reading.contains(&key);
        let known = self.worktrees.get(&key).map(Vec::len);
        if let Some(Overlay::Open(hub)) = self.view.overlay.as_mut() {
            hub.follow(content, reading, known);
        }
        let state = self.view.tab(key)?;
        let shows_log = state.panel == Some(er_engine::ai::PanelContent::AgentLog);
        let jobs: Jobs = agent_log::read(self.host.tasks(), &content.context.er_dir, shows_log);
        let frame = tui_view::Frame {
            tab_names,
            active_tab,
            content,
            config: &settings.config,
            config_load_error: settings.load_banner.as_deref(),
            ai_selection_label,
            reviewed_count,
            watching: self.watch.watching(),
            jobs: &jobs,
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
            TuiAction::Overlay(action) if matches!(self.view.overlay, Some(Overlay::Review)) => {
                return self.review_key(action);
            }
            TuiAction::ReviewSummary(edit) => {
                let done = self
                    .view
                    .review
                    .as_mut()
                    .is_some_and(|hub| hub.summary_key(edit));
                if done {
                    self.view.input_mode = Mode::Normal;
                    self.view.overlay = Some(Overlay::Review);
                }
                return;
            }
            TuiAction::Write(Write::RemoveReviewComment) => {
                self.view.input_mode = Mode::Normal;
                self.view.overlay = Some(Overlay::Review);
                let id = self
                    .view
                    .review
                    .as_mut()
                    .and_then(|hub| hub.removing.take());
                if let Some(id) = id {
                    self.review_write(After::ReviewEdit, |_, content| {
                        review_hub::remove(content, id)
                    });
                }
                return;
            }
            TuiAction::Overlay(action)
                if matches!(self.view.overlay, Some(Overlay::FilterHistory { .. })) =>
            {
                let shown = self.watched_shown(key, content);
                let done = filter::history(&mut self.view, key, content, shown, action);
                return self.filtered(key, workspace, content, done);
            }
            TuiAction::Overlay(action) if matches!(self.view.overlay, Some(Overlay::Open(_))) => {
                return self.open_hub_key(action, key, workspace, content)
            }
            TuiAction::Overlay(action)
                if matches!(
                    self.view.overlay,
                    Some(Overlay::Worktrees(_) | Overlay::Directory(_))
                ) =>
            {
                return self.folder_key(action)
            }
            TuiAction::Overlay(action)
                if matches!(
                    self.view.overlay,
                    Some(Overlay::Copy(_) | Overlay::ExportPicker { .. })
                ) =>
            {
                return self.copy_overlay_key(action, key, content)
            }
            TuiAction::Overlay(action) if matches!(self.view.overlay, Some(Overlay::Help(_))) => {
                if let Some(Overlay::Help(hub)) = self.view.overlay.as_mut() {
                    if !hub.act(action) {
                        self.view.overlay = None;
                    }
                }
                return;
            }
            TuiAction::Overlay(action) => return self.overlay_key(action, key, content),
            TuiAction::BaseBranch(edit) => return self.base_key(edit, key, workspace, content),
            TuiAction::RemoteUrl(edit) => return self.remote_url_key(edit),
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
            TuiAction::History(action) => return self.history_key(action, key, workspace, content),
            TuiAction::Tour(action) => return self.tour_key(action, key, workspace, content),
            TuiAction::Comment(action) => return self.comment_key(action, key, workspace, content),
            TuiAction::AiSummary(action) => {
                return self.ai_summary_key(action, key, workspace, content)
            }
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
            TuiAction::Write(write @ (Write::ClearQuestionsAndNotes | Write::ClearReviews)) => {
                self.view.input_mode = Mode::Normal;
                let clear = match write {
                    Write::ClearQuestionsAndNotes => Clear::QuestionsAndNotes,
                    _ => Clear::Reviews,
                };
                let command = clears::command(clear, content);
                return self.post(key, workspace, command, After::Cleared(clear));
            }
            TuiAction::Write(
                write @ (Write::ClearReviewAndRun
                | Write::ClearAnswersAndRunQuestions
                | Write::ClearRepliesAndRunNotes),
            ) => {
                self.view.input_mode = Mode::Normal;
                let rerun = match write {
                    Write::ClearReviewAndRun => Rerun::Review,
                    Write::ClearAnswersAndRunQuestions => Rerun::Questions,
                    _ => Rerun::Notes,
                };
                if let Some(refused) = rerun.refused_before_clear(content) {
                    return self.notify(refused);
                }
                let command = rerun.clear(content);
                return self.post(key, workspace, command, After::Rerun(rerun));
            }
            TuiAction::Confirm(
                confirm @ (ConfirmKey::KeepAndRunReview
                | ConfirmKey::KeepAndRunQuestions
                | ConfirmKey::KeepAndRunNotes),
            ) => {
                self.view.input_mode = Mode::Normal;
                let rerun = match confirm {
                    ConfirmKey::KeepAndRunReview => Rerun::Review,
                    ConfirmKey::KeepAndRunQuestions => Rerun::Questions,
                    _ => Rerun::Notes,
                };
                return self.start_run(key, rerun.request());
            }
            TuiAction::Confirm(ConfirmKey::Cancel) => {
                if self.view.input_mode == Mode::Confirm(Ask::RemoveReviewComment) {
                    self.view.overlay = Some(Overlay::Review);
                }
                self.view.input_mode = Mode::Normal;
                return;
            }
        };
        let shown = self.watched_shown(key, content);
        let done = filter::normal(&mut self.view, key, content, shown, action.clone());
        if let Some(done) = done {
            return self.filtered(key, workspace, content, done);
        }
        let settings = self.host.settings().view();
        let config = settings.config.watched.clone();
        let split_diff = settings.config.display.split_diff;
        let threshold = self.auto_context();
        let width = self.terminal_width;
        let state = self.view.tab_mut(key, content);
        let split = chrome::split_side(state, split_diff);
        let file = content
            .diff
            .files
            .get(state.selected_file)
            .filter(|_| state.watched.selected.is_none())
            .map(|file| &**file);
        let shown = state.watched.is_shown(&config);
        let selected = match action {
            A::Quit => return self.quit(),
            A::NextFile | A::PrevFile => {
                let moved = state.move_file(content, shown, action == A::NextFile);
                let at = state.selected_file;
                let auto = context::auto(key, state, content, at, threshold);
                if let Some(command) = auto {
                    self.post(key, workspace, command, After::Context { auto: true });
                }
                moved
            }
            A::ExpandContext | A::CollapseContext => {
                let expand = action == A::ExpandContext;
                return match context::step(key, state, content, file, expand) {
                    Ok(Some(command @ Command::ParseFiles { .. })) => {
                        self.post(key, workspace, command, After::Nothing);
                    }
                    Ok(Some(command)) => {
                        self.post(key, workspace, command, After::Context { auto: false });
                    }
                    Ok(None) => {}
                    Err(message) => self.notify(message),
                };
            }
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
            A::ExtendSelectionDown | A::ExtendSelectionUp => {
                state.extend_selection(file, action == A::ExtendSelectionDown);
                None
            }
            A::OpenCopyHub => {
                self.view.overlay = Some(Overlay::Copy(CopyHub::open(state, content)));
                return;
            }
            A::OpenExportPicker => {
                self.view.overlay = Some(export::open());
                return;
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
            A::ScrollRight | A::ScrollLeft | A::ScrollHome if split.is_some() => {
                let by = match action {
                    A::ScrollRight => Some(1),
                    A::ScrollLeft => Some(-1),
                    _ => None,
                };
                chrome::scroll_split(state, state.split_focus, by);
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
                let has_diff = !content.diff.files.is_empty();
                let published = self.host.published_view(workspace);
                let live = published
                    .as_ref()
                    .and_then(|view| view.review_flow(&content.context.er_dir));
                let flow = flows::FlowFacts::of(content, live);
                let facts = ai_picker::HubFacts::of(&content.ai.state, has_diff, flow);
                return self.run_step(ai_picker::open_ai_hub(&view, &label, facts));
            }
            A::OpenVerifyHub => {
                let view = self.host.settings().view();
                let label = self.settings.ai_label(&view);
                return self.run_step(ai_picker::open_verify_hub(&view, &label));
            }
            A::AskClearQuestions | A::AskClearReviews => {
                let clear = match action {
                    A::AskClearQuestions => Clear::QuestionsAndNotes,
                    _ => Clear::Reviews,
                };
                if let Some(ask) = clears::ask(clear, content, true) {
                    self.view.input_mode = Mode::Confirm(ask);
                }
                return;
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
                let known = self.view.pr_known(key, content);
                self.view.overlay = Some(Overlay::GitHub(GitHub::open(content, known)));
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
            A::CloseTab => return self.close_tab(key, workspace),
            A::ResizeFileTree(delta) => {
                chrome::resize_tree(state, delta, width);
                return;
            }
            A::ResizePanel(delta) => {
                chrome::resize_panel(state, delta, width);
                return;
            }
            A::ToggleSplitFocus => {
                chrome::toggle_split_focus(state);
                return;
            }
            A::OpenHelpHub => {
                self.view.overlay = Some(Overlay::Help(HelpHub::open()));
                return;
            }
            A::StartQuestion => {
                let question = er_engine::ai::CommentType::Question;
                if threads::start(state, content, question, split) {
                    self.view.input_mode = Mode::Comment;
                }
                return;
            }
            A::StartGithubComment => {
                let known = self.view.pr_known(key, content);
                let state = self.view.tab_mut(key, content);
                let started = github::start_line(state, content, known, split);
                return self.compose(started);
            }
            A::SyncGithubComments => return self.pull_comments(key, workspace, content),
            A::ResumeComment => {
                self.view.input_mode = Mode::Comment;
                return;
            }
            A::Reply if state.focused_comment_id.is_none() => {
                let focus = findings::start_reply(state, content);
                return self.focus(focus);
            }
            A::Edit => match threads::start_edit(state, content) {
                threads::Focus::Editor => {
                    let (file, hunk) = (state.shown_file(content), state.current_hunk);
                    return self.edit_file(content, file, hunk);
                }
                focus => return self.focus(focus),
            },
            A::Reply | A::AskDeleteComment => {
                let focus = match action {
                    A::Reply => threads::start_reply(state, content),
                    _ => threads::ask_delete(state, content),
                };
                return self.focus(focus);
            }
            A::NextHint | A::PrevHint => threads::jump_hint(state, content, action == A::NextHint),
            A::NextFinding | A::PrevFinding => {
                findings::jump(state, content, action == A::NextFinding)
            }
            A::NavigatePanelFinding(forward) => {
                findings::step_panel(state, content, forward);
                return;
            }
            A::JumpToFocusedFinding => {
                if let Some(notice) = findings::jump_to_focused(state, content) {
                    self.notify(notice);
                }
                return;
            }
            A::SwitchMode(mode) => {
                if let Some(command) = modes::switch(key, content, mode) {
                    state.follow = Some(Follow::of(state, content));
                    // Queued commit moves would each move the tab under
                    // the switch; the switch drops them.
                    state.history.queued.clear();
                    if mode == DiffMode::History && content.tab.mode != DiffMode::History {
                        state.history.left = Some(Follow::of(state, content));
                    }
                    let pr_diff = mode == DiffMode::PrDiff;
                    self.post(key, workspace, command, After::ModeSwitched { pr_diff });
                }
                return;
            }
            A::Refresh => return self.refresh(key, workspace, content, Refresh::Key),
            A::ToggleMtimeSort => {
                state.sort_by_mtime = !state.sort_by_mtime;
                let recent = state.sort_by_mtime;
                self.notify(if recent {
                    "Sort: recent first".into()
                } else {
                    "Sort: default".into()
                });
                return self.refresh(key, workspace, content, Refresh::Sort);
            }
            A::ToggleWatch => {
                let message = self.watch.toggle(key, content);
                return self.notify(message);
            }
            A::OpenOpenHub => {
                let reading = self.read_stack(key, workspace, content, false);
                // Asked again on each opening, as the App reads it on each.
                self.worktrees.remove(&key);
                let list = Command::ListWorktrees { key };
                self.post(key, workspace, list, After::Worktrees);
                self.view.overlay = Some(Overlay::Open(OpenHub::open(content, reading, None)));
                return;
            }
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
                state.focused_finding_id = None;
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
            // `filter::normal` took these above.
            A::StartSearch
            | A::StartFilter
            | A::OpenFilterHistory
            | A::ClearSearch
            | A::ClearFilter => return,
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
        let kept = after.follows() || matches!(after, After::Context { .. });
        let retry = kept.then(|| (command.clone(), 0));
        let replies = self.send_reload(workspace, command, after);
        self.pending.push(Pending {
            key,
            workspace: workspace.to_string(),
            replies,
            after,
            retry,
        });
    }

    /// Posts a command; a test may refuse every reload as moved here.
    fn send_reload(&mut self, workspace: &str, command: Command, after: After) -> ReplyReceiver {
        #[cfg(test)]
        if let Some(moved) = self
            .refuse_reloads
            .clone()
            .filter(|_| after.follows() || after == After::CommitSelected)
        {
            self.reload_sends += 1;
            let (tx, rx) = std::sync::mpsc::channel();
            let _ = tx.send(Err(er_service::WorkspaceError::Failed(moved)));
            return rx;
        }
        let _ = after;
        self.host.post(workspace, command)
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
        }
    }

    /// Runs an AI Summary key; Space sends the checklist toggle, and Enter
    /// parses the file it selected when that is a stub.
    fn ai_summary_key(
        &mut self,
        action: AiSummaryAction,
        key: TabKey,
        workspace: &str,
        content: &TabContent,
    ) {
        let state = self.view.tab_mut(key, content);
        match findings::summary_key(state, content, action) {
            Summary::Done => {}
            Summary::Notice(notice) => self.notify(notice),
            Summary::Jumped { index, notice } => {
                self.notify(notice);
                self.parse_if_stub(key, workspace, content, index);
            }
            Summary::Toggle { command, checked } => {
                self.post(key, workspace, *command, After::Checklist { checked });
            }
        }
    }

    /// Runs what a thread key on the focused comment asks.
    fn focus(&mut self, focus: threads::Focus) {
        match focus {
            threads::Focus::Nothing | threads::Focus::Editor => {}
            threads::Focus::Opened => self.view.input_mode = Mode::Comment,
            threads::Focus::AskDelete => {
                self.view.input_mode = Mode::Confirm(Ask::DeleteComment);
            }
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
        let Some(workspace) = self.workspace_of(key) else {
            return;
        };
        let Some(content) = self.host.content(&workspace, key) else {
            return;
        };
        let Some((_, probe)) = self.probed.take() else {
            return;
        };
        match probe {
            BranchPrProbe::Present { number, base } => {
                self.view.learn_pr(key, PrKnown::Present);
                // The PR panel reads the PR section, which the branch's PR
                // fills without making the tab a PR tab.
                if content.tab.pr_number.is_none() {
                    let branch = content.tab.current_branch.clone();
                    self.branch_prs.insert(key, (number, branch));
                    let command = Command::LoadBranchPr { key, number };
                    self.post(key, &workspace, command, After::Nothing);
                }
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

    /// A Tour key; Space and `b` send their reviewed write to the tab's
    /// actor, and `b` says so at the press, as the App's does.
    fn tour_key(&mut self, action: TourAction, key: TabKey, workspace: &str, content: &TabContent) {
        let pillar = action == TourAction::ReviewPillar;
        let state = self.view.tab_mut(key, content);
        if let Some(command) = tour::key(&mut state.tour, content, action) {
            self.post(
                key,
                workspace,
                command,
                After::Reviewed { advance_from: None },
            );
        }
        if pillar {
            self.notify("Reviewed all files in pillar".into());
        }
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
            HubStep::Run(GitPick::Push) => {
                if content.tab.mode == DiffMode::Staged {
                    self.view.input_mode = Mode::Confirm(Ask::Push);
                }
            }
            HubStep::Run(GitPick::StageFile) => self.toggle_staged(key, workspace, content),
            HubStep::Run(GitPick::StageAll) => {
                self.post(key, workspace, Command::StageAll { key }, After::Nothing);
            }
            HubStep::Run(GitPick::Refresh) => self.refresh(key, workspace, content, Refresh::Key),
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
            HubStep::Run(GitPick::Review) => {
                self.view.review = Some(ReviewHub::open(key, workspace, content));
                self.view.overlay = Some(Overlay::Review);
                self.post(key, workspace, review_hub::read(content), After::ReviewRead);
            }
        }
    }

    /// A refresh with auto-unmark; the selection stays on its file.
    fn refresh(&mut self, key: TabKey, workspace: &str, content: &TabContent, why: Refresh) {
        // The App's refresh leaves History alone: its notice, and no reload.
        if content.tab.mode == DiffMode::History {
            let notice = match why {
                Refresh::Key => Some("Refreshed".to_string()),
                Refresh::Watched { files } => {
                    self.watch.landed(key);
                    modes::watch_notice(0, files)
                }
                Refresh::Sort => None,
            };
            if let Some(notice) = notice {
                self.notify(notice);
            }
            return;
        }
        let state = self.view.tab_mut(key, content);
        state.refresh_follow = Some(Follow::of(state, content));
        let command = Command::RefreshDiff {
            key,
            force: false,
            adopt_head_oid: None,
        };
        self.post(key, workspace, command, After::Refreshed(why));
    }

    /// A reload landed: the App's selection rule, and its notice.
    fn reloaded(&mut self, key: TabKey, workspace: &str, after: After, cleared: usize) {
        if matches!(after, After::Refreshed(Refresh::Watched { .. })) {
            self.watch.landed(key);
        }
        let Some(content) = self.host.content(workspace, key) else {
            return;
        };
        let shown = self.watched_shown(key, &content);
        let state = self.view.tab_mut(key, &content);
        let selected = match after {
            After::ModeSwitched { .. } => state
                .follow
                .take()
                .and_then(|follow| modes::switched(state, follow, &content, shown)),
            After::Refreshed(_) => state
                .refresh_follow
                .take()
                .and_then(|follow| modes::refreshed(state, follow, &content)),
            _ => state
                .follow
                .take()
                .and_then(|follow| modes::refreshed(state, follow, &content)),
        };
        let sent = state.base_sent.take();
        // Hidden lists the watched files at once, as the App's switch does.
        if content.tab.mode == DiffMode::Hidden && matches!(after, After::ModeSwitched { .. }) {
            self.watched.rescan = true;
        }
        match after {
            After::Refreshed(Refresh::Key) => self.notify("Refreshed".into()),
            After::Refreshed(Refresh::Watched { files }) => {
                if let Some(notice) = modes::watch_notice(cleared, files) {
                    self.notify(notice);
                }
            }
            After::BaseChanged => {
                let base_ref = content.tab.base_branch.clone();
                let saved = self
                    .host
                    .registry()
                    .send(er_service::AppCommand::SetBaseRef { key, base_ref });
                match (saved, sent) {
                    (Err(error), _) => self.notify(error.to_string()),
                    (Ok(_), Some(base)) => self.notify(format!("Base set to {base}")),
                    (Ok(_), None) => {}
                }
            }
            _ => {}
        }
        if let Some(index) = selected {
            self.parse_if_stub(key, workspace, &content, index);
        }
    }

    /// A History key; a commit move goes to the service.
    fn history_key(
        &mut self,
        action: HistoryAction,
        key: TabKey,
        workspace: &str,
        content: &TabContent,
    ) {
        let state = self.view.tab_mut(key, content);
        let command = history::key(&mut state.history, key, content, action);
        self.send_history(key, workspace, command);
    }

    fn send_history(&mut self, key: TabKey, workspace: &str, command: Option<Command>) {
        let Some(command) = command else {
            return;
        };
        let after = match command {
            Command::LoadMoreHistory { .. } => After::MoreHistory,
            _ => After::CommitSelected,
        };
        let replies = self.send_reload(workspace, command, after);
        self.pending.push(Pending {
            key,
            workspace: workspace.to_string(),
            replies,
            after,
            retry: None,
        });
    }

    /// A commit load or page landed: a commit shown takes its press off the
    /// queue, a tab moved meanwhile sends it again up to `RELOAD_TRIES`
    /// times, any other refusal drops the rest, and the next press goes out.
    fn history_landed(
        &mut self,
        key: TabKey,
        workspace: &str,
        after: After,
        reply: Result<Reply, er_service::WorkspaceError>,
    ) {
        let content = self.host.content(workspace, key);
        let (Some(content), Some(state)) = (content, self.view.tab_mut_if_open(key)) else {
            return;
        };
        let pos = &mut state.history;
        pos.in_flight = false;
        let refused = match reply {
            Err(error) => {
                let moved = er_service::refused_as_moved(&error.to_string());
                pos.moved_tries = pos.moved_tries.saturating_add(u8::from(moved));
                (!moved || pos.moved_tries > RELOAD_TRIES).then(|| error.to_string())
            }
            Ok(_) => {
                pos.moved_tries = 0;
                if after == After::CommitSelected {
                    pos.queued.pop_front();
                }
                None
            }
        };
        if refused.is_some() {
            pos.queued.clear();
            pos.moved_tries = 0;
        }
        let command = history::pump(pos, key, &content);
        if let Some(message) = refused {
            self.failure(Side::Reply, message);
        }
        self.send_history(key, workspace, command);
    }

    /// A reload was refused: its own waits let go, and a restore keeps its filter.
    fn reload_refused(&mut self, key: TabKey, after: After, error: &er_service::WorkspaceError) {
        if matches!(after, After::Refreshed(Refresh::Watched { .. })) {
            self.watch.landed(key);
        }
        let workspace = self.workspace_of(key);
        let content = workspace.and_then(|workspace| self.host.content(&workspace, key));
        if let (Some(state), Some(content)) = (self.view.tab_mut_if_open(key), content) {
            match after {
                After::Refreshed(_) => state.refresh_follow = None,
                After::ModeSwitched { .. } => {
                    state.follow = None;
                    if let Some(saved) = state.restore.take_if(|saved| saved.sent) {
                        super::session::restore_in_view(state, saved.session, &content);
                    }
                }
                _ => {
                    state.follow = None;
                    state.base_sent = None;
                }
            }
        }
        let message = match after {
            After::ModeSwitched { pr_diff: true } => format!("PR diff unavailable: {error}"),
            After::BaseChanged => format!("Could not change base: {error}"),
            _ => error.to_string(),
        };
        self.notify(message);
    }

    fn base_key(&mut self, edit: Edit, key: TabKey, workspace: &str, content: &TabContent) {
        let step = modes::base_key(&mut self.view.base_input, edit);
        let BaseStep::Send(base) = step else {
            if matches!(step, BaseStep::Close) {
                self.view.input_mode = Mode::Normal;
            }
            return;
        };
        self.view.input_mode = Mode::Normal;
        let state = self.view.tab_mut(key, content);
        state.follow = Some(Follow::of(state, content));
        state.history.queued.clear();
        state.base_sent = Some(base.clone());
        self.post(
            key,
            workspace,
            Command::ChangeBase { key, base },
            After::BaseChanged,
        );
    }

    fn open_hub_key(
        &mut self,
        action: OverlayAction,
        key: TabKey,
        workspace: &str,
        content: &TabContent,
    ) {
        let Some(Overlay::Open(hub)) = self.view.overlay.as_mut() else {
            return;
        };
        let step = hub.act(action);
        if !matches!(step, OpenStep::Stay) {
            self.view.overlay = None;
        }
        match step {
            OpenStep::Stay | OpenStep::Close => {}
            OpenStep::Notify(message) => self.notify(message),
            OpenStep::Browse => {
                let browser = open::Directory::open(&content.tab.repo_root);
                self.view.overlay = Some(Overlay::Directory(browser));
            }
            OpenStep::Worktrees => {
                let worktrees = self.worktrees.get(&key).cloned().unwrap_or_default();
                let picker = open::Worktrees {
                    worktrees,
                    selected: 0,
                };
                self.view.overlay = Some(Overlay::Worktrees(picker));
            }
            OpenStep::Browser => {
                if let Some(number) = content.tab.pr_number {
                    let (root, slug) = (&content.tab.repo_root, content.tab.remote_repo.as_deref());
                    self.host
                        .ports()
                        .github
                        .open_pr_in_browser(root, number, slug);
                    self.notify("Opening PR in browser...".into());
                }
            }
            OpenStep::StackBrowser(pr) => {
                let slug = github_pr_slug(&pr.url);
                let root = &content.tab.repo_root;
                let github = &self.host.ports().github;
                if github.open_pr_in_browser(root, pr.number, slug.as_deref()) {
                    let (branch, number) = (pr.branch, pr.number);
                    self.notify(format!("Opening {branch} (#{number}) in browser..."));
                }
            }
            OpenStep::ChangeBase => {
                self.view.base_input.clear();
                self.view.input_mode = Mode::BaseBranch;
            }
            OpenStep::RemoteUrl => {
                self.view.base_input.clear();
                self.view.input_mode = Mode::RemoteUrl;
            }
            OpenStep::OpenPr(pr) => {
                let failed = format!("Failed to open {} (#{})", pr.branch, pr.number);
                self.open_pr(&pr.url, &failed);
            }
            // As the App's, the hub opens again on its loading row.
            OpenStep::RefreshStack => {
                let reading = self.read_stack(key, workspace, content, true);
                let known = self.worktrees.get(&key).map(Vec::len);
                let hub = OpenHub::open(content, reading, known);
                self.view.overlay = Some(Overlay::Open(hub));
            }
        }
    }

    /// Runs a key on the worktree picker or the folder browser; a pick opens
    /// the folder as a tab, or selects the tab already on its root.
    fn folder_key(&mut self, action: OverlayAction) {
        let pick = match self.view.overlay.as_mut() {
            Some(Overlay::Worktrees(picker)) => picker.act(action),
            Some(Overlay::Directory(browser)) => browser.act(action),
            _ => return,
        };
        let path = match pick {
            open::Pick::Stay => return,
            open::Pick::Close => return self.view.overlay = None,
            open::Pick::Open(path) => path,
        };
        self.view.overlay = None;
        match open_pr::open_folder(&self.host, &path) {
            Ok(Opened::Selected(key)) => {
                self.load_if_new(key);
                let name = self.label_of(key);
                self.notify(format!("Switched to tab: {name}"));
            }
            Ok(Opened::Listed(key, start)) => {
                self.pending.push(startup::load(&self.host, key, &start));
                self.starts.insert(key, start);
                let name = self.label_of(key);
                self.notify(format!("Opened: {name}"));
            }
            Err(error) => self.notify(format!("Failed to open {path}: {error}")),
        }
    }

    /// Posts `key`'s stack lookup unless one is on the way: on the Open hub's
    /// first open while the PR section holds no stack, and on "Refresh
    /// stack" (`again`). Answers whether a lookup is on the way.
    fn read_stack(
        &mut self,
        key: TabKey,
        workspace: &str,
        content: &TabContent,
        again: bool,
    ) -> bool {
        if self.stacks_reading.contains(&key) {
            return true;
        }
        let wanted =
            er_service::stack_source_of(content).is_some() && (again || content.pr.stack.is_none());
        if wanted {
            self.stacks_reading.insert(key);
            self.post(
                key,
                workspace,
                Command::RefreshStack { key },
                After::StackRead,
            );
        }
        wanted
    }

    fn remote_url_key(&mut self, edit: Edit) {
        match modes::base_key(&mut self.view.base_input, edit) {
            BaseStep::Stay => {}
            BaseStep::Close => self.view.input_mode = Mode::Normal,
            BaseStep::Send(url) => {
                self.view.input_mode = Mode::Normal;
                self.open_pr(&url, "Failed");
            }
        }
    }

    /// Opens `url`'s PR as a tab, or selects the tab that holds it, with the
    /// App's notices; a failure says `failed` and why.
    fn open_pr(&mut self, url: &str, failed: &str) {
        match open_pr::open(&self.host, url) {
            Ok(Opened::Selected(key)) => {
                self.load_if_new(key);
                let name = self.label_of(key);
                self.notify(format!("Switched to tab: {name}"));
            }
            Ok(Opened::Listed(key, start)) => {
                self.pending.push(startup::load(&self.host, key, &start));
                self.starts.insert(key, start);
                let name = self.label_of(key);
                self.notify(format!("Opened remote: {name}"));
            }
            Err(error) => self.notify(format!("{failed}: {error}")),
        }
    }

    /// A pull landed: the PR section is read again, as the App's pull reads
    /// the overview, and the notice waits for that read. A working tab whose
    /// branch PR the probe did not number says only what the pull did.
    fn refresh_pr(
        &mut self,
        key: TabKey,
        workspace: &str,
        github_count: usize,
        local_count: usize,
    ) {
        let content = self.host.content(workspace, key);
        let command = match content {
            Some(content) if content.tab.pr_number.is_some() => Some(Command::RefreshDiff {
                key,
                force: true,
                adopt_head_oid: None,
            }),
            Some(content) => self
                .branch_prs
                .get(&key)
                .filter(|(_, branch)| *branch == content.tab.current_branch)
                .map(|&(number, _)| Command::LoadBranchPr { key, number }),
            None => None,
        };
        let Some(command) = command else {
            return self.notify(github::synced(github_count, local_count, false));
        };
        let after = After::PrRefreshed {
            github_count,
            local_count,
        };
        self.post(key, workspace, command, after);
    }

    /// The watcher follows the active tab and refreshes settled changes.
    fn sync_watch(&mut self, now: Instant) {
        let active = self.active();
        let shown = active
            .as_ref()
            .and_then(|(key, _, content)| Some((*key, &**content.as_ref()?)));
        // Changes already queued belong to the tab watched when they came.
        self.watch.count(now);
        self.watch.follow(shown, now);
        let Some((key, files)) = self.watch.ready(now) else {
            return;
        };
        let workspace = self.workspace_of(key);
        let content = workspace
            .as_ref()
            .and_then(|workspace| self.host.content(workspace, key));
        match (workspace, content) {
            (Some(workspace), Some(content)) => {
                self.refresh(key, &workspace, &content, Refresh::Watched { files });
            }
            _ => self.watch.landed(key),
        }
    }

    fn send_restore(&mut self) {
        let Some((key, workspace, Some(content))) = self.active() else {
            return;
        };
        let Some(state) = self.view.tab_mut_if_open(key) else {
            return;
        };
        let Some(restore) = state.restore.as_mut().filter(|restore| !restore.sent) else {
            return;
        };
        restore.sent = true;
        let mode = restore.mode;
        match modes::switch(key, &content, mode) {
            Some(command) => {
                state.follow = Some(Follow::of(state, &content));
                let after = After::ModeSwitched { pr_diff: false };
                self.post(key, &workspace, command, after);
            }
            _ => {
                if let Some(saved) = state.restore.take() {
                    super::session::restore_in_view(state, saved.session, &content);
                }
            }
        }
    }

    /// Runs a key on the review hub. A remove asks first, the summary opens
    /// its input, and a submit goes once.
    fn review_key(&mut self, action: OverlayAction) {
        let Some(hub) = self.view.review.as_mut() else {
            self.view.overlay = None;
            return;
        };
        match hub.act(action) {
            review_hub::HubStep::Stay => {}
            review_hub::HubStep::Close => self.close_review(),
            review_hub::HubStep::AskRemove => {
                self.view.overlay = None;
                self.view.input_mode = Mode::Confirm(Ask::RemoveReviewComment);
            }
            review_hub::HubStep::EditSummary => {
                self.view.overlay = None;
                self.view.input_mode = Mode::ReviewSummary;
            }
            review_hub::HubStep::Submit => {
                self.review_write(After::ReviewSubmit, |hub, content| hub.submit(content));
            }
        }
    }

    /// Sends a write of the open hub's review on its own tab, unless one is
    /// in flight there: GitHub holds one draft, so writes go one at a time.
    fn review_write(
        &mut self,
        after: After,
        command: impl FnOnce(&ReviewHub, &TabContent) -> Command,
    ) {
        let Some(hub) = self.view.review.as_ref() else {
            return;
        };
        let (key, workspace) = (hub.key, hub.workspace.clone());
        if self.writing(key, After::ReviewEdit) || self.writing(key, After::ReviewSubmit) {
            return;
        }
        let Some(content) = self.host.content(&workspace, key) else {
            return;
        };
        let command = command(hub, &content);
        self.post(key, &workspace, command, after);
    }

    /// Another write changed the review the open hub shows: it reads again,
    /// unless a read or a remove of its own is on the way.
    fn review_changed(&mut self, context: &er_service::TabContext) {
        let Some(hub) = self.view.review.as_ref().filter(|hub| hub.watches(context)) else {
            return;
        };
        let (key, workspace) = (hub.key, hub.workspace.clone());
        if self.writing(key, After::ReviewRead) || self.writing(key, After::ReviewEdit) {
            return;
        }
        if let Some(content) = self.host.content(&workspace, key) {
            self.post(
                key,
                &workspace,
                review_hub::read(&content),
                After::ReviewRead,
            );
        }
    }

    fn close_review(&mut self) {
        self.view.review = None;
        if matches!(self.view.overlay, Some(Overlay::Review)) {
            self.view.overlay = None;
        }
        if self.view.input_mode == Mode::ReviewSummary {
            self.view.input_mode = Mode::Normal;
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
            Some(Overlay::FlowProgress(progress)) => {
                let then = flows::progress_key(progress, action);
                return self.flow_then(key, then);
            }
            Some(Overlay::FlowResults(results)) => {
                let then = flows::results_key(results, action);
                return self.flow_then(key, then);
            }
            // `execute` routes each of these to its own handler.
            Some(
                Overlay::GitHub(_)
                | Overlay::Review
                | Overlay::Open(_)
                | Overlay::FilterHistory { .. }
                | Overlay::Copy(_)
                | Overlay::Help(_)
                | Overlay::Worktrees(_)
                | Overlay::Directory(_)
                | Overlay::ExportPicker { .. },
            )
            | None => return,
        };
        if let Step::Flow(act) = step {
            self.view.overlay = None;
            return self.flow_act(key, act);
        }
        if let Step::Layer(row) = step {
            self.view.overlay = None;
            let state = self.view.tab_mut(key, content);
            let message = layers::hub_row(state, row);
            return self.notify(message);
        }
        if let Step::Copy(pick) = step {
            self.view.overlay = None;
            return self.copy(pick, key, content);
        }
        if let Step::Clear(clear) = step {
            self.view.overlay = None;
            if let Some(ask) = clears::ask(clear, content, false) {
                self.view.input_mode = Mode::Confirm(ask);
            }
            return;
        }
        if let Step::Run(start) = step {
            self.view.overlay = None;
            return match ai_runs::next(start, content) {
                Next::Ask(rerun) => self.view.input_mode = Mode::Confirm(Ask::Rerun(rerun)),
                Next::Run(request) => self.start_run(key, request),
            };
        }
        self.run_step(step);
    }

    fn flow_then(&mut self, key: TabKey, then: FlowThen) {
        match then {
            FlowThen::Stay => {}
            FlowThen::Close => self.view.overlay = None,
            FlowThen::Notify(message) => self.notify(message.into()),
            FlowThen::Send {
                request,
                ask,
                close,
            } => {
                if close {
                    self.view.overlay = None;
                }
                self.flow_replies.push((ask, self.host.flow(key, request)));
            }
        }
    }

    fn run_step(&mut self, step: Step) {
        let settings = self.host.settings();
        let (close, send) = match step {
            Step::Stay => (false, None),
            Step::Close
            | Step::Layer(_)
            | Step::Clear(_)
            | Step::Copy(_)
            | Step::Run(_)
            | Step::Flow(_) => (true, None),
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
        };
        if close {
            self.view.overlay = None;
        }
        if let Some((command, then)) = send {
            self.settings.send(&settings, command, then);
        }
    }

    /// Runs a key on the Copy hub or the export picker.
    fn copy_overlay_key(&mut self, action: OverlayAction, key: TabKey, content: &TabContent) {
        match self.view.overlay.as_mut() {
            Some(Overlay::Copy(hub)) => match hub.act(action) {
                CopyStep::Stay => {}
                CopyStep::Close => self.view.overlay = None,
                CopyStep::Notify(message) => {
                    self.view.overlay = None;
                    self.notify(message);
                }
                CopyStep::Copy(pick) => {
                    self.view.overlay = None;
                    self.copy(pick, key, content);
                }
            },
            Some(overlay) => match export::act(overlay, action, content) {
                ExportStep::Stay => {}
                ExportStep::Close => self.view.overlay = None,
                ExportStep::Reopen(message) => {
                    self.view.overlay = Some(export::open());
                    self.notify(message.into());
                }
                ExportStep::Copy(copied) => {
                    self.view.overlay = None;
                    self.clipboard(Ok(copied));
                }
            },
            None => {}
        }
    }

    /// Copies from `key`'s tab as it stands at the press.
    fn copy(&mut self, pick: copy::Pick, key: TabKey, content: &TabContent) {
        let state = self.view.tab_mut(key, content);
        let built = copy::build(pick, state, content);
        self.clipboard(built);
    }

    /// Hands the text to the clipboard on the loop, as the App does; a
    /// failure says so and the loop goes on.
    fn clipboard(&mut self, built: er_engine::copy_text::Built) {
        let message = match built {
            Ok(copied) => match self.launch.clipboard(&copied.text) {
                Ok(()) => copied.notice,
                Err(error) => format!("{error:#}"),
            },
            Err(notice) => notice,
        };
        self.notify(message);
    }

    /// `e` with no comment of yours focused: the selected file opens in the
    /// editor at the current hunk's first new line, as the App's does.
    fn edit_file(
        &mut self,
        content: &TabContent,
        file: Option<&er_engine::git::DiffFile>,
        hunk: usize,
    ) {
        if content.tab.identity().is_remote() {
            return self.notify("Editor not available in remote mode".into());
        }
        let Some(file) = file else { return };
        if tui_view::is_stub(file) {
            return self.notify("File is still loading".into());
        }
        let line = file.hunks.get(hunk).map_or(1, |hunk| hunk.new_start);
        let root = &content.tab.repo_root;
        let path = std::path::Path::new(root).join(&file.path);
        if let Err(error) = self.launch.editor(root, &path, line) {
            self.notify(format!("{error:#}"));
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
            retry: None,
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
        self.load_if_new(key);
    }

    /// `x`: closes the active tab, as the App's `close_tab`. The last tab
    /// stays; the tab that moves into the closed one's place takes focus;
    /// the closed tab's content is dropped on its actor.
    fn close_tab(&mut self, key: TabKey, workspace: &str) {
        let tabs = self.host.registry().tabs_view();
        if tabs.tabs.len() <= 1 {
            return self.notify("Cannot close last tab".into());
        }
        let Some(at) = tabs.tabs.iter().position(|tab| tab.key == key) else {
            return;
        };
        let name = self.label_of(key);
        let registry = self.host.registry();
        if let Err(error) = registry.send(er_service::AppCommand::CloseTab { key }) {
            return self.notify(error.to_string());
        }
        let left = registry.tabs_view();
        let next = left.tabs.get(at.min(left.tabs.len().saturating_sub(1)));
        if let Some(next) = next.map(|entry| entry.key) {
            if let Err(error) = registry.send(er_service::AppCommand::SelectTab { key: next }) {
                self.notify(error.to_string());
            }
            self.load_if_new(next);
        }
        let started = self.host.content(workspace, key).is_some()
            || self.pending.iter().any(|pending| pending.key == key);
        if started {
            drop(self.host.post(workspace, Command::DropTab { key }));
        }
        self.pending.retain(|pending| pending.key != key);
        self.starts.remove(&key);
        self.load_errors.remove(&key);
        self.unsaved.remove(&key);
        self.view.forget(key);
        self.notify(format!("Closed: {name}"));
    }

    /// Loads `key`'s tab the first time it is shown.
    fn load_if_new(&mut self, key: TabKey) {
        let Some(workspace) = self.workspace_of(key) else {
            return;
        };
        let loaded = self.host.content(&workspace, key).is_some();
        let loading = self.pending.iter().any(|pending| pending.key == key);
        if let (false, false, Some(start_tab)) = (loaded, loading, self.starts.get(&key)) {
            self.load_errors.remove(&key);
            let pending = startup::load(&self.host, key, start_tab);
            self.pending.push(pending);
        }
    }

    /// `key`'s name in the tab strip.
    fn label_of(&self, key: TabKey) -> String {
        let (names, _, keys) = self.tab_names();
        let at = keys.iter().position(|listed| *listed == key);
        at.and_then(|at| names.get(at).cloned()).unwrap_or_default()
    }

    /// The tab strip's names, the active tab's place in it, and the keys.
    /// A loaded tab is named as the App names it; one not loaded yet keeps
    /// the registry's label until it is first shown.
    fn tab_names(&self) -> (Vec<String>, usize, Vec<TabKey>) {
        let tabs = self.host.registry().tabs_view();
        let rows = er_service::tabs_resource(&tabs);
        let names = tabs
            .tabs
            .iter()
            .zip(rows.tabs)
            .map(|(entry, row)| {
                let content = self
                    .host
                    .content(&entry.descriptor.workspace_key(), entry.key);
                content.map_or(row.label, |content| tui_view::tab_name(&content))
            })
            .collect();
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
        self.terminal_width = f.area().width;
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

/// A PR URL's `owner/repo`: a stack layer can live in another repo than
/// the tab, so its browser open names the URL's.
fn github_pr_slug(url: &str) -> Option<String> {
    let pr = er_engine::github::parse_github_pr_url(url)?;
    Some(format!("{}/{}", pr.owner, pr.repo))
}
