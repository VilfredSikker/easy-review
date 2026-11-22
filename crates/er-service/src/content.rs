//! The content of each open tab, as the Workspace publishes it.
//!
//! A tab's content is four sections behind their own `Arc`s: the diff, the
//! reviewed marks, the AI state and the PR. Each section carries the revision that
//! last replaced it, so a publish copies pointers and a reader rebuilds only
//! the sections whose revision moved (ADR 0071).
//!
//! This module holds the types and the patch steps, which run on the actor
//! thread and touch no port. The loads that fill a section run as effects.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;
use std::sync::Arc;
use std::time::SystemTime;

use er_engine::ai::{self, AiState};
use er_engine::annotations::WrittenPayload;
use er_engine::git::{CommitInfo, DiffFile, DiffFileHeader};
use er_engine::github::PrOverviewData;
use er_engine::history::ScrollerCommit;
use er_engine::preview_key::{PreviewKeyInputs, PreviewStamps};
use er_engine::tab_identity::{DiffMode, ResolvedTab};
use er_engine::TabKey;

use crate::freshness::Freshness;
use crate::protocol::TabContext;

/// One open tab's content.
#[derive(Clone)]
pub struct TabContent {
    pub key: TabKey,
    /// What the tab reviews, in the mode it shows.
    pub tab: ResolvedTab,
    /// The inputs a command on this tab carries, resolved at its last load.
    pub context: TabContext,
    /// Whether this tab fetched its PR head and base refs, which entering the
    /// PR diff does once.
    pub pr_refs_fetched: bool,
    /// The revision of the load that set `tab.mode`.
    pub mode_revision: u64,
    pub diff: Arc<DiffSection>,
    pub reviewed: Arc<ReviewedSection>,
    pub ai: Arc<AiSection>,
    pub pr: Arc<PrSection>,
    pub freshness: Arc<FreshnessSection>,
    /// History's commits once the tab has shown History, carried across
    /// loads in the other views as the App keeps them.
    pub history: Option<Arc<HistorySection>>,
}

/// How far origin has moved past what the tab shows. A probe's post moves
/// it without a load, so a load over it is never refused.
pub struct FreshnessSection {
    pub revision: u64,
    pub value: Freshness,
}

impl TabContent {
    /// The latest revision of any section. A load is applied only over the
    /// content it started from, so a write that landed meanwhile survives. A
    /// sidecar reload is no write: the load reads the sidecars too.
    pub fn generation(&self) -> u64 {
        self.diff
            .revision
            .max(self.reviewed.revision)
            .max(self.ai.written)
            .max(self.pr.revision)
            .max(self.mode_revision)
    }

