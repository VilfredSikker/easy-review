use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

const INBOX_SCHEMA_VERSION: u32 = 1;
const MAX_ITEMS: usize = 200;
const MAX_SEEN_NOTIFICATIONS: usize = 500;
pub const CI_TTL_MS: u64 = 10 * 60 * 1000;
pub const REFRESH_ERROR_TTL_MS: u64 = 10 * 60 * 1000;

pub type InboxHandle = Arc<Mutex<InboxState>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InboxCategory {
    PrComment,
    ReviewReceived,
    CommentReply,
    Approved,
    ChangesRequested,
    ReviewRequested,
    Mention,
    Ci,
    Lifecycle,
    Ai,
    Other,
}

impl InboxCategory {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PrComment => "pr_comment",
            Self::ReviewReceived => "review_received",
            Self::CommentReply => "comment_reply",
            Self::Approved => "approved",
            Self::ChangesRequested => "changes_requested",
            Self::ReviewRequested => "review_requested",
            Self::Mention => "mention",
            Self::Ci => "ci",
            Self::Lifecycle => "lifecycle",
            Self::Ai => "ai",
            Self::Other => "other",
        }
    }

    #[cfg(test)]
    pub fn label(self) -> &'static str {
        match self {
            Self::PrComment => "Comment on your PR",
            Self::ReviewReceived => "Review received",
            Self::CommentReply => "Reply on comment",
            Self::Approved => "Approved",
            Self::ChangesRequested => "Changes requested",
            Self::ReviewRequested => "Review requested",
            Self::Mention => "Mention",
            Self::Ci => "CI",
            Self::Lifecycle => "Merged / closed",
            Self::Ai => "AI",
            Self::Other => "Other",
        }
    }
}

pub fn category_for_kind(kind: &str) -> InboxCategory {
    match kind {
        "pr_comment" | "new_comment" | "comment" => InboxCategory::PrComment,
        "pr_review_received" => InboxCategory::ReviewReceived,
        "pr_comment_reply" => InboxCategory::CommentReply,
        "pr_review_approved" => InboxCategory::Approved,
        "pr_review_changes_requested" => InboxCategory::ChangesRequested,
        "review_requested" | "review_rerequested" | "review" => InboxCategory::ReviewRequested,
        "mention" => InboxCategory::Mention,
        "ci_failed" | "ci-fail" | "check_failed" => InboxCategory::Ci,
        "pr_merged" | "pr_closed" | "merged" => InboxCategory::Lifecycle,
        "ai_review_done"
        | "ai_review_failed"
        | "ai_triage_done"
        | "ai_triage_failed"
        | "ai_review_cancelled" => InboxCategory::Ai,
        _ => InboxCategory::Other,
    }
}

pub fn is_review_edge_kind(kind: &str) -> bool {
    matches!(
        kind,
        "pr_review_approved" | "pr_review_changes_requested" | "pr_review_received"
    )
}

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
    /// Last seen head commit SHA (for push detection).
    #[serde(default)]
    pub head_oid: String,
    /// Head SHA we already queued auto-triage for (opt-in projects only).
    #[serde(default)]
    pub triaged_head_oid: Option<String>,
    /// Latest review state per reviewer login.
    #[serde(default)]
    pub latest_reviewer_states: Vec<(String, String)>,
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
    #[serde(default)]
    seen_notification_ids: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct InboxState {
    pub items: Vec<InboxItem>,
    pub observed_pr: HashMap<String, ObservedPrState>,
    pub ci_state: HashMap<String, ObservedCiState>,
    pub notified_item_ids: HashSet<String>,
    pub refresh_error_at_ms: HashMap<String, u64>,
    pub last_refresh_ms: u64,
    pub seen_notification_ids: Vec<String>,
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
            .sort_by_key(|item| std::cmp::Reverse(item.created_at_ms));
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

    pub fn has_seen_notification(&self, id: &str) -> bool {
        self.seen_notification_ids.iter().any(|s| s == id)
    }

    pub fn remember_notification(&mut self, id: String) {
        if self.has_seen_notification(&id) {
            return;
        }
        self.seen_notification_ids.push(id);
        if self.seen_notification_ids.len() > MAX_SEEN_NOTIFICATIONS {
            let drop_n = self.seen_notification_ids.len() - MAX_SEEN_NOTIFICATIONS;
            self.seen_notification_ids.drain(0..drop_n);
        }
    }
}

