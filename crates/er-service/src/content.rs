//! The content of each open tab, as the Workspace publishes it.
//!
//! A tab's content is three sections behind their own `Arc`s: the diff, the
//! reviewed marks and the AI state. Each section carries the revision that
//! last replaced it, so a publish copies pointers and a reader rebuilds only
//! the sections whose revision moved (ADR 0071).
//!
//! This module holds the types and the patch steps, which run on the actor
//! thread and touch no port. The loads that fill a section run as effects.

use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::sync::Arc;

use er_engine::ai::AiState;
use er_engine::annotations::WrittenPayload;
use er_engine::git::{DiffFile, DiffFileHeader};
use er_engine::tab_identity::ResolvedTab;
use er_engine::TabKey;

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
    pub diff: Arc<DiffSection>,
    pub reviewed: Arc<ReviewedSection>,
    pub ai: Arc<AiSection>,
}

impl TabContent {
    /// The latest revision of any section. A load is applied only over the
    /// content it started from, so a write that landed meanwhile survives.
    pub fn generation(&self) -> u64 {
        self.diff
            .revision
            .max(self.reviewed.revision)
            .max(self.ai.revision)
    }

    pub(crate) fn from_loaded(key: TabKey, loaded: LoadedTab, revision: u64) -> Self {
        Self {
            key,
            tab: loaded.tab,
            context: loaded.context,
            pr_refs_fetched: loaded.pr_refs_fetched,
            diff: Arc::new(DiffSection {
                revision,
                files: loaded.diff.files.into_iter().map(Arc::new).collect(),
                meta: Arc::new(loaded.diff.meta),
            }),
            reviewed: Arc::new(ReviewedSection {
                revision,
                marks: loaded.marks,
                file_hashes: loaded.file_hashes,
            }),
            ai: Arc::new(AiSection {
                revision,
                state: loaded.ai,
            }),
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
        Some(Self {
            ai: Arc::new(AiSection { revision, state }),
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
    /// `Arc`s; the rest are shared with the previous section.
    fn with_parsed(&self, parsed: Vec<(usize, DiffFile)>, revision: u64) -> Self {
        let mut files = self.diff.files.clone();
        for (index, file) in parsed {
            if let Some(slot) = files.get_mut(index) {
                *slot = Arc::new(file);
            }
        }
        Self {
            diff: Arc::new(DiffSection {
                revision,
                files,
                meta: Arc::clone(&self.diff.meta),
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
            .field("diff", &self.diff.revision)
            .field("reviewed", &self.reviewed.revision)
            .field("ai", &self.ai.revision)
            .finish_non_exhaustive()
    }
}

/// The tab's parsed diff.
pub struct DiffSection {
    pub revision: u64,
    pub files: Vec<Arc<DiffFile>>,
    /// Shared with the sections a lazy parse makes from this one.
    pub meta: Arc<DiffMeta>,
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
    pub state: AiState,
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
    /// How many reviewed marks the load cleared.
    pub cleared: usize,
}

pub(crate) struct LoadedDiff {
    pub files: Vec<DiffFile>,
    pub meta: DiffMeta,
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
    /// Lazy files parsed from the diff section at revision `since`.
    Parsed {
        key: TabKey,
        since: u64,
        files: Vec<(usize, DiffFile)>,
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
        ContentPatch::Sidecar { .. }
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
        ContentPatch::Parsed { key, files, .. } => {
            if let Some(content) = tabs.get_mut(&key) {
                *content = Arc::new(content.with_parsed(files, revision));
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