    /// The inputs this content's preview keys read. Only History names its
    /// commit, so the other views key the same before and after a visit to it.
    pub fn preview_key_inputs(&self) -> PreviewKeyInputs<'_> {
        let history_commit = self
            .history
            .as_ref()
            .filter(|_| self.tab.mode == DiffMode::History)
            .and_then(|history| history.selected.as_deref());
        PreviewKeyInputs {
            repo_root: &self.tab.repo_root,
            remote_repo: self.tab.remote_repo.as_deref(),
            mode: self.tab.mode,
            checkout_root: self.tab.checkout_root.as_deref(),
            preview_head_oid: self.diff.meta.preview_head_oid.as_deref(),
            history_commit,
            raw: Some(&self.diff.meta.raw),
            diff_hash: &self.diff.meta.diff_hash,
            headers: &self.diff.meta.headers,
            mtimes: &self.diff.meta.stamps.mtimes,
            blob_ids: &self.diff.meta.stamps.blob_ids,
        }
    }

    pub(crate) fn from_loaded(key: TabKey, loaded: LoadedTab, revision: u64) -> Self {
        let pr_number = loaded.tab.pr_number;
        Self {
            key,
            tab: loaded.tab,
            context: loaded.context,
            pr_refs_fetched: loaded.pr_refs_fetched,
            mode_revision: revision,
            diff: Arc::new(DiffSection {
                revision,
                files: loaded.diff.files.into_iter().map(Arc::new).collect(),
                meta: Arc::new(loaded.diff.meta),
                expanded: loaded.diff.expanded,
            }),
            reviewed: Arc::new(ReviewedSection {
                revision,
                marks: loaded.marks,
                file_hashes: loaded.file_hashes,
            }),
            ai: Arc::new(AiSection {
                revision,
                written: revision,
                state: loaded.ai,
                stamp: loaded.ai_stamp,
            }),
            pr: Arc::new(PrSection {
                revision,
                pr_number,
                overview: loaded.pr.overview,
                commits: loaded.pr.commits,
                head_oid: loaded.pr.head_oid,
            }),
            freshness: Arc::new(FreshnessSection {
                revision,
                value: loaded.freshness,
            }),
            history: loaded.history,
        }
    }

    /// This content with its AI state read again from the sidecars.
    fn with_ai(&self, ai: ReloadedAi, revision: u64) -> Self {
        Self {
            ai: Arc::new(AiSection {
                revision,
                written: self.ai.written,
                state: ai.state,
                stamp: ai.stamp,
            }),
            ..self.clone()
        }
    }

    /// This content with one stored sidecar adopted, when it reads that
    /// sidecar's bucket.
    fn with_sidecar(&self, dir: &str, payload: &WrittenPayload, revision: u64) -> Option<Self> {
        let reads = match payload {
            WrittenPayload::Questions(_) | WrittenPayload::Notes(_) => self.context.er_dir == dir,
            WrittenPayload::Comments(_) => self.context.comments_dir == dir,
        };
        if !reads {
            return None;
        }
        let mut state = self.ai.state.clone();
        payload.clone().adopt(&mut state);
        // The stamp stays: the next reload reads this write back once.
        let stamp = self.ai.stamp.clone();
        Some(Self {
            ai: Arc::new(AiSection {
                revision,
                written: revision,
                state,
                stamp,
            }),
            ..self.clone()
        })
    }

    /// This content with the saved reviewed marks, when it reads `er_dir`.
    fn with_marks(
        &self,
        er_dir: &str,
        marks: &HashMap<String, String>,
        revision: u64,
    ) -> Option<Self> {
        if self.context.er_dir != er_dir {
            return None;
        }
        Some(Self {
            reviewed: Arc::new(ReviewedSection {
                revision,
                marks: marks.clone(),
                file_hashes: self.reviewed.file_hashes.clone(),
            }),
            ..self.clone()
        })
    }

    /// This content with lazy files parsed. Only the parsed files get new
    /// `Arc`s; the rest are shared with the previous section. An expanded
    /// file stays expanded across the next loads.
    fn with_parsed(
        &self,
        parsed: Vec<(usize, DiffFile)>,
        expanded: &[String],
        revision: u64,
    ) -> Self {
        let mut files = self.diff.files.clone();
        for (index, file) in parsed {
            if let Some(slot) = files.get_mut(index) {
                *slot = Arc::new(file);
            }
        }
        let mut kept = self.diff.expanded.clone();
        kept.extend(expanded.iter().cloned());
        Self {
            diff: Arc::new(DiffSection {
                revision,
                files,
                meta: Arc::clone(&self.diff.meta),
                expanded: kept,
            }),
            ..self.clone()
        }
    }
}

impl fmt::Debug for TabContent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TabContent")
            .field("key", &self.key)
            .field("mode", &self.tab.mode)
            .field("mode_revision", &self.mode_revision)
            .field("diff", &self.diff.revision)
            .field("reviewed", &self.reviewed.revision)
            .field("ai", &self.ai.revision)
            .field("pr", &self.pr.revision)
            .field("freshness", &self.freshness.revision)
            .finish_non_exhaustive()
    }
}

/// The tab's parsed diff.
pub struct DiffSection {
    pub revision: u64,
    pub files: Vec<Arc<DiffFile>>,
    /// Shared with the sections a lazy parse makes from this one.
    pub meta: Arc<DiffMeta>,
    /// The compacted files the user expanded, which a load parses in full.
    pub expanded: BTreeSet<String>,
}

/// What a diff section keeps beside its files.
pub struct DiffMeta {
    pub headers: Vec<DiffFileHeader>,
    /// The raw diff: a lazy file parses from it, and a mark hashes its file
    /// in it.
    pub raw: String,
    /// Whether some files are stubs that a parse fills.
    pub lazy: bool,
    pub diff_hash: String,
    /// The SHA-256 of the branch diff, which the sidecars are checked against.
    pub branch_diff_hash: String,
    /// The PR head the diff was taken at, for the views that preview it.
    pub preview_head_oid: Option<String>,
    /// What the preview keys read beside the diff, taken when it loaded.
    pub stamps: PreviewStamps,
    /// The checkout's unstaged and staged counts, which the scope picker
    /// shows without switching views. Zero for a tab with no live checkout.
    pub scope_stats: ScopeStats,
    /// The key the repo's `[file_kinds]` and risk tables live under, and its
    /// committed risk table, read when the diff loaded.
    pub rules_key: String,
    pub risk_artifact: Option<er_engine::config::RiskRuleTable>,
    /// The commit scroller's rows, read with the diff in every view.
    pub commits: Vec<ScrollerCommit>,
}