/// PR fields the inbox edge detector needs. Decoupled from the snapshot `PrInfo`.
#[derive(Debug, Clone)]
pub struct PrInboxView {
    pub number: u64,
    pub title: String,
    pub head_ref: String,
    pub state: String,
    pub author: String,
    pub requested_reviewers: Vec<String>,
    pub review_decision: Option<String>,
    pub latest_reviewer_states: Vec<(String, String)>,
}

pub struct PrTransitionCtx<'a> {
    pub remote: &'a str,
    pub gh_user: &'a str,
    pub now_ms: u64,
    pub project_id: Option<String>,
    pub repo_root: Option<String>,
}

/// GitHub notification fields the inbox mapper needs.
#[derive(Debug, Clone)]
pub struct InboxNotification {
    pub id: String,
    pub reason: String,
    pub title: String,
    pub remote: String,
    pub pr_number: Option<u64>,
    pub subject_type: String,
}

pub struct NotificationInboxCtx<'a> {
    pub now_ms: u64,
    pub allowed_remotes: &'a HashSet<String>,
    pub project_id: Option<String>,
    pub repo_root: Option<String>,
    pub skip_review_prs: &'a HashSet<(String, u64)>,
}

pub fn inbox_items_from_pr_transition(
    prev: Option<&ObservedPrState>,
    pr: &PrInboxView,
    ctx: &PrTransitionCtx<'_>,
) -> Vec<InboxItem> {
    let Some(prev_state) = prev else {
        return Vec::new();
    };
    let mut items = Vec::new();
    let is_my_pr = pr.author == ctx.gh_user;

    if is_my_pr {
        items.extend(review_items_for_my_pr(prev_state, pr, ctx));
    } else {
        let requested_me = pr.requested_reviewers.iter().any(|r| r == ctx.gh_user);
        let prev_requested = prev_state
            .requested_reviewers
            .iter()
            .any(|r| r == ctx.gh_user);
        if requested_me && !prev_requested {
            let kind = if prev_state
                .requested_reviewers
                .iter()
                .any(|r| r == ctx.gh_user)
            {
                "review_rerequested"
            } else {
                "review_requested"
            };
            items.push(pr_item(
                kind,
                "info",
                format!("Review requested: PR #{}", pr.number),
                pr.title.clone(),
                ctx,
                pr,
                format!("github:{}:{}:{kind}", ctx.remote, pr.number),
                None,
            ));
        }
    }

    if prev_state.pr_state != pr.state {
        if pr.state == "MERGED" {
            items.push(pr_item(
                "pr_merged",
                "success",
                format!("PR #{} merged", pr.number),
                pr.title.clone(),
                ctx,
                pr,
                format!("github:{}:{}:merged", ctx.remote, pr.number),
                None,
            ));
        } else if pr.state == "CLOSED" {
            items.push(pr_item(
                "pr_closed",
                "info",
                format!("PR #{} closed", pr.number),
                pr.title.clone(),
                ctx,
                pr,
                format!("github:{}:{}:closed", ctx.remote, pr.number),
                None,
            ));
        }
    }

    items
}

fn review_items_for_my_pr(
    prev_state: &ObservedPrState,
    pr: &PrInboxView,
    ctx: &PrTransitionCtx<'_>,
) -> Vec<InboxItem> {
    if !pr.latest_reviewer_states.is_empty() {
        return reviewer_state_items(prev_state, pr, ctx);
    }
    aggregate_review_decision_items(prev_state, pr, ctx)
}

fn reviewer_state_items(
    prev_state: &ObservedPrState,
    pr: &PrInboxView,
    ctx: &PrTransitionCtx<'_>,
) -> Vec<InboxItem> {
    let prev_map: HashMap<&str, &str> = prev_state
        .latest_reviewer_states
        .iter()
        .map(|(login, state)| (login.as_str(), state.as_str()))
        .collect();
    let mut items = Vec::new();
    for (login, state) in &pr.latest_reviewer_states {
        if login == ctx.gh_user {
            continue;
        }
        if prev_map.get(login.as_str()) == Some(&state.as_str()) {
            continue;
        }
        let (kind, severity, title) = match state.as_str() {
            "APPROVED" => (
                "pr_review_approved",
                "success",
                format!("{login} approved PR #{}", pr.number),
            ),
            "CHANGES_REQUESTED" => (
                "pr_review_changes_requested",
                "warning",
                format!("{login} requested changes on PR #{}", pr.number),
            ),
            "COMMENTED" => (
                "pr_review_received",
                "info",
                format!("{login} reviewed PR #{}", pr.number),
            ),
            _ => continue,
        };
        items.push(pr_item(
            kind,
            severity,
            title,
            pr.title.clone(),
            ctx,
            pr,
            format!(
                "github:{}:{}:reviewer:{login}:{state}",
                ctx.remote, pr.number
            ),
            Some(login.as_str()),
        ));
    }
    items
}

