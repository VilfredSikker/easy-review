use er_engine::ai::{CommentRef, RiskLevel};
use er_engine::app::{App, DiffMode, InputMode, TabState};
use er_engine::git::{DiffFile, FileStatus, LineType};
use er_engine::highlight::Highlighter;
use serde::Serialize;

// ── Wire types ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct AppSnapshot {
    pub mode: String,
    pub branch: String,
    pub base: String,
    pub input_mode: String,
    pub files: Vec<FileSnapshot>,
    pub selected_file: usize,
    pub current_hunk: Option<usize>,
    pub filter: Option<String>,
    pub reviewed_count: usize,
    pub total_count: usize,
    pub ai: AiSnapshot,
    pub pr: Option<PrSnapshot>,
    pub panels: Panels,
    pub theme: String,
    pub watch_active: bool,
    pub worktrees: Vec<WorktreeSnapshot>,
    pub notification: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Panels {
    pub left: bool,
    pub tree: bool,
    pub right: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct FileSnapshot {
    pub path: String,
    pub status: String,
    pub additions: usize,
    pub deletions: usize,
    pub reviewed: bool,
    pub compacted: bool,
    pub risk: Option<String>,
    pub finding_count: usize,
    pub comment_count: usize,
    pub question_count: usize,
    pub hunks: Vec<HunkSnapshot>,
}

#[derive(Debug, Clone, Serialize)]
pub struct HunkSnapshot {
    pub header: String,
    pub old_start: usize,
    pub old_count: usize,
    pub new_start: usize,
    pub new_count: usize,
    pub lines: Vec<LineSnapshot>,
    pub threads: Vec<ThreadSnapshot>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LineSnapshot {
    pub old_num: Option<usize>,
    pub new_num: Option<usize>,
    pub kind: String,
    pub spans: Vec<SpanSnapshot>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SpanSnapshot {
    pub text: String,
    pub color: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ThreadSnapshot {
    pub id: String,
    pub kind: String,    // "comment" | "question"
    pub file: String,
    pub line: usize,
    pub source: String,  // "local" | "github"
    pub synced: bool,
    pub stale: bool,
    pub resolved: bool,
    pub root: ThreadMessage,
    pub replies: Vec<ThreadMessage>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ThreadMessage {
    pub author: String,
    pub kind: String,    // "you" | "human" | "ai"
    pub timestamp: String,
    pub body_markdown: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct FlatFinding {
    pub id: String,
    pub file: String,
    pub line: Option<usize>,
    pub severity: String,  // "high" | "med" | "low"
    pub title: String,
    pub message_markdown: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AiSnapshot {
    pub fresh: bool,
    pub summary_markdown: Option<String>,
    pub high: usize,
    pub med: usize,
    pub low: usize,
    pub comments: usize,
    pub questions: usize,
    pub unpushed: usize,
    pub threads: Vec<ThreadSnapshot>,
    pub findings: Vec<FlatFinding>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PrSnapshot {
    pub number: u64,
    pub title: String,
    pub state: String,
    pub base: String,
    pub head: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct WorktreeSnapshot {
    pub path: String,
    pub branch: String,
    pub is_current: bool,
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn severity_str(r: &RiskLevel) -> &'static str {
    match r {
        RiskLevel::High => "high",
        RiskLevel::Medium => "med",
        RiskLevel::Low | RiskLevel::Info => "low",
    }
}

fn comment_ref_to_thread(c: &CommentRef<'_>, file: &str, hunk_idx: usize) -> ThreadSnapshot {
    let kind = match c.comment_type() {
        er_engine::ai::CommentType::Question => "question",
        er_engine::ai::CommentType::GitHubComment => "comment",
    };
    let source = match c {
        CommentRef::GitHubComment(gc) => gc.source.clone(),
        _ => "local".to_string(),
    };
    let line = match c {
        CommentRef::Question(q) => q.line_start.unwrap_or(0),
        CommentRef::GitHubComment(gc) => gc.line_start.unwrap_or(0),
        CommentRef::Legacy(lc) => lc.line_start.unwrap_or(0),
    };
    let author_kind = if c.author() == "You" { "you" } else { "human" };
    ThreadSnapshot {
        id: c.id().to_string(),
        kind: kind.to_string(),
        file: file.to_string(),
        line,
        source,
        synced: c.is_synced(),
        stale: match c {
            CommentRef::Question(q) => q.stale,
            CommentRef::GitHubComment(gc) => gc.stale,
            CommentRef::Legacy(_) => false,
        },
        resolved: c.is_resolved(),
        root: ThreadMessage {
            author: c.author().to_string(),
            kind: author_kind.to_string(),
            timestamp: c.timestamp().to_string(),
            body_markdown: c.text().to_string(),
        },
        replies: build_replies(c, hunk_idx),
    }
}

fn build_replies(c: &CommentRef<'_>, _hunk_idx: usize) -> Vec<ThreadMessage> {
    // Replies live as separate comments with in_reply_to set — the snapshot
    // builder collects them here for the selected file's hunks. For the flat
    // all-threads list we emit root-only and let the UI fetch replies on expand.
    // For now return empty — replies are a Phase 3.C enhancement.
    let _ = c;
    vec![]
}

// ── Builder ──────────────────────────────────────────────────────────────────

pub fn build_snapshot(app: &App, highlighter: &mut Highlighter) -> AppSnapshot {
    let tab = app.tab();

    let mode = match tab.mode {
        DiffMode::Branch => "branch",
        DiffMode::Unstaged => "unstaged",
        DiffMode::Staged => "staged",
        DiffMode::History => "history",
        DiffMode::Conflicts => "conflicts",
        DiffMode::Hidden => "hidden",
    };

    let input_mode = match &app.input_mode {
        InputMode::Normal => "normal",
        InputMode::Search => "search",
        InputMode::Comment => "comment",
        InputMode::Filter => "filter",
        InputMode::Commit => "commit",
        InputMode::Confirm(_) => "confirm",
        InputMode::RemoteUrl => "remoteurl",
    };

    let reviewed_count = tab.reviewed.len();
    let total_count = tab.files.len();

    let files: Vec<FileSnapshot> = tab
        .files
        .iter()
        .enumerate()
        .map(|(i, f)| {
            let is_selected = i == tab.selected_file;
            let hunks = if is_selected && !f.compacted {
                build_hunks(f, tab, highlighter)
            } else {
                vec![]
            };

            // Per-file counts from AI state
            let finding_count = tab
                .ai
                .review
                .as_ref()
                .map(|r| {
                    r.files
                        .get(&f.path)
                        .map(|fr| fr.findings.len())
                        .unwrap_or(0)
                })
                .unwrap_or(0);
            let comment_count = tab.ai.file_github_comment_count(&f.path);
            let question_count = tab.ai.file_question_count(&f.path);

            // File-level risk from AI review
            let risk = tab
                .ai
                .review
                .as_ref()
                .and_then(|r| r.files.get(&f.path))
                .map(|fr| severity_str(&fr.risk).to_string());

            FileSnapshot {
                path: f.path.clone(),
                status: status_str(&f.status),
                additions: f.adds,
                deletions: f.dels,
                reviewed: tab.reviewed.contains_key(&f.path),
                compacted: f.compacted,
                risk,
                finding_count,
                comment_count,
                question_count,
                hunks,
            }
        })
        .collect();

    let ai = build_ai_snapshot(tab);
    let pr = build_pr_snapshot(tab);

    let filter = if tab.filter_expr.is_empty() {
        None
    } else {
        Some(tab.filter_expr.clone())
    };

    AppSnapshot {
        mode: mode.to_string(),
        branch: tab.current_branch.clone(),
        base: tab.base_branch.clone(),
        input_mode: input_mode.to_string(),
        files,
        selected_file: tab.selected_file,
        current_hunk: Some(tab.current_hunk),
        filter,
        reviewed_count,
        total_count,
        ai,
        pr,
        panels: Panels {
            left: app.panels_visible.left,
            tree: app.panels_visible.tree,
            right: app.panels_visible.right,
        },
        theme: "dark".to_string(),
        watch_active: app.watching,
        worktrees: er_engine::git::list_worktrees(&tab.repo_root)
            .unwrap_or_default()
            .into_iter()
            .map(|wt| WorktreeSnapshot {
                is_current: wt.path == tab.repo_root,
                branch: wt.branch,
                path: wt.path,
            })
            .collect(),
        notification: app.watch_message.clone(),
    }
}

fn build_hunks(file: &DiffFile, tab: &TabState, highlighter: &mut Highlighter) -> Vec<HunkSnapshot> {
    let filename = std::path::Path::new(&file.path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(&file.path);

    file.hunks
        .iter()
        .enumerate()
        .map(|(hunk_idx, hunk)| {
            let lines = hunk
                .lines
                .iter()
                .map(|line| {
                    let kind = match line.line_type {
                        LineType::Add => "add",
                        LineType::Delete => "del",
                        LineType::Context => "context",
                        LineType::Fold(_) => "fold",
                    };
                    let spans = if kind != "fold" {
                        highlighter
                            .highlight_line(&line.content, filename, "base16-ocean.dark")
                            .into_iter()
                            .map(|hs| SpanSnapshot { text: hs.text, color: hs.color })
                            .collect()
                    } else {
                        vec![SpanSnapshot {
                            text: line.content.clone(),
                            color: String::new(),
                        }]
                    };
                    LineSnapshot {
                        old_num: line.old_num,
                        new_num: line.new_num,
                        kind: kind.to_string(),
                        spans,
                    }
                })
                .collect();

            let old_count = hunk
                .lines
                .iter()
                .filter(|l| matches!(l.line_type, LineType::Context | LineType::Delete))
                .count();
            let new_count = hunk
                .lines
                .iter()
                .filter(|l| matches!(l.line_type, LineType::Context | LineType::Add))
                .count();

            // Collect threads for this hunk
            let threads: Vec<ThreadSnapshot> = tab
                .ai
                .comments_for_hunk(&file.path, hunk_idx)
                .iter()
                .filter(|c| !c.is_resolved())
                .map(|c| comment_ref_to_thread(c, &file.path, hunk_idx))
                .collect();

            HunkSnapshot {
                header: hunk.header.clone(),
                old_start: hunk.old_start,
                old_count,
                new_start: hunk.new_start,
                new_count,
                lines,
                threads,
            }
        })
        .collect()
}

fn build_ai_snapshot(tab: &TabState) -> AiSnapshot {
    let ai = &tab.ai;

    let (high, med, low) = if let Some(review) = &ai.review {
        let mut h = 0usize;
        let mut m = 0usize;
        let mut l = 0usize;
        for file_review in review.files.values() {
            for finding in &file_review.findings {
                match finding.severity {
                    RiskLevel::High => h += 1,
                    RiskLevel::Medium => m += 1,
                    RiskLevel::Low | RiskLevel::Info => l += 1,
                }
            }
        }
        (h, m, l)
    } else {
        (0, 0, 0)
    };

    let questions = ai.questions.as_ref().map(|q| q.questions.len()).unwrap_or(0);
    let comments = ai.github_comments.as_ref().map(|c| c.comments.len()).unwrap_or(0);
    let unpushed = ai
        .github_comments
        .as_ref()
        .map(|c| c.comments.iter().filter(|c| !c.synced).count())
        .unwrap_or(0);

    // Flat thread list for CommentsCard / QuestionsCard
    let threads: Vec<ThreadSnapshot> = {
        let mut result = Vec::new();
        if let Some(qs) = &ai.questions {
            for q in &qs.questions {
                if q.in_reply_to.is_none() && !q.resolved {
                    result.push(ThreadSnapshot {
                        id: q.id.clone(),
                        kind: "question".to_string(),
                        file: q.file.clone(),
                        line: q.line_start.unwrap_or(0),
                        source: "local".to_string(),
                        synced: false,
                        stale: q.stale,
                        resolved: q.resolved,
                        root: ThreadMessage {
                            author: if q.author.is_empty() { "You".to_string() } else { q.author.clone() },
                            kind: "you".to_string(),
                            timestamp: q.timestamp.clone(),
                            body_markdown: q.text.clone(),
                        },
                        replies: vec![],
                    });
                }
            }
        }
        if let Some(gc) = &ai.github_comments {
            for c in &gc.comments {
                if c.in_reply_to.is_none() && !c.resolved {
                    let author_kind = if c.author.is_empty() || c.author == "You" { "you" } else { "human" };
                    result.push(ThreadSnapshot {
                        id: c.id.clone(),
                        kind: "comment".to_string(),
                        file: c.file.clone(),
                        line: c.line_start.unwrap_or(0),
                        source: c.source.clone(),
                        synced: c.synced,
                        stale: c.stale,
                        resolved: c.resolved,
                        root: ThreadMessage {
                            author: if c.author.is_empty() { "You".to_string() } else { c.author.clone() },
                            kind: author_kind.to_string(),
                            timestamp: c.timestamp.clone(),
                            body_markdown: c.comment.clone(),
                        },
                        replies: vec![],
                    });
                }
            }
        }
        result
    };

    // Flat findings list for AiReviewCard
    let findings: Vec<FlatFinding> = if let Some(review) = &ai.review {
        review
            .files
            .iter()
            .flat_map(|(path, fr)| {
                fr.findings.iter().map(move |f| FlatFinding {
                    id: f.id.clone(),
                    file: path.clone(),
                    line: f.line_start,
                    severity: severity_str(&f.severity).to_string(),
                    title: f.title.clone(),
                    message_markdown: f.description.clone(),
                })
            })
            .collect()
    } else {
        vec![]
    };

    AiSnapshot {
        fresh: !ai.is_stale,
        summary_markdown: ai.summary.clone(),
        high,
        med,
        low,
        comments,
        questions,
        unpushed,
        threads,
        findings,
    }
}

fn build_pr_snapshot(tab: &TabState) -> Option<PrSnapshot> {
    let pr = tab.pr_data.as_ref()?;
    Some(PrSnapshot {
        number: pr.number,
        title: pr.title.clone(),
        state: pr.state.clone(),
        base: pr.base_branch.clone(),
        head: tab.current_branch.clone(),
    })
}

fn status_str(status: &FileStatus) -> String {
    match status {
        FileStatus::Added => "added",
        FileStatus::Modified => "modified",
        FileStatus::Deleted => "deleted",
        FileStatus::Renamed(_) => "renamed",
        FileStatus::Copied(_) => "copied",
        FileStatus::Unmerged => "unmerged",
    }
    .to_string()
}