/// The commits History lists and the one it shows.
#[derive(Debug)]
pub struct HistorySection {
    /// Newest first.
    pub commits: Vec<CommitInfo>,
    /// Whether a later page would add nothing.
    pub all_loaded: bool,
    /// The commit whose diff the diff section holds while the tab shows
    /// History; the first listed until one is picked.
    pub selected: Option<String>,
    /// The view History was entered from, which leaving it returns to.
    pub return_mode: DiffMode,
}

/// Files, added and deleted lines in the working tree and in the index.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ScopeStats {
    pub unstaged: (usize, usize, usize),
    pub staged: (usize, usize, usize),
}

/// The tab's reviewed marks, as saved.
pub struct ReviewedSection {
    pub revision: u64,
    /// Path to the file's diff hash when it was marked.
    pub marks: HashMap<String, String>,
    /// The reviewed paths' hashes in the diff of the last full refresh.
    pub file_hashes: HashMap<String, String>,
}

/// The tab's AI state, assembled from its sidecars.
pub struct AiSection {
    pub revision: u64,
    /// The revision of the last load or write, which a reload keeps.
    pub written: u64,
    pub state: AiState,
    /// The sidecars as they stood when `state` was read.
    pub stamp: SidecarStamp,
}

/// The PR a tab reviews: what GitHub says about it, and the head its diff
/// was taken at. Empty for a tab with no PR.
pub struct PrSection {
    pub revision: u64,
    /// The PR this section was filled for; a tab that gains a PR fills again.
    pub pr_number: Option<u64>,
    pub overview: Option<PrOverviewData>,
    /// Newest first.
    pub commits: Vec<CommitInfo>,
    /// The PR head the diff was taken at, which the stale pill compares with
    /// the newest head.
    pub head_oid: Option<String>,
}

/// When a bucket's sidecars last changed, as an external writer leaves them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SidecarStamp {
    pub latest: Option<SystemTime>,
    pub arbiter: Option<Vec<u8>>,
}

impl SidecarStamp {
    pub(crate) fn read(er_dir: &str) -> Self {
        Self {
            latest: ai::latest_er_mtime(er_dir),
            arbiter: ai::arbiter_overlay_stamp(er_dir),
        }
    }

    /// Whether the sidecars moved between `self` and `now`: one is newer,
    /// the first appeared, the last was deleted, or the arbiter changed.
    pub(crate) fn moved_to(&self, now: &Self) -> bool {
        if self.arbiter != now.arbiter {
            return true;
        }
        match now.latest {
            Some(latest) => self.latest.is_none_or(|last| latest > last),
            None => self.latest.is_some(),
        }
    }
}

/// What a load effect read, before the actor puts it behind `Arc`s.
pub(crate) struct LoadedTab {
    pub tab: ResolvedTab,
    pub context: TabContext,
    pub pr_refs_fetched: bool,
    pub diff: LoadedDiff,
    pub marks: HashMap<String, String>,
    pub file_hashes: HashMap<String, String>,
    pub ai: AiState,
    pub ai_stamp: SidecarStamp,
    pub pr: LoadedPr,
    pub freshness: Freshness,
    pub history: Option<Arc<HistorySection>>,
    /// How many reviewed marks the load cleared.
    pub cleared: usize,
}

pub(crate) struct LoadedDiff {
    pub files: Vec<DiffFile>,
    pub meta: DiffMeta,
    pub expanded: BTreeSet<String>,
}

#[derive(Default)]
pub(crate) struct LoadedPr {
    pub overview: Option<PrOverviewData>,
    pub commits: Vec<CommitInfo>,
    pub head_oid: Option<String>,
}

/// An AI state read again from the sidecars, with their stamp.
pub(crate) struct ReloadedAi {
    pub state: AiState,
    pub stamp: SidecarStamp,
}

/// What an effect changed in the tabs' content. The actor applies it after
/// the effect returns, on its own thread.
pub(crate) enum ContentPatch {
    /// A load or refresh of one tab. `since` is the generation the load
    /// started from, `None` for a tab not loaded yet.
    Loaded {
        key: TabKey,
        since: Option<u64>,
        loaded: Box<LoadedTab>,
    },
    /// Lazy files parsed from the diff section at revision `since`;
    /// `expanded` names the compacted ones the parse expanded.
    Parsed {
        key: TabKey,
        since: u64,
        files: Vec<(usize, DiffFile)>,
        expanded: Vec<String>,
    },
    /// The tab's AI state read again from its sidecars, over the AI section
    /// at revision `since`. A section that moved meanwhile keeps the newer
    /// state, and the patch drops without an error: the read was a refresh,
    /// and the next one reads again.
    Ai {
        key: TabKey,
        since: u64,
        ai: Box<ReloadedAi>,
    },
    /// A sidecar a write stored in `dir`.
    Sidecar {
        dir: String,
        payload: WrittenPayload,
    },
    /// The reviewed marks a write saved in `er_dir`.
    Reviewed {
        er_dir: String,
        marks: HashMap<String, String>,
    },
    /// What a probe's tips show against the tab, measured off the actor.
    Freshness {
        key: TabKey,
        freshness: Box<Freshness>,
    },
    /// The tab closed: its content leaves the view.
    Dropped {
        key: TabKey,
    },
    Several(Vec<ContentPatch>),
}

