//! The settings actor: the one service-level owner of the global config.
//!
//! Config is app-scoped, so no workspace actor owns it: one settings actor per
//! process holds the config, applies every settings write as a command, and
//! publishes a [`SettingsView`] per revision, read the way a
//! [`crate::WorkspaceView`] is (ADR 0067, ADR 0069). The caller never picks it
//! from a tab.
//!
//! The config is loaded on the caller's thread before the loop starts, so no
//! command can apply to a default config and save it over the user's file.
//! Saves, risk-artifact reads, model discovery and uninstall run on effect
//! threads; the loop only mutates the config in memory and republishes.
//!
//! A write is published before its save lands. A save the file refuses rolls
//! the config back to what was last written, so once the reply arrives the
//! view matches the disk.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, RwLock};
use std::thread;
use std::time::Duration;

use er_engine::config::{
    adopt_config_from, apply_config_field, config_hub_view, config_pair_load_banner,
    config_pair_load_failed, hub_edits, normalize_effort, overlay_discovered_models,
    risk_rules_view, save_config_pair_to, AiSelection, ConfigFieldValue, ConfigHubView,
    ConfigLoadError, ErConfig, ModelUpsert, ProviderUpsert, ReviewModelGroups, RiskFilesView,
    RiskRuleTable, RiskRulesView,
};
use er_engine::model_discovery::DiscoveredModel;
use er_engine::uninstall::{UninstallOptions, UninstallPreview, UninstallResult};

use crate::actor::WorkspaceDeps;
use crate::guard::ActorThread;
use crate::handle::{WorkspaceError, REPLY_TIMEOUT};
use crate::protocol::Event;

/// A settings write or read the actor runs. Every parameter travels with it.
#[derive(Clone, Debug)]
pub enum SettingsCommand {
    /// Write one config field through `apply_config_field`, then save.
    ApplyConfigPatch {
        key: String,
        value: ConfigFieldValue,
    },
    /// Save the current config as it is.
    SaveConfig,
    SetReviewFlowModels {
        groups: ReviewModelGroups,
    },
    /// The palette's or AI Settings' default provider and model, resolved
    /// against the actor's config. Answers `AiSelectionSaved`.
    SetDefaultAiSelection {
        provider_id: String,
        model_id: Option<String>,
        effort: Option<String>,
    },
    /// The default effort, normalized for the default selection. `None` is
    /// Auto. Answers `AiSelectionSaved`.
    SetDefaultAiEffort {
        effort: Option<String>,
    },
    /// List models from provider CLIs and overlay them; never saved.
    RefreshAiModels {
        provider_id: Option<String>,
        force: bool,
    },
    UpsertAiProvider {
        provider: ProviderUpsert,
    },
    DeleteAiProvider {
        provider_id: String,
    },
    UpsertAiModel {
        provider_id: String,
        model: ModelUpsert,
    },
    DeleteAiModel {
        provider_id: String,
        model_id: String,
    },
    /// List what an uninstall would remove into the view.
    PreviewUninstall {
        options: UninstallOptions,
    },
    RunUninstall {
        options: UninstallOptions,
    },
    /// Read one repo's committed risk rules and publish its view, with
    /// `paths` resolved against them. The rules key is resolved off-actor,
    /// since it asks the clone for its remote.
    LoadRiskRules {
        repo_root: String,
        /// `owner/repo`, only for a target with no clone (`storage::rules_key`).
        remote_repo: Option<String>,
        /// The caller's changed paths. The last load's paths are the ones
        /// the view resolves, so a read never needs the caller's diff.
        paths: Vec<String>,
    },
    /// Settings reopened: adopt a config file repaired after a failed load.
    ReloadIfFailed,
}

