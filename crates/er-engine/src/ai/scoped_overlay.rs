//! Per-lens scoped re-review overlays and the host stamps that make them
//! trustworthy.
//!
//! A scoped run writes only `scoped/general.json`, `scoped/professor.json` or
//! `scoped/experts/<id>.json`; the full review's files stay untouched. The
//! loader applies a fresh overlay to its lens before projection, and replies
//! and resolves find their row in that overlay.
//!
//! The agent writes the payload; the host stamps `scope_paths`, the base file's
//! identity and a completion marker when the run exits. Freshness is checked by
//! the projection/loader, never by the store (#401): an overlay is applied only
//! while its captured base identity still matches.

use super::experts::{expert_by_id, expert_id_for_id_prefix, ExpertFileReview, ExpertReview};
use super::finding_responses::apply_to_findings;
use super::identity::finding_key;
use super::professor::{ProfessorFileReview, ProfessorReview, PROFESSOR_ID_PREFIX};
use super::review::{ErFileReview, ErReview, Finding};
use super::validation::ValidationReview;
use er_store::{Artifact, ArtifactKey, ArtifactStore, ExpertId, Mode, Scope};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ScopedReview {
    pub version: u32,
    #[serde(default)]
    pub diff_hash: String,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub base_branch: String,
    #[serde(default)]
    pub head_branch: String,
    #[serde(default)]
    pub files: HashMap<String, ErFileReview>,
    #[serde(default)]
    pub file_hashes: HashMap<String, String>,
    #[serde(default)]
    pub scope_paths: Vec<String>,
    /// The base file's `diff_hash` and `created_at` at run start.
    #[serde(default)]
    pub base_diff_hash: String,
    #[serde(default)]
    pub base_created_at: String,
    /// A completed-run marker, set only by `stamp_scoped_overlay`. It tells a
    /// finished run from a crash leftover; it is not a forgery guard.
    #[serde(default)]
    pub host_stamped: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ScopedProfessorReview {
    pub version: u32,
    #[serde(default)]
    pub diff_hash: String,
    #[serde(default)]
    pub diff_scope: String,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub focus_prompt: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub files: HashMap<String, ProfessorFileReview>,
    #[serde(default)]
    pub scope_paths: Vec<String>,
    #[serde(default)]
    pub base_diff_hash: String,
    #[serde(default)]
    pub base_created_at: String,
    #[serde(default)]
    pub host_stamped: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ScopedExpertReview {
    pub version: u32,
    pub expert_id: String,
    #[serde(default)]
    pub diff_hash: String,
    #[serde(default)]
    pub diff_scope: String,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub files: HashMap<String, ExpertFileReview>,
    #[serde(default)]
    pub scope_paths: Vec<String>,
    #[serde(default)]
    pub base_diff_hash: String,
    #[serde(default)]
    pub base_created_at: String,
    #[serde(default)]
    pub host_stamped: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScopedRunStamp {
    /// Host-written from `review-files.txt`, sorted and de-duplicated.
    pub scope_paths: Vec<String>,
    /// The lens's base file identity captured before the agent starts.
    pub base_diff_hash: String,
    pub base_created_at: String,
    /// The overlay bytes at spawn, for the second-run merge and the unchanged check.
    pub previous_overlay: Option<Vec<u8>>,
}

impl Artifact for ScopedReview {
    const KIND: &'static str = "scoped-general";
    const VERSION: u32 = 1;
    const MODE: Mode = Mode::Bare;
    const FILE: &'static str = "scoped/general.json";
    type Slot = ();

    fn diff_hash(&self) -> Option<&str> {
        Some(&self.diff_hash)
    }
}

impl Artifact for ScopedProfessorReview {
    const KIND: &'static str = "scoped-professor";
    const VERSION: u32 = 1;
    const MODE: Mode = Mode::Bare;
    const FILE: &'static str = "scoped/professor.json";
    type Slot = ();

    fn diff_hash(&self) -> Option<&str> {
        Some(&self.diff_hash)
    }
}

impl Artifact for ScopedExpertReview {
    const KIND: &'static str = "scoped-expert";
    const VERSION: u32 = 1;
    const MODE: Mode = Mode::Bare;
    const FILE: &'static str = "scoped/experts/{slot}.json";
    type Slot = ExpertId;

    fn diff_hash(&self) -> Option<&str> {
        Some(&self.diff_hash)
    }
}

/// `review` -> `scoped/general.json`, `professor` -> `scoped/professor.json`,
/// `expert-<id>` -> `scoped/experts/<id>.json`; other names -> None.
pub fn scoped_overlay_path(command_name: &str) -> Option<String> {
    match command_name {
        "review" => Some("scoped/general.json".to_string()),
        "professor" => Some("scoped/professor.json".to_string()),
        _ => command_name
            .strip_prefix("expert-")
            .filter(|id| ExpertId::new(id).is_ok())
            .map(|id| format!("scoped/experts/{id}.json")),
    }
}

fn scope_set(paths: &[String]) -> HashSet<&str> {
    paths.iter().map(String::as_str).collect()
}

/// An overlay applies only when the host completed the run, it names paths,
/// and the base's identity is still the one captured at spawn.
fn identity_matches(
    overlay_base_diff_hash: &str,
    overlay_base_created_at: &str,
    base: Option<(&str, &str)>,
) -> bool {
    match base {
        Some((diff_hash, created_at)) => {
            overlay_base_diff_hash == diff_hash && overlay_base_created_at == created_at
        }
        None => overlay_base_diff_hash.is_empty() && overlay_base_created_at.is_empty(),
    }
}

pub fn apply_scoped_review(base: &mut Option<ErReview>, overlay: &ScopedReview) -> bool {
    if !overlay.host_stamped || overlay.scope_paths.is_empty() {
        return false;
    }
    let identity = base
        .as_ref()
        .map(|review| (review.diff_hash.as_str(), review.created_at.as_str()));
    if !identity_matches(&overlay.base_diff_hash, &overlay.base_created_at, identity) {
        return false;
    }
    let scope = scope_set(&overlay.scope_paths);
    match base {
        Some(review) => {
            for path in &scope {
                review.files.remove(*path);
                review.file_hashes.remove(*path);
            }
            for (path, file) in &overlay.files {
                if scope.contains(path.as_str()) {
                    review.files.insert(path.clone(), file.clone());
                }
            }
            for (path, hash) in &overlay.file_hashes {
                if scope.contains(path.as_str()) {
                    review.file_hashes.insert(path.clone(), hash.clone());
                }
            }
            if !overlay.base_branch.is_empty() {
                review.base_branch.clone_from(&overlay.base_branch);
            }
            if !overlay.head_branch.is_empty() {
                review.head_branch.clone_from(&overlay.head_branch);
            }
            true
        }
        None => {
            let mut review = ErReview {
                version: overlay.version,
                diff_hash: overlay.diff_hash.clone(),
                created_at: overlay.created_at.clone(),
                base_branch: overlay.base_branch.clone(),
                head_branch: overlay.head_branch.clone(),
                files: HashMap::new(),
                file_hashes: HashMap::new(),
            };
            for (path, file) in &overlay.files {
                if scope.contains(path.as_str()) {
                    review.files.insert(path.clone(), file.clone());
                }
            }
            for (path, hash) in &overlay.file_hashes {
                if scope.contains(path.as_str()) {
                    review.file_hashes.insert(path.clone(), hash.clone());
                }
            }
            if review.files.is_empty() {
                return false;
            }
            *base = Some(review);
            true
        }
    }
}

pub fn apply_scoped_professor(
    base: &mut Option<ProfessorReview>,
    overlay: &ScopedProfessorReview,
) -> bool {
    if !overlay.host_stamped || overlay.scope_paths.is_empty() {
        return false;
    }
    let identity = base
        .as_ref()
        .map(|review| (review.diff_hash.as_str(), review.created_at.as_str()));
    if !identity_matches(&overlay.base_diff_hash, &overlay.base_created_at, identity) {
        return false;
    }
    let scope = scope_set(&overlay.scope_paths);
    match base {
        Some(review) => {
            for path in &scope {
                review.files.remove(*path);
            }
            for (path, file) in &overlay.files {
                if scope.contains(path.as_str()) {
                    review.files.insert(path.clone(), file.clone());
                }
            }
            if !overlay.diff_scope.is_empty() {
                review.diff_scope.clone_from(&overlay.diff_scope);
            }
            if !overlay.summary.is_empty() {
                review.summary.clone_from(&overlay.summary);
            }
            if !overlay.focus_prompt.is_empty() {
                review.focus_prompt.clone_from(&overlay.focus_prompt);
            }
            true
        }
        None => {
            let mut review = ProfessorReview {
                version: overlay.version,
                diff_hash: overlay.diff_hash.clone(),
                diff_scope: overlay.diff_scope.clone(),
                created_at: overlay.created_at.clone(),
                focus_prompt: overlay.focus_prompt.clone(),
                summary: overlay.summary.clone(),
                files: HashMap::new(),
            };
            for (path, file) in &overlay.files {
                if scope.contains(path.as_str()) {
                    review.files.insert(path.clone(), file.clone());
                }
            }
            if review.files.is_empty() {
                return false;
            }
            *base = Some(review);
            true
        }
    }
}

pub fn apply_scoped_expert(experts: &mut Vec<ExpertReview>, overlay: &ScopedExpertReview) -> bool {
    if !overlay.host_stamped || overlay.scope_paths.is_empty() {
        return false;
    }
    let scope = scope_set(&overlay.scope_paths);
    if let Some(review) = experts
        .iter_mut()
        .find(|expert| expert.expert_id == overlay.expert_id)
    {
        if !identity_matches(
            &overlay.base_diff_hash,
            &overlay.base_created_at,
            Some((&review.diff_hash, &review.created_at)),
        ) {
            return false;
        }
        for path in &scope {
            review.files.remove(*path);
        }
        for (path, file) in &overlay.files {
            if scope.contains(path.as_str()) {
                review.files.insert(path.clone(), file.clone());
            }
        }
        if !overlay.diff_scope.is_empty() {
            review.diff_scope.clone_from(&overlay.diff_scope);
        }
        if !overlay.summary.is_empty() {
            review.summary.clone_from(&overlay.summary);
        }
        return true;
    }
    if !identity_matches(&overlay.base_diff_hash, &overlay.base_created_at, None) {
        return false;
    }
    let mut review = ExpertReview {
        version: overlay.version,
        expert_id: overlay.expert_id.clone(),
        diff_hash: overlay.diff_hash.clone(),
        diff_scope: overlay.diff_scope.clone(),
        created_at: overlay.created_at.clone(),
        summary: overlay.summary.clone(),
        files: HashMap::new(),
    };
    for (path, file) in &overlay.files {
        if scope.contains(path.as_str()) {
            review.files.insert(path.clone(), file.clone());
        }
    }
    if review.files.is_empty() {
        return false;
    }
    experts.push(review);
    true
}

fn next_unique_id(used: &mut HashSet<String>, key: &str) -> String {
    let base = format!("scoped-{key}");
    let mut id = base.clone();
    let mut suffix = 2;
    while !used.insert(id.clone()) {
        id = format!("{base}-{suffix}");
        suffix += 1;
    }
    id
}

fn re_id_review_files(files: &mut HashMap<String, ErFileReview>) {
    let mut paths: Vec<String> = files.keys().cloned().collect();
    paths.sort();
    let mut used = HashSet::new();
    for path in paths {
        let target_path = path.clone();
        if let Some(file) = files.get_mut(&path) {
            for finding in &mut file.findings {
                let key = finding_key(&target_path, finding.line_start, &finding.title);
                finding.id = next_unique_id(&mut used, &key);
            }
        }
    }
}

fn re_id_professor_files(files: &mut HashMap<String, ProfessorFileReview>) {
    let mut paths: Vec<String> = files.keys().cloned().collect();
    paths.sort();
    let mut used = HashSet::new();
    for path in paths {
        let target_path = path.clone();
        if let Some(file) = files.get_mut(&path) {
            for finding in &mut file.findings {
                let key = finding_key(&target_path, finding.line_start, &finding.title);
                finding.id = next_unique_id(&mut used, &key);
            }
        }
    }
}

fn re_id_expert_files(files: &mut HashMap<String, ExpertFileReview>) {
    let mut paths: Vec<String> = files.keys().cloned().collect();
    paths.sort();
    let mut used = HashSet::new();
    for path in paths {
        let target_path = path.clone();
        if let Some(file) = files.get_mut(&path) {
            for finding in &mut file.findings {
                let key = finding_key(&target_path, finding.line_start, &finding.title);
                finding.id = next_unique_id(&mut used, &key);
            }
        }
    }
}

fn sorted_unique(mut paths: Vec<String>) -> Vec<String> {
    paths.sort();
    paths.dedup();
    paths
}

fn review_files_for_edit(overlay: &mut ScopedReview) -> Vec<&mut Vec<Finding>> {
    let scope = scope_set(&overlay.scope_paths);
    overlay
        .files
        .iter_mut()
        .filter(|(path, _)| scope.contains(path.as_str()))
        .map(|(_, file)| &mut file.findings)
        .collect()
}

fn professor_files_for_edit(overlay: &mut ScopedProfessorReview) -> Vec<&mut Vec<Finding>> {
    let scope = scope_set(&overlay.scope_paths);
    overlay
        .files
        .iter_mut()
        .filter(|(path, _)| scope.contains(path.as_str()))
        .map(|(_, file)| &mut file.findings)
        .collect()
}

fn expert_files_for_edit(overlay: &mut ScopedExpertReview) -> Vec<&mut Vec<Finding>> {
    let scope = scope_set(&overlay.scope_paths);
    overlay
        .files
        .iter_mut()
        .filter(|(path, _)| scope.contains(path.as_str()))
        .map(|(_, file)| &mut file.findings)
        .collect()
}

/// Why one overlay update stopped without writing.
#[derive(Debug)]
enum OverlayStop {
    /// The file is absent, or exists but reads as `None`; never overwrite it.
    Missing,
    /// The closure found nothing to change in this lens.
    NoMatch,
}

/// What one stamp did with the lens's overlay.
#[derive(Clone, Copy, PartialEq, Eq)]
enum StampWrite {
    /// A merged overlay was written under the lock.
    Written,
    /// The agent left the file exactly as captured at spawn.
    Unchanged,
    /// No readable agent overlay; there was nothing to stamp.
    Absent,
}

/// Apply `apply` under the store lock and write the result.
///
/// A file that exists but cannot be read arrives as `None` here, so the update
/// refuses instead of replacing bytes this build cannot see. `false` means the
/// lens had no row to change or no readable overlay.
fn update_overlay_files<T, F>(
    store: &ArtifactStore,
    key: &ArtifactKey<T>,
    apply: F,
) -> anyhow::Result<bool>
where
    T: Artifact,
    F: FnOnce(&mut T) -> anyhow::Result<bool>,
{
    let mut failure = None;
    let outcome = store.update(key, er_store::DEFAULT_LOCK_TIMEOUT, |current| {
        let Some(mut overlay) = current else {
            return Err(OverlayStop::Missing);
        };
        match apply(&mut overlay) {
            Ok(true) => Ok(overlay),
            Ok(false) => Err(OverlayStop::NoMatch),
            Err(error) => {
                failure = Some(error);
                Err(OverlayStop::NoMatch)
            }
        }
    });
    match outcome {
        Ok(_) => Ok(true),
        Err(er_store::UpdateError::Apply(OverlayStop::NoMatch)) => match failure {
            Some(error) => Err(error),
            None => Ok(false),
        },
        Err(er_store::UpdateError::Apply(OverlayStop::Missing)) => Ok(false),
        Err(er_store::UpdateError::Store(er_store::StoreError::NewerVersion { .. })) => Ok(false),
        Err(er_store::UpdateError::Store(error)) => Err(error.into()),
    }
}

/// Apply `f` to the display finding in its lens overlay, when its file is in
/// `scope_paths`. false when no overlay row matches.
pub fn with_finding_in_overlay<F>(
    er_dir: &str,
    finding_id: &str,
    f: &mut Option<F>,
) -> anyhow::Result<bool>
where
    F: FnOnce(&mut Finding) -> anyhow::Result<()>,
{
    let store = crate::store_registry::store();
    let scope = er_store::Scope::dir(er_dir)?;
    let prefix = finding_id.split_once('-').map(|(prefix, _)| prefix);

    if let Some(expert_id) = prefix.and_then(expert_id_for_id_prefix) {
        let Ok(slot) = ExpertId::new(expert_id) else {
            return Ok(false);
        };
        let key = ArtifactKey::<ScopedExpertReview>::new(scope, slot)?;
        return update_overlay_files(&store, &key, |overlay| {
            let mut files = expert_files_for_edit(overlay);
            apply_to_findings(&mut files, finding_id, prefix, f)
        });
    }

    if prefix == Some(PROFESSOR_ID_PREFIX) {
        let key = ArtifactKey::<ScopedProfessorReview>::new(scope, ())?;
        return update_overlay_files(&store, &key, |overlay| {
            let mut files = professor_files_for_edit(overlay);
            apply_to_findings(&mut files, finding_id, Some(PROFESSOR_ID_PREFIX), f)
        });
    }

    let key = ArtifactKey::<ScopedReview>::new(scope, ())?;
    update_overlay_files(&store, &key, |overlay| {
        let mut files = review_files_for_edit(overlay);
        apply_to_findings(&mut files, finding_id, None, f)
    })
}

/// Apply `edit` to every lens overlay, in general, professor, expert order.
/// `edit` receives the same prefix the projection adds for that lens.
pub fn edit_finding_overlays<F>(er_dir: &str, edit: &mut F) -> anyhow::Result<bool>
where
    F: FnMut(&mut Vec<Finding>, Option<&str>) -> bool,
{
    let store = crate::store_registry::store();
    let scope = er_store::Scope::dir(er_dir)?;
    let mut changed = false;

    let review_key = ArtifactKey::<ScopedReview>::new(scope.clone(), ())?;
    changed |= update_overlay_files(&store, &review_key, |overlay| {
        let mut edited = false;
        for findings in review_files_for_edit(overlay) {
            edited |= edit(findings, None);
        }
        Ok(edited)
    })?;

    let professor_key = ArtifactKey::<ScopedProfessorReview>::new(scope.clone(), ())?;
    changed |= update_overlay_files(&store, &professor_key, |overlay| {
        let mut edited = false;
        for findings in professor_files_for_edit(overlay) {
            edited |= edit(findings, Some(PROFESSOR_ID_PREFIX));
        }
        Ok(edited)
    })?;

    for slot in store.list::<ScopedExpertReview>(&scope)? {
        let key = ArtifactKey::<ScopedExpertReview>::new(scope.clone(), slot.clone())?;
        let mut prefix = None;
        changed |= update_overlay_files(&store, &key, |overlay| {
            prefix = expert_by_id(&overlay.expert_id).map(|def| def.id_prefix);
            let mut edited = false;
            for findings in expert_files_for_edit(overlay) {
                edited |= edit(findings, prefix);
            }
            Ok(edited)
        })?;
    }

    Ok(changed)
}

/// Merge the agent's output with the previous overlay, re-id new rows, write
/// the stamps, and delete this lens's own `*.prev.json`. false when the agent
/// wrote no overlay.
pub fn stamp_scoped_overlay(
    store: &ArtifactStore,
    scope: &Scope,
    command_name: &str,
    stamp: &ScopedRunStamp,
) -> anyhow::Result<bool> {
    match command_name {
        "review" => stamp_review(store, scope, stamp),
        "professor" => stamp_professor(store, scope, stamp),
        _ => match command_name.strip_prefix("expert-") {
            Some(id) => stamp_expert(store, scope, id, stamp),
            None => Ok(false),
        },
    }
}

fn unchanged_since_spawn(path: &std::path::Path, stamp: &ScopedRunStamp) -> bool {
    let current = super::overlay_files::read_bytes(path);
    stamp
        .previous_overlay
        .as_deref()
        .is_some_and(|captured| captured == current.as_slice())
}

fn previous_identity_matches(
    stamp: &ScopedRunStamp,
    base_diff_hash: &str,
    base_created_at: &str,
) -> bool {
    base_diff_hash == stamp.base_diff_hash && base_created_at == stamp.base_created_at
}

fn read_previous<T: serde::de::DeserializeOwned>(stamp: &ScopedRunStamp) -> Option<T> {
    serde_json::from_slice(stamp.previous_overlay.as_deref()?).ok()
}

/// One stamp's read-merge-write, under the store lock.
///
/// `previous` is the overlay captured at spawn, already filtered to this base
/// generation. The merge starts from the file's current value, so a lifecycle
/// write or a second stamp that landed in between is kept.
fn update_stamp<T, S, M>(
    store: &ArtifactStore,
    key: &ArtifactKey<T>,
    command_name: &str,
    stamp: &ScopedRunStamp,
    previous: Option<T>,
    strict: S,
    merge: M,
) -> anyhow::Result<StampWrite>
where
    T: Artifact,
    S: FnOnce(&T) -> anyhow::Result<()>,
    M: FnOnce(T, Option<&T>, &ScopedRunStamp) -> T,
{
    let scope = key.scope();
    let path = store.path(key);
    let mut unchanged = false;
    let mut failure = None;
    let outcome = store.update(key, er_store::DEFAULT_LOCK_TIMEOUT, |current| {
        if unchanged_since_spawn(&path, stamp) {
            unchanged = true;
            return Err(OverlayStop::NoMatch);
        }
        let Some(agent) = current else {
            // A file that exists but reads as None arrived from `update` with
            // its bytes unreadable; never replace it.
            if store.exists(key) {
                let reason = match store.read(key) {
                    Ok(er_store::Loaded::Unreadable(reason)) => format!("{reason:?}"),
                    _ => "unreadable".to_string(),
                };
                failure = Some(anyhow::anyhow!(
                    "{} is unreadable: {reason}",
                    path.display()
                ));
            }
            return Err(OverlayStop::Missing);
        };
        if let Err(error) = strict(&agent) {
            failure = Some(error);
            return Err(OverlayStop::NoMatch);
        }
        Ok(merge(agent, previous.as_ref(), stamp))
    });
    match outcome {
        Ok(_) => {
            remove_prev_file(store, scope, command_name);
            Ok(StampWrite::Written)
        }
        Err(er_store::UpdateError::Apply(OverlayStop::NoMatch)) => match failure {
            Some(error) => Err(error),
            None if unchanged => {
                remove_prev_file(store, scope, command_name);
                Ok(StampWrite::Unchanged)
            }
            None => Ok(StampWrite::Absent),
        },
        Err(er_store::UpdateError::Apply(OverlayStop::Missing)) => match failure {
            Some(error) => Err(error),
            None => Ok(StampWrite::Absent),
        },
        Err(er_store::UpdateError::Store(er_store::StoreError::NewerVersion { found, .. })) => Err(
            anyhow::anyhow!("{} is unreadable: newer version {found}", path.display()),
        ),
        Err(er_store::UpdateError::Store(error)) => Err(error.into()),
    }
}

/// Union of the previous overlay's paths, the file's current paths (a write
/// that landed between spawn and stamp) and this run's manifest.
fn merge_all_scope_paths(previous: &[String], current: &[String], new: &[String]) -> Vec<String> {
    sorted_unique(previous.iter().chain(current).chain(new).cloned().collect())
}

fn merge_review(
    agent: ScopedReview,
    previous: Option<&ScopedReview>,
    stamp: &ScopedRunStamp,
) -> ScopedReview {
    let mut files = previous.map(|p| p.files.clone()).unwrap_or_default();
    let mut file_hashes = previous.map(|p| p.file_hashes.clone()).unwrap_or_default();
    for (path, file) in &agent.files {
        files.insert(path.clone(), file.clone());
    }
    for (path, hash) in &agent.file_hashes {
        file_hashes.insert(path.clone(), hash.clone());
    }
    let mut merged = ScopedReview {
        version: agent.version,
        diff_hash: agent.diff_hash.clone(),
        created_at: previous
            .map(|p| p.created_at.clone())
            .unwrap_or_else(|| agent.created_at.clone()),
        base_branch: if agent.base_branch.is_empty() {
            previous.map(|p| p.base_branch.clone()).unwrap_or_default()
        } else {
            agent.base_branch.clone()
        },
        head_branch: if agent.head_branch.is_empty() {
            previous.map(|p| p.head_branch.clone()).unwrap_or_default()
        } else {
            agent.head_branch.clone()
        },
        files,
        file_hashes,
        scope_paths: merge_all_scope_paths(
            previous
                .map(|p| p.scope_paths.as_slice())
                .unwrap_or_default(),
            &agent.scope_paths,
            &stamp.scope_paths,
        ),
        base_diff_hash: stamp.base_diff_hash.clone(),
        base_created_at: stamp.base_created_at.clone(),
        host_stamped: true,
    };
    for path in &stamp.scope_paths {
        merged.files.remove(path);
        merged.file_hashes.remove(path);
    }
    let manifest = scope_set(&stamp.scope_paths);
    for (path, file) in agent.files {
        if manifest.contains(path.as_str()) {
            merged.files.insert(path, file);
        }
    }
    for (path, hash) in agent.file_hashes {
        if manifest.contains(path.as_str()) {
            merged.file_hashes.insert(path, hash);
        }
    }
    re_id_review_files(&mut merged.files);
    merged
}

fn stamp_review(
    store: &ArtifactStore,
    scope: &Scope,
    stamp: &ScopedRunStamp,
) -> anyhow::Result<bool> {
    let key = ArtifactKey::<ScopedReview>::new(scope.clone(), ())?;
    let previous = read_previous::<ScopedReview>(stamp).filter(|previous| {
        previous_identity_matches(stamp, &previous.base_diff_hash, &previous.base_created_at)
    });
    let outcome = update_stamp(
        store,
        &key,
        "review",
        stamp,
        previous,
        |_| Ok(()),
        merge_review,
    )?;
    if outcome == StampWrite::Written {
        // The rows on these paths were re-produced, so the validate pass's
        // outcomes for them no longer describe the rows now shown. Other paths
        // keep theirs; the base's identity did not move, so those still apply.
        drop_validation_outcomes_for_paths(store, scope, &stamp.scope_paths)?;
    }
    Ok(outcome != StampWrite::Absent)
}

fn drop_validation_outcomes_for_paths(
    store: &ArtifactStore,
    scope: &Scope,
    paths: &[String],
) -> anyhow::Result<()> {
    let key = ArtifactKey::<ValidationReview>::new(scope.clone(), ())?;
    let outcome = store.update(&key, er_store::DEFAULT_LOCK_TIMEOUT, |current| {
        let Some(mut validation) = current else {
            return Err(OverlayStop::Missing);
        };
        let scope = scope_set(paths);
        let before = validation.outcomes.len();
        validation
            .outcomes
            .retain(|outcome| !scope.contains(outcome.file.as_str()));
        if validation.outcomes.len() == before {
            return Err(OverlayStop::NoMatch);
        }
        Ok(validation)
    });
    match outcome {
        Ok(_) => Ok(()),
        // Absent, no outcome on these paths, or unreadable: leave the bytes.
        Err(er_store::UpdateError::Apply(_)) => Ok(()),
        // A file a newer build owns stays as it is.
        Err(er_store::UpdateError::Store(er_store::StoreError::NewerVersion { .. })) => Ok(()),
        Err(er_store::UpdateError::Store(error)) => Err(error.into()),
    }
}

fn merge_professor(
    agent: ScopedProfessorReview,
    previous: Option<&ScopedProfessorReview>,
    stamp: &ScopedRunStamp,
) -> ScopedProfessorReview {
    let mut files = previous.map(|p| p.files.clone()).unwrap_or_default();
    for (path, file) in &agent.files {
        files.insert(path.clone(), file.clone());
    }
    let mut merged = ScopedProfessorReview {
        version: agent.version,
        diff_hash: agent.diff_hash.clone(),
        diff_scope: if agent.diff_scope.is_empty() {
            previous.map(|p| p.diff_scope.clone()).unwrap_or_default()
        } else {
            agent.diff_scope.clone()
        },
        created_at: previous
            .map(|p| p.created_at.clone())
            .unwrap_or_else(|| agent.created_at.clone()),
        focus_prompt: if agent.focus_prompt.is_empty() {
            previous.map(|p| p.focus_prompt.clone()).unwrap_or_default()
        } else {
            agent.focus_prompt.clone()
        },
        summary: if agent.summary.is_empty() {
            previous.map(|p| p.summary.clone()).unwrap_or_default()
        } else {
            agent.summary.clone()
        },
        files,
        scope_paths: merge_all_scope_paths(
            previous
                .map(|p| p.scope_paths.as_slice())
                .unwrap_or_default(),
            &agent.scope_paths,
            &stamp.scope_paths,
        ),
        base_diff_hash: stamp.base_diff_hash.clone(),
        base_created_at: stamp.base_created_at.clone(),
        host_stamped: true,
    };
    for path in &stamp.scope_paths {
        merged.files.remove(path);
    }
    let manifest = scope_set(&stamp.scope_paths);
    for (path, file) in agent.files {
        if manifest.contains(path.as_str()) {
            merged.files.insert(path, file);
        }
    }
    re_id_professor_files(&mut merged.files);
    merged
}

fn stamp_professor(
    store: &ArtifactStore,
    scope: &Scope,
    stamp: &ScopedRunStamp,
) -> anyhow::Result<bool> {
    let key = ArtifactKey::<ScopedProfessorReview>::new(scope.clone(), ())?;
    let previous = read_previous::<ScopedProfessorReview>(stamp).filter(|previous| {
        previous_identity_matches(stamp, &previous.base_diff_hash, &previous.base_created_at)
    });
    let outcome = update_stamp(
        store,
        &key,
        "professor",
        stamp,
        previous,
        |_| Ok(()),
        merge_professor,
    )?;
    Ok(outcome != StampWrite::Absent)
}

fn merge_expert(
    agent: ScopedExpertReview,
    previous: Option<&ScopedExpertReview>,
    stamp: &ScopedRunStamp,
) -> ScopedExpertReview {
    let mut files = previous.map(|p| p.files.clone()).unwrap_or_default();
    for (path, file) in &agent.files {
        files.insert(path.clone(), file.clone());
    }
    let mut merged = ScopedExpertReview {
        version: agent.version,
        expert_id: agent.expert_id.clone(),
        diff_hash: agent.diff_hash.clone(),
        diff_scope: if agent.diff_scope.is_empty() {
            previous.map(|p| p.diff_scope.clone()).unwrap_or_default()
        } else {
            agent.diff_scope.clone()
        },
        created_at: previous
            .map(|p| p.created_at.clone())
            .unwrap_or_else(|| agent.created_at.clone()),
        summary: if agent.summary.is_empty() {
            previous.map(|p| p.summary.clone()).unwrap_or_default()
        } else {
            agent.summary.clone()
        },
        files,
        scope_paths: merge_all_scope_paths(
            previous
                .map(|p| p.scope_paths.as_slice())
                .unwrap_or_default(),
            &agent.scope_paths,
            &stamp.scope_paths,
        ),
        base_diff_hash: stamp.base_diff_hash.clone(),
        base_created_at: stamp.base_created_at.clone(),
        host_stamped: true,
    };
    for path in &stamp.scope_paths {
        merged.files.remove(path);
    }
    let manifest = scope_set(&stamp.scope_paths);
    for (path, file) in agent.files {
        if manifest.contains(path.as_str()) {
            merged.files.insert(path, file);
        }
    }
    re_id_expert_files(&mut merged.files);
    merged
}

fn stamp_expert(
    store: &ArtifactStore,
    scope: &Scope,
    expert_id: &str,
    stamp: &ScopedRunStamp,
) -> anyhow::Result<bool> {
    let Ok(slot) = ExpertId::new(expert_id) else {
        return Ok(false);
    };
    let key = ArtifactKey::<ScopedExpertReview>::new(scope.clone(), slot)?;
    let previous = read_previous::<ScopedExpertReview>(stamp).filter(|previous| {
        previous_identity_matches(stamp, &previous.base_diff_hash, &previous.base_created_at)
    });
    let outcome = update_stamp(
        store,
        &key,
        &format!("expert-{expert_id}"),
        stamp,
        previous,
        |agent| {
            anyhow::ensure!(
                agent.expert_id == expert_id,
                "{expert_id} wrote an overlay naming expert_id {:?}",
                agent.expert_id
            );
            Ok(())
        },
        merge_expert,
    )?;
    Ok(outcome != StampWrite::Absent)
}

/// Delete exactly this lens's old `*.prev.json`; never the questions or notes
/// backups.
fn remove_prev_file(store: &ArtifactStore, scope: &Scope, command_name: &str) {
    let bucket = store.scope_dir(scope);
    match command_name {
        "review" => {
            super::overlay_files::remove(&bucket.join("review.prev.json"));
        }
        "professor" => {
            super::overlay_files::remove(&bucket.join("professor.prev.json"));
        }
        _ => {
            if let Some(id) = command_name.strip_prefix("expert-") {
                super::overlay_files::remove(&bucket.join(format!("experts/{id}.prev.json")));
            }
        }
    }
}

/// Delete exactly a full run's overlay, old snapshot, `review-files.txt`, and
/// for general `validation.json`.
pub fn clear_full_run_artifacts(
    store: &ArtifactStore,
    scope: &Scope,
    command_name: &str,
) -> anyhow::Result<()> {
    let bucket = store.scope_dir(scope);
    match command_name {
        "review" => {
            store.delete(&ArtifactKey::<ScopedReview>::new(scope.clone(), ())?)?;
            store.delete(&ArtifactKey::<ValidationReview>::new(scope.clone(), ())?)?;
            super::overlay_files::remove(&bucket.join("review.prev.json"));
        }
        "professor" => {
            store.delete(&ArtifactKey::<ScopedProfessorReview>::new(
                scope.clone(),
                (),
            )?)?;
            super::overlay_files::remove(&bucket.join("professor.prev.json"));
        }
        _ => {
            if let Some(id) = command_name.strip_prefix("expert-") {
                if let Ok(slot) = ExpertId::new(id) {
                    store.delete(&ArtifactKey::<ScopedExpertReview>::new(
                        scope.clone(),
                        slot,
                    )?)?;
                }
                super::overlay_files::remove(&bucket.join(format!("experts/{id}.prev.json")));
            }
        }
    }
    super::overlay_files::remove(&bucket.join("review-files.txt"));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::review::{Confidence, ErFileReview, RiskLevel};
    use crate::storage::with_test_storage_root;

    fn finding(id: &str, title: &str, line: usize) -> Finding {
        Finding {
            id: id.to_string(),
            severity: RiskLevel::Medium,
            lens: String::new(),
            category: "correctness".to_string(),
            raised_by: Vec::new(),
            providers: Vec::new(),
            title: title.to_string(),
            description: String::new(),
            hunk_index: Some(0),
            line_start: Some(line),
            line_end: None,
            line_content: String::new(),
            stale: false,
            suggestion: String::new(),
            related_files: vec![],
            outside_diff: false,
            confidence: Confidence::Confirmed,
            verification_plan: String::new(),
            evidence: vec![],
            responses: vec![],
            resolved: false,
            resolved_note: String::new(),
            resolved_at: String::new(),
            promoted_to: None,
        }
    }

    fn er_file(findings: Vec<Finding>) -> ErFileReview {
        ErFileReview {
            risk: RiskLevel::Medium,
            risk_reason: String::new(),
            summary: String::new(),
            findings,
        }
    }

    fn base_review(files: Vec<(&str, Vec<Finding>)>) -> ErReview {
        ErReview {
            version: 1,
            diff_hash: "hash".to_string(),
            created_at: "t0".to_string(),
            base_branch: "main".to_string(),
            head_branch: "feat".to_string(),
            files: files
                .into_iter()
                .map(|(path, findings)| (path.to_string(), er_file(findings)))
                .collect(),
            file_hashes: HashMap::new(),
        }
    }

    fn scoped(files: Vec<(&str, Vec<Finding>)>, scope: &[&str]) -> ScopedReview {
        ScopedReview {
            version: 1,
            diff_hash: "hash".to_string(),
            created_at: "t1".to_string(),
            base_branch: "main".to_string(),
            head_branch: "feat".to_string(),
            files: files
                .into_iter()
                .map(|(path, findings)| (path.to_string(), er_file(findings)))
                .collect(),
            file_hashes: HashMap::new(),
            scope_paths: scope.iter().map(|path| path.to_string()).collect(),
            base_diff_hash: "hash".to_string(),
            base_created_at: "t0".to_string(),
            host_stamped: true,
        }
    }

    fn temp_dir(label: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("er-scoped-overlay-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn scoped_overlay_replaces_only_its_paths() {
        let mut review = Some(base_review(vec![
            ("src/a.rs", vec![finding("a1", "Old A", 1)]),
            ("src/b.rs", vec![finding("b1", "Old B", 2)]),
        ]));
        let overlay = scoped(
            vec![("src/a.rs", vec![finding("n1", "New A", 5)])],
            &["src/a.rs"],
        );
        assert!(apply_scoped_review(&mut review, &overlay));
        let review = review.unwrap();
        let a = &review.files["src/a.rs"].findings;
        assert_eq!(a.len(), 1);
        assert_eq!(a[0].title, "New A");
        assert_eq!(review.files["src/b.rs"].findings[0].title, "Old B");
    }

    #[test]
    fn scoped_overlay_with_no_entry_clears_the_path() {
        let mut review = Some(base_review(vec![
            ("src/a.rs", vec![finding("a1", "Old A", 1)]),
            ("src/b.rs", vec![finding("b1", "Old B", 2)]),
        ]));
        let overlay = scoped(vec![], &["src/a.rs"]);
        assert!(apply_scoped_review(&mut review, &overlay));
        let review = review.unwrap();
        assert!(!review.files.contains_key("src/a.rs"));
        assert!(review.files.contains_key("src/b.rs"));
    }

    #[test]
    fn scoped_overlay_keeps_the_full_diff_hash_and_stays_fresh() {
        let mut review = Some(base_review(vec![(
            "src/a.rs",
            vec![finding("a1", "Old A", 1)],
        )]));
        let mut overlay = scoped(
            vec![("src/a.rs", vec![finding("n1", "New A", 5)])],
            &["src/a.rs"],
        );
        overlay.diff_hash = "scoped-filtered-hash".to_string();
        overlay.created_at = "t9".to_string();
        assert!(apply_scoped_review(&mut review, &overlay));
        let review = review.unwrap();
        assert_eq!(review.diff_hash, "hash");
        assert_eq!(review.created_at, "t0");
    }

    #[test]
    fn scoped_overlay_for_professor_and_expert_lenses() {
        let mut professor = Some(ProfessorReview {
            version: 1,
            diff_hash: "hash".to_string(),
            diff_scope: String::new(),
            created_at: "t0".to_string(),
            focus_prompt: String::new(),
            summary: "old summary".to_string(),
            files: [(
                "src/a.rs".to_string(),
                ProfessorFileReview {
                    findings: vec![finding("p1", "Old P", 1)],
                },
            )]
            .into_iter()
            .collect(),
        });
        let professor_overlay = ScopedProfessorReview {
            version: 1,
            diff_hash: "hash".to_string(),
            diff_scope: String::new(),
            created_at: "t1".to_string(),
            focus_prompt: String::new(),
            summary: "new summary".to_string(),
            files: [(
                "src/a.rs".to_string(),
                ProfessorFileReview {
                    findings: vec![finding("n2", "New P", 7)],
                },
            )]
            .into_iter()
            .collect(),
            scope_paths: vec!["src/a.rs".to_string()],
            base_diff_hash: "hash".to_string(),
            base_created_at: "t0".to_string(),
            host_stamped: true,
        };
        assert!(apply_scoped_professor(&mut professor, &professor_overlay));
        let professor = professor.unwrap();
        assert_eq!(professor.files["src/a.rs"].findings[0].title, "New P");
        assert_eq!(professor.summary, "new summary");
        assert_eq!(professor.diff_hash, "hash");

        let mut experts = vec![ExpertReview {
            version: 1,
            expert_id: "security".to_string(),
            diff_hash: "hash".to_string(),
            diff_scope: String::new(),
            created_at: "t0".to_string(),
            summary: String::new(),
            files: [(
                "src/a.rs".to_string(),
                ExpertFileReview {
                    findings: vec![finding("s1", "Old S", 1)],
                },
            )]
            .into_iter()
            .collect(),
        }];
        let expert_overlay = ScopedExpertReview {
            version: 1,
            expert_id: "security".to_string(),
            diff_hash: "hash".to_string(),
            diff_scope: String::new(),
            created_at: "t1".to_string(),
            summary: String::new(),
            files: [(
                "src/a.rs".to_string(),
                ExpertFileReview {
                    findings: vec![finding("n3", "New S", 3)],
                },
            )]
            .into_iter()
            .collect(),
            scope_paths: vec!["src/a.rs".to_string()],
            base_diff_hash: "hash".to_string(),
            base_created_at: "t0".to_string(),
            host_stamped: true,
        };
        assert!(apply_scoped_expert(&mut experts, &expert_overlay));
        assert_eq!(experts[0].files["src/a.rs"].findings[0].title, "New S");

        // An expert with no base file is created only when it also has no base.
        let fresh = ScopedExpertReview {
            base_diff_hash: String::new(),
            base_created_at: String::new(),
            ..expert_overlay.clone()
        };
        let mut empty: Vec<ExpertReview> = Vec::new();
        assert!(apply_scoped_expert(&mut empty, &fresh));
        assert_eq!(empty.len(), 1);
    }

    #[test]
    fn scoped_overlay_without_a_base_becomes_the_review() {
        let mut review = None;
        let overlay = scoped(
            vec![("src/a.rs", vec![finding("n1", "New A", 5)])],
            &["src/a.rs"],
        );
        let empty_identity = ScopedReview {
            base_diff_hash: String::new(),
            base_created_at: String::new(),
            ..overlay.clone()
        };
        assert!(apply_scoped_review(&mut review, &empty_identity));
        let review = review.unwrap();
        assert_eq!(review.diff_hash, "hash");
        assert_eq!(review.created_at, "t1");
        assert_eq!(review.files["src/a.rs"].findings[0].title, "New A");

        // With a base identity but no base file, there is nothing to match.
        let mut unmatched = None;
        assert!(!apply_scoped_review(&mut unmatched, &overlay));
        assert!(unmatched.is_none());
    }

    #[test]
    fn scoped_overlay_with_a_changed_base_is_ignored() {
        let mut review = Some(base_review(vec![(
            "src/a.rs",
            vec![finding("a1", "Old A", 1)],
        )]));
        let mut overlay = scoped(
            vec![("src/a.rs", vec![finding("n1", "New A", 5)])],
            &["src/a.rs"],
        );
        overlay.base_created_at = "t-new".to_string();
        assert!(!apply_scoped_review(&mut review, &overlay));
        assert_eq!(review.unwrap().files["src/a.rs"].findings[0].title, "Old A");
    }

    #[test]
    fn scoped_overlay_without_a_host_stamp_is_ignored() {
        let mut review = Some(base_review(vec![(
            "src/a.rs",
            vec![finding("a1", "Old A", 1)],
        )]));
        let mut overlay = scoped(
            vec![("src/a.rs", vec![finding("n1", "New A", 5)])],
            &["src/a.rs"],
        );
        overlay.host_stamped = false;
        assert!(!apply_scoped_review(&mut review, &overlay));
        assert_eq!(review.unwrap().files["src/a.rs"].findings[0].title, "Old A");
    }

    #[test]
    fn no_overlay_files_changes_nothing() {
        let dir = temp_dir("no-files");
        with_test_storage_root(&dir, || {
            let store = crate::store_registry::store();
            let scope = Scope::dir(dir.to_str().unwrap()).unwrap();
            let stamp = ScopedRunStamp {
                scope_paths: vec!["src/a.rs".to_string()],
                base_diff_hash: String::new(),
                base_created_at: String::new(),
                previous_overlay: None,
            };
            assert!(!stamp_scoped_overlay(&store, &scope, "review", &stamp).unwrap());
            assert!(!stamp_scoped_overlay(&store, &scope, "professor", &stamp).unwrap());
            assert!(!stamp_scoped_overlay(&store, &scope, "expert-security", &stamp).unwrap());
        });
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn stamp_overlay_merges_and_re_ids() {
        let dir = temp_dir("merge");
        with_test_storage_root(&dir, || {
            let store = crate::store_registry::store();
            let scope = Scope::dir(dir.to_str().unwrap()).unwrap();
            std::fs::create_dir_all(dir.join("scoped/experts")).unwrap();
            let path = dir.join("scoped/general.json");

            let first = scoped(
                vec![
                    (
                        "src/a.rs",
                        vec![finding("", "Dup", 4), finding("", "Dup", 4)],
                    ),
                    ("src/b.rs", vec![finding("", "First B", 2)]),
                ],
                &["src/a.rs", "src/b.rs"],
            );
            std::fs::write(&path, serde_json::to_vec(&first).unwrap()).unwrap();
            let stamp = ScopedRunStamp {
                scope_paths: vec!["src/a.rs".to_string(), "src/b.rs".to_string()],
                base_diff_hash: String::new(),
                base_created_at: String::new(),
                previous_overlay: None,
            };
            assert!(stamp_scoped_overlay(&store, &scope, "review", &stamp).unwrap());

            let stamped: ScopedReview =
                serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            assert!(stamped.host_stamped);
            assert_eq!(stamped.scope_paths, vec!["src/a.rs", "src/b.rs"]);
            let a_ids: Vec<&str> = stamped.files["src/a.rs"]
                .findings
                .iter()
                .map(|finding| finding.id.as_str())
                .collect();
            assert_eq!(a_ids.len(), 2);
            assert_ne!(a_ids[0], a_ids[1], "duplicate rows must get unique ids");
            assert!(a_ids.iter().all(|id| id.starts_with("scoped-")));

            // A second run for a different path keeps the first run's rows.
            let previous = std::fs::read(&path).unwrap();
            let second = scoped(
                vec![("src/c.rs", vec![finding("", "Second C", 8)])],
                &["src/c.rs"],
            );
            std::fs::write(&path, serde_json::to_vec(&second).unwrap()).unwrap();
            let stamp = ScopedRunStamp {
                scope_paths: vec!["src/c.rs".to_string()],
                base_diff_hash: String::new(),
                base_created_at: String::new(),
                previous_overlay: Some(previous),
            };
            assert!(stamp_scoped_overlay(&store, &scope, "review", &stamp).unwrap());
            let merged: ScopedReview =
                serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            assert_eq!(merged.scope_paths, vec!["src/a.rs", "src/b.rs", "src/c.rs"]);
            assert!(merged.files.contains_key("src/a.rs"));
            assert!(merged.files.contains_key("src/b.rs"));
            assert!(merged.files.contains_key("src/c.rs"));
        });
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn second_scoped_run_keeps_the_first_paths() {
        let dir = temp_dir("second-run");
        with_test_storage_root(&dir, || {
            let store = crate::store_registry::store();
            let scope = Scope::dir(dir.to_str().unwrap()).unwrap();
            std::fs::create_dir_all(dir.join("scoped/experts")).unwrap();
            let path = dir.join("scoped/general.json");
            let stamp = |scope_paths: Vec<String>| ScopedRunStamp {
                scope_paths,
                base_diff_hash: "hash".to_string(),
                base_created_at: "t0".to_string(),
                previous_overlay: None,
            };

            let first = scoped(vec![("src/a.rs", vec![finding("", "A", 1)])], &["src/a.rs"]);
            std::fs::write(&path, serde_json::to_vec(&first).unwrap()).unwrap();
            assert!(stamp_scoped_overlay(
                &store,
                &scope,
                "review",
                &stamp(vec!["src/a.rs".into()])
            )
            .unwrap());

            let previous = std::fs::read(&path).unwrap();
            let second = scoped(vec![("src/b.rs", vec![finding("", "B", 2)])], &["src/b.rs"]);
            std::fs::write(&path, serde_json::to_vec(&second).unwrap()).unwrap();
            let mut second_stamp = stamp(vec!["src/b.rs".into()]);
            second_stamp.previous_overlay = Some(previous);
            assert!(stamp_scoped_overlay(&store, &scope, "review", &second_stamp).unwrap());

            let merged: ScopedReview =
                serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            assert_eq!(merged.scope_paths, vec!["src/a.rs", "src/b.rs"]);
            assert_eq!(merged.files["src/a.rs"].findings[0].title, "A");
            assert_eq!(merged.files["src/b.rs"].findings[0].title, "B");
        });
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn duplicate_overlay_rows_get_unique_ids() {
        let dir = temp_dir("duplicate-ids");
        with_test_storage_root(&dir, || {
            let store = crate::store_registry::store();
            let scope = Scope::dir(dir.to_str().unwrap()).unwrap();
            std::fs::create_dir_all(dir.join("scoped/experts")).unwrap();
            let overlay = scoped(
                vec![(
                    "src/a.rs",
                    vec![finding("", "Dup", 4), finding("", "Dup", 4)],
                )],
                &["src/a.rs"],
            );
            std::fs::write(
                dir.join("scoped/general.json"),
                serde_json::to_vec(&overlay).unwrap(),
            )
            .unwrap();
            let stamp = ScopedRunStamp {
                scope_paths: vec!["src/a.rs".to_string()],
                base_diff_hash: "hash".to_string(),
                base_created_at: "t0".to_string(),
                previous_overlay: None,
            };
            assert!(stamp_scoped_overlay(&store, &scope, "review", &stamp).unwrap());
            let stamped: ScopedReview =
                serde_json::from_slice(&std::fs::read(dir.join("scoped/general.json")).unwrap())
                    .unwrap();
            let ids: Vec<&str> = stamped.files["src/a.rs"]
                .findings
                .iter()
                .map(|finding| finding.id.as_str())
                .collect();
            assert_eq!(ids.len(), 2);
            assert_ne!(ids[0], ids[1]);
            assert!(ids[0].starts_with("scoped-"));
            assert!(ids[1].starts_with("scoped-"));
        });
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn unchanged_scoped_output_is_kept() {
        use crate::ai::validation::{ValidationOutcome, ValidationResult};
        let dir = temp_dir("unchanged");
        with_test_storage_root(&dir, || {
            let store = crate::store_registry::store();
            let scope = Scope::dir(dir.to_str().unwrap()).unwrap();
            std::fs::create_dir_all(dir.join("scoped/experts")).unwrap();
            let path = dir.join("scoped/general.json");
            let overlay = scoped(
                vec![("src/a.rs", vec![finding("scoped-k", "Kept", 1)])],
                &["src/a.rs"],
            );
            std::fs::write(&path, serde_json::to_vec(&overlay).unwrap()).unwrap();
            let bytes = std::fs::read(&path).unwrap();
            let validation = ValidationReview {
                version: 1,
                diff_hash: "hash".to_string(),
                review_created_at: "t0".to_string(),
                created_at: "t1".to_string(),
                outcomes: vec![ValidationOutcome {
                    key: "k-a".to_string(),
                    result: ValidationResult::Persists,
                    file: "src/a.rs".to_string(),
                    patch: Default::default(),
                }],
            };
            std::fs::write(
                dir.join("validation.json"),
                serde_json::to_vec(&validation).unwrap(),
            )
            .unwrap();
            let stamp = ScopedRunStamp {
                scope_paths: vec!["src/a.rs".to_string()],
                base_diff_hash: "hash".to_string(),
                base_created_at: "t0".to_string(),
                previous_overlay: Some(bytes.clone()),
            };
            assert!(stamp_scoped_overlay(&store, &scope, "review", &stamp).unwrap());
            assert_eq!(std::fs::read(&path).unwrap(), bytes);
            let kept: ValidationReview =
                serde_json::from_slice(&std::fs::read(dir.join("validation.json")).unwrap())
                    .unwrap();
            assert_eq!(
                kept.outcomes.len(),
                1,
                "no row was re-produced, so its validation outcome still applies"
            );
        });
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn stale_base_identity_drops_the_previous_overlay() {
        let dir = temp_dir("stale-previous");
        with_test_storage_root(&dir, || {
            let store = crate::store_registry::store();
            let scope = Scope::dir(dir.to_str().unwrap()).unwrap();
            std::fs::create_dir_all(dir.join("scoped/experts")).unwrap();
            let path = dir.join("scoped/general.json");
            let mut previous = scoped(
                vec![("src/old.rs", vec![finding("", "Old", 1)])],
                &["src/old.rs"],
            );
            previous.base_diff_hash = "older-base".to_string();
            let previous_bytes = serde_json::to_vec(&previous).unwrap();
            std::fs::write(&path, &previous_bytes).unwrap();
            let agent = scoped(
                vec![("src/new.rs", vec![finding("", "New", 2)])],
                &["src/new.rs"],
            );
            std::fs::write(&path, serde_json::to_vec(&agent).unwrap()).unwrap();
            let stamp = ScopedRunStamp {
                scope_paths: vec!["src/new.rs".to_string()],
                base_diff_hash: "hash".to_string(),
                base_created_at: "t0".to_string(),
                previous_overlay: Some(previous_bytes),
            };
            assert!(stamp_scoped_overlay(&store, &scope, "review", &stamp).unwrap());
            let merged: ScopedReview =
                serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            assert!(!merged.files.contains_key("src/old.rs"));
            assert_eq!(merged.scope_paths, vec!["src/new.rs"]);
        });
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn full_run_of_a_lens_drops_its_scoped_overlay() {
        let dir = temp_dir("full-run");
        with_test_storage_root(&dir, || {
            let store = crate::store_registry::store();
            let scope = Scope::dir(dir.to_str().unwrap()).unwrap();
            std::fs::create_dir_all(dir.join("scoped/experts")).unwrap();
            let write = |relative: &str, value: &str| {
                let path = dir.join(relative);
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent).unwrap();
                }
                std::fs::write(path, value).unwrap();
            };
            write("scoped/general.json", r#"{"version":1,"files":{}}"#);
            write("scoped/professor.json", r#"{"version":1,"files":{}}"#);
            write(
                "scoped/experts/security.json",
                r#"{"version":1,"expert_id":"security","files":{}}"#,
            );
            write("validation.json", r#"{"version":1,"outcomes":[]}"#);
            write("review-files.txt", "src/a.rs\n");
            write("review.prev.json", "{}");
            write("professor.prev.json", "{}");
            write("experts/security.prev.json", "{}");

            clear_full_run_artifacts(&store, &scope, "review").unwrap();
            assert!(!dir.join("scoped/general.json").exists());
            assert!(!dir.join("validation.json").exists());
            assert!(!dir.join("review-files.txt").exists());
            assert!(!dir.join("review.prev.json").exists());
            assert!(dir.join("scoped/professor.json").exists());
            assert!(dir.join("scoped/experts/security.json").exists());

            clear_full_run_artifacts(&store, &scope, "expert-security").unwrap();
            assert!(!dir.join("scoped/experts/security.json").exists());
            assert!(!dir.join("experts/security.prev.json").exists());
            assert!(dir.join("scoped/professor.json").exists());

            clear_full_run_artifacts(&store, &scope, "professor").unwrap();
            assert!(!dir.join("scoped/professor.json").exists());
            assert!(!dir.join("professor.prev.json").exists());
        });
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn reply_to_an_overlay_row_writes_the_overlay() {
        let dir = temp_dir("reply");
        with_test_storage_root(&dir, || {
            let er_dir = dir.to_str().unwrap();
            std::fs::create_dir_all(dir.join("scoped/experts")).unwrap();
            let overlay = scoped(
                vec![("src/a.rs", vec![finding("scoped-k", "Row", 1)])],
                &["src/a.rs"],
            );
            std::fs::write(
                dir.join("scoped/general.json"),
                serde_json::to_vec(&overlay).unwrap(),
            )
            .unwrap();
            let mut edit = Some(|f: &mut Finding| -> anyhow::Result<()> {
                f.responses.push(crate::ai::review::AiResponse {
                    id: "r1".to_string(),
                    in_reply_to: String::new(),
                    timestamp: "t1".to_string(),
                    text: "answered".to_string(),
                    new_findings: Vec::new(),
                });
                Ok(())
            });
            assert!(with_finding_in_overlay(er_dir, "scoped-k", &mut edit).unwrap());
            let stored: ScopedReview =
                serde_json::from_slice(&std::fs::read(dir.join("scoped/general.json")).unwrap())
                    .unwrap();
            assert_eq!(
                stored.files["src/a.rs"].findings[0].responses[0].text,
                "answered"
            );
            assert!(!stored.files["src/a.rs"].findings[0].resolved);
        });
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn resolve_an_overlay_row_writes_the_overlay() {
        let dir = temp_dir("resolve");
        with_test_storage_root(&dir, || {
            let er_dir = dir.to_str().unwrap();
            std::fs::create_dir_all(dir.join("scoped/experts")).unwrap();
            let overlay = scoped(
                vec![("src/a.rs", vec![finding("scoped-k", "Row", 1)])],
                &["src/a.rs"],
            );
            std::fs::write(
                dir.join("scoped/general.json"),
                serde_json::to_vec(&overlay).unwrap(),
            )
            .unwrap();
            let mut edit = Some(|f: &mut Finding| -> anyhow::Result<()> {
                f.resolved = true;
                f.resolved_note = "fixed".to_string();
                Ok(())
            });
            assert!(with_finding_in_overlay(er_dir, "scoped-k", &mut edit).unwrap());
            let stored: ScopedReview =
                serde_json::from_slice(&std::fs::read(dir.join("scoped/general.json")).unwrap())
                    .unwrap();
            assert!(stored.files["src/a.rs"].findings[0].resolved);
            assert_eq!(stored.files["src/a.rs"].findings[0].resolved_note, "fixed");
        });
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn edit_finding_overlays_reaches_the_professor_overlay() {
        let dir = temp_dir("edit");
        with_test_storage_root(&dir, || {
            let er_dir = dir.to_str().unwrap();
            std::fs::create_dir_all(dir.join("scoped/experts")).unwrap();
            let overlay = ScopedProfessorReview {
                version: 1,
                diff_hash: "hash".to_string(),
                diff_scope: String::new(),
                created_at: "t1".to_string(),
                focus_prompt: String::new(),
                summary: String::new(),
                files: [(
                    "src/a.rs".to_string(),
                    ProfessorFileReview {
                        findings: vec![finding("scoped-k", "Row", 1)],
                    },
                )]
                .into_iter()
                .collect(),
                scope_paths: vec!["src/a.rs".to_string()],
                base_diff_hash: "hash".to_string(),
                base_created_at: "t0".to_string(),
                host_stamped: true,
            };
            std::fs::write(
                dir.join("scoped/professor.json"),
                serde_json::to_vec(&overlay).unwrap(),
            )
            .unwrap();
            let mut edit = |findings: &mut Vec<Finding>, prefix: Option<&str>| {
                if prefix != Some(PROFESSOR_ID_PREFIX) {
                    return false;
                }
                findings[0].description = "edited".to_string();
                true
            };
            assert!(edit_finding_overlays(er_dir, &mut edit).unwrap());
            let stored: ScopedProfessorReview =
                serde_json::from_slice(&std::fs::read(dir.join("scoped/professor.json")).unwrap())
                    .unwrap();
            assert_eq!(stored.files["src/a.rs"].findings[0].description, "edited");
        });
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn scoped_general_run_drops_outcomes_for_its_paths() {
        use crate::ai::validation::{ValidationOutcome, ValidationResult};
        let dir = temp_dir("a1-outcomes");
        with_test_storage_root(&dir, || {
            let store = crate::store_registry::store();
            let scope = Scope::dir(dir.to_str().unwrap()).unwrap();
            std::fs::create_dir_all(dir.join("scoped/experts")).unwrap();
            let outcome = |key: &str, file: &str| ValidationOutcome {
                key: key.to_string(),
                result: ValidationResult::Resolved,
                file: file.to_string(),
                patch: Default::default(),
            };
            let validation = ValidationReview {
                version: 1,
                diff_hash: "hash".to_string(),
                review_created_at: "t0".to_string(),
                created_at: "t1".to_string(),
                outcomes: vec![outcome("k-a", "src/a.rs"), outcome("k-b", "src/b.rs")],
            };
            std::fs::write(
                dir.join("validation.json"),
                serde_json::to_vec(&validation).unwrap(),
            )
            .unwrap();
            let agent = scoped(
                vec![("src/a.rs", vec![finding("", "New A", 2)])],
                &["src/a.rs"],
            );
            std::fs::write(
                dir.join("scoped/general.json"),
                serde_json::to_vec(&agent).unwrap(),
            )
            .unwrap();
            let stamp = ScopedRunStamp {
                scope_paths: vec!["src/a.rs".to_string()],
                base_diff_hash: "hash".to_string(),
                base_created_at: "t0".to_string(),
                previous_overlay: None,
            };
            assert!(stamp_scoped_overlay(&store, &scope, "review", &stamp).unwrap());

            let stored: ValidationReview =
                serde_json::from_slice(&std::fs::read(dir.join("validation.json")).unwrap())
                    .unwrap();
            let files: Vec<&str> = stored
                .outcomes
                .iter()
                .map(|outcome| outcome.file.as_str())
                .collect();
            assert_eq!(files, ["src/b.rs"], "only re-reviewed paths drop out");
        });
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn stamp_leaves_review_json_byte_identical() {
        let dir = temp_dir("stamp-review-bytes");
        with_test_storage_root(&dir, || {
            let store = crate::store_registry::store();
            let scope = Scope::dir(dir.to_str().unwrap()).unwrap();
            std::fs::create_dir_all(dir.join("scoped/experts")).unwrap();
            let review_bytes: &[u8] =
                br#"{"version":1,"diff_hash":"hash","created_at":"t0","files":{}}"#;
            std::fs::write(dir.join("review.json"), review_bytes).unwrap();
            let agent = scoped(vec![("src/a.rs", vec![finding("", "A", 1)])], &["src/a.rs"]);
            std::fs::write(
                dir.join("scoped/general.json"),
                serde_json::to_vec(&agent).unwrap(),
            )
            .unwrap();
            let stamp = ScopedRunStamp {
                scope_paths: vec!["src/a.rs".to_string()],
                base_diff_hash: "hash".to_string(),
                base_created_at: "t0".to_string(),
                previous_overlay: None,
            };
            assert!(stamp_scoped_overlay(&store, &scope, "review", &stamp).unwrap());
            assert_eq!(
                std::fs::read(dir.join("review.json")).unwrap(),
                review_bytes,
                "a scoped stamp writes only its overlay"
            );
        });
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn stamp_refuses_a_newer_version_overlay() {
        let dir = temp_dir("newer-overlay");
        with_test_storage_root(&dir, || {
            let store = crate::store_registry::store();
            let scope = Scope::dir(dir.to_str().unwrap()).unwrap();
            std::fs::create_dir_all(dir.join("scoped/experts")).unwrap();
            let newer: &[u8] = br#"{"version":99,"files":{}}"#;
            std::fs::write(dir.join("scoped/general.json"), newer).unwrap();
            let stamp = ScopedRunStamp {
                scope_paths: vec!["src/a.rs".to_string()],
                base_diff_hash: String::new(),
                base_created_at: String::new(),
                previous_overlay: None,
            };
            let error = stamp_scoped_overlay(&store, &scope, "review", &stamp).unwrap_err();
            assert!(error.to_string().contains("unreadable"), "{error}");
            assert_eq!(
                std::fs::read(dir.join("scoped/general.json")).unwrap(),
                newer,
                "an overlay a newer build owns is not rewritten"
            );
        });
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn reply_does_not_rewrite_an_unreadable_overlay() {
        let dir = temp_dir("broken-overlay");
        with_test_storage_root(&dir, || {
            // A newer build's overlay parses here unless the store's version
            // check runs, and it names a row this reply would match.
            let newer: &[u8] = br#"{"version":99,"host_stamped":true,"scope_paths":["src/a.rs"],"files":{"src/a.rs":{"risk":"medium","findings":[{"id":"scoped-k","severity":"medium","title":"Row","line_start":1}]}}}"#;
            let broken: &[u8] = b"not json";
            std::fs::create_dir_all(dir.join("scoped/experts")).unwrap();
            for bytes in [newer, broken] {
                std::fs::write(dir.join("scoped/general.json"), bytes).unwrap();
                let mut edit = Some(|_: &mut Finding| -> anyhow::Result<()> { Ok(()) });
                assert!(
                    !with_finding_in_overlay(dir.to_str().unwrap(), "scoped-k", &mut edit).unwrap()
                );
                assert_eq!(
                    std::fs::read(dir.join("scoped/general.json")).unwrap(),
                    bytes,
                    "an overlay this build cannot read is left for the next run to replace"
                );
            }
        });
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn stamp_does_not_rewrite_a_newer_or_unreadable_validation() {
        let dir = temp_dir("newer-validation");
        with_test_storage_root(&dir, || {
            let store = crate::store_registry::store();
            let scope = Scope::dir(dir.to_str().unwrap()).unwrap();
            std::fs::create_dir_all(dir.join("scoped/experts")).unwrap();
            std::fs::write(
                dir.join("review.json"),
                br#"{"version":1,"diff_hash":"hash","created_at":"t0","files":{}}"#,
            )
            .unwrap();
            let agent = scoped(vec![("src/a.rs", vec![finding("", "A", 1)])], &["src/a.rs"]);
            std::fs::write(
                dir.join("scoped/general.json"),
                serde_json::to_vec(&agent).unwrap(),
            )
            .unwrap();
            let stamp = ScopedRunStamp {
                scope_paths: vec!["src/a.rs".to_string()],
                base_diff_hash: "hash".to_string(),
                base_created_at: "t0".to_string(),
                previous_overlay: None,
            };
            for bytes in [&br#"{"version":99,"outcomes":[]}"#[..], b"not json"] {
                std::fs::write(dir.join("validation.json"), bytes).unwrap();
                assert!(stamp_scoped_overlay(&store, &scope, "review", &stamp).unwrap());
                assert_eq!(
                    std::fs::read(dir.join("validation.json")).unwrap(),
                    bytes,
                    "a validation a newer build owns, or one that cannot be read, is left alone"
                );
            }
        });
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn overlay_ids_do_not_collide_with_base_ids() {
        let dir = temp_dir("ids");
        with_test_storage_root(&dir, || {
            let store = crate::store_registry::store();
            let scope = Scope::dir(dir.to_str().unwrap()).unwrap();
            std::fs::create_dir_all(dir.join("scoped/experts")).unwrap();
            let overlay = scoped(
                vec![("src/a.rs", vec![finding("", "Same row", 1)])],
                &["src/a.rs"],
            );
            std::fs::write(
                dir.join("scoped/general.json"),
                serde_json::to_vec(&overlay).unwrap(),
            )
            .unwrap();
            let stamp = ScopedRunStamp {
                scope_paths: vec!["src/a.rs".to_string()],
                base_diff_hash: String::new(),
                base_created_at: String::new(),
                previous_overlay: None,
            };
            assert!(stamp_scoped_overlay(&store, &scope, "review", &stamp).unwrap());

            let stored: ScopedReview =
                serde_json::from_slice(&std::fs::read(dir.join("scoped/general.json")).unwrap())
                    .unwrap();
            let key = finding_key("src/a.rs", Some(1), "Same row");
            let overlay_id = stored.files["src/a.rs"].findings[0].id.clone();
            assert_eq!(overlay_id, format!("scoped-{key}"));
            assert_ne!(overlay_id, key, "the overlay row cannot shadow the base id");
        });
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn scoped_overlay_path_names_one_file_per_lens() {
        assert_eq!(
            scoped_overlay_path("review").as_deref(),
            Some("scoped/general.json")
        );
        assert_eq!(
            scoped_overlay_path("professor").as_deref(),
            Some("scoped/professor.json")
        );
        assert_eq!(
            scoped_overlay_path("expert-security").as_deref(),
            Some("scoped/experts/security.json")
        );
        assert_eq!(scoped_overlay_path("triage"), None);
        assert_eq!(scoped_overlay_path("summary"), None);
    }

    // Both writers start together, and each edit sleeps after its read, so a
    // read-then-write loses one of them.
    #[test]
    fn concurrent_overlay_replies_keep_both_rows() {
        let dir = temp_dir("race-reply");
        crate::storage::with_process_test_storage_root(&dir, || {
            std::fs::create_dir_all(dir.join("scoped/experts")).unwrap();
            let overlay = scoped(
                vec![(
                    "src/a.rs",
                    vec![finding("scoped-a", "A", 1), finding("scoped-b", "B", 2)],
                )],
                &["src/a.rs"],
            );
            std::fs::write(
                dir.join("scoped/general.json"),
                serde_json::to_vec(&overlay).unwrap(),
            )
            .unwrap();
            let er_dir = dir.to_string_lossy().into_owned();
            let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
            let spawn = |id: &'static str, text: &'static str| {
                let er_dir = er_dir.clone();
                let barrier = std::sync::Arc::clone(&barrier);
                std::thread::spawn(move || {
                    barrier.wait();
                    let mut edit = Some(|f: &mut Finding| -> anyhow::Result<()> {
                        std::thread::sleep(std::time::Duration::from_millis(80));
                        f.responses.push(crate::ai::review::AiResponse {
                            id: text.to_string(),
                            in_reply_to: String::new(),
                            timestamp: "t1".to_string(),
                            text: text.to_string(),
                            new_findings: Vec::new(),
                        });
                        Ok(())
                    });
                    assert!(with_finding_in_overlay(&er_dir, id, &mut edit).unwrap());
                })
            };
            let first = spawn("scoped-a", "one");
            let second = spawn("scoped-b", "two");
            barrier.wait();
            first.join().unwrap();
            second.join().unwrap();

            let stored: ScopedReview =
                serde_json::from_slice(&std::fs::read(dir.join("scoped/general.json")).unwrap())
                    .unwrap();
            let rows = &stored.files["src/a.rs"].findings;
            assert!(
                rows.iter()
                    .any(|f| f.responses.iter().any(|r| r.text == "one")),
                "the first reply survived"
            );
            assert!(
                rows.iter()
                    .any(|f| f.responses.iter().any(|r| r.text == "two")),
                "the second reply survived"
            );
        });
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn concurrent_overlay_resolves_keep_both_rows() {
        let dir = temp_dir("race-resolve");
        crate::storage::with_process_test_storage_root(&dir, || {
            std::fs::create_dir_all(dir.join("scoped/experts")).unwrap();
            let overlay = scoped(
                vec![(
                    "src/a.rs",
                    vec![finding("scoped-a", "A", 1), finding("scoped-b", "B", 2)],
                )],
                &["src/a.rs"],
            );
            std::fs::write(
                dir.join("scoped/general.json"),
                serde_json::to_vec(&overlay).unwrap(),
            )
            .unwrap();
            let er_dir = dir.to_string_lossy().into_owned();
            let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
            let spawn = |id: &'static str| {
                let er_dir = er_dir.clone();
                let barrier = std::sync::Arc::clone(&barrier);
                std::thread::spawn(move || {
                    barrier.wait();
                    let mut edit = Some(|f: &mut Finding| -> anyhow::Result<()> {
                        std::thread::sleep(std::time::Duration::from_millis(80));
                        f.resolved = true;
                        f.resolved_note = id.to_string();
                        Ok(())
                    });
                    assert!(with_finding_in_overlay(&er_dir, id, &mut edit).unwrap());
                })
            };
            let first = spawn("scoped-a");
            let second = spawn("scoped-b");
            barrier.wait();
            first.join().unwrap();
            second.join().unwrap();

            let stored: ScopedReview =
                serde_json::from_slice(&std::fs::read(dir.join("scoped/general.json")).unwrap())
                    .unwrap();
            let rows = &stored.files["src/a.rs"].findings;
            assert!(rows.iter().all(|f| f.resolved), "both resolves survived");
        });
        let _ = std::fs::remove_dir_all(dir);
    }

    /// The edit one racing thread applies: rename its own row after a pause,
    /// so a read-then-write has time to overlap the other thread's write.
    fn edit_row(
        id: &'static str,
        note: &'static str,
    ) -> impl FnMut(&mut Vec<Finding>, Option<&str>) -> bool {
        move |findings, _prefix| {
            let Some(finding) = findings.iter_mut().find(|f| f.id == id) else {
                return false;
            };
            std::thread::sleep(std::time::Duration::from_millis(80));
            finding.description = note.to_string();
            true
        }
    }

    #[test]
    fn concurrent_overlay_edits_keep_both_rows() {
        let dir = temp_dir("race-edit");
        crate::storage::with_process_test_storage_root(&dir, || {
            std::fs::create_dir_all(dir.join("scoped/experts")).unwrap();
            let overlay = scoped(
                vec![(
                    "src/a.rs",
                    vec![finding("scoped-a", "A", 1), finding("scoped-b", "B", 2)],
                )],
                &["src/a.rs"],
            );
            std::fs::write(
                dir.join("scoped/general.json"),
                serde_json::to_vec(&overlay).unwrap(),
            )
            .unwrap();
            let er_dir = dir.to_string_lossy().into_owned();
            let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
            let spawn = |id: &'static str, note: &'static str| {
                let er_dir = er_dir.clone();
                let barrier = std::sync::Arc::clone(&barrier);
                std::thread::spawn(move || {
                    barrier.wait();
                    let mut edit = edit_row(id, note);
                    assert!(edit_finding_overlays(&er_dir, &mut edit).unwrap());
                })
            };
            let first = spawn("scoped-a", "first edit");
            let second = spawn("scoped-b", "second edit");
            barrier.wait();
            first.join().unwrap();
            second.join().unwrap();

            let stored: ScopedReview =
                serde_json::from_slice(&std::fs::read(dir.join("scoped/general.json")).unwrap())
                    .unwrap();
            let rows = &stored.files["src/a.rs"].findings;
            assert!(
                rows.iter().any(|f| f.description == "first edit"),
                "the first edit survived"
            );
            assert!(
                rows.iter().any(|f| f.description == "second edit"),
                "the second edit survived"
            );
        });
        let _ = std::fs::remove_dir_all(dir);
    }

    // Each stamp re-produces `src/a.rs` and restores one earlier path from the
    // overlay it captured at spawn. Each must start from the file's current
    // content, or the second stamp drops the path the first one restored.
    #[test]
    fn concurrent_stamps_keep_both_previous_paths() {
        let dir = temp_dir("race-stamp");
        crate::storage::with_process_test_storage_root(&dir, || {
            std::fs::create_dir_all(dir.join("scoped/experts")).unwrap();
            let agent = scoped(vec![("src/a.rs", vec![finding("", "A", 1)])], &["src/a.rs"]);
            std::fs::write(
                dir.join("scoped/general.json"),
                serde_json::to_vec(&agent).unwrap(),
            )
            .unwrap();
            let er_dir = dir.to_string_lossy().into_owned();
            let previous_for = |path: &str, title: &str| {
                serde_json::to_vec(&scoped(vec![(path, vec![finding("", title, 2)])], &[path]))
                    .unwrap()
            };
            let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
            let spawn = |path: &'static str, title: &'static str| {
                let er_dir = er_dir.clone();
                let barrier = std::sync::Arc::clone(&barrier);
                let previous = previous_for(path, title);
                std::thread::spawn(move || {
                    barrier.wait();
                    let store = crate::store_registry::store();
                    let scope = Scope::dir(&er_dir).unwrap();
                    let stamp = ScopedRunStamp {
                        scope_paths: vec!["src/a.rs".to_string()],
                        base_diff_hash: "hash".to_string(),
                        base_created_at: "t0".to_string(),
                        previous_overlay: Some(previous),
                    };
                    assert!(stamp_scoped_overlay(&store, &scope, "review", &stamp).unwrap());
                })
            };
            let first = spawn("src/b.rs", "B");
            let second = spawn("src/c.rs", "C");
            barrier.wait();
            first.join().unwrap();
            second.join().unwrap();

            let stored: ScopedReview =
                serde_json::from_slice(&std::fs::read(dir.join("scoped/general.json")).unwrap())
                    .unwrap();
            for path in ["src/a.rs", "src/b.rs", "src/c.rs"] {
                assert!(
                    stored.files.contains_key(path),
                    "{path} survived the concurrent stamps"
                );
            }
        });
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn concurrent_validation_drops_keep_both_paths() {
        let dir = temp_dir("race-drop");
        crate::storage::with_process_test_storage_root(&dir, || {
            std::fs::create_dir_all(dir.join("scoped/experts")).unwrap();
            let agent = scoped(vec![("src/a.rs", vec![finding("", "A", 1)])], &["src/a.rs"]);
            std::fs::write(
                dir.join("scoped/general.json"),
                serde_json::to_vec(&agent).unwrap(),
            )
            .unwrap();
            let outcome = |key: &str, file: &str| crate::ai::validation::ValidationOutcome {
                key: key.to_string(),
                result: crate::ai::validation::ValidationResult::Persists,
                file: file.to_string(),
                patch: Default::default(),
            };
            let validation = ValidationReview {
                version: 1,
                diff_hash: "hash".to_string(),
                review_created_at: "t0".to_string(),
                created_at: "t1".to_string(),
                outcomes: vec![outcome("k-a", "src/a.rs"), outcome("k-b", "src/b.rs")],
            };
            std::fs::write(
                dir.join("validation.json"),
                serde_json::to_vec(&validation).unwrap(),
            )
            .unwrap();
            let er_dir = dir.to_string_lossy().into_owned();
            let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
            let spawn = |manifest: &'static str| {
                let er_dir = er_dir.clone();
                let barrier = std::sync::Arc::clone(&barrier);
                std::thread::spawn(move || {
                    barrier.wait();
                    let store = crate::store_registry::store();
                    let scope = Scope::dir(&er_dir).unwrap();
                    let stamp = ScopedRunStamp {
                        scope_paths: vec![manifest.to_string()],
                        base_diff_hash: "hash".to_string(),
                        base_created_at: "t0".to_string(),
                        previous_overlay: None,
                    };
                    assert!(stamp_scoped_overlay(&store, &scope, "review", &stamp).unwrap());
                })
            };
            let first = spawn("src/a.rs");
            let second = spawn("src/b.rs");
            barrier.wait();
            first.join().unwrap();
            second.join().unwrap();

            let stored: ValidationReview =
                serde_json::from_slice(&std::fs::read(dir.join("validation.json")).unwrap())
                    .unwrap();
            assert!(
                stored.outcomes.is_empty(),
                "both paths' outcomes were dropped"
            );
        });
        let _ = std::fs::remove_dir_all(dir);
    }
}
