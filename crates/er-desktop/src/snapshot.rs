use er_engine::app::{App, DiffMode, InputMode, TabState};
use er_engine::git::{DiffFile, FileStatus, LineType};
use er_engine::highlight::Highlighter;
use er_engine::ai::RiskLevel;
use serde::Serialize;

// ── Wire types ──

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
    pub kind: String,
    pub line: usize,
    pub source: String,
    pub synced: bool,
    pub stale: bool,
    pub root: ThreadMessage,
    pub replies: Vec<ThreadMessage>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ThreadMessage {
    pub author: String,
    pub kind: String,
    pub timestamp: String,
    pub body_markdown: String,
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

// ── Builder ──

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
                build_hunks(f, highlighter)
            } else {
                vec![]
            };

            FileSnapshot {
                path: f.path.clone(),
                status: status_str(&f.status),
                additions: f.adds,
                deletions: f.dels,
                reviewed: tab.reviewed.contains_key(&f.path),
                compacted: f.compacted,
                risk: None,
                finding_count: 0,
                comment_count: 0,
                question_count: 0,
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
            left: true,
            tree: true,
            right: true,
        },
        theme: "dark".to_string(),
        watch_active: app.watching,
        worktrees: vec![],
    }
}

fn build_hunks(file: &DiffFile, highlighter: &mut Highlighter) -> Vec<HunkSnapshot> {
    let filename = std::path::Path::new(&file.path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(&file.path);

    file.hunks
        .iter()
        .map(|hunk| {
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

            let old_count = hunk.lines.iter().filter(|l| {
                matches!(l.line_type, LineType::Context | LineType::Delete)
            }).count();
            let new_count = hunk.lines.iter().filter(|l| {
                matches!(l.line_type, LineType::Context | LineType::Add)
            }).count();

            HunkSnapshot {
                header: hunk.header.clone(),
                old_start: hunk.old_start,
                old_count,
                new_start: hunk.new_start,
                new_count,
                lines,
                threads: vec![],
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
    let unpushed = ai.github_comments.as_ref().map(|c| {
        c.comments.iter().filter(|c| !c.synced).count()
    }).unwrap_or(0);

    AiSnapshot {
        fresh: !ai.is_stale,
        summary_markdown: ai.summary.clone(),
        high,
        med,
        low,
        comments,
        questions,
        unpushed,
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