/// The settings actor's answer to one command.
#[derive(Clone, Debug)]
pub enum SettingsReply {
    /// The config changed (and, for a write, is saved) at `revision`.
    ConfigChanged {
        revision: u64,
        /// The editor's soft validation messages (e.g. a missing `{prompt}`).
        warnings: Vec<String>,
    },
    /// The default AI selection is saved; the caller's session follows it.
    AiSelectionSaved {
        revision: u64,
        selection: AiSelection,
    },
    ModelsRefreshed {
        revision: u64,
        /// One line per provider whose listing failed; its prior cache applies.
        errors: Vec<String>,
    },
    UninstallPreviewed(UninstallPreview),
    Uninstalled(UninstallResult),
    /// The repo's risk view is published under `key`.
    RiskRulesLoaded {
        key: String,
        version: u64,
    },
}

/// What a reader sees, published per revision.
#[derive(Clone, Debug)]
pub struct SettingsView {
    pub revision: u64,
    pub config: Arc<ErConfig>,
    /// The Settings page's rows and provider lists, built from `config`.
    pub hub: ConfigHubView,
    /// Per-repo risk rules, by rules key. Each carries its own version, which
    /// rises only when that repo's rules change.
    pub risk: BTreeMap<String, RiskRulesView>,
    /// The last loaded paths of each rules key, resolved against its rules.
    /// It moves with its rules' version.
    pub risk_files: BTreeMap<String, RiskFilesView>,
    /// The rules key each loaded `(repo_root, remote_repo)` resolved to.
    pub risk_keys: BTreeMap<(String, Option<String>), String>,
    pub uninstall_preview: Option<UninstallPreview>,
    /// Why the config file failed to load; saves refuse until it reloads.
    pub load_error: Option<String>,
    /// The one-line banner a front end keeps up while saves refuse: the
    /// failed file's banner from a load, or the generic one when a save
    /// finds a file broken mid-session.
    pub load_banner: Option<String>,
    /// The last command failure, cleared by the next success.
    pub last_error: Option<String>,
}

impl SettingsView {
    /// The risk view a caller holding only the repo's root and remote reads.
    pub fn risk_for(&self, repo_root: &str, remote_repo: Option<&str>) -> Option<&RiskRulesView> {
        let key = self
            .risk_keys
            .get(&(repo_root.to_string(), remote_repo.map(str::to_string)))?;
        self.risk.get(key)
    }
}

/// A handle to the settings actor. Cheap to clone; never owns the state.
#[derive(Clone)]
pub struct SettingsHandle {
    tx: Sender<SettingsMsg>,
    subscribers: Arc<Mutex<Vec<Sender<Event>>>>,
    view: Arc<RwLock<Arc<SettingsView>>>,
}

type SettingsReplySender = Sender<Result<SettingsReply, WorkspaceError>>;

enum SettingsMsg {
    Command {
        command: SettingsCommand,
        reply: SettingsReplySender,
    },
    EffectDone {
        outcome: Result<Outcome, WorkspaceError>,
        reply: SettingsReplySender,
    },
}

/// An effect's typed result, applied on the actor thread.
enum Outcome {
    Saved {
        warnings: Vec<String>,
        selection: Option<AiSelection>,
    },
    /// The file refused the save, or an earlier refusal dropped this write.
    SaveRefused {
        message: String,
    },
    ModelsDiscovered {
        found: Vec<(String, Vec<DiscoveredModel>)>,
        attempted: Vec<String>,
        errors: Vec<String>,
    },
    UninstallPreviewed(UninstallPreview),
    Uninstalled(UninstallResult),
    RiskLoaded {
        repo_root: String,
        remote_repo: Option<String>,
        key: String,
        artifact: Option<RiskRuleTable>,
        paths: Vec<String>,
    },
    Reloaded {
        config: Box<ErConfig>,
        load_error: Option<ConfigLoadError>,
    },
}

impl SettingsHandle {
    /// Sends a command and returns the reply channel immediately.
    pub fn send(
        &self,
        command: SettingsCommand,
    ) -> Receiver<Result<SettingsReply, WorkspaceError>> {
        let (reply, replies) = mpsc::channel();
        let _ = self.tx.send(SettingsMsg::Command { command, reply });
        replies
    }