fn aggregate_review_decision_items(
    prev_state: &ObservedPrState,
    pr: &PrInboxView,
    ctx: &PrTransitionCtx<'_>,
) -> Vec<InboxItem> {
    let mut items = Vec::new();
    if pr.review_decision.as_deref() == Some("APPROVED")
        && prev_state.review_decision.as_deref() != Some("APPROVED")
    {
        items.push(pr_item(
            "pr_review_approved",
            "success",
            format!("PR #{} approved", pr.number),
            pr.title.clone(),
            ctx,
            pr,
            format!(
                "github:{}:{}:review_decision:APPROVED",
                ctx.remote, pr.number
            ),
            None,
        ));
    }
    if pr.review_decision.as_deref() == Some("CHANGES_REQUESTED")
        && prev_state.review_decision.as_deref() != Some("CHANGES_REQUESTED")
    {
        items.push(pr_item(
            "pr_review_changes_requested",
            "warning",
            format!("Changes requested on PR #{}", pr.number),
            pr.title.clone(),
            ctx,
            pr,
            format!(
                "github:{}:{}:review_decision:CHANGES_REQUESTED",
                ctx.remote, pr.number
            ),
            None,
        ));
    }
    items
}

#[allow(clippy::too_many_arguments)]
fn pr_item(
    kind: &str,
    severity: &str,
    title: String,
    body: String,
    ctx: &PrTransitionCtx<'_>,
    pr: &PrInboxView,
    dedupe_key: String,
    reviewer: Option<&str>,
) -> InboxItem {
    let id = match reviewer {
        Some(login) => format!(
            "inbox-{kind}-{}-{}-{login}-{}",
            ctx.remote, pr.number, ctx.now_ms
        ),
        None => format!("inbox-{kind}-{}-{}-{}", ctx.remote, pr.number, ctx.now_ms),
    };
    InboxItem {
        id,
        kind: kind.to_string(),
        severity: severity.to_string(),
        title,
        body,
        source: "github".to_string(),
        target: InboxTarget {
            project_id: ctx.project_id.clone(),
            repo_root: ctx.repo_root.clone(),
            remote: Some(ctx.remote.to_string()),
            pr_number: Some(pr.number),
            branch: Some(pr.head_ref.clone()),
            url: None,
        },
        created_at_ms: ctx.now_ms,
        read_at_ms: None,
        dedupe_key,
    }
}

