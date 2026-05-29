use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

const INBOX_SCHEMA_VERSION: u32 = 1;
const MAX_ITEMS: usize = 200;
pub const CI_TTL_MS: u64 = 10 * 60 * 1000;
pub const REFRESH_ERROR_TTL_MS: u64 = 10 * 60 * 1000;

pub type InboxHandle = Arc<Mutex<InboxState>>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InboxTarget {
    pub project_id: Option<String>,
    pub repo_root: Option<String>,
    pub remote: Option<String>,
    pub pr_number: Option<u64>,
    pub branch: Option<String>,
    pub url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InboxItem {
    pub id: String,
    pub kind: String,
    pub severity: String,
    pub title: String,
    pub body: String,
    pub source: String,
    pub target: InboxTarget,
    pub created_at_ms: u64,
    pub read_at_ms: Option<u64>,
    pub dedupe_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ObservedPrState {
    pub review_decision: Option<String>,
    pub requested_reviewers: Vec<String>,
    pub pr_state: String,
    pub is_my_pr: bool,
    pub check_state: Option<String>,
    pub failing_checks: Vec<String>,
    #[serde(default)]
    pub in_to_review: bool,
    /// Permanent once preemptive triage succeeds or is skipped (review already exists).
    #[serde(default)]
    pub triage_done: bool,
    /// Set when a triage attempt failed so one retry is allowed while still in To Review.
    #[serde(default)]
    pub triage_failed: bool,
    #[serde(default, rename = "triage_head_oid", skip_serializing)]
    triage_head_oid: Option<String>,
}

impl ObservedPrState {
    pub fn migrate_legacy_fields(&mut self) {
        if !self.triage_done && self.triage_head_oid.is_some() {
            self.triage_done = true;
        }
        self.triage_head_oid = None;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObservedCiState {
    pub fetched_at_ms: u64,
    pub check_state: String,
    pub failing_checks: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct InboxFile {
    version: u32,
    items: Vec<InboxItem>,
    observed_pr: HashMap<String, ObservedPrState>,
    ci_state: HashMap<String, ObservedCiState>,
    notified_item_ids: Vec<String>,
    refresh_error_at_ms: HashMap<String, u64>,
    #[serde(default)]
    last_refresh_ms: u64,
}

#[derive(Debug, Clone, Default)]
pub struct InboxState {
    pub items: Vec<InboxItem>,
    pub observed_pr: HashMap<String, ObservedPrState>,
    pub ci_state: HashMap<String, ObservedCiState>,
    pub notified_item_ids: HashSet<String>,
    pub refresh_error_at_ms: HashMap<String, u64>,
    pub last_refresh_ms: u64,
}

impl InboxState {
    pub fn add_item(&mut self, mut item: InboxItem) -> bool {
        if self.items.iter().any(|i| i.dedupe_key == item.dedupe_key) {
            return false;
        }
        if item.id.is_empty() {
            item.id = format!("inbox-{}", item.created_at_ms);
        }
        self.items.push(item);
        self.items
            .sort_by(|a, b| b.created_at_ms.cmp(&a.created_at_ms));
        if self.items.len() > MAX_ITEMS {
            self.items.truncate(MAX_ITEMS);
        }
        true
    }

    pub fn mark_item_read(&mut self, id: &str, now_ms: u64) -> bool {
        if let Some(item) = self.items.iter_mut().find(|i| i.id == id) {
            if item.read_at_ms.is_none() {
                item.read_at_ms = Some(now_ms);
            }
            return true;
        }
        false
    }

    pub fn mark_all_read(&mut self, now_ms: u64) {
        for item in &mut self.items {
            if item.read_at_ms.is_none() {
                item.read_at_ms = Some(now_ms);
            }
        }
    }

    pub fn clear_read(&mut self) {
        self.items.retain(|i| i.read_at_ms.is_none());
    }

    pub fn unread_count(&self) -> usize {
        self.items.iter().filter(|i| i.read_at_ms.is_none()).count()
    }
}

fn inbox_path() -> Option<PathBuf> {
    let dir = dirs::config_dir()?.join("er");
    Some(dir.join("inbox.json"))
}

pub fn now_epoch_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub fn load_inbox_state() -> InboxState {
    let Some(path) = inbox_path() else {
        return InboxState::default();
    };
    let Ok(raw) = std::fs::read_to_string(path) else {
        return InboxState::default();
    };
    let Ok(file) = serde_json::from_str::<InboxFile>(&raw) else {
        return InboxState::default();
    };
    if file.version != INBOX_SCHEMA_VERSION {
        return InboxState::default();
    }
    let mut observed_pr = file.observed_pr;
    for obs in observed_pr.values_mut() {
        obs.migrate_legacy_fields();
    }
    InboxState {
        items: file.items,
        observed_pr,
        ci_state: file.ci_state,
        notified_item_ids: file.notified_item_ids.into_iter().collect(),
        refresh_error_at_ms: file.refresh_error_at_ms,
        last_refresh_ms: file.last_refresh_ms,
    }
}

pub fn save_inbox_state(handle: &InboxHandle) {
    let Some(path) = inbox_path() else {
        return;
    };
    let snapshot = handle.lock().ok().map(|g| g.clone()).unwrap_or_default();
    let payload = InboxFile {
        version: INBOX_SCHEMA_VERSION,
        items: snapshot.items,
        observed_pr: snapshot.observed_pr,
        ci_state: snapshot.ci_state,
        notified_item_ids: snapshot.notified_item_ids.into_iter().collect(),
        refresh_error_at_ms: snapshot.refresh_error_at_ms,
        last_refresh_ms: snapshot.last_refresh_ms,
    };
    let Ok(json) = serde_json::to_string_pretty(&payload) else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let tmp = path.with_extension("json.tmp");
    if std::fs::write(&tmp, json).is_ok() {
        let _ = std::fs::rename(&tmp, &path);
    }
}