    /// Sends a command and waits up to [`REPLY_TIMEOUT`].
    pub fn send_and_wait(&self, command: SettingsCommand) -> Result<SettingsReply, WorkspaceError> {
        self.send_and_wait_with(command, REPLY_TIMEOUT)
    }

    /// Sends a command and waits up to `timeout`; the command keeps running.
    pub fn send_and_wait_with(
        &self,
        command: SettingsCommand,
        timeout: Duration,
    ) -> Result<SettingsReply, WorkspaceError> {
        match self.send(command).recv_timeout(timeout) {
            Ok(result) => result,
            Err(RecvTimeoutError::Timeout) => Err(WorkspaceError::Timeout(timeout)),
            Err(RecvTimeoutError::Disconnected) => Err(WorkspaceError::Disconnected),
        }
    }

    /// Subscribes to `ViewPublished` and `CommandFailed` from now on.
    pub fn subscribe(&self) -> Receiver<Event> {
        let (tx, rx) = mpsc::channel();
        if let Ok(mut subscribers) = self.subscribers.lock() {
            subscribers.push(tx);
        }
        rx
    }

    /// The last published view. Never blocks on the actor.
    pub fn view(&self) -> Arc<SettingsView> {
        match self.view.read() {
            Ok(view) => Arc::clone(&view),
            Err(poisoned) => Arc::clone(&poisoned.into_inner()),
        }
    }
}

/// Loads the config at `config_path` on the calling thread, then starts the
/// actor that owns it. `config_path` is `config.toml`; `agents.toml` sits
/// beside it.
pub fn spawn_settings(deps: WorkspaceDeps, config_path: PathBuf) -> SettingsHandle {
    let (mut config, error) = adopt_config_from(&config_path);
    let saves = Arc::new(SaveSlot::new(&config));
    overlay_cached_models(&mut config);
    let state = SettingsState {
        config,
        config_path,
        revision: 0,
        artifacts: BTreeMap::new(),
        risk: BTreeMap::new(),
        risk_paths: BTreeMap::new(),
        risk_files: BTreeMap::new(),
        risk_keys: BTreeMap::new(),
        uninstall_preview: None,
        load_banner: error.as_ref().map(ConfigLoadError::banner),
        load_error: error.map(|e| e.to_string()),
        last_error: None,
        discovery_inflight: HashSet::new(),
        saves,
    };
    let (tx, rx) = mpsc::channel();
    let subscribers = Arc::new(Mutex::new(Vec::new()));
    let view = Arc::new(RwLock::new(Arc::new(state.publishable())));
    let handle = SettingsHandle {
        tx: tx.clone(),
        subscribers: Arc::clone(&subscribers),
        view: Arc::clone(&view),
    };
    #[expect(
        clippy::expect_used,
        reason = "the actor thread is process setup: if it cannot start, no settings command can run"
    )]
    let _ = thread::Builder::new()
        .name("er-settings-actor".into())
        .spawn(move || run(deps, state, tx, rx, subscribers, view))
        .expect("spawn the settings actor thread");
    handle
}

/// Overlays each provider's cached discovered models, as the App does at
/// start. It runs no discovery: a provider with no cache keeps its presets
/// until a refresh.
fn overlay_cached_models(config: &mut ErConfig) {
    if !config.features.model_discovery {
        return;
    }
    for (id, provider) in &mut config.ai_hub.providers {
        if provider.models_command.is_empty() {
            continue;
        }
        let cache = er_engine::model_discovery::load_valid_cache(id, &provider.models_command);
        if let Some(cache) = cache {
            overlay_discovered_models(provider, &cache.models);
        }
    }
}