pub fn inbox_item_from_notification(
    note: &InboxNotification,
    ctx: &NotificationInboxCtx<'_>,
) -> Option<InboxItem> {
    if !note.subject_type.eq_ignore_ascii_case("PullRequest") {
        return None;
    }
    if !ctx.allowed_remotes.contains(&note.remote) {
        return None;
    }
    let pr_number = note.pr_number?;
    if ctx
        .skip_review_prs
        .contains(&(note.remote.clone(), pr_number))
    {
        return None;
    }
    let reason = note.reason.to_ascii_lowercase();
    let (kind, title) = match reason.as_str() {
        "author" => ("pr_comment", format!("Comment on your PR #{pr_number}")),
        "comment" => ("pr_comment_reply", format!("Reply on PR #{pr_number}")),
        "mention" | "team_mention" => ("mention", format!("You were mentioned in PR #{pr_number}")),
        _ => return None,
    };
    Some(InboxItem {
        id: format!("inbox-{kind}-{}-{pr_number}-{}", note.remote, note.id),
        kind: kind.to_string(),
        severity: "info".to_string(),
        title,
        body: note.title.clone(),
        source: "github".to_string(),
        target: InboxTarget {
            project_id: ctx.project_id.clone(),
            repo_root: ctx.repo_root.clone(),
            remote: Some(note.remote.clone()),
            pr_number: Some(pr_number),
            branch: None,
            url: Some(format!(
                "https://github.com/{}/pull/{pr_number}",
                note.remote
            )),
        },
        created_at_ms: ctx.now_ms,
        read_at_ms: None,
        dedupe_key: format!(
            "github:{}:{pr_number}:notification:{}",
            note.remote, note.id
        ),
    })
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
    InboxState {
        items: file.items,
        observed_pr: file.observed_pr,
        ci_state: file.ci_state,
        notified_item_ids: file.notified_item_ids.into_iter().collect(),
        refresh_error_at_ms: file.refresh_error_at_ms,
        last_refresh_ms: file.last_refresh_ms,
        seen_notification_ids: file.seen_notification_ids,
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
        seen_notification_ids: snapshot.seen_notification_ids,
    };
    if let Err(e) = crate::persist::save_json_atomic(&path, &payload) {
        log::error!(
            "[inbox] failed to persist inbox state at {}: {e}",
            path.display()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> PrTransitionCtx<'static> {
        PrTransitionCtx {
            remote: "org/repo",
            gh_user: "me",
            now_ms: 1_000,
            project_id: Some("p1".into()),
            repo_root: Some("/tmp/repo".into()),
        }
    }

    fn pr(author: &str) -> PrInboxView {
        PrInboxView {
            number: 42,
            title: "Fix the thing".into(),
            head_ref: "feat/x".into(),
            state: "OPEN".into(),
            author: author.into(),
            requested_reviewers: vec![],
            review_decision: None,
            latest_reviewer_states: vec![],
        }
    }

    fn prev() -> ObservedPrState {
        ObservedPrState {
            review_decision: None,
            requested_reviewers: vec![],
            pr_state: "OPEN".into(),
            is_my_pr: true,
            check_state: None,
            failing_checks: vec![],
            head_oid: String::new(),
            triaged_head_oid: None,
            latest_reviewer_states: vec![],
        }
    }

    #[test]
    fn category_for_kind_maps_known_and_unknown() {
        assert_eq!(category_for_kind("pr_comment").as_str(), "pr_comment");
        assert_eq!(
            category_for_kind("pr_review_received").as_str(),
            "review_received"
        );
        assert_eq!(
            category_for_kind("pr_comment_reply").as_str(),
            "comment_reply"
        );
        assert_eq!(category_for_kind("pr_review_approved").as_str(), "approved");
        assert_eq!(
            category_for_kind("pr_review_changes_requested").as_str(),
            "changes_requested"
        );
        assert_eq!(
            category_for_kind("review_requested").as_str(),
            "review_requested"
        );
        assert_eq!(category_for_kind("mention").as_str(), "mention");
        assert_eq!(category_for_kind("ci_failed").as_str(), "ci");
        assert_eq!(category_for_kind("pr_merged").as_str(), "lifecycle");
        assert_eq!(category_for_kind("ai_review_done").as_str(), "ai");
        assert_eq!(category_for_kind("github_refresh_failed").as_str(), "other");
        assert_eq!(category_for_kind("brand_new_kind").as_str(), "other");
        assert_eq!(InboxCategory::PrComment.label(), "Comment on your PR");
        assert_eq!(InboxCategory::ReviewReceived.label(), "Review received");
    }

    #[test]
    fn cold_start_emits_nothing() {
        let items = inbox_items_from_pr_transition(None, &pr("me"), &ctx());
        assert!(items.is_empty());
    }

    #[test]
    fn commented_review_emits_review_received() {
        let mut current = pr("me");
        current.latest_reviewer_states = vec![("alex".into(), "COMMENTED".into())];
        let items = inbox_items_from_pr_transition(Some(&prev()), &current, &ctx());
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].kind, "pr_review_received");
        assert!(items[0].title.contains("alex"));
        assert_eq!(
            category_for_kind(&items[0].kind),
            InboxCategory::ReviewReceived
        );
    }

    #[test]
    fn approved_review_emits_approved_not_aggregate_duplicate() {
        let mut current = pr("me");
        current.review_decision = Some("APPROVED".into());
        current.latest_reviewer_states = vec![("alex".into(), "APPROVED".into())];
        let items = inbox_items_from_pr_transition(Some(&prev()), &current, &ctx());
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].kind, "pr_review_approved");
        assert!(items[0].dedupe_key.contains("reviewer:alex:APPROVED"));
        assert!(!items[0].dedupe_key.contains("review_decision"));
    }

    #[test]
    fn changes_requested_review_emits_warning() {
        let mut current = pr("me");
        current.latest_reviewer_states = vec![("sam".into(), "CHANGES_REQUESTED".into())];
        let items = inbox_items_from_pr_transition(Some(&prev()), &current, &ctx());
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].kind, "pr_review_changes_requested");
        assert_eq!(items[0].severity, "warning");
    }

    #[test]
    fn unchanged_reviewer_state_emits_nothing() {
        let mut previous = prev();
        previous.latest_reviewer_states = vec![("alex".into(), "COMMENTED".into())];
        let mut current = pr("me");
        current.latest_reviewer_states = vec![("alex".into(), "COMMENTED".into())];
        let items = inbox_items_from_pr_transition(Some(&previous), &current, &ctx());
        assert!(items.is_empty());
    }

    #[test]
    fn aggregate_review_decision_used_when_latest_reviews_empty() {
        let mut current = pr("me");
        current.review_decision = Some("APPROVED".into());
        let items = inbox_items_from_pr_transition(Some(&prev()), &current, &ctx());
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].kind, "pr_review_approved");
        assert!(items[0].dedupe_key.contains("review_decision:APPROVED"));
    }

    #[test]
    fn own_review_is_ignored() {
        let mut current = pr("me");
        current.latest_reviewer_states = vec![("me".into(), "COMMENTED".into())];
        let items = inbox_items_from_pr_transition(Some(&prev()), &current, &ctx());
        assert!(items.is_empty());
    }

    fn note(reason: &str) -> InboxNotification {
        InboxNotification {
            id: "n1".into(),
            reason: reason.into(),
            title: "Fix the thing".into(),
            remote: "org/repo".into(),
            pr_number: Some(42),
            subject_type: "PullRequest".into(),
        }
    }

    fn note_ctx<'a>(
        remotes: &'a HashSet<String>,
        skip: &'a HashSet<(String, u64)>,
    ) -> NotificationInboxCtx<'a> {
        NotificationInboxCtx {
            now_ms: 1_000,
            allowed_remotes: remotes,
            project_id: Some("p1".into()),
            repo_root: Some("/tmp/repo".into()),
            skip_review_prs: skip,
        }
    }

    #[test]
    fn notification_author_maps_to_pr_comment() {
        let remotes = HashSet::from(["org/repo".into()]);
        let skip = HashSet::new();
        let item = inbox_item_from_notification(&note("author"), &note_ctx(&remotes, &skip))
            .expect("comment");
        assert_eq!(item.kind, "pr_comment");
        assert_eq!(category_for_kind(&item.kind), InboxCategory::PrComment);
    }

    #[test]
    fn notification_comment_maps_to_reply() {
        let remotes = HashSet::from(["org/repo".into()]);
        let skip = HashSet::new();
        let item = inbox_item_from_notification(&note("comment"), &note_ctx(&remotes, &skip))
            .expect("reply");
        assert_eq!(item.kind, "pr_comment_reply");
    }

    #[test]
    fn notification_mention_maps() {
        let remotes = HashSet::from(["org/repo".into()]);
        let skip = HashSet::new();
        let item = inbox_item_from_notification(&note("mention"), &note_ctx(&remotes, &skip))
            .expect("mention");
        assert_eq!(item.kind, "mention");
    }

    #[test]
    fn notification_skips_review_requested_and_ci() {
        let remotes = HashSet::from(["org/repo".into()]);
        let skip = HashSet::new();
        assert!(inbox_item_from_notification(
            &note("review_requested"),
            &note_ctx(&remotes, &skip)
        )
        .is_none());
        assert!(
            inbox_item_from_notification(&note("ci_activity"), &note_ctx(&remotes, &skip))
                .is_none()
        );
        assert!(
            inbox_item_from_notification(&note("state_change"), &note_ctx(&remotes, &skip))
                .is_none()
        );
        assert!(
            inbox_item_from_notification(&note("subscribed"), &note_ctx(&remotes, &skip)).is_none()
        );
    }

    #[test]
    fn notification_skips_pr_with_review_edge_this_cycle() {
        let remotes = HashSet::from(["org/repo".into()]);
        let skip = HashSet::from([("org/repo".into(), 42)]);
        assert!(
            inbox_item_from_notification(&note("author"), &note_ctx(&remotes, &skip)).is_none()
        );
    }

    #[test]
    fn notification_skips_unknown_remote() {
        let remotes = HashSet::from(["other/repo".into()]);
        let skip = HashSet::new();
        assert!(
            inbox_item_from_notification(&note("author"), &note_ctx(&remotes, &skip)).is_none()
        );
    }
}
