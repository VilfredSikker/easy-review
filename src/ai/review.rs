use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

// ── View Modes ──

/// Which AI view mode is active
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ViewMode {
    /// No AI data shown
    Default,
    /// AI findings rendered inline in the diff
    Overlay,
    /// Dedicated right panel showing findings for the current file
    SidePanel,
    /// Full-screen AI review summary (summary, checklist, review order)
    AiReview,
}

impl ViewMode {
    pub fn label(&self) -> &'static str {
        match self {
            ViewMode::Default => "DEFAULT",
            ViewMode::Overlay => "AI OVERLAY",
            ViewMode::SidePanel => "SIDE PANEL",
            ViewMode::AiReview => "AI REVIEW",
        }
    }

    /// Cycle to the next available mode
    pub fn next(&self) -> ViewMode {
        match self {
            ViewMode::Default => ViewMode::Overlay,
            ViewMode::Overlay => ViewMode::SidePanel,
            ViewMode::SidePanel => ViewMode::AiReview,
            ViewMode::AiReview => ViewMode::Default,
        }
    }

    /// Cycle to the previous available mode
    pub fn prev(&self) -> ViewMode {
        match self {
            ViewMode::Default => ViewMode::AiReview,
            ViewMode::Overlay => ViewMode::Default,
            ViewMode::SidePanel => ViewMode::Overlay,
            ViewMode::AiReview => ViewMode::SidePanel,
        }
    }
}