/// The latest config to save, and what is on disk. Effects write whatever is
/// latest under the lock, so two saves finishing out of order still leave the
/// newest config on disk.
pub(crate) struct SaveSlot {
    latest: Mutex<Option<(u64, ErConfig)>>,
    written: Mutex<Written>,
}

/// The config on disk and the generation that wrote it.
struct Written {
    generation: u64,
    config: ErConfig,
    /// Generation ranges `(on disk, staged]` a rollback dropped unwritten.
    dropped: Vec<(u64, u64)>,
}

impl SaveSlot {
    pub(crate) fn new(loaded: &ErConfig) -> Self {
        Self {
            latest: Mutex::new(None),
            written: Mutex::new(Written {
                generation: 0,
                config: loaded.clone(),
                dropped: Vec::new(),
            }),
        }
    }

    pub(crate) fn stage(&self, generation: u64, config: &ErConfig) {
        if let Ok(mut latest) = self.latest.lock() {
            *latest = Some((generation, config.clone()));
        }
    }

    /// Writes the latest staged config unless a newer one is already written,
    /// then says whether `generation`'s write is on disk.
    pub(crate) fn write_latest(&self, path: &Path, generation: u64) -> Result<(), String> {
        let mut written = self
            .written
            .lock()
            .map_err(|_| "the save lock is poisoned".to_string())?;
        let latest = self.latest.lock().ok().and_then(|slot| slot.clone());
        if let Some((staged, config)) = latest {
            if staged > written.generation {
                save_config_pair_to(path, &config).map_err(|e| e.to_string())?;
                written.generation = staged;
                written.config = config;
            }
        }
        let dropped = written
            .dropped
            .iter()
            .any(|&(on_disk, staged)| generation > on_disk && generation <= staged);
        if dropped || written.generation < generation {
            return Err("An earlier settings save was refused, so this change was undone.".into());
        }
        Ok(())
    }

    /// Drops every staged write through `through` and returns the config on
    /// disk, replaced first by `reloaded` when the file was just re-read.
    pub(crate) fn reset(&self, through: u64, reloaded: Option<&ErConfig>) -> Option<ErConfig> {
        let mut written = self.written.lock().ok()?;
        if let Ok(mut latest) = self.latest.lock() {
            *latest = None;
        }
        let on_disk = written.generation;
        written.dropped.push((on_disk, through));
        if let Some(config) = reloaded {
            written.config = config.clone();
        }
        Some(written.config.clone())
    }
}

struct SettingsState {
    config: ErConfig,
    config_path: PathBuf,
    revision: u64,
    /// The committed risk table per rules key, as last read.
    artifacts: BTreeMap<String, Option<RiskRuleTable>>,
    risk: BTreeMap<String, RiskRulesView>,
    risk_paths: BTreeMap<String, Vec<String>>,
    risk_files: BTreeMap<String, RiskFilesView>,
    risk_keys: BTreeMap<(String, Option<String>), String>,
    uninstall_preview: Option<UninstallPreview>,
    load_error: Option<String>,
    load_banner: Option<String>,
    last_error: Option<String>,
    discovery_inflight: HashSet<String>,
    saves: Arc<SaveSlot>,
}

impl SettingsState {
    fn publishable(&self) -> SettingsView {
        SettingsView {
            revision: self.revision,
            config: Arc::new(self.config.clone()),
            hub: config_hub_view(&self.config),
            risk: self.risk.clone(),
            risk_files: self.risk_files.clone(),
            risk_keys: self.risk_keys.clone(),
            uninstall_preview: self.uninstall_preview.clone(),
            load_error: self.load_error.clone(),
            load_banner: self.load_banner.clone(),
            last_error: self.last_error.clone(),
        }
    }

