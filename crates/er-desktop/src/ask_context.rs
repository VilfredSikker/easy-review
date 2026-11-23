//! What a card ask sends the AI and where its answer lands, built by one rule
//! from either reader of a tab: the App's tab with the flag off, a tab's
//! published content with it on. Only the inputs differ, so the two paths
//! cannot drift apart on what a prompt says.

use er_engine::ai::{AiState, CommentType, PrivateAnnotation};
use er_engine::app::{build_card_ai_system_context, CardAiContextParams};
use er_engine::git::DiffFile;

/// What an ask reads from its tab.
pub struct AskTab<'a> {
    pub repo_root: &'a str,
    pub base_branch: &'a str,
    pub current_branch: &'a str,
    /// Where the provider runs: the checkout, or a remote tab's bucket.
    pub work_dir: String,
    pub files: Vec<DiffFile>,
    pub raw_diff: Option<String>,
    pub ai: &'a AiState,
}

/// Where a thread ask's answer lands: a reply in the thread, on its line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnswerTarget {
    pub file: String,
    pub hunk_idx: usize,
    pub line_num: Option<usize>,
    pub kind: CommentType,
}

/// The prompt context of one ask.
#[derive(Clone, Debug)]
pub struct AskContext {
    pub repo_root: String,
    pub base_branch: String,
    pub current_branch: String,
    pub work_dir: String,
    pub files: Vec<DiffFile>,
    pub raw_diff: Option<String>,
    pub file: String,
    pub hunk_index: usize,
    pub line_start: Option<usize>,
    pub line_content: Option<String>,
    pub thread_body: String,
    pub finding_title: Option<String>,
    pub finding_description: Option<String>,
}

impl AskContext {
    fn new(tab: AskTab<'_>, file: String, hunk_index: usize, line_start: Option<usize>) -> Self {
        Self {
            repo_root: tab.repo_root.to_string(),
            base_branch: tab.base_branch.to_string(),
            current_branch: tab.current_branch.to_string(),
            work_dir: tab.work_dir,
            files: tab.files,
            raw_diff: tab.raw_diff,
            file,
            hunk_index,
            line_start,
            line_content: None,
            thread_body: String::new(),
            finding_title: None,
            finding_description: None,
        }
    }

    /// The system prompt the provider runs with.
    pub fn system_prompt(&self) -> String {
        build_card_ai_system_context(&CardAiContextParams {
            repo_root: &self.repo_root,
            base_branch: &self.base_branch,
            current_branch: &self.current_branch,
            files: &self.files,
            raw_diff: self.raw_diff.as_deref(),
            file: &self.file,
            hunk_index: self.hunk_index,
            line_start: self.line_start,
            line_content: self.line_content.as_deref(),
            thread_body: &self.thread_body,
            finding_title: self.finding_title.as_deref(),
            finding_description: self.finding_description.as_deref(),
        })
    }
}

fn nonblank(text: &str) -> Option<String> {
    (!text.trim().is_empty()).then(|| text.to_string())
}

/// The context of an ask about a question, note or local comment thread, and
/// where its answer lands.
pub fn thread_ask(tab: AskTab<'_>, thread_id: &str) -> Result<(AskContext, AnswerTarget), String> {
    let ai = tab.ai;
    if thread_id.starts_with("q-") || thread_id.starts_with("n-") {
        // Questions and notes share the `PrivateAnnotation` shape; pick the
        // collection (and resulting CommentType) by id prefix.
        let is_note = thread_id.starts_with("n-");
        let items: &[PrivateAnnotation] = if is_note {
            ai.notes.as_ref().map(|ns| ns.notes.as_slice()).unwrap_or(&[])
        } else {
            ai.questions
                .as_ref()
                .map(|qs| qs.questions.as_slice())
                .unwrap_or(&[])
        };
        let q = items.iter().find(|q| q.id == thread_id).ok_or(if is_note {
            "Note not found"
        } else {
            "Question not found"
        })?;
        let mut thread_body = format!("{}:{}\n{}", q.file, q.line_start.unwrap_or(0), q.text);
        for r in items
            .iter()
            .filter(|r| r.in_reply_to.as_deref() == Some(thread_id))
        {
            thread_body.push_str(&format!("\n\n**{}** replied:\n{}", r.author, r.text));
        }
        let (finding_title, finding_description) =
            finding_fields_for_ref(ai, q.finding_ref.as_deref());
        let hunk_idx = q.hunk_index.unwrap_or(0);
        let target = AnswerTarget {
            file: q.file.clone(),
            hunk_idx,
            line_num: q.line_start,
            kind: if is_note {
                CommentType::Note
            } else {
                CommentType::Question
            },
        };
        let line_content = nonblank(&q.line_content);
        let mut context = AskContext::new(tab, q.file.clone(), hunk_idx, q.line_start);
        context.thread_body = thread_body;
        context.line_content = line_content;
        context.finding_title = finding_title;
        context.finding_description = finding_description;
        return Ok((context, target));
    }
    let comments = ai.github_comments.as_ref();
    let c = comments
        .and_then(|gc| gc.comments.iter().find(|c| c.id == thread_id))
        .ok_or_else(|| "Comment not found".to_string())?;
    let mut thread_body = format!("{}:{}\n{}", c.file, c.line_start.unwrap_or(0), c.comment);
    for r in comments
        .into_iter()
        .flat_map(|gc| gc.comments.iter())
        .filter(|r| r.in_reply_to.as_deref() == Some(thread_id))
    {
        thread_body.push_str(&format!("\n\n**{}** replied:\n{}", r.author, r.comment));
    }
    let (finding_title, finding_description) = finding_fields_for_ref(ai, c.finding_ref.as_deref());
    let hunk_idx = c.hunk_index.unwrap_or(0);
    let target = AnswerTarget {
        file: c.file.clone(),
        hunk_idx,
        line_num: c.line_start,
        kind: CommentType::GitHubComment,
    };
    let line_content = nonblank(&c.line_content);
    let mut context = AskContext::new(tab, c.file.clone(), hunk_idx, c.line_start);
    context.thread_body = thread_body;
    context.line_content = line_content;
    context.finding_title = finding_title;
    context.finding_description = finding_description;
    Ok((context, target))
}

