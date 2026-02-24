use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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

// ── .er-feedback.json ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErFeedback {
    pub version: u32,
    pub diff_hash: String,
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
#[derive(Debug, Clone)]
pub struct AiState {
    pub review: Option<ErReview>,
    pub order: Option<ErOrder>,
    pub summary: Option<String>,
    pub checklist: Option<ErChecklist>,
    pub feedback: Option<ErFeedback>,
    /// Whether the loaded data matches the current diff
    pub is_stale: bool,
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
            feedback: None,
            is_stale: false,
            view_mode: ViewMode::Default,
            review_focus: ReviewFocus::Files,
            review_cursor: 0,
        }
    }
}

impl AiState {
    /// Whether any AI data is loaded
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

    /// Get all findings for a specific file and hunk
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

    /// Get feedback comments for a specific file and hunk
    pub fn comments_for_hunk(&self, path: &str, hunk_index: usize) -> Vec<&FeedbackComment> {
        match &self.feedback {
            Some(fb) => fb
                .comments
                .iter()
                .filter(|c| c.file == path && c.hunk_index == Some(hunk_index))
                .collect(),
            None => Vec::new(),
        }
    }

    /// Total number of findings across all files
    pub fn total_findings(&self) -> usize {
        match &self.review {
            Some(r) => r.files.values().map(|f| f.findings.len()).sum(),
            None => 0,
        }
    }

    /// Cycle to next available view mode
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