    /// Rebuilds one repo's risk view from the actor's config; its version
    /// rises only when the rules it shows, or the paths it resolves, changed.
    fn rebuild_risk(&mut self, key: &str, paths_moved: bool) {
        let artifact = self.artifacts.get(key).cloned().flatten();
        let mut next = risk_rules_view(&self.config, key, artifact);
        match self.risk.get(key) {
            Some(current) if !paths_moved && current.same_rules(&next) => return,
            Some(current) => next.version = current.version + 1,
            None => next.version = 1,
        }
        let paths = self.risk_paths.get(key).map_or(&[][..], Vec::as_slice);
        self.risk_files.insert(key.to_string(), next.resolve(paths));
        self.risk.insert(key.to_string(), next);
    }

    fn rebuild_all_risk(&mut self) {
        let keys: Vec<String> = self.artifacts.keys().cloned().collect();
        for key in keys {
            self.rebuild_risk(&key, false);
        }
    }

    /// Raises the revision and stages the current config as the next save.
    fn stage_save(&mut self) {
        self.revision += 1;
        self.saves.stage(self.revision, &self.config);
    }
}

fn run(
    deps: WorkspaceDeps,
    mut state: SettingsState,
    tx: Sender<SettingsMsg>,
    rx: Receiver<SettingsMsg>,
    subscribers: Arc<Mutex<Vec<Sender<Event>>>>,
    view: Arc<RwLock<Arc<SettingsView>>>,
) {
    let _actor = ActorThread::enter();
    while let Ok(message) = rx.recv() {
        match message {
            SettingsMsg::Command { command, reply } => {
                handle_command(&deps, &mut state, &tx, command, reply, &subscribers, &view);
            }
            SettingsMsg::EffectDone { outcome, reply } => {
                apply(&mut state, outcome, reply, &subscribers, &view);
            }
        }
    }
}

fn spawn_effect(
    tx: &Sender<SettingsMsg>,
    reply: SettingsReplySender,
    work: impl FnOnce() -> Result<Outcome, WorkspaceError> + Send + 'static,
) {
    let tx = tx.clone();
    // Unnamed on purpose, like the workspace actor's effects: the storage
    // guard lets only unnamed worker threads use a process-wide test root.
    let _ = thread::Builder::new().spawn(move || {
        let outcome = work();
        let _ = tx.send(SettingsMsg::EffectDone { outcome, reply });
    });
}

/// Publishes the in-memory change, then saves it off-actor and answers when
/// the save lands.
fn write_and_save(
    state: &mut SettingsState,
    tx: &Sender<SettingsMsg>,
    reply: SettingsReplySender,
    saved: Outcome,
    subscribers: &Mutex<Vec<Sender<Event>>>,
    view: &RwLock<Arc<SettingsView>>,
) {
    state.rebuild_all_risk();
    state.stage_save();
    publish(view, state);
    broadcast(
        subscribers,
        Event::ViewPublished {
            revision: state.revision,
        },
    );
    let saves = Arc::clone(&state.saves);
    let path = state.config_path.clone();
    let generation = state.revision;
    spawn_effect(tx, reply, move || {
        Ok(match saves.write_latest(&path, generation) {
            Ok(()) => saved,
            Err(message) => Outcome::SaveRefused { message },
        })
    });
}

/// Runs a default-selection edit on a copy and saves it when it resolves.
fn save_selection(
    state: &mut SettingsState,
    tx: &Sender<SettingsMsg>,
    reply: SettingsReplySender,
    subscribers: &Mutex<Vec<Sender<Event>>>,
    view: &RwLock<Arc<SettingsView>>,
    change: impl FnOnce(&mut ErConfig) -> Result<AiSelection, String>,
) {
    let mut next = state.config.clone();
    match change(&mut next) {
        Ok(selection) => {
            state.config = next;
            let saved = Outcome::Saved {
                warnings: Vec::new(),
                selection: Some(selection),
            };
            write_and_save(state, tx, reply, saved, subscribers, view);
        }
        Err(message) => {
            let _ = reply.send(Err(WorkspaceError::Failed(message)));
        }
    }
}