/// The context of an ask about a finding, whose answer lands in its
/// responses. The line content is read from the finding's file in
/// `tab.files`, so that file must be parsed.
pub fn finding_ask(tab: AskTab<'_>, finding_id: &str) -> Result<AskContext, String> {
    let ai = tab.ai;
    let (file, hunk_idx, line_num, title, description) = lookup_finding_fields(ai, finding_id)?;
    let line_content = line_num.and_then(|ls| {
        tab.files
            .iter()
            .find(|f| f.path == file)
            .and_then(|df| df.hunks.get(hunk_idx))
            .and_then(|h| h.lines.iter().find(|l| l.new_num == Some(ls)))
            .map(|l| l.content.clone())
    });
    let mut thread_body = format!("**Finding:** {title}\n\n{description}");
    if let Some(review) = ai.review.as_ref() {
        if let Some(f) = review
            .files
            .values()
            .find_map(|fr| fr.findings.iter().find(|f| f.id == finding_id))
        {
            for r in &f.responses {
                thread_body.push_str(&format!("\n\n**AI** replied:\n{}", r.text));
            }
        }
    }
    let mut context = AskContext::new(tab, file, hunk_idx, line_num);
    context.thread_body = thread_body;
    context.line_content = line_content;
    context.finding_title = Some(title);
    context.finding_description = Some(description);
    Ok(context)
}

/// The finding a thread was opened on, when it was.
pub fn finding_id_for_thread(ai: &AiState, thread_id: &str) -> Option<String> {
    if thread_id.starts_with("q-") {
        ai.questions
            .as_ref()
            .and_then(|qs| qs.questions.iter().find(|q| q.id == thread_id))
            .and_then(|q| q.finding_ref.clone())
    } else if thread_id.starts_with("n-") {
        ai.notes
            .as_ref()
            .and_then(|ns| ns.notes.iter().find(|n| n.id == thread_id))
            .and_then(|n| n.finding_ref.clone())
    } else {
        ai.github_comments
            .as_ref()
            .and_then(|gc| gc.comments.iter().find(|c| c.id == thread_id))
            .and_then(|c| c.finding_ref.clone())
    }
}

/// A finding's file, hunk, line, title and description.
pub fn lookup_finding_fields(
    ai: &AiState,
    finding_id: &str,
) -> Result<(String, usize, Option<usize>, String, String), String> {
    let review = ai
        .review
        .as_ref()
        .ok_or_else(|| "No review loaded".to_string())?;
    for (path, fr) in review.files.iter() {
        for f in fr.findings.iter() {
            if f.id == finding_id {
                return Ok((
                    path.clone(),
                    f.hunk_index.unwrap_or(0),
                    f.line_start,
                    f.title.clone(),
                    f.description.clone(),
                ));
            }
        }
    }
    Err(format!("Finding not found: {finding_id}"))
}

/// The title and description of the finding a thread refers to.
pub fn finding_fields_for_ref(
    ai: &AiState,
    finding_ref: Option<&str>,
) -> (Option<String>, Option<String>) {
    let Some(fid) = finding_ref else {
        return (None, None);
    };
    ai.review
        .as_ref()
        .and_then(|review| {
            review
                .files
                .values()
                .find_map(|fr| fr.findings.iter().find(|f| f.id == fid))
        })
        .map_or((None, None), |f| {
            (Some(f.title.clone()), Some(f.description.clone()))
        })
}