/// Why a patch was not applied.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum PatchRefused {
    /// The tab was closed or changed after the effect started.
    Moved,
}

impl fmt::Display for PatchRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Moved => write!(f, "the tab changed while it loaded; refresh it again"),
        }
    }
}

/// The keys `patch` loads into the view and the keys it drops from it.
pub(crate) fn lifecycle_keys(
    patch: &ContentPatch,
    loads: &mut Vec<TabKey>,
    drops: &mut Vec<TabKey>,
) {
    match patch {
        ContentPatch::Loaded { key, .. } => loads.push(*key),
        ContentPatch::Dropped { key } => drops.push(*key),
        ContentPatch::Several(patches) => {
            for patch in patches {
                lifecycle_keys(patch, loads, drops);
            }
        }
        ContentPatch::Parsed { .. }
        | ContentPatch::Ai { .. }
        | ContentPatch::Freshness { .. }
        | ContentPatch::Sidecar { .. }
        | ContentPatch::Reviewed { .. } => {}
    }
}

/// Checks that `patch` still applies to `tabs`, before any revision moves.
pub(crate) fn check(
    tabs: &BTreeMap<TabKey, Arc<TabContent>>,
    patch: &ContentPatch,
) -> Result<(), PatchRefused> {
    match patch {
        ContentPatch::Loaded { key, since, .. } => {
            if tabs.get(key).map(|content| content.generation()) == *since {
                Ok(())
            } else {
                Err(PatchRefused::Moved)
            }
        }
        ContentPatch::Parsed { key, since, .. } => match tabs.get(key) {
            Some(content) if content.diff.revision == *since => Ok(()),
            _ => Err(PatchRefused::Moved),
        },
        ContentPatch::Several(patches) => patches.iter().try_for_each(|patch| check(tabs, patch)),
        ContentPatch::Ai { .. }
        | ContentPatch::Freshness { .. }
        | ContentPatch::Sidecar { .. }
        | ContentPatch::Reviewed { .. }
        | ContentPatch::Dropped { .. } => Ok(()),
    }
}

/// Applies a checked patch at `revision`. A sidecar or reviewed patch reaches
/// every tab that reads its bucket, duplicate tabs included. Answers how many
/// reviewed marks a load cleared.
pub(crate) fn apply(
    tabs: &mut BTreeMap<TabKey, Arc<TabContent>>,
    patch: ContentPatch,
    revision: u64,
) -> usize {
    match patch {
        ContentPatch::Loaded { key, loaded, .. } => {
            let cleared = loaded.cleared;
            tabs.insert(
                key,
                Arc::new(TabContent::from_loaded(key, *loaded, revision)),
            );
            cleared
        }
        ContentPatch::Parsed {
            key,
            files,
            expanded,
            ..
        } => {
            if let Some(content) = tabs.get_mut(&key) {
                *content = Arc::new(content.with_parsed(files, &expanded, revision));
            }
            0
        }
        ContentPatch::Ai { key, since, ai } => {
            if let Some(content) = tabs.get_mut(&key).filter(|c| c.ai.revision == since) {
                *content = Arc::new(content.with_ai(*ai, revision));
            }
            0
        }
        ContentPatch::Sidecar { dir, payload } => {
            for content in tabs.values_mut() {
                if let Some(next) = content.with_sidecar(&dir, &payload, revision) {
                    *content = Arc::new(next);
                }
            }
            0
        }
        ContentPatch::Reviewed { er_dir, marks } => {
            for content in tabs.values_mut() {
                if let Some(next) = content.with_marks(&er_dir, &marks, revision) {
                    *content = Arc::new(next);
                }
            }
            0
        }
        ContentPatch::Freshness { key, freshness } => {
            if let Some(content) = tabs.get_mut(&key) {
                // An unchanged measure keeps its revision, so the diff's
                // version moves only when what it shows does.
                if content.freshness.value != *freshness {
                    let mut next = TabContent::clone(content);
                    next.freshness = Arc::new(FreshnessSection {
                        revision,
                        value: *freshness,
                    });
                    *content = Arc::new(next);
                }
            }
            0
        }
        ContentPatch::Dropped { key } => {
            tabs.remove(&key);
            0
        }
        ContentPatch::Several(patches) => patches
            .into_iter()
            .map(|patch| apply(tabs, patch, revision))
            .sum(),
    }
}