/// Dispatches one command on the actor thread. It edits the config in memory
/// and never touches a port or the disk; its effects do.
fn handle_command(
    deps: &WorkspaceDeps,
    state: &mut SettingsState,
    tx: &Sender<SettingsMsg>,
    command: SettingsCommand,
    reply: SettingsReplySender,
    subscribers: &Mutex<Vec<Sender<Event>>>,
    view: &RwLock<Arc<SettingsView>>,
) {
    let edit = |state: &mut SettingsState,
                change: &dyn Fn(&mut ErConfig) -> Result<Vec<String>, String>| {
        let mut next = state.config.clone();
        let warnings = change(&mut next)?;
        state.config = next;
        Ok(warnings)
    };
    let warnings = match command {
        SettingsCommand::ApplyConfigPatch { key, value } => edit(state, &|config| {
            apply_config_field(config, &key, value.clone());
            Ok(Vec::new())
        }),
        SettingsCommand::SaveConfig => Ok(Vec::new()),
        SettingsCommand::SetReviewFlowModels { groups } => edit(state, &|config| {
            config
                .ai_hub
                .validate_review_models(&groups)
                .map_err(|e| e.to_string())?;
            config.ai_hub.review_models = groups.clone();
            Ok(Vec::new())
        }),
        SettingsCommand::SetDefaultAiSelection {
            provider_id,
            model_id,
            effort,
        } => {
            return save_selection(state, tx, reply, subscribers, view, |config| {
                let agent = config.agent.clone();
                config
                    .ai_hub
                    .set_default_selection_with_effort(
                        &provider_id,
                        model_id.as_deref(),
                        effort.as_deref(),
                        &agent,
                    )
                    .map_err(|e| e.to_string())
            });
        }
        SettingsCommand::SetDefaultAiEffort { effort } => {
            return save_selection(state, tx, reply, subscribers, view, |config| {
                let default = config.ai_hub.resolve_default_selection(&config.agent);
                let normalized = normalize_effort(
                    &config.ai_hub,
                    default.provider_id.as_deref(),
                    default.model_id.as_deref(),
                    effort.as_deref(),
                );
                if effort.is_some() && normalized.is_none() {
                    return Err("Effort is unsupported for the selected model".into());
                }
                config.ai_hub.default_effort.clone_from(&normalized);
                Ok(AiSelection {
                    effort: normalized,
                    ..default
                })
            });
        }
        SettingsCommand::UpsertAiProvider { provider } => edit(state, &|config| {
            hub_edits::upsert_provider(config, provider.clone()).map(|edit| edit.warnings)
        }),
        SettingsCommand::DeleteAiProvider { provider_id } => edit(state, &|config| {
            hub_edits::delete_provider(config, &provider_id).map(|()| Vec::new())
        }),
        SettingsCommand::UpsertAiModel { provider_id, model } => edit(state, &|config| {
            hub_edits::upsert_model(config, &provider_id, model.clone()).map(|edit| edit.warnings)
        }),
        SettingsCommand::DeleteAiModel {
            provider_id,
            model_id,
        } => edit(state, &|config| {
            hub_edits::delete_model(config, &provider_id, &model_id).map(|()| Vec::new())
        }),
        SettingsCommand::RefreshAiModels { provider_id, force } => {
            return refresh_models(state, tx, reply, provider_id, force);
        }
        SettingsCommand::PreviewUninstall { options } => {
            return spawn_effect(tx, reply, move || {
                Ok(Outcome::UninstallPreviewed(er_engine::uninstall::preview(
                    &options,
                )))
            });
        }
        SettingsCommand::RunUninstall { options } => {
            return spawn_effect(tx, reply, move || {
                er_engine::uninstall::run(&options)
                    .map(Outcome::Uninstalled)
                    .map_err(WorkspaceError::Failed)
            });
        }
        SettingsCommand::LoadRiskRules {
            repo_root,
            remote_repo,
            paths,
        } => {
            let deps = deps.clone();
            return spawn_effect(tx, reply, move || {
                let key = er_engine::storage::rules_key(
                    &*deps.ports().git,
                    &repo_root,
                    remote_repo.as_deref(),
                );
                let artifact = er_engine::config::load_risk_rules_table(&key);
                Ok(Outcome::RiskLoaded {
                    repo_root,
                    remote_repo,
                    key,
                    artifact,
                    paths,
                })
            });
        }
        SettingsCommand::ReloadIfFailed => {
            // A save that finds a file broken mid-session records only the
            // engine's mark, so `load_error` alone would skip the reload and
            // leave every later save refused.
            if state.load_error.is_none() && !config_pair_load_failed(&state.config_path) {
                let _ = reply.send(Ok(SettingsReply::ConfigChanged {
                    revision: state.revision,
                    warnings: Vec::new(),
                }));
                return;
            }
            let path = state.config_path.clone();
            return spawn_effect(tx, reply, move || {
                let (config, error) = adopt_config_from(&path);
                Ok(Outcome::Reloaded {
                    config: Box::new(config),
                    load_error: error,
                })
            });
        }
    };
    match warnings {
        Ok(warnings) => {
            let saved = Outcome::Saved {
                warnings,
                selection: None,
            };
            write_and_save(state, tx, reply, saved, subscribers, view);
        }
        // A refused edit changes nothing, so nothing is published or saved.
        Err(message) => {
            let _ = reply.send(Err(WorkspaceError::Failed(message)));
        }
    }
}