// ── .er-review.json ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErReview {
    pub version: u32,
    pub diff_hash: String,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub base_branch: String,
    #[serde(default)]
    pub head_branch: String,
    #[serde(default)]
    pub files: HashMap<String, ErFileReview>,
    /// Per-file diff hashes for incremental staleness detection
    #[serde(default)]
    pub file_hashes: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErFileReview {
    pub risk: RiskLevel,
    #[serde(default)]
    pub risk_reason: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub findings: Vec<Finding>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RiskLevel {
    High,
    Medium,
    Low,
    Info,
}

impl RiskLevel {
    pub fn symbol(&self) -> &'static str {
        match self {
            RiskLevel::High => "●",
            RiskLevel::Medium => "●",
            RiskLevel::Low => "●",
            RiskLevel::Info => "○",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Finding {
    pub id: String,
    pub severity: RiskLevel,
    #[serde(default)]
    pub category: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    /// 0-based index into the file's hunks
    pub hunk_index: Option<usize>,
    pub line_start: Option<usize>,
    pub line_end: Option<usize>,
    #[serde(default)]
    pub suggestion: String,
    #[serde(default)]
    pub related_files: Vec<String>,
    #[serde(default)]
    pub responses: Vec<AiResponse>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiResponse {
    pub id: String,
    #[serde(default)]
    pub in_reply_to: String,
    #[serde(default)]
    pub timestamp: String,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub new_findings: Vec<String>,
}

// ── .er-order.json ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErOrder {
    pub version: u32,
    pub diff_hash: String,
    #[serde(default)]
    pub order: Vec<OrderEntry>,
    #[serde(default)]
    pub groups: HashMap<String, OrderGroup>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderEntry {
    pub path: String,
    #[serde(default)]
    pub reason: String,
    #[serde(default)]
    pub group: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderGroup {
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub color: String,
}

// ── .er-checklist.json ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErChecklist {
    pub version: u32,
    pub diff_hash: String,
    #[serde(default)]
    pub items: Vec<ChecklistItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChecklistItem {
    pub id: String,
    pub text: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub checked: bool,
    #[serde(default)]
    pub related_findings: Vec<String>,
    #[serde(default)]
    pub related_files: Vec<String>,
}

// ── Comment type discriminator ──

/// Distinguishes between personal review questions and GitHub PR comments
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CommentType {
    /// Personal internal question (stored in .er-questions.json)
    Question,
    /// GitHub PR comment (stored in .er-github-comments.json)
    GitHubComment,
}

/// Unified reference to either a question, GitHub comment, or legacy comment.
/// Used by query methods and UI rendering to handle both types uniformly.
#[derive(Debug, Clone)]
pub enum CommentRef<'a> {
    Question(&'a ReviewQuestion),
    GitHubComment(&'a GitHubReviewComment),
    Legacy(&'a FeedbackComment),
}

impl<'a> CommentRef<'a> {
    pub fn id(&self) -> &str {
        match self {
            CommentRef::Question(q) => &q.id,
            CommentRef::GitHubComment(c) => &c.id,
            CommentRef::Legacy(c) => &c.id,
        }
    }

    pub fn comment_type(&self) -> CommentType {
        match self {
            CommentRef::Question(_) => CommentType::Question,
            CommentRef::GitHubComment(_) | CommentRef::Legacy(_) => CommentType::GitHubComment,
        }
    }

    pub fn text(&self) -> &str {
        match self {
            CommentRef::Question(q) => &q.text,
            CommentRef::GitHubComment(c) => &c.comment,
            CommentRef::Legacy(c) => &c.comment,
        }
    }

    pub fn author(&self) -> &str {
        match self {
            CommentRef::Question(_) => "You",
            CommentRef::GitHubComment(c) => if c.author.is_empty() { "You" } else { &c.author },
            CommentRef::Legacy(c) => if c.author.is_empty() { "You" } else { &c.author },
        }
    }

    pub fn timestamp(&self) -> &str {
        match self {
            CommentRef::Question(q) => &q.timestamp,
            CommentRef::GitHubComment(c) => &c.timestamp,
            CommentRef::Legacy(c) => &c.timestamp,
        }
    }

    pub fn is_synced(&self) -> bool {
        match self {
            CommentRef::Question(_) => false,
            CommentRef::GitHubComment(c) => c.synced,
            CommentRef::Legacy(c) => c.synced,
        }
    }

    #[allow(dead_code)]
    pub fn is_resolved(&self) -> bool {
        match self {
            CommentRef::Question(q) => q.resolved,
            CommentRef::GitHubComment(c) => c.resolved,
            CommentRef::Legacy(c) => c.resolved,
        }
    }

    pub fn is_stale(&self) -> bool {
        match self {
            CommentRef::Question(q) => q.stale,
            CommentRef::GitHubComment(c) => c.stale,
            CommentRef::Legacy(_) => false,
        }
    }

    pub fn in_reply_to(&self) -> Option<&str> {
        match self {
            CommentRef::Question(_) => None,
            CommentRef::GitHubComment(c) => c.in_reply_to.as_deref(),
            CommentRef::Legacy(c) => c.in_reply_to.as_deref(),
        }
    }

    #[allow(dead_code)]
    pub fn file(&self) -> &str {
        match self {
            CommentRef::Question(q) => &q.file,
            CommentRef::GitHubComment(c) => &c.file,
            CommentRef::Legacy(c) => &c.file,
        }
    }

    pub fn hunk_index(&self) -> Option<usize> {
        match self {
            CommentRef::Question(q) => q.hunk_index,
            CommentRef::GitHubComment(c) => c.hunk_index,
            CommentRef::Legacy(c) => c.hunk_index,
        }
    }

    pub fn line_start(&self) -> Option<usize> {
        match self {
            CommentRef::Question(q) => q.line_start,
            CommentRef::GitHubComment(c) => c.line_start,
            CommentRef::Legacy(c) => c.line_start,
        }
    }

    /// Whether this comment can be replied to (only GitHub comments, not replies themselves)
    pub fn can_reply(&self) -> bool {
        match self {
            CommentRef::Question(_) => false,
            CommentRef::GitHubComment(c) => c.in_reply_to.is_none(),
            CommentRef::Legacy(c) => c.in_reply_to.is_none(),
        }
    }

    /// Whether this comment can be deleted by the user
    pub fn can_delete(&self) -> bool {
        match self {
            CommentRef::Question(_) => true,
            CommentRef::GitHubComment(c) => c.source != "github" || c.author == "You",
            CommentRef::Legacy(c) => c.source != "github" || c.author == "You",
        }
    }

    #[allow(dead_code)]
    pub fn github_id(&self) -> Option<u64> {
        match self {
            CommentRef::Question(_) => None,
            CommentRef::GitHubComment(c) => c.github_id,
            CommentRef::Legacy(c) => c.github_id,
        }
    }

    #[allow(dead_code)]
    pub fn source(&self) -> &str {
        match self {
            CommentRef::Question(_) => "local",
            CommentRef::GitHubComment(c) => &c.source,
            CommentRef::Legacy(c) => &c.source,
        }
    }
}

// ── .er-questions.json — personal review notes ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErQuestions {
    pub version: u32,
    pub diff_hash: String,
    #[serde(default)]
    pub questions: Vec<ReviewQuestion>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewQuestion {
    pub id: String,
    #[serde(default)]
    pub timestamp: String,
    pub file: String,
    pub hunk_index: Option<usize>,
    pub line_start: Option<usize>,
    #[serde(default)]
    pub line_content: String,
    pub text: String,
    #[serde(default)]
    pub resolved: bool,
    /// Runtime-only staleness flag (not persisted)
    #[serde(skip)]
    pub stale: bool,
}

// ── .er-github-comments.json — GitHub PR comments ──

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GitHubSyncState {
    #[serde(default)]
    pub pr_number: Option<u64>,
    #[serde(default)]
    pub owner: String,
    #[serde(default)]
    pub repo: String,
    #[serde(default)]
    pub last_synced: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErGitHubComments {
    pub version: u32,
    pub diff_hash: String,
    #[serde(default)]
    pub github: Option<GitHubSyncState>,
    #[serde(default)]
    pub comments: Vec<GitHubReviewComment>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitHubReviewComment {
    pub id: String,
    #[serde(default)]
    pub timestamp: String,
    pub file: String,
    pub hunk_index: Option<usize>,
    pub line_start: Option<usize>,
    pub line_end: Option<usize>,
    #[serde(default)]
    pub line_content: String,
    pub comment: String,
    #[serde(default)]
    pub in_reply_to: Option<String>,
    #[serde(default)]
    pub resolved: bool,
    /// "local" | "github"
    #[serde(default = "default_source")]
    pub source: String,
    /// GitHub comment ID (for sync/dedup)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub github_id: Option<u64>,
    /// Author display name ("You" for local, GitHub login for remote)
    #[serde(default = "default_author")]
    pub author: String,
    /// Whether this comment was pushed to GitHub
    #[serde(default)]
    pub synced: bool,
    /// Runtime-only staleness flag (not persisted)
    #[serde(skip)]
    pub stale: bool,
}

fn default_source() -> String {
    "local".to_string()
}

fn default_author() -> String {
    "You".to_string()
}

// ── Legacy .er-feedback.json (for migration) ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErFeedback {
    pub version: u32,
    pub diff_hash: String,
    #[serde(default)]
    pub github: Option<GitHubSyncState>,
    #[serde(default)]
    pub comments: Vec<FeedbackComment>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedbackComment {
    pub id: String,
    #[serde(default)]
    pub timestamp: String,
    pub file: String,
    pub hunk_index: Option<usize>,
    pub line_start: Option<usize>,
    pub line_end: Option<usize>,
    #[serde(default)]
    pub line_content: String,
    pub comment: String,
    #[serde(default)]
    pub in_reply_to: Option<String>,
    #[serde(default)]
    pub resolved: bool,
    /// "local" | "github"
    #[serde(default = "default_source")]
    pub source: String,
    /// GitHub comment ID (for sync/dedup)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub github_id: Option<u64>,
    /// Author display name ("You" for local, GitHub login for remote)
    #[serde(default = "default_author")]
    pub author: String,
    /// Whether this comment was pushed to GitHub
    #[serde(default)]
    pub synced: bool,
}

// ── AiReview navigation ──

/// Which column has focus in AiReview mode
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ReviewFocus {
    /// Left column: file risk overview
    Files,
    /// Right column: checklist items
    Checklist,
}

// ── Aggregate AI state for a tab ──

/// All loaded AI data for a single repo tab
pub struct AiState {
    pub review: Option<ErReview>,
    pub order: Option<ErOrder>,
    pub summary: Option<String>,
    pub checklist: Option<ErChecklist>,
    /// Personal review questions (.er-questions.json)
    pub questions: Option<ErQuestions>,
    /// GitHub PR comments (.er-github-comments.json)
    pub github_comments: Option<ErGitHubComments>,
    /// Legacy feedback (kept for migration, not used for new comments)
    pub feedback: Option<ErFeedback>,
    /// Whether the loaded data matches the current diff
    pub is_stale: bool,
    /// Files whose diff has changed since the review (per-file staleness)
    pub stale_files: HashSet<String>,
    /// Current view mode
    pub view_mode: ViewMode,
    /// Which column has focus in AiReview mode
    pub review_focus: ReviewFocus,
    /// Cursor position within the focused section in AiReview mode
    pub review_cursor: usize,
}

impl Default for AiState {
    fn default() -> Self {
        AiState {
            review: None,
            order: None,
            summary: None,
            checklist: None,
            questions: None,
            github_comments: None,
            feedback: None,
            is_stale: false,
            stale_files: HashSet::new(),
            view_mode: ViewMode::Default,
            review_focus: ReviewFocus::Files,
            review_cursor: 0,
        }
    }
}

impl AiState {
    /// Whether a specific file's findings are stale (its diff changed since the review)
    pub fn is_file_stale(&self, path: &str) -> bool {
        self.stale_files.contains(path)
    }

    /// Whether any AI-generated data is loaded (excludes user feedback).
    pub fn has_data(&self) -> bool {
        self.review.is_some()
            || self.order.is_some()
            || self.summary.is_some()
            || self.checklist.is_some()
    }

    /// Whether overlay mode is available (requires review data)
    pub fn overlay_available(&self) -> bool {
        self.review.is_some()
    }

    /// Get file review for a given path
    pub fn file_review(&self, path: &str) -> Option<&ErFileReview> {
        self.review.as_ref()?.files.get(path)
    }

    /// Get all findings for a specific file and hunk (by positional index)
    pub fn findings_for_hunk(&self, path: &str, hunk_index: usize) -> Vec<&Finding> {
        match self.file_review(path) {
            Some(fr) => fr
                .findings
                .iter()
                .filter(|f| f.hunk_index == Some(hunk_index))
                .collect(),
            None => Vec::new(),
        }
    }

    /// Get findings whose `line_start` falls within a hunk's new-side line range.
    /// Used for non-branch diff modes where `hunk_index` doesn't match.
    pub fn findings_for_hunk_by_line_range(
        &self,
        path: &str,
        new_start: usize,
        new_count: usize,
    ) -> Vec<&Finding> {
        match self.file_review(path) {
            Some(fr) => fr
                .findings
                .iter()
                .filter(|f| {
                    if let Some(ls) = f.line_start {
                        ls >= new_start && ls < new_start + new_count
                    } else {
                        false
                    }
                })
                .collect(),
            None => Vec::new(),
        }
    }

    // ── Unified comment queries (questions + github comments) ──

    /// Get all comments (questions + GitHub) for a specific file and hunk (including replies)
    pub fn comments_for_hunk(&self, path: &str, hunk_index: usize) -> Vec<CommentRef<'_>> {
        let mut result = Vec::new();
        if let Some(qs) = &self.questions {
            for q in &qs.questions {
                if q.file == path && q.hunk_index == Some(hunk_index) {
                    result.push(CommentRef::Question(q));
                }
            }
        }
        if let Some(gc) = &self.github_comments {
            for c in &gc.comments {
                if c.file == path && c.hunk_index == Some(hunk_index) {
                    result.push(CommentRef::GitHubComment(c));
                }
            }
        }
        // Legacy fallback
        if result.is_empty() {
            if let Some(fb) = &self.feedback {
                for c in &fb.comments {
                    if c.file == path && c.hunk_index == Some(hunk_index) {
                        result.push(CommentRef::Legacy(c));
                    }
                }
            }
        }
        result
    }

    /// Comments targeting a specific line within a hunk (top-level only, no replies)
    pub fn comments_for_line(&self, path: &str, hunk_idx: usize, line_num: usize) -> Vec<CommentRef<'_>> {
        let mut result = Vec::new();
        if let Some(qs) = &self.questions {
            for q in &qs.questions {
                if q.file == path
                    && q.hunk_index == Some(hunk_idx)
                    && q.line_start == Some(line_num)
                {
                    result.push(CommentRef::Question(q));
                }
            }
        }
        if let Some(gc) = &self.github_comments {
            for c in &gc.comments {
                if c.file == path
                    && c.hunk_index == Some(hunk_idx)
                    && c.line_start == Some(line_num)
                    && c.in_reply_to.is_none()
                {
                    result.push(CommentRef::GitHubComment(c));
                }
            }
        }
        // Legacy fallback
        if result.is_empty() {
            if let Some(fb) = &self.feedback {
                for c in &fb.comments {
                    if c.file == path
                        && c.hunk_index == Some(hunk_idx)
                        && c.line_start == Some(line_num)
                        && c.in_reply_to.is_none()
                    {
                        result.push(CommentRef::Legacy(c));
                    }
                }
            }
        }
        result
    }

    /// Comments targeting the hunk as a whole (no specific line, top-level only)
    pub fn comments_for_hunk_only(&self, path: &str, hunk_idx: usize) -> Vec<CommentRef<'_>> {
        let mut result = Vec::new();
        if let Some(qs) = &self.questions {
            for q in &qs.questions {
                if q.file == path
                    && q.hunk_index == Some(hunk_idx)
                    && q.line_start.is_none()
                {
                    result.push(CommentRef::Question(q));
                }
            }
        }
        if let Some(gc) = &self.github_comments {
            for c in &gc.comments {
                if c.file == path
                    && c.hunk_index == Some(hunk_idx)
                    && c.line_start.is_none()
                    && c.in_reply_to.is_none()
                {
                    result.push(CommentRef::GitHubComment(c));
                }
            }
        }
        // Legacy fallback
        if result.is_empty() {
            if let Some(fb) = &self.feedback {
                for c in &fb.comments {
                    if c.file == path
                        && c.hunk_index == Some(hunk_idx)
                        && c.line_start.is_none()
                        && c.in_reply_to.is_none()
                    {
                        result.push(CommentRef::Legacy(c));
                    }
                }
            }
        }
        result
    }

    /// Replies to a specific comment (GitHub comments only — questions don't have replies)
    pub fn replies_to(&self, comment_id: &str) -> Vec<CommentRef<'_>> {
        let mut result = Vec::new();
        if let Some(gc) = &self.github_comments {
            for c in &gc.comments {
                if c.in_reply_to.as_deref() == Some(comment_id) {
                    result.push(CommentRef::GitHubComment(c));
                }
            }
        }
        // Legacy fallback
        if result.is_empty() {
            if let Some(fb) = &self.feedback {
                for c in &fb.comments {
                    if c.in_reply_to.as_deref() == Some(comment_id) {
                        result.push(CommentRef::Legacy(c));
                    }
                }
            }
        }
        result
    }

    /// Get GitHub comments data (for sync operations)
    #[allow(dead_code)]
    pub fn github_comments_data(&self) -> Option<&ErGitHubComments> {
        self.github_comments.as_ref()
    }

    /// Get questions data
    #[allow(dead_code)]
    pub fn questions_data(&self) -> Option<&ErQuestions> {
        self.questions.as_ref()
    }

    /// Total number of findings across all files
    pub fn total_findings(&self) -> usize {
        match &self.review {
            Some(r) => r.files.values().map(|f| f.findings.len()).sum(),
            None => 0,
        }
    }

    /// Cycle to next available view mode
    // Invariant: view_mode != ViewMode::Default requires overlay_available().
    // When overlay data is lost (e.g. stale .er-review.json deleted), reload_ai_state
    // collapses view_mode back to Default via the same guard.
    pub fn cycle_view_mode(&mut self) {
        let next = self.view_mode.next();
        // Skip modes that need review data when it's not present
        if !self.overlay_available() && next != ViewMode::Default {
            self.view_mode = ViewMode::Default;
        } else {
            self.view_mode = next;
        }
    }

    /// Cycle to previous available view mode
    pub fn cycle_view_mode_prev(&mut self) {
        let prev = self.view_mode.prev();
        if !self.overlay_available() && prev != ViewMode::Default {
            self.view_mode = ViewMode::Default;
        } else {
            self.view_mode = prev;
        }
    }

    // ── AiReview navigation ──

    /// Number of items in the left column (file risk list, sorted high→low)
    pub fn review_file_count(&self) -> usize {
        self.review.as_ref().map(|r| r.files.len()).unwrap_or(0)
    }

    /// Number of items in the right column (checklist items)
    pub fn review_checklist_count(&self) -> usize {
        self.checklist.as_ref().map(|c| c.items.len()).unwrap_or(0)
    }

    /// Max cursor value for the current focus
    fn review_item_count(&self) -> usize {
        match self.review_focus {
            ReviewFocus::Files => self.review_file_count(),
            ReviewFocus::Checklist => self.review_checklist_count(),
        }
    }

    /// Move cursor down in AiReview
    pub fn review_next(&mut self) {
        let count = self.review_item_count();
        if count > 0 && self.review_cursor + 1 < count {
            self.review_cursor += 1;
        }
    }

    /// Move cursor up in AiReview
    pub fn review_prev(&mut self) {
        if self.review_cursor > 0 {
            self.review_cursor -= 1;
        }
    }

    /// Switch focus between columns, resetting cursor to 0
    pub fn review_toggle_focus(&mut self) {
        self.review_focus = match self.review_focus {
            ReviewFocus::Files => ReviewFocus::Checklist,
            ReviewFocus::Checklist => ReviewFocus::Files,
        };
        self.review_cursor = 0;
    }

    /// Get the file path at the given cursor index in the risk list (sorted high→low)
    pub fn review_file_at(&self, index: usize) -> Option<String> {
        let review = self.review.as_ref()?;
        let mut entries: Vec<(&String, &ErFileReview)> = review.files.iter().collect();
        entries.sort_by(|a, b| {
            let risk_ord = |r: &RiskLevel| match r {
                RiskLevel::High => 0,
                RiskLevel::Medium => 1,
                RiskLevel::Low => 2,
                RiskLevel::Info => 3,
            };
            risk_ord(&a.1.risk).cmp(&risk_ord(&b.1.risk))
                .then_with(|| a.0.cmp(b.0))
        });
        entries.get(index).map(|(path, _)| (*path).clone())
    }

    /// Toggle checklist item at the given cursor index
    pub fn toggle_checklist_item(&mut self, index: usize) {
        if let Some(ref mut checklist) = self.checklist {
            if let Some(item) = checklist.items.get_mut(index) {
                item.checked = !item.checked;
            }
        }
    }

    /// Get the first related file from the checklist item at cursor
    pub fn checklist_file_at(&self, index: usize) -> Option<String> {
        let checklist = self.checklist.as_ref()?;
        let item = checklist.items.get(index)?;
        item.related_files.first().cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    // ── Helpers ──

    fn make_review_with_files(files: Vec<(&str, RiskLevel, Vec<Finding>)>) -> ErReview {
        let mut file_map = HashMap::new();
        for (path, risk, findings) in files {
            file_map.insert(
                path.to_string(),
                ErFileReview {
                    risk,
                    risk_reason: String::new(),
                    summary: String::new(),
                    findings,
                },
            );
        }
        ErReview {
            version: 1,
            diff_hash: "test".to_string(),
            created_at: String::new(),
            base_branch: String::new(),
            head_branch: String::new(),
            files: file_map,
            file_hashes: HashMap::new(),
        }
    }

    fn make_finding(id: &str, hunk_index: Option<usize>, severity: RiskLevel) -> Finding {
        Finding {
            id: id.to_string(),
            severity,
            category: String::new(),
            title: format!("Finding {}", id),
            description: String::new(),
            hunk_index,
            line_start: None,
            line_end: None,
            suggestion: String::new(),
            related_files: Vec::new(),
            responses: Vec::new(),
        }
    }

    fn make_finding_with_lines(
        id: &str,
        hunk_index: Option<usize>,
        line_start: Option<usize>,
        line_end: Option<usize>,
        severity: RiskLevel,
    ) -> Finding {
        Finding {
            id: id.to_string(),
            severity,
            category: String::new(),
            title: format!("Finding {}", id),
            description: String::new(),
            hunk_index,
            line_start,
            line_end,
            suggestion: String::new(),
            related_files: Vec::new(),
            responses: Vec::new(),
        }
    }

    fn make_feedback_comment(file: &str, hunk_index: Option<usize>) -> FeedbackComment {
        FeedbackComment {
            id: format!("{}-{:?}", file, hunk_index),
            timestamp: String::new(),
            file: file.to_string(),
            hunk_index,
            line_start: None,
            line_end: None,
            line_content: String::new(),
            comment: "test comment".to_string(),
            in_reply_to: None,
            resolved: false,
            source: "local".to_string(),
            github_id: None,
            author: "You".to_string(),
            synced: false,
        }
    }

    fn make_feedback_comment_with_line(file: &str, hunk_index: Option<usize>, line_start: Option<usize>) -> FeedbackComment {
        FeedbackComment {
            id: format!("{}-{:?}-{:?}", file, hunk_index, line_start),
            timestamp: String::new(),
            file: file.to_string(),
            hunk_index,
            line_start,
            line_end: None,
            line_content: String::new(),
            comment: "test comment".to_string(),
            in_reply_to: None,
            resolved: false,
            source: "local".to_string(),
            github_id: None,
            author: "You".to_string(),
            synced: false,
        }
    }

    fn make_feedback_reply(file: &str, hunk_index: Option<usize>, in_reply_to: &str) -> FeedbackComment {
        FeedbackComment {
            id: format!("reply-{}-{:?}", file, hunk_index),
            timestamp: String::new(),
            file: file.to_string(),
            hunk_index,
            line_start: None,
            line_end: None,
            line_content: String::new(),
            comment: "reply comment".to_string(),
            in_reply_to: Some(in_reply_to.to_string()),
            resolved: false,
            source: "local".to_string(),
            github_id: None,
            author: "You".to_string(),
            synced: false,
        }
    }

    // ── ViewMode ──

    #[test]
    fn view_mode_next_cycles_forward() {
        assert_eq!(ViewMode::Default.next(), ViewMode::Overlay);
        assert_eq!(ViewMode::Overlay.next(), ViewMode::SidePanel);
        assert_eq!(ViewMode::SidePanel.next(), ViewMode::AiReview);
        assert_eq!(ViewMode::AiReview.next(), ViewMode::Default);
    }

    #[test]
    fn view_mode_prev_cycles_backward() {
        assert_eq!(ViewMode::Default.prev(), ViewMode::AiReview);
        assert_eq!(ViewMode::AiReview.prev(), ViewMode::SidePanel);
        assert_eq!(ViewMode::SidePanel.prev(), ViewMode::Overlay);
        assert_eq!(ViewMode::Overlay.prev(), ViewMode::Default);
    }

    #[test]
    fn view_mode_label_returns_correct_string() {
        assert_eq!(ViewMode::Default.label(), "DEFAULT");
        assert_eq!(ViewMode::Overlay.label(), "AI OVERLAY");
        assert_eq!(ViewMode::SidePanel.label(), "SIDE PANEL");
        assert_eq!(ViewMode::AiReview.label(), "AI REVIEW");
    }

    // ── RiskLevel ──

    #[test]
    fn risk_level_symbol_returns_correct_symbol() {
        assert_eq!(RiskLevel::High.symbol(), "●");
        assert_eq!(RiskLevel::Medium.symbol(), "●");
        assert_eq!(RiskLevel::Low.symbol(), "●");
        assert_eq!(RiskLevel::Info.symbol(), "○");
    }

    // ── AiState::has_data ──

    #[test]
    fn has_data_default_is_false() {
        let state = AiState::default();
        assert!(!state.has_data());
    }

    #[test]
    fn has_data_with_review_is_true() {
        let mut state = AiState::default();
        state.review = Some(make_review_with_files(vec![]));
        assert!(state.has_data());
    }

    #[test]
    fn has_data_with_only_summary_is_true() {
        let mut state = AiState::default();
        state.summary = Some("some summary".to_string());
        assert!(state.has_data());
    }

    #[test]
    fn has_data_with_only_checklist_is_true() {
        let mut state = AiState::default();
        state.checklist = Some(ErChecklist {
            version: 1,
            diff_hash: "test".to_string(),
            items: vec![],
        });
        assert!(state.has_data());
    }

    // ── AiState::overlay_available ──

    #[test]
    fn overlay_available_no_review_is_false() {
        let state = AiState::default();
        assert!(!state.overlay_available());
    }

    #[test]
    fn overlay_available_with_review_is_true() {
        let mut state = AiState::default();
        state.review = Some(make_review_with_files(vec![]));
        assert!(state.overlay_available());
    }

    // ── AiState::total_findings ──

    #[test]
    fn total_findings_no_review_is_zero() {
        let state = AiState::default();
        assert_eq!(state.total_findings(), 0);
    }

    #[test]
    fn total_findings_sums_across_files() {
        let mut state = AiState::default();
        state.review = Some(make_review_with_files(vec![
            (
                "a.rs",
                RiskLevel::High,
                vec![
                    make_finding("1", Some(0), RiskLevel::High),
                    make_finding("2", Some(1), RiskLevel::Medium),
                ],
            ),
            (
                "b.rs",
                RiskLevel::Low,
                vec![make_finding("3", Some(0), RiskLevel::Low)],
            ),
        ]));
        assert_eq!(state.total_findings(), 3);
    }

    #[test]
    fn total_findings_files_with_no_findings_is_zero() {
        let mut state = AiState::default();
        state.review = Some(make_review_with_files(vec![
            ("a.rs", RiskLevel::High, vec![]),
            ("b.rs", RiskLevel::Low, vec![]),
        ]));
        assert_eq!(state.total_findings(), 0);
    }

    // ── AiState::findings_for_hunk ──

    #[test]
    fn findings_for_hunk_no_review_returns_empty() {
        let state = AiState::default();
        assert!(state.findings_for_hunk("a.rs", 0).is_empty());
    }

    #[test]
    fn findings_for_hunk_matching_hunk_index_returned() {
        let mut state = AiState::default();
        state.review = Some(make_review_with_files(vec![(
            "a.rs",
            RiskLevel::High,
            vec![
                make_finding("1", Some(0), RiskLevel::High),
                make_finding("2", Some(1), RiskLevel::Medium),
                make_finding("3", Some(0), RiskLevel::Low),
            ],
        )]));
        let results = state.findings_for_hunk("a.rs", 0);
        assert_eq!(results.len(), 2);
        assert!(results.iter().any(|f| f.id == "1"));
        assert!(results.iter().any(|f| f.id == "3"));
    }

    #[test]
    fn findings_for_hunk_non_matching_hunk_returns_empty() {
        let mut state = AiState::default();
        state.review = Some(make_review_with_files(vec![(
            "a.rs",
            RiskLevel::High,
            vec![make_finding("1", Some(0), RiskLevel::High)],
        )]));
        let results = state.findings_for_hunk("a.rs", 99);
        assert!(results.is_empty());
    }

    #[test]
    fn findings_for_hunk_unknown_file_returns_empty() {
        let mut state = AiState::default();
        state.review = Some(make_review_with_files(vec![(
            "a.rs",
            RiskLevel::High,
            vec![make_finding("1", Some(0), RiskLevel::High)],
        )]));
        let results = state.findings_for_hunk("unknown.rs", 0);
        assert!(results.is_empty());
    }

    // ── AiState::findings_for_hunk_by_line_range ──

    #[test]
    fn line_range_matches_finding_within_hunk() {
        let mut state = AiState::default();
        state.review = Some(make_review_with_files(vec![(
            "a.rs",
            RiskLevel::High,
            vec![
                make_finding_with_lines("1", Some(0), Some(10), Some(12), RiskLevel::High),
                make_finding_with_lines("2", Some(1), Some(25), Some(30), RiskLevel::Medium),
            ],
        )]));
        // Hunk covers lines 10..20 → finding "1" (line_start=10) matches
        let results = state.findings_for_hunk_by_line_range("a.rs", 10, 10);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "1");
    }

    #[test]
    fn line_range_excludes_finding_outside_hunk() {
        let mut state = AiState::default();
        state.review = Some(make_review_with_files(vec![(
            "a.rs",
            RiskLevel::High,
            vec![make_finding_with_lines("1", Some(0), Some(50), Some(55), RiskLevel::High)],
        )]));
        // Hunk covers lines 10..20 → finding at line 50 does not match
        let results = state.findings_for_hunk_by_line_range("a.rs", 10, 10);
        assert!(results.is_empty());
    }

    #[test]
    fn line_range_excludes_finding_without_line_start() {
        let mut state = AiState::default();
        state.review = Some(make_review_with_files(vec![(
            "a.rs",
            RiskLevel::High,
            vec![make_finding("1", Some(0), RiskLevel::High)],
        )]));
        // Finding has no line_start → cannot match by line range
        let results = state.findings_for_hunk_by_line_range("a.rs", 1, 100);
        assert!(results.is_empty());
    }

    #[test]
    fn line_range_boundary_start_inclusive() {
        let mut state = AiState::default();
        state.review = Some(make_review_with_files(vec![(
            "a.rs",
            RiskLevel::High,
            vec![make_finding_with_lines("1", None, Some(10), None, RiskLevel::Low)],
        )]));
        // new_start=10, new_count=5 → range [10, 15). line_start=10 is included.
        let results = state.findings_for_hunk_by_line_range("a.rs", 10, 5);
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn line_range_boundary_end_exclusive() {
        let mut state = AiState::default();
        state.review = Some(make_review_with_files(vec![(
            "a.rs",
            RiskLevel::High,
            vec![make_finding_with_lines("1", None, Some(15), None, RiskLevel::Low)],
        )]));
        // new_start=10, new_count=5 → range [10, 15). line_start=15 is excluded.
        let results = state.findings_for_hunk_by_line_range("a.rs", 10, 5);
        assert!(results.is_empty());
    }

    #[test]
    fn line_range_unknown_file_returns_empty() {
        let mut state = AiState::default();
        state.review = Some(make_review_with_files(vec![(
            "a.rs",
            RiskLevel::High,
            vec![make_finding_with_lines("1", None, Some(10), None, RiskLevel::High)],
        )]));
        let results = state.findings_for_hunk_by_line_range("unknown.rs", 10, 5);
        assert!(results.is_empty());
    }

    #[test]
    fn line_range_no_review_returns_empty() {
        let state = AiState::default();
        let results = state.findings_for_hunk_by_line_range("a.rs", 10, 5);
        assert!(results.is_empty());
    }

    // ── AiState::comments_for_hunk ──

    #[test]
    fn comments_for_hunk_no_feedback_returns_empty() {
        let state = AiState::default();
        assert!(state.comments_for_hunk("a.rs", 0).is_empty());
    }

    #[test]
    fn comments_for_hunk_matching_file_and_hunk_returned() {
        let mut state = AiState::default();
        state.feedback = Some(ErFeedback {
            version: 1,
            diff_hash: "test".to_string(),
            github: None,
            comments: vec![
                make_feedback_comment("a.rs", Some(0)),
                make_feedback_comment("a.rs", Some(1)),
                make_feedback_comment("b.rs", Some(0)),
            ],
        });
        let results = state.comments_for_hunk("a.rs", 0);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].file(), "a.rs");
        assert_eq!(results[0].hunk_index(), Some(0));
    }

    #[test]
    fn comments_for_hunk_wrong_file_returns_empty() {
        let mut state = AiState::default();
        state.feedback = Some(ErFeedback {
            version: 1,
            diff_hash: "test".to_string(),
            github: None,
            comments: vec![make_feedback_comment("a.rs", Some(0))],
        });
        let results = state.comments_for_hunk("b.rs", 0);
        assert!(results.is_empty());
    }

    // ── AiState::comments_for_line ──

    #[test]
    fn comments_for_line_returns_matching_line_comments() {
        let mut state = AiState::default();
        state.feedback = Some(ErFeedback {
            version: 1,
            diff_hash: "test".to_string(),
            github: None,
            comments: vec![
                make_feedback_comment_with_line("a.rs", Some(0), Some(10)),
                make_feedback_comment_with_line("a.rs", Some(0), Some(20)),
                make_feedback_comment("a.rs", Some(0)), // hunk-level, no line
            ],
        });
        let results = state.comments_for_line("a.rs", 0, 10);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].line_start(), Some(10));
    }

    #[test]
    fn comments_for_line_excludes_replies() {
        let mut state = AiState::default();
        let parent = make_feedback_comment_with_line("a.rs", Some(0), Some(10));
        let reply = make_feedback_reply("a.rs", Some(0), &parent.id);
        state.feedback = Some(ErFeedback {
            version: 1,
            diff_hash: "test".to_string(),
            github: None,
            comments: vec![parent, reply],
        });
        let results = state.comments_for_line("a.rs", 0, 10);
        assert_eq!(results.len(), 1);
    }

    // ── AiState::comments_for_hunk_only ──

    #[test]
    fn comments_for_hunk_only_returns_hunk_level_comments() {
        let mut state = AiState::default();
        state.feedback = Some(ErFeedback {
            version: 1,
            diff_hash: "test".to_string(),
            github: None,
            comments: vec![
                make_feedback_comment("a.rs", Some(0)),         // hunk-level
                make_feedback_comment_with_line("a.rs", Some(0), Some(10)), // line-level
            ],
        });
        let results = state.comments_for_hunk_only("a.rs", 0);
        assert_eq!(results.len(), 1);
        assert!(results[0].line_start().is_none());
    }

    #[test]
    fn comments_for_hunk_only_excludes_replies() {
        let mut state = AiState::default();
        let parent = make_feedback_comment("a.rs", Some(0));
        let reply = make_feedback_reply("a.rs", Some(0), &parent.id);
        state.feedback = Some(ErFeedback {
            version: 1,
            diff_hash: "test".to_string(),
            github: None,
            comments: vec![parent, reply],
        });
        let results = state.comments_for_hunk_only("a.rs", 0);
        assert_eq!(results.len(), 1);
    }

    // ── AiState::replies_to ──

    #[test]
    fn replies_to_returns_replies() {
        let mut state = AiState::default();
        let parent = make_feedback_comment("a.rs", Some(0));
        let parent_id = parent.id.clone();
        let reply = make_feedback_reply("a.rs", Some(0), &parent_id);
        state.feedback = Some(ErFeedback {
            version: 1,
            diff_hash: "test".to_string(),
            github: None,
            comments: vec![parent, reply],
        });
        let results = state.replies_to(&parent_id);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].in_reply_to(), Some(parent_id.as_str()));
    }

    #[test]
    fn replies_to_no_replies_returns_empty() {
        let mut state = AiState::default();
        state.feedback = Some(ErFeedback {
            version: 1,
            diff_hash: "test".to_string(),
            github: None,
            comments: vec![make_feedback_comment("a.rs", Some(0))],
        });
        let results = state.replies_to("nonexistent");
        assert!(results.is_empty());
    }

    // ── AiState::cycle_view_mode ──

    #[test]
    fn cycle_view_mode_with_overlay_available_cycles_all_modes() {
        let mut state = AiState::default();
        state.review = Some(make_review_with_files(vec![]));
        assert_eq!(state.view_mode, ViewMode::Default);
        state.cycle_view_mode();
        assert_eq!(state.view_mode, ViewMode::Overlay);
        state.cycle_view_mode();
        assert_eq!(state.view_mode, ViewMode::SidePanel);
        state.cycle_view_mode();
        assert_eq!(state.view_mode, ViewMode::AiReview);
        state.cycle_view_mode();
        assert_eq!(state.view_mode, ViewMode::Default);
    }

    #[test]
    fn cycle_view_mode_without_overlay_stays_at_default() {
        let mut state = AiState::default();
        state.cycle_view_mode();
        assert_eq!(state.view_mode, ViewMode::Default);
    }

    // ── AiState::cycle_view_mode_prev ──

    #[test]
    fn cycle_view_mode_prev_with_overlay_available_cycles_backward() {
        let mut state = AiState::default();
        state.review = Some(make_review_with_files(vec![]));
        assert_eq!(state.view_mode, ViewMode::Default);
        state.cycle_view_mode_prev();
        assert_eq!(state.view_mode, ViewMode::AiReview);
        state.cycle_view_mode_prev();
        assert_eq!(state.view_mode, ViewMode::SidePanel);
        state.cycle_view_mode_prev();
        assert_eq!(state.view_mode, ViewMode::Overlay);
        state.cycle_view_mode_prev();
        assert_eq!(state.view_mode, ViewMode::Default);
    }

    #[test]
    fn cycle_view_mode_prev_without_overlay_stays_at_default() {
        let mut state = AiState::default();
        state.cycle_view_mode_prev();
        assert_eq!(state.view_mode, ViewMode::Default);
    }

    // ── AiState::review_next / review_prev ──

    #[test]
    fn review_next_increments_cursor() {
        let mut state = AiState::default();
        state.review = Some(make_review_with_files(vec![
            ("a.rs", RiskLevel::High, vec![]),
            ("b.rs", RiskLevel::Low, vec![]),
        ]));
        assert_eq!(state.review_cursor, 0);
        state.review_next();
        assert_eq!(state.review_cursor, 1);
    }

    #[test]
    fn review_next_at_last_item_stays() {
        let mut state = AiState::default();
        state.review = Some(make_review_with_files(vec![("a.rs", RiskLevel::High, vec![])]));
        state.review_cursor = 0;
        state.review_next();
        assert_eq!(state.review_cursor, 0);
    }

    #[test]
    fn review_prev_decrements_cursor() {
        let mut state = AiState::default();
        state.review = Some(make_review_with_files(vec![
            ("a.rs", RiskLevel::High, vec![]),
            ("b.rs", RiskLevel::Low, vec![]),
        ]));
        state.review_cursor = 1;
        state.review_prev();
        assert_eq!(state.review_cursor, 0);
    }

    #[test]
    fn review_prev_at_zero_stays() {
        let mut state = AiState::default();
        state.review_cursor = 0;
        state.review_prev();
        assert_eq!(state.review_cursor, 0);
    }

    // ── AiState::review_toggle_focus ──

    #[test]
    fn review_toggle_focus_files_to_checklist_resets_cursor() {
        let mut state = AiState::default();
        state.review_cursor = 3;
        assert_eq!(state.review_focus, ReviewFocus::Files);
        state.review_toggle_focus();
        assert_eq!(state.review_focus, ReviewFocus::Checklist);
        assert_eq!(state.review_cursor, 0);
    }

    #[test]
    fn review_toggle_focus_checklist_to_files_resets_cursor() {
        let mut state = AiState::default();
        state.review_focus = ReviewFocus::Checklist;
        state.review_cursor = 5;
        state.review_toggle_focus();
        assert_eq!(state.review_focus, ReviewFocus::Files);
        assert_eq!(state.review_cursor, 0);
    }

    // ── AiState::review_file_at ──

    #[test]
    fn review_file_at_returns_files_sorted_by_risk_high_first() {
        let mut state = AiState::default();
        state.review = Some(make_review_with_files(vec![
            ("low.rs", RiskLevel::Low, vec![]),
            ("high.rs", RiskLevel::High, vec![]),
            ("medium.rs", RiskLevel::Medium, vec![]),
        ]));
        assert_eq!(state.review_file_at(0), Some("high.rs".to_string()));
        assert_eq!(state.review_file_at(1), Some("medium.rs".to_string()));
        assert_eq!(state.review_file_at(2), Some("low.rs".to_string()));
    }

    #[test]
    fn review_file_at_out_of_bounds_returns_none() {
        let mut state = AiState::default();
        state.review = Some(make_review_with_files(vec![("a.rs", RiskLevel::High, vec![])]));
        assert_eq!(state.review_file_at(99), None);
    }

    #[test]
    fn review_file_at_no_review_returns_none() {
        let state = AiState::default();
        assert_eq!(state.review_file_at(0), None);
    }

    // ── AiState::toggle_checklist_item ──

    #[test]
    fn toggle_checklist_item_toggles_checked_on() {
        let mut state = AiState::default();
        state.checklist = Some(ErChecklist {
            version: 1,
            diff_hash: "test".to_string(),
            items: vec![ChecklistItem {
                id: "1".to_string(),
                text: "item".to_string(),
                category: String::new(),
                checked: false,
                related_findings: vec![],
                related_files: vec![],
            }],
        });
        state.toggle_checklist_item(0);
        assert!(state.checklist.as_ref().unwrap().items[0].checked);
    }

    #[test]
    fn toggle_checklist_item_toggles_checked_off() {
        let mut state = AiState::default();
        state.checklist = Some(ErChecklist {
            version: 1,
            diff_hash: "test".to_string(),
            items: vec![ChecklistItem {
                id: "1".to_string(),
                text: "item".to_string(),
                category: String::new(),
                checked: true,
                related_findings: vec![],
                related_files: vec![],
            }],
        });
        state.toggle_checklist_item(0);
        assert!(!state.checklist.as_ref().unwrap().items[0].checked);
    }

    #[test]
    fn toggle_checklist_item_out_of_bounds_does_not_panic() {
        let mut state = AiState::default();
        state.checklist = Some(ErChecklist {
            version: 1,
            diff_hash: "test".to_string(),
            items: vec![],
        });
        state.toggle_checklist_item(99);
    }

    // ── AiState::checklist_file_at ──

    #[test]
    fn checklist_file_at_returns_first_related_file() {
        let mut state = AiState::default();
        state.checklist = Some(ErChecklist {
            version: 1,
            diff_hash: "test".to_string(),
            items: vec![ChecklistItem {
                id: "1".to_string(),
                text: "item".to_string(),
                category: String::new(),
                checked: false,
                related_findings: vec![],
                related_files: vec!["first.rs".to_string(), "second.rs".to_string()],
            }],
        });
        assert_eq!(
            state.checklist_file_at(0),
            Some("first.rs".to_string())
        );
    }

    #[test]
    fn checklist_file_at_no_related_files_returns_none() {
        let mut state = AiState::default();
        state.checklist = Some(ErChecklist {
            version: 1,
            diff_hash: "test".to_string(),
            items: vec![ChecklistItem {
                id: "1".to_string(),
                text: "item".to_string(),
                category: String::new(),
                checked: false,
                related_findings: vec![],
                related_files: vec![],
            }],
        });
        assert_eq!(state.checklist_file_at(0), None);
    }

    #[test]
    fn checklist_file_at_no_checklist_returns_none() {
        let state = AiState::default();
        assert_eq!(state.checklist_file_at(0), None);
    }
}