fn refresh_models(
    state: &mut SettingsState,
    tx: &Sender<SettingsMsg>,
    reply: SettingsReplySender,
    provider_id: Option<String>,
    force: bool,
) {
    if !state.config.features.model_discovery {
        let _ = reply.send(Ok(SettingsReply::ModelsRefreshed {
            revision: state.revision,
            errors: vec!["Model discovery is turned off in Settings → AI".into()],
        }));
        return;
    }
    let targets: Vec<(String, Vec<String>)> = state
        .config
        .ai_hub
        .providers
        .iter()
        .filter(|(id, p)| {
            !p.models_command.is_empty()
                && provider_id
                    .as_deref()
                    .is_none_or(|want| want == id.as_str())
                && !state.discovery_inflight.contains(id.as_str())
        })
        .map(|(id, p)| (id.clone(), p.models_command.clone()))
        .collect();
    state
        .discovery_inflight
        .extend(targets.iter().map(|(id, _)| id.clone()));
    spawn_effect(tx, reply, move || {
        let mut found = Vec::new();
        let mut errors = Vec::new();
        let attempted = targets.iter().map(|(id, _)| id.clone()).collect();
        for (pid, command) in targets {
            let cache = er_engine::model_discovery::load_valid_cache(&pid, &command);
            let fresh = cache
                .as_ref()
                .is_some_and(er_engine::model_discovery::cache_is_fresh);
            if fresh && !force {
                if let Some(cache) = cache {
                    found.push((pid, cache.models));
                }
                continue;
            }
            match er_engine::model_discovery::discover_and_cache(&pid, &command) {
                Ok(models) => found.push((pid, models)),
                Err(e) => {
                    // Stale-while-revalidate: keep serving the prior cache.
                    if let Some(cache) = cache {
                        found.push((pid.clone(), cache.models));
                    }
                    errors.push(format!("{pid}: {e:#}"));
                }
            }
        }
        Ok(Outcome::ModelsDiscovered {
            found,
            attempted,
            errors,
        })
    });
}

/// Applies an effect's result on the actor thread and answers the caller.
fn apply(
    state: &mut SettingsState,
    outcome: Result<Outcome, WorkspaceError>,
    reply: SettingsReplySender,
    subscribers: &Mutex<Vec<Sender<Event>>>,
    view: &RwLock<Arc<SettingsView>>,
) {
    let answer = match outcome {
        Ok(Outcome::Saved {
            warnings,
            selection,
        }) => {
            state.last_error = None;
            // The view already carries the change; the save adds nothing to
            // show, so the revision stays.
            let revision = state.revision;
            let _ = reply.send(Ok(match selection {
                Some(selection) => SettingsReply::AiSelectionSaved {
                    revision,
                    selection,
                },
                None => SettingsReply::ConfigChanged { revision, warnings },
            }));
            return;
        }
        Ok(Outcome::SaveRefused { message }) => {
            // The view must match the disk, so the refused edit, and any
            // write staged after it, is undone. Discovered models added since
            // the last save drop with it until the next refresh.
            if let Some(on_disk) = state.saves.reset(state.revision, None) {
                state.config = on_disk;
                state.rebuild_all_risk();
            }
            if let Some(banner) = config_pair_load_banner(&state.config_path) {
                state.load_banner = Some(banner);
            }
            state.last_error = Some(message.clone());
            state.revision += 1;
            Err(WorkspaceError::Failed(message))
        }
        Ok(Outcome::ModelsDiscovered {
            found,
            attempted,
            errors,
        }) => {
            for pid in attempted {
                state.discovery_inflight.remove(&pid);
            }
            for (pid, models) in found {
                if let Some(provider) = state.config.ai_hub.providers.get_mut(&pid) {
                    overlay_discovered_models(provider, &models);
                }
            }
            state.revision += 1;
            Ok(SettingsReply::ModelsRefreshed {
                revision: state.revision,
                errors,
            })
        }
        Ok(Outcome::UninstallPreviewed(preview)) => {
            state.uninstall_preview = Some(preview.clone());
            state.revision += 1;
            Ok(SettingsReply::UninstallPreviewed(preview))
        }
        Ok(Outcome::Uninstalled(result)) => Ok(SettingsReply::Uninstalled(result)),
        Ok(Outcome::RiskLoaded {
            repo_root,
            remote_repo,
            key,
            artifact,
            paths,
        }) => {
            state.artifacts.insert(key.clone(), artifact);
            state
                .risk_keys
                .insert((repo_root, remote_repo), key.clone());
            let paths_moved = state.risk_paths.get(&key) != Some(&paths);
            state.risk_paths.insert(key.clone(), paths);
            state.rebuild_risk(&key, paths_moved);
            state.revision += 1;
            let version = state.risk.get(&key).map_or(0, |view| view.version);
            Ok(SettingsReply::RiskRulesLoaded { key, version })
        }
        Ok(Outcome::Reloaded { config, load_error }) => {
            if load_error.is_none() {
                state.saves.reset(state.revision, Some(&*config));
                state.config = *config;
                state.rebuild_all_risk();
            }
            state.load_banner = load_error.as_ref().map(ConfigLoadError::banner);
            state.load_error = load_error.map(|e| e.to_string());
            state.revision += 1;
            Ok(SettingsReply::ConfigChanged {
                revision: state.revision,
                warnings: Vec::new(),
            })
        }
        Err(error) => {
            state.last_error = Some(error.to_string());
            state.revision += 1;
            Err(error)
        }
    };
    publish(view, state);
    broadcast(
        subscribers,
        Event::ViewPublished {
            revision: state.revision,
        },
    );
    if let Err(error) = &answer {
        broadcast(
            subscribers,
            Event::CommandFailed {
                message: error.to_string(),
            },
        );
    }
    let _ = reply.send(answer);
}

fn publish(view: &RwLock<Arc<SettingsView>>, state: &SettingsState) {
    let next = Arc::new(state.publishable());
    match view.write() {
        Ok(mut current) => *current = next,
        Err(poisoned) => *poisoned.into_inner() = next,
    }
}

fn broadcast(subscribers: &Mutex<Vec<Sender<Event>>>, event: Event) {
    if let Ok(mut subscribers) = subscribers.lock() {
        subscribers.retain(|sender| sender.send(event.clone()).is_ok());
    }
}
