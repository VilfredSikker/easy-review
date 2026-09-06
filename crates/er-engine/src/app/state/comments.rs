use super::*;

/// Review agents never need a second checkout: they run inside the repo (or, for a remote PR
/// whose repo is not checked out locally, work from the prepared diff plus `gh`). Cloning has
/// only ever produced stray copies under `/tmp` that later runs then read stale code out of.
/// A speed bump rather than a wall — `gh repo clone` and `cd x && git clone` slip past it.
const CLONE_DENY_RULE: &str = "Bash(git clone*)";

fn mint_comment_id(prefix: &str) -> String {
    let seq = COMMENT_SEQ.fetch_add(1, Ordering::Relaxed);
    format!(
        "{prefix}{}-{seq}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    )
}

fn take_comment_id(tab: &mut TabState, prefix: &str) -> String {
    let usable = tab
        .comment_id_override
        .as_deref()
        .is_some_and(|id| !id.is_empty() && id.starts_with(prefix));
    if usable {
        return tab
            .comment_id_override
            .take()
            .unwrap_or_else(|| mint_comment_id(prefix));
    }
    mint_comment_id(prefix)
}

impl App {
    // ── Comment System ──

    /// Enter comment mode for the current file + hunk (and optionally line)
    pub fn start_comment(&mut self, comment_type: CommentType) {
        let split_active = self.split_diff_active(&self.config);
        let split_focus = self.tab().split_focus;
        let tab = self.tab_mut();
        let file_path = match tab.selected_diff_file() {
            Some(f) => f.path.clone(),
            None => return,
        };
        tab.comment_textarea = TextArea::default();
        tab.comment_file = file_path;
        tab.comment_hunk = tab.current_hunk;
        tab.comment_line_num = if split_active {
            tab.current_line_number_for_split(split_focus)
        } else {
            tab.current_line_number()
        };
        tab.comment_reply_to = None;
        tab.comment_finding_ref = None;
        tab.comment_type = comment_type;
        let side = tab.comment_side_for_cursor(split_active);
        tab.comment_side = Some(side);
        self.input_mode = InputMode::Comment;
    }

    /// Start typing a general PR comment (not attached to any file/line)
    pub fn start_general_comment(&mut self) {
        let tab = self.tab_mut();
        tab.comment_textarea = TextArea::default();
        tab.comment_file = String::new();
        tab.comment_hunk = 0;
        tab.comment_line_num = None;
        tab.comment_line_end = None;
        tab.comment_reply_to = None;
        tab.comment_finding_ref = None;
        tab.comment_type = CommentType::GitHubComment;
        tab.comment_edit_id = None;
        self.input_mode = InputMode::Comment;
    }

    /// Start editing an existing comment — opens comment input pre-filled with its text
    pub fn start_edit_comment(&mut self, comment_id: &str) {
        let tab = self.tab();
        // Find the comment text and type
        let (text, comment_type) = if comment_id.starts_with("q-") {
            if let Some(qs) = &tab.ai.questions {
                if let Some(q) = qs.questions.iter().find(|q| q.id == comment_id) {
                    (q.text.clone(), CommentType::Question)
                } else {
                    return;
                }
            } else {
                return;
            }
        } else if comment_id.starts_with("n-") {
            if let Some(ns) = &tab.ai.notes {
                if let Some(n) = ns.notes.iter().find(|n| n.id == comment_id) {
                    (n.text.clone(), CommentType::Note)
                } else {
                    return;
                }
            } else {
                return;
            }
        } else if let Some(gc) = &tab.ai.github_comments {
            if let Some(c) = gc.comments.iter().find(|c| c.id == comment_id) {
                (c.comment.clone(), CommentType::GitHubComment)
            } else {
                return;
            }
        } else {
            return;
        };

        let tab = self.tab_mut();
        let file_path = match tab.selected_diff_file() {
            Some(f) => f.path.clone(),
            None => return,
        };
        tab.comment_textarea = TextArea::new(vec![text]);
        tab.comment_file = file_path;
        tab.comment_hunk = tab.current_hunk;
        tab.comment_line_num = tab.current_line_number();
        tab.comment_reply_to = None;
        tab.comment_type = comment_type;
        tab.comment_edit_id = Some(comment_id.to_string());
        self.input_mode = InputMode::Comment;
    }

    /// Start replying to a comment or question — creates a threaded reply
    pub fn start_reply_comment(&mut self, comment_id: &str) {
        let tab = self.tab();
        // Determine type from ID prefix and find the parent comment's location
        let (file, hunk_index, line_start, comment_type) = if comment_id.starts_with("q-") {
            if let Some(qs) = &tab.ai.questions {
                if let Some(q) = qs.questions.iter().find(|q| q.id == comment_id) {
                    (
                        q.file.clone(),
                        q.hunk_index.unwrap_or(0),
                        q.line_start,
                        CommentType::Question,
                    )
                } else {
                    return;
                }
            } else {
                return;
            }
        } else if comment_id.starts_with("n-") {
            if let Some(ns) = &tab.ai.notes {
                if let Some(n) = ns.notes.iter().find(|n| n.id == comment_id) {
                    (
                        n.file.clone(),
                        n.hunk_index.unwrap_or(0),
                        n.line_start,
                        CommentType::Note,
                    )
                } else {
                    return;
                }
            } else {
                return;
            }
        } else if let Some(gc) = &tab.ai.github_comments {
            if let Some(c) = gc.comments.iter().find(|c| c.id == comment_id) {
                (
                    c.file.clone(),
                    c.hunk_index.unwrap_or(0),
                    c.line_start,
                    CommentType::GitHubComment,
                )
            } else {
                return;
            }
        } else {
            return;
        };

        let tab = self.tab_mut();
        tab.comment_textarea = TextArea::default();
        tab.comment_file = file;
        tab.comment_hunk = hunk_index;
        tab.comment_line_num = line_start;
        tab.comment_reply_to = Some(comment_id.to_string());
        tab.comment_finding_ref = None;
        tab.comment_type = comment_type;
        tab.comment_edit_id = None;
        self.input_mode = InputMode::Comment;
    }

    /// Start replying to an AI finding — creates a GitHubComment referencing the finding
    pub fn start_reply_finding(&mut self, finding_id: &str) {
        let tab = self.tab();
        // Find the finding's file and location
        let (file, hunk_index, line_start) = if let Some(review) = &tab.ai.review {
            let mut found = None;
            for (file_path, file_review) in &review.files {
                for finding in &file_review.findings {
                    if finding.id == finding_id {
                        found = Some((
                            file_path.clone(),
                            finding.hunk_index.unwrap_or(0),
                            finding.line_start,
                        ));
                        break;
                    }
                }
                if found.is_some() {
                    break;
                }
            }
            match found {
                Some(f) => f,
                None => {
                    self.notify("Finding not found — review may be stale");
                    return;
                }
            }
        } else {
            self.notify("No AI review loaded — cannot reply to finding");
            return;
        };

        let tab = self.tab_mut();
        tab.comment_textarea = TextArea::default();
        tab.comment_file = file;
        tab.comment_hunk = hunk_index;
        tab.comment_line_num = line_start;
        tab.comment_reply_to = None;
        tab.comment_finding_ref = Some(finding_id.to_string());
        tab.comment_type = CommentType::GitHubComment;
        tab.comment_edit_id = None;
        self.input_mode = InputMode::Comment;
    }

    /// Submit the current comment/question to the appropriate file
    pub fn submit_comment(&mut self) -> Result<()> {
        let tab = self.tab();
        let text = tab.comment_text();
        if text.is_empty() {
            self.tab_mut().comment_id_override = None;
            self.input_mode = InputMode::Normal;
            return Ok(());
        }

        // If editing an existing comment, update it in-place
        if let Some(edit_id) = tab.comment_edit_id.clone() {
            return self.update_comment(edit_id, text);
        }

        let comment_type = tab.comment_type;
        match comment_type {
            CommentType::Question => self.submit_question(text),
            CommentType::Note => self.submit_note(text),
            CommentType::GitHubComment => self.submit_github_comment(text),
        }
    }

    /// Submit a personal review question to .er-questions.json
    fn submit_question(&mut self, text: String) -> Result<()> {
        let tab = self.tab();
        let er_dir = tab.er_dir();
        let repo_root = tab.repo_root.clone();
        let mut diff_hash = tab.branch_diff_hash.clone();
        let base_branch = tab.base_branch.clone();
        let file_path = tab.comment_file.clone();
        let hunk_index = tab.comment_hunk;
        let comment_line_num = tab.comment_line_num;
        let comment_line_end = tab.comment_line_end;
        let reply_to = tab.comment_reply_to.clone();
        let pr_head_ref_owned = tab.pr_head_ref.clone();

        // Compute branch_diff_hash on-demand when not yet set (e.g., non-Branch mode with no AI data).
        // Without this, questions would always be marked stale because the hash would be empty.
        // Skip in remote mode — git_diff_raw requires a local git repo.
        if diff_hash.is_empty() && !self.tab().is_remote() {
            if let Ok(br) = git::git_diff_raw(
                "branch",
                &base_branch,
                &repo_root,
                pr_head_ref_owned.as_deref(),
            ) {
                diff_hash = ai::compute_diff_hash(&br);
                self.tab_mut().branch_diff_hash = diff_hash.clone();
            }
        }

        let anchor = self.get_line_anchor(hunk_index, comment_line_num);

        // Load or create questions.json
        let questions_path = format!("{}/questions.json", er_dir);
        let mut questions: ai::ErQuestions = match std::fs::read_to_string(&questions_path) {
            Ok(content) => match serde_json::from_str(&content) {
                Ok(qs) => qs,
                Err(_) => {
                    self.notify("Warning: .er/questions.json is invalid JSON — starting fresh");
                    ai::ErQuestions {
                        version: 1,
                        diff_hash: diff_hash.clone(),
                        questions: Vec::new(),
                    }
                }
            },
            Err(_) => ai::ErQuestions {
                version: 1,
                diff_hash: diff_hash.clone(),
                questions: Vec::new(),
            },
        };

        // If diff hash changed, update it but preserve existing questions
        // (the relocation system handles comment drift)
        if questions.diff_hash != diff_hash {
            questions.diff_hash = diff_hash;
        }

        let id = take_comment_id(self.tab_mut(), "q-");

        let is_reply = reply_to.is_some();
        let finding_ref = self.tab().comment_finding_ref.clone();
        let author = self
            .tab_mut()
            .comment_author_override
            .take()
            .unwrap_or_else(|| "You".to_string());
        let side = self
            .tab_mut()
            .comment_side
            .take()
            .unwrap_or_else(|| "RIGHT".to_string());
        questions.questions.push(ai::ReviewQuestion {
            id,
            timestamp: chrono_now(),
            file: file_path,
            hunk_index: Some(hunk_index),
            line_start: anchor.line_start,
            line_end: Self::normalize_line_end(anchor.line_start, comment_line_end),
            line_content: anchor.line_content,
            text: text.clone(),
            resolved: false,
            stale: false,
            context_before: anchor.context_before,
            context_after: anchor.context_after,
            old_line_start: anchor.old_line_start,
            side,
            hunk_header: anchor.hunk_header,
            anchor_status: "original".to_string(),
            relocated_at_hash: self.tab().diff_hash.clone(),
            in_reply_to: reply_to,
            author,
            promoted_to: None,
            finding_ref,
        });

        // Write atomically
        std::fs::create_dir_all(&er_dir)?;
        let json = serde_json::to_string_pretty(&questions)?;
        let tmp_path = format!("{}.tmp", questions_path);
        std::fs::write(&tmp_path, json)?;
        std::fs::rename(&tmp_path, &questions_path)?;

        self.tab_mut().ai.questions = Some(questions);
        self.tab_mut().ai.rebuild_comment_index();
        self.tab_mut().mark_sidecar_written(&questions_path);
        self.tab_mut().comment_textarea = TextArea::default();
        self.input_mode = InputMode::Normal;
        let label = if is_reply { "Reply" } else { "Question" };
        self.notify(&format!("{} added: {}", label, truncate(&text, 40)));
        Ok(())
    }

    /// Submit a local note to notes.json. Mirrors `submit_question` but writes to
    /// the separate notes sidecar and uses an `n-` id prefix.
    fn submit_note(&mut self, text: String) -> Result<()> {
        let tab = self.tab();
        let er_dir = tab.er_dir();
        let repo_root = tab.repo_root.clone();
        let mut diff_hash = tab.branch_diff_hash.clone();
        let base_branch = tab.base_branch.clone();
        let file_path = tab.comment_file.clone();
        let hunk_index = tab.comment_hunk;
        let comment_line_num = tab.comment_line_num;
        let comment_line_end = tab.comment_line_end;
        let reply_to = tab.comment_reply_to.clone();
        let pr_head_ref_owned = tab.pr_head_ref.clone();

        if diff_hash.is_empty() && !self.tab().is_remote() {
            if let Ok(br) = git::git_diff_raw(
                "branch",
                &base_branch,
                &repo_root,
                pr_head_ref_owned.as_deref(),
            ) {
                diff_hash = ai::compute_diff_hash(&br);
                self.tab_mut().branch_diff_hash = diff_hash.clone();
            }
        }

        let anchor = self.get_line_anchor(hunk_index, comment_line_num);

        // Load or create notes.json
        let notes_path = format!("{}/notes.json", er_dir);
        let mut notes: ai::ErNotes = match std::fs::read_to_string(&notes_path) {
            Ok(content) => match serde_json::from_str(&content) {
                Ok(ns) => ns,
                Err(_) => {
                    self.notify("Warning: .er/notes.json is invalid JSON — starting fresh");
                    ai::ErNotes {
                        version: 1,
                        diff_hash: diff_hash.clone(),
                        notes: Vec::new(),
                    }
                }
            },
            Err(_) => ai::ErNotes {
                version: 1,
                diff_hash: diff_hash.clone(),
                notes: Vec::new(),
            },
        };

        if notes.diff_hash != diff_hash {
            notes.diff_hash = diff_hash;
        }

        let id = take_comment_id(self.tab_mut(), "n-");

        let is_reply = reply_to.is_some();
        let finding_ref = self.tab().comment_finding_ref.clone();
        let author = self
            .tab_mut()
            .comment_author_override
            .take()
            .unwrap_or_else(|| "You".to_string());
        let side = self
            .tab_mut()
            .comment_side
            .take()
            .unwrap_or_else(|| "RIGHT".to_string());
        notes.notes.push(ai::ReviewQuestion {
            id,
            timestamp: chrono_now(),
            file: file_path,
            hunk_index: Some(hunk_index),
            line_start: anchor.line_start,
            line_end: Self::normalize_line_end(anchor.line_start, comment_line_end),
            line_content: anchor.line_content,
            text: text.clone(),
            resolved: false,
            stale: false,
            context_before: anchor.context_before,
            context_after: anchor.context_after,
            old_line_start: anchor.old_line_start,
            side,
            hunk_header: anchor.hunk_header,
            anchor_status: "original".to_string(),
            relocated_at_hash: self.tab().diff_hash.clone(),
            in_reply_to: reply_to,
            author,
            promoted_to: None,
            finding_ref,
        });

        // Write atomically
        std::fs::create_dir_all(&er_dir)?;
        let json = serde_json::to_string_pretty(&notes)?;
        let tmp_path = format!("{}.tmp", notes_path);
        std::fs::write(&tmp_path, json)?;
        std::fs::rename(&tmp_path, &notes_path)?;

        self.tab_mut().ai.notes = Some(notes);
        self.tab_mut().ai.rebuild_comment_index();
        self.tab_mut().mark_sidecar_written(&notes_path);
        self.tab_mut().comment_textarea = TextArea::default();
        self.input_mode = InputMode::Normal;
        let label = if is_reply { "Reply" } else { "Note" };
        self.notify(&format!("{} added: {}", label, truncate(&text, 40)));
        Ok(())
    }

    /// Submit a GitHub PR comment to .er/github-comments.json
    fn submit_github_comment(&mut self, text: String) -> Result<()> {
        let tab = self.tab();
        let diff_hash = tab.branch_diff_hash.clone();
        let file_path = tab.comment_file.clone();
        let hunk_index = tab.comment_hunk;
        let reply_to = tab.comment_reply_to.clone();
        let finding_ref = tab.comment_finding_ref.clone();
        let comment_line_num = tab.comment_line_num;
        let comment_line_end = tab.comment_line_end;

        let anchor = self.get_line_anchor(hunk_index, comment_line_num);

        // Load or create github-comments.json (uses cache dir in remote mode)
        let comments_path = self.tab().github_comments_path();
        let mut gh_comments: ai::ErGitHubComments = match std::fs::read_to_string(&comments_path) {
            Ok(content) => match serde_json::from_str(&content) {
                Ok(gc) => gc,
                Err(_) => {
                    self.notify(
                        "Warning: .er/github-comments.json is invalid JSON — starting fresh",
                    );
                    ai::ErGitHubComments {
                        version: 1,
                        diff_hash: diff_hash.clone(),
                        github: None,
                        comments: Vec::new(),
                    }
                }
            },
            Err(_) => ai::ErGitHubComments {
                version: 1,
                diff_hash: diff_hash.clone(),
                github: None,
                comments: Vec::new(),
            },
        };

        // If diff hash changed, update it but preserve existing comments
        // (the relocation system handles comment drift)
        if gh_comments.diff_hash != diff_hash {
            gh_comments.diff_hash = diff_hash;
        }

        let id = take_comment_id(self.tab_mut(), "c-");

        let is_reply = reply_to.is_some();
        let author = self
            .tab_mut()
            .comment_author_override
            .take()
            .unwrap_or_else(|| "You".to_string());
        let side = self
            .tab_mut()
            .comment_side
            .take()
            .unwrap_or_else(|| "RIGHT".to_string());
        gh_comments.comments.push(ai::GitHubReviewComment {
            id,
            timestamp: chrono_now(),
            file: file_path,
            hunk_index: Some(hunk_index),
            line_start: anchor.line_start,
            line_end: Self::normalize_line_end(anchor.line_start, comment_line_end),
            line_content: anchor.line_content,
            comment: text.clone(),
            in_reply_to: reply_to,
            resolved: false,
            source: "local".to_string(),
            github_id: None,
            author,
            synced: false,
            outdated: false,
            stale: false,
            context_before: anchor.context_before,
            context_after: anchor.context_after,
            old_line_start: anchor.old_line_start,
            hunk_header: anchor.hunk_header,
            anchor_status: "original".to_string(),
            relocated_at_hash: self.tab().diff_hash.clone(),
            finding_ref,
            side,
        });

        // Write atomically (github-comments.json is PR-scoped — shared PR bucket)
        let comments_dir = self.tab().github_comments_dir();
        std::fs::create_dir_all(&comments_dir)?;
        let json = serde_json::to_string_pretty(&gh_comments)?;
        let tmp_path = format!("{}.tmp", comments_path);
        std::fs::write(&tmp_path, json)?;
        std::fs::rename(&tmp_path, &comments_path)?;

        // Keep the in-memory copy. `reload_ai_state()` re-reads every sidecar
        // (review/experts/tour/…) and is what made each local inline comment
        // feel like a GitHub round-trip. The comment is local and unpushed.
        self.tab_mut().ai.github_comments = Some(gh_comments);
        self.tab_mut().ai.rebuild_comment_index();
        self.tab_mut().mark_sidecar_written(&comments_path);
        self.tab_mut().comment_textarea = TextArea::default();
        self.input_mode = InputMode::Normal;
        let label = if is_reply { "Reply" } else { "Comment" };
        self.notify(&format!("{} added: {}", label, truncate(&text, 40)));
        Ok(())
    }

    /// Richer anchor data captured when placing a comment
    pub(crate) fn get_line_anchor(
        &self,
        hunk_index: usize,
        comment_line_num: Option<usize>,
    ) -> LineAnchor {
        let tab = self.tab();
        let diff_file = if tab.comment_file.is_empty() {
            tab.selected_diff_file()
        } else if tab.mode == DiffMode::History {
            tab.history.as_ref().and_then(|history| {
                history
                    .commit_files
                    .iter()
                    .find(|f| f.path == tab.comment_file)
            })
        } else {
            tab.files.iter().find(|f| f.path == tab.comment_file)
        };

        if let Some(df) = diff_file {
            if let Some(hunk) = df.hunks.get(hunk_index) {
                if let Some(ln) = comment_line_num {
                    // Find the target line index within the hunk
                    let target_idx = match tab.comment_side.as_deref() {
                        Some("LEFT") => hunk
                            .lines
                            .iter()
                            .position(|l| l.old_num == Some(ln))
                            .or_else(|| hunk.lines.iter().position(|l| l.new_num == Some(ln))),
                        Some("RIGHT") => hunk
                            .lines
                            .iter()
                            .position(|l| l.new_num == Some(ln))
                            .or_else(|| hunk.lines.iter().position(|l| l.old_num == Some(ln))),
                        _ => hunk
                            .lines
                            .iter()
                            .position(|l| l.new_num == Some(ln))
                            .or_else(|| hunk.lines.iter().position(|l| l.old_num == Some(ln))),
                    };
                    let (line_content, old_line_start) = if let Some(idx) = target_idx {
                        let dl = &hunk.lines[idx];
                        (dl.content.clone(), dl.old_num)
                    } else {
                        (String::new(), None)
                    };

                    // Collect up to 3 content lines before the target (same hunk)
                    let context_before = if let Some(idx) = target_idx {
                        let start = idx.saturating_sub(3);
                        hunk.lines[start..idx]
                            .iter()
                            .map(|l| l.content.clone())
                            .collect()
                    } else {
                        Vec::new()
                    };

                    // Collect up to 3 content lines after the target (same hunk)
                    let context_after = if let Some(idx) = target_idx {
                        let end = (idx + 4).min(hunk.lines.len());
                        hunk.lines[(idx + 1)..end]
                            .iter()
                            .map(|l| l.content.clone())
                            .collect()
                    } else {
                        Vec::new()
                    };

                    LineAnchor {
                        line_start: Some(ln),
                        line_content,
                        context_before,
                        context_after,
                        old_line_start,
                        hunk_header: hunk.header.clone(),
                    }
                } else {
                    // Hunk-level comment
                    LineAnchor {
                        line_start: None,
                        line_content: hunk.header.clone(),
                        context_before: Vec::new(),
                        context_after: Vec::new(),
                        old_line_start: None,
                        hunk_header: hunk.header.clone(),
                    }
                }
            } else {
                LineAnchor::default()
            }
        } else {
            LineAnchor::default()
        }
    }

    /// Inclusive end line for a multi-line anchor; `None` when single-line or invalid.
    pub(crate) const fn normalize_line_end(
        line_start: Option<usize>,
        line_end: Option<usize>,
    ) -> Option<usize> {
        match (line_start, line_end) {
            (Some(start), Some(end)) if end > start => Some(end),
            _ => None,
        }
    }

    /// Submit a comment or question without going through InputMode flow.
    /// Used by the desktop app where there is no TextArea widget.
    #[allow(clippy::too_many_arguments)]
    pub fn submit_comment_text(
        &mut self,
        file: String,
        hunk_idx: usize,
        line_num: Option<usize>,
        line_num_end: Option<usize>,
        text: String,
        comment_type: CommentType,
        reply_to: Option<String>,
        finding_ref: Option<String>,
    ) -> Result<()> {
        self.submit_comment_text_inner(
            file,
            hunk_idx,
            line_num,
            line_num_end,
            text,
            comment_type,
            reply_to,
            finding_ref,
            None,
        )
    }

    /// Submit a comment/question whose `author` field is set to the provided
    /// value (e.g. "ai") instead of "You". Used by the desktop `ask_ai` flow
    /// to attribute AI-generated replies. Mirrors `submit_comment_text` and
    /// sets a transient override consumed by submit_question/submit_github_comment.
    #[allow(clippy::too_many_arguments)]
    pub fn submit_comment_text_as_author(
        &mut self,
        file: String,
        hunk_idx: usize,
        line_num: Option<usize>,
        line_num_end: Option<usize>,
        text: String,
        comment_type: CommentType,
        reply_to: Option<String>,
        finding_ref: Option<String>,
        author: String,
    ) -> Result<()> {
        self.submit_comment_text_inner(
            file,
            hunk_idx,
            line_num,
            line_num_end,
            text,
            comment_type,
            reply_to,
            finding_ref,
            Some(author),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn submit_comment_text_inner(
        &mut self,
        file: String,
        hunk_idx: usize,
        line_num: Option<usize>,
        line_num_end: Option<usize>,
        text: String,
        comment_type: CommentType,
        reply_to: Option<String>,
        finding_ref: Option<String>,
        author: Option<String>,
    ) -> Result<()> {
        {
            let tab = self.tab_mut();
            tab.comment_file = file;
            tab.comment_hunk = hunk_idx;
            tab.comment_line_num = line_num;
            tab.comment_line_end = Self::normalize_line_end(line_num, line_num_end);
            tab.comment_reply_to = reply_to;
            tab.comment_finding_ref = finding_ref;
            tab.comment_type = comment_type;
            tab.comment_edit_id = None;
            tab.comment_textarea = TextArea::new(vec![text]);
            tab.comment_author_override = author;
        }
        self.input_mode = InputMode::Comment;
        self.submit_comment()
    }

    /// Delete a comment or question by ID, bypassing the confirmation dialog.
    /// Used by the desktop app.
    pub fn delete_comment_direct(&mut self, comment_id: &str) -> Result<()> {
        self.confirm_delete_comment(comment_id)
    }

    /// Update comment/question body in `.er/` sidecars. Syncs to GitHub when `github_id` is set.
    pub fn update_comment_text(&mut self, comment_id: &str, new_text: &str) -> Result<()> {
        let er_dir = self.tab().er_dir();
        let repo_root = self.tab().repo_root.clone();

        if comment_id.starts_with("q-") {
            let path = format!("{}/questions.json", er_dir);
            let content =
                std::fs::read_to_string(&path).with_context(|| format!("Failed to read {path}"))?;
            let mut qs: ai::ErQuestions =
                serde_json::from_str(&content).context("Failed to parse questions.json")?;
            let q = qs
                .questions
                .iter_mut()
                .find(|q| q.id == comment_id)
                .context("Question not found")?;
            if q.author == "ai" {
                anyhow::bail!("Cannot edit AI-generated text");
            }
            q.text = new_text.to_string();
            let json = serde_json::to_string_pretty(&qs)?;
            let tmp = format!("{path}.tmp");
            std::fs::write(&tmp, json)?;
            std::fs::rename(&tmp, &path)?;
        } else if comment_id.starts_with("n-") {
            let path = format!("{}/notes.json", er_dir);
            let content =
                std::fs::read_to_string(&path).with_context(|| format!("Failed to read {path}"))?;
            let mut ns: ai::ErNotes =
                serde_json::from_str(&content).context("Failed to parse notes.json")?;
            let n = ns
                .notes
                .iter_mut()
                .find(|n| n.id == comment_id)
                .context("Note not found")?;
            if n.author == "ai" {
                anyhow::bail!("Cannot edit AI-generated text");
            }
            n.text = new_text.to_string();
            let json = serde_json::to_string_pretty(&ns)?;
            let tmp = format!("{path}.tmp");
            std::fs::write(&tmp, json)?;
            std::fs::rename(&tmp, &path)?;
        } else {
            let path = self.tab().github_comments_path();
            let content =
                std::fs::read_to_string(&path).with_context(|| format!("Failed to read {path}"))?;
            let mut gc: ai::ErGitHubComments =
                serde_json::from_str(&content).context("Failed to parse github-comments.json")?;
            let (github_id, gh_meta, author) = {
                let c = gc
                    .comments
                    .iter()
                    .find(|c| c.id == comment_id)
                    .context("Comment not found")?;
                (c.github_id, gc.github.clone(), c.author.clone())
            };
            if author == "ai" {
                anyhow::bail!("Cannot edit AI-generated text");
            }
            if let (Some(gh_id), Some(gh)) = (github_id, gh_meta.as_ref()) {
                crate::github::gh_pr_update_review_comment(
                    &gh.owner, &gh.repo, gh_id, new_text, &repo_root,
                )?;
            }
            if let Some(c) = gc.comments.iter_mut().find(|c| c.id == comment_id) {
                c.comment = new_text.to_string();
            }
            let json = serde_json::to_string_pretty(&gc)?;
            let tmp = format!("{path}.tmp");
            std::fs::write(&tmp, json)?;
            std::fs::rename(&tmp, &path)?;
        }

        self.tab_mut().reload_ai_state();
        Ok(())
    }

    /// Cancel comment input
    pub fn cancel_comment(&mut self) {
        self.tab_mut().comment_textarea = TextArea::default();
        self.tab_mut().comment_edit_id = None;
        self.input_mode = InputMode::Normal;
    }

    /// Check if there is a non-empty comment draft that is paused (not actively being edited)
    pub fn has_comment_draft(&self) -> bool {
        let lines = self.tab().comment_textarea.lines();
        let has_text = lines.len() > 1 || !lines[0].is_empty();
        has_text && self.input_mode != InputMode::Comment
    }

    /// Pause comment editing — return to normal mode but keep the draft
    pub fn pause_comment(&mut self) {
        self.input_mode = InputMode::Normal;
    }

    /// Resume editing a paused comment draft
    pub fn resume_comment(&mut self) {
        if self.has_comment_draft() {
            self.input_mode = InputMode::Comment;
        }
    }

    /// Whether the user may switch question ↔ GitHub comment while composing.
    pub fn can_toggle_comment_type(&self) -> bool {
        let tab = self.tab();
        tab.comment_reply_to.is_none()
            && tab.comment_edit_id.is_none()
            && tab.comment_finding_ref.is_none()
            && !tab.comment_file.is_empty()
    }

    /// Flip question ↔ GitHub comment for a new file-anchored draft.
    pub fn toggle_comment_type(&mut self) {
        if !self.can_toggle_comment_type() {
            return;
        }
        let tab = self.tab_mut();
        // Cycle through all three local draft kinds: question → note → comment.
        tab.comment_type = match tab.comment_type {
            CommentType::Question => CommentType::Note,
            CommentType::Note => CommentType::GitHubComment,
            CommentType::GitHubComment => CommentType::Question,
        };
    }

    /// Update an existing comment in-place: new text, re-anchored to current position
    fn update_comment(&mut self, comment_id: String, new_text: String) -> Result<()> {
        let tab = self.tab();
        let er_dir = tab.er_dir();
        let hunk_index = tab.comment_hunk;
        let comment_line_num = tab.comment_line_num;

        let anchor = self.get_line_anchor(hunk_index, comment_line_num);
        let diff_hash = self.tab().diff_hash.clone();

        if comment_id.starts_with("q-") {
            let path = format!("{}/questions.json", er_dir);
            if let Ok(content) = std::fs::read_to_string(&path) {
                if let Ok(mut qs) = serde_json::from_str::<ai::ErQuestions>(&content) {
                    if let Some(q) = qs.questions.iter_mut().find(|q| q.id == comment_id) {
                        q.text = new_text.clone();
                        q.line_start = anchor.line_start;
                        q.line_content = anchor.line_content.clone();
                        q.context_before = anchor.context_before.clone();
                        q.context_after = anchor.context_after.clone();
                        q.old_line_start = anchor.old_line_start;
                        q.hunk_header = anchor.hunk_header;
                        q.hunk_index = Some(hunk_index);
                        q.anchor_status = "original".to_string();
                        q.relocated_at_hash = diff_hash;
                        q.stale = false;
                    }
                    let json = serde_json::to_string_pretty(&qs)?;
                    let tmp = format!("{}.tmp", path);
                    std::fs::write(&tmp, json)?;
                    std::fs::rename(&tmp, &path)?;
                }
            }
        } else if comment_id.starts_with("n-") {
            let path = format!("{}/notes.json", er_dir);
            if let Ok(content) = std::fs::read_to_string(&path) {
                if let Ok(mut ns) = serde_json::from_str::<ai::ErNotes>(&content) {
                    if let Some(n) = ns.notes.iter_mut().find(|n| n.id == comment_id) {
                        n.text = new_text.clone();
                        n.line_start = anchor.line_start;
                        n.line_content = anchor.line_content.clone();
                        n.context_before = anchor.context_before.clone();
                        n.context_after = anchor.context_after.clone();
                        n.old_line_start = anchor.old_line_start;
                        n.hunk_header = anchor.hunk_header;
                        n.hunk_index = Some(hunk_index);
                        n.anchor_status = "original".to_string();
                        n.relocated_at_hash = diff_hash;
                        n.stale = false;
                    }
                    let json = serde_json::to_string_pretty(&ns)?;
                    let tmp = format!("{}.tmp", path);
                    std::fs::write(&tmp, json)?;
                    std::fs::rename(&tmp, &path)?;
                }
            }
        } else {
            let path = self.tab().github_comments_path();
            if let Ok(content) = std::fs::read_to_string(&path) {
                if let Ok(mut gc) = serde_json::from_str::<ai::ErGitHubComments>(&content) {
                    if let Some(c) = gc.comments.iter_mut().find(|c| c.id == comment_id) {
                        c.comment = new_text.clone();
                        c.line_start = anchor.line_start;
                        c.line_content = anchor.line_content.clone();
                        c.context_before = anchor.context_before.clone();
                        c.context_after = anchor.context_after.clone();
                        c.old_line_start = anchor.old_line_start;
                        c.hunk_header = anchor.hunk_header;
                        c.hunk_index = Some(hunk_index);
                        c.anchor_status = "original".to_string();
                        c.relocated_at_hash = diff_hash;
                        c.stale = false;
                    }
                    let json = serde_json::to_string_pretty(&gc)?;
                    let tmp = format!("{}.tmp", path);
                    std::fs::write(&tmp, json)?;
                    std::fs::rename(&tmp, &path)?;
                }
            }
        }

        self.tab_mut().comment_textarea = TextArea::default();
        self.tab_mut().comment_edit_id = None;
        self.input_mode = InputMode::Normal;
        self.tab_mut().reload_ai_state();
        self.notify(&format!("Comment updated: {}", truncate(&new_text, 40)));
        Ok(())
    }

    // ── Comment Navigation ──

    /// Jump to the next comment across all files.
    #[allow(dead_code)]
    pub fn next_comment(&mut self) {
        self.jump_comment(true, false);
    }

    /// Jump to the previous comment across all files.
    #[allow(dead_code)]
    pub fn prev_comment(&mut self) {
        self.jump_comment(false, false);
    }

    /// Jump to the next question across all files.
    #[allow(dead_code)]
    pub fn next_question(&mut self) {
        self.jump_comment(true, true);
    }

    /// Jump to the previous question across all files.
    #[allow(dead_code)]
    pub fn prev_question(&mut self) {
        self.jump_comment(false, true);
    }

    /// Core jump logic: navigate forward/backward through comments or questions across all files.
    /// Uses focused_comment_id for exact position tracking instead of file+hunk guessing.
    fn jump_comment(&mut self, forward: bool, questions_only: bool) {
        let tab = self.tab_mut();
        let all = if questions_only {
            // Convert 3-tuple to 4-tuple for uniform handling
            tab.ai
                .all_questions_ordered()
                .into_iter()
                .map(|(f, h, id)| (f, h, None::<usize>, id))
                .collect::<Vec<_>>()
        } else {
            tab.ai.all_comments_ordered()
        };

        if all.is_empty() {
            return;
        }

        // Find current position by exact ID match first, then fallback to file position
        let current_pos = tab
            .focused_comment_id
            .as_ref()
            .and_then(|fid| all.iter().position(|(_, _, _, id)| id == fid))
            .or_else(|| {
                let current_file = tab.files.get(tab.selected_file).map(|f| &f.path);
                current_file.and_then(|cf| {
                    if forward {
                        all.iter().position(|(f, _, _, _)| f == cf)
                    } else {
                        all.iter().rposition(|(f, _, _, _)| f == cf)
                    }
                })
            });

        let next_idx = match current_pos {
            Some(pos) => {
                if forward {
                    if pos + 1 < all.len() {
                        pos + 1
                    } else {
                        0
                    }
                } else if pos > 0 {
                    pos - 1
                } else {
                    all.len() - 1
                }
            }
            None => {
                if forward {
                    0
                } else {
                    all.len() - 1
                }
            }
        };

        let (ref file, hunk_index, _, ref comment_id) = all[next_idx];

        tab.focused_comment_id = Some(comment_id.clone());
        tab.focused_finding_id = None;

        let needs_file_change = tab
            .files
            .get(tab.selected_file)
            .is_none_or(|f| f.path != *file);

        if needs_file_change {
            if let Some(idx) = tab.files.iter().position(|f| f.path == *file) {
                tab.selected_file = idx;
                tab.current_hunk = hunk_index.unwrap_or(0);
                tab.current_line = None;
                tab.selection_anchor = None;
                tab.diff_scroll = 0;
                tab.h_scroll = 0;
                tab.ensure_file_parsed();
                tab.rebuild_hunk_offsets();
            }
        } else if let Some(hi) = hunk_index {
            tab.current_hunk = hi;
            tab.current_line = None;
        }

        tab.scroll_to_current_hunk();
    }

    /// Jump forward to the next AI finding.
    pub fn next_finding(&mut self) {
        self.jump_finding(true);
    }

    /// Jump backward to the previous AI finding.
    pub fn prev_finding(&mut self) {
        self.jump_finding(false);
    }

    /// Navigate to the next/prev finding within the current file's panel list.
    /// Uses the same sort order as the FileDetail panel renderer.
    pub fn navigate_panel_finding(&mut self, forward: bool) {
        use crate::ai::RiskLevel;
        let tab = self.tab_mut();
        let path = match tab.files.get(tab.selected_file) {
            Some(f) => f.path.clone(),
            None => return,
        };
        let fr = match tab.ai.file_review(&path) {
            Some(fr) if !fr.findings.is_empty() => fr,
            _ => return,
        };
        let sev_ord = |r: &RiskLevel| match r {
            RiskLevel::High => 0,
            RiskLevel::Medium => 1,
            RiskLevel::Low => 2,
            RiskLevel::Info => 3,
        };
        let conf_ord = |c: &crate::ai::Confidence| match c {
            crate::ai::Confidence::Confirmed => 0,
            crate::ai::Confidence::Tentative => 1,
            crate::ai::Confidence::Informational => 2,
            crate::ai::Confidence::Dropped => 3,
        };
        let mut sorted: Vec<&crate::ai::Finding> = fr.findings.iter().collect();
        sorted.sort_by(|a, b| {
            conf_ord(&a.confidence)
                .cmp(&conf_ord(&b.confidence))
                .then_with(|| a.hunk_index.cmp(&b.hunk_index))
                .then_with(|| a.line_start.cmp(&b.line_start))
                .then_with(|| sev_ord(&a.severity).cmp(&sev_ord(&b.severity)))
        });

        let current = tab
            .focused_finding_id
            .as_ref()
            .and_then(|id| sorted.iter().position(|f| &f.id == id));

        let next_idx = match current {
            Some(i) => {
                if forward {
                    (i + 1) % sorted.len()
                } else {
                    i.checked_sub(1).unwrap_or(sorted.len() - 1)
                }
            }
            None => {
                if forward {
                    0
                } else {
                    sorted.len() - 1
                }
            }
        };
        tab.focused_finding_id = Some(sorted[next_idx].id.clone());
    }

    /// Jump the diff view to the inline location of the currently focused finding.
    /// Scoped to the current file — does not change selected_file.
    /// Unfocuses the panel. Notifies if the finding is outside the diff.
    pub fn jump_to_focused_finding(&mut self) {
        let fid = match self.tab().focused_finding_id.clone() {
            Some(id) => id,
            None => return,
        };
        let path = match self.tab().files.get(self.tab().selected_file) {
            Some(f) => f.path.clone(),
            None => return,
        };
        let (hunk_index, line_start) = {
            let tab = self.tab();
            let fr = match tab.ai.file_review(&path) {
                Some(fr) => fr,
                None => return,
            };
            match fr.findings.iter().find(|f| f.id == fid) {
                Some(f) => (f.hunk_index, f.line_start),
                None => return,
            }
        };

        // Validate the hunk and line exist in the parsed diff.
        let hunk_ok = hunk_index.is_none_or(|h| {
            self.tab()
                .files
                .get(self.tab().selected_file)
                .is_some_and(|f| h < f.hunks.len())
        });
        let line_ok = line_start.is_none_or(|ls| {
            self.tab()
                .files
                .get(self.tab().selected_file)
                .is_some_and(|f| {
                    f.hunks
                        .iter()
                        .any(|hunk| hunk.lines.iter().any(|l| l.new_num == Some(ls)))
                })
        });

        if !hunk_ok || !line_ok {
            let ln = line_start
                .map(|l| l.to_string())
                .unwrap_or_else(|| "?".into());
            self.tab_mut().panel_focus = false;
            self.notify(&format!(
                "Line {} is outside the diff — open in editor to view",
                ln
            ));
            return;
        }

        let tab = self.tab_mut();
        if let Some(hi) = hunk_index {
            tab.current_hunk = hi;
            tab.current_line = None;
        }
        let hi = hunk_index.unwrap_or(0);
        if let Some(hunk) = tab
            .files
            .get(tab.selected_file)
            .and_then(|f| f.hunks.get(hi))
        {
            tab.current_line = if let Some(ls) = line_start {
                hunk.lines.iter().position(|l| l.new_num == Some(ls))
            } else {
                Some(hunk.lines.len().saturating_sub(1))
            };
        }
        tab.panel_focus = false;
        tab.scroll_to_current_hunk();
    }

    /// Core jump logic: navigate forward/backward through AI findings across all files.
    /// Uses focused_finding_id for exact position tracking.
    fn jump_finding(&mut self, forward: bool) {
        let tab = self.tab_mut();
        let file_paths: std::collections::HashSet<&str> =
            tab.files.iter().map(|f| f.path.as_str()).collect();
        let all: Vec<_> = tab
            .ai
            .all_findings_ordered()
            .into_iter()
            .filter(|(file, _, _, _)| file_paths.contains(file.as_str()))
            .collect();

        if all.is_empty() {
            return;
        }

        // Find current position by exact ID match first, then fallback to file position
        let current_pos = tab
            .focused_finding_id
            .as_ref()
            .and_then(|fid| all.iter().position(|(_, _, _, id)| id == fid))
            .filter(|&pos| {
                // Ignore stale focused ID if user moved to a different file
                tab.files
                    .get(tab.selected_file)
                    .is_some_and(|f| f.path == all[pos].0)
            })
            .or_else(|| {
                let current_file = tab.files.get(tab.selected_file).map(|f| f.path.as_str());
                let current_hunk = tab.current_hunk;
                current_file.and_then(|cf| {
                    if forward {
                        // Find first finding at or after current position
                        all.iter().position(|(f, hi, _, _)| {
                            f.as_str() > cf || (f == cf && hi.unwrap_or(0) >= current_hunk)
                        })
                    } else {
                        // Find last finding at or before current position
                        all.iter().rposition(|(f, hi, _, _)| {
                            f.as_str() < cf || (f == cf && hi.unwrap_or(0) <= current_hunk)
                        })
                    }
                })
            });

        let next_idx = match current_pos {
            Some(pos) => {
                if forward {
                    if pos + 1 < all.len() {
                        pos + 1
                    } else {
                        0
                    }
                } else if pos > 0 {
                    pos - 1
                } else {
                    all.len() - 1
                }
            }
            None => {
                if forward {
                    0
                } else {
                    all.len() - 1
                }
            }
        };

        let (ref file, hunk_index, line_start, ref finding_id) = all[next_idx];

        tab.focused_finding_id = Some(finding_id.clone());
        tab.focused_comment_id = None;

        let needs_file_change = tab
            .files
            .get(tab.selected_file)
            .is_none_or(|f| f.path != *file);

        if needs_file_change {
            if let Some(idx) = tab.files.iter().position(|f| f.path == *file) {
                tab.selected_file = idx;
                tab.current_hunk = hunk_index.unwrap_or(0);
                tab.current_line = None;
                tab.selection_anchor = None;
                tab.diff_scroll = 0;
                tab.h_scroll = 0;
                tab.ensure_file_parsed();
                tab.rebuild_hunk_offsets();
            }
        } else if let Some(hi) = hunk_index {
            tab.current_hunk = hi;
            tab.current_line = None;
        }

        // Compute current_line from finding's line_start for precise scroll positioning
        let hi = hunk_index.unwrap_or(0);
        if let Some(diff_file) = tab.files.get(tab.selected_file) {
            if let Some(hunk) = diff_file.hunks.get(hi) {
                if let Some(ls) = line_start {
                    // Line-level finding: scroll to the specific line within the hunk
                    if let Some(line_idx) = hunk.lines.iter().position(|l| l.new_num == Some(ls)) {
                        tab.current_line = Some(line_idx);
                    }
                } else {
                    // Hunk-level finding: renders at end of hunk, scroll near the end
                    tab.current_line = Some(hunk.lines.len().saturating_sub(1));
                }
            }
        }

        tab.scroll_to_current_hunk();
    }

    /// Jump to the next comment/question (Shift+J). Excludes findings.
    pub fn next_hint(&mut self) {
        self.jump_hint(true);
    }

    /// Jump to the previous comment/question (Shift+K). Excludes findings.
    pub fn prev_hint(&mut self) {
        self.jump_hint(false);
    }

    /// Navigation across comments and questions only (excludes findings).
    fn jump_hint(&mut self, forward: bool) {
        use crate::ai::HintType;

        let tab = self.tab_mut();
        let all: Vec<_> = tab
            .ai
            .all_hints_ordered()
            .into_iter()
            .filter(|(_, _, _, _, ht)| *ht != HintType::Finding)
            .collect();

        if all.is_empty() {
            return;
        }

        // Find current position by matching the currently focused ID
        let current_id = tab
            .focused_comment_id
            .as_ref()
            .or(tab.focused_finding_id.as_ref());
        let current_pos = current_id
            .and_then(|fid| all.iter().position(|(_, _, _, id, _)| id == fid))
            .or_else(|| {
                let current_file = tab.files.get(tab.selected_file).map(|f| &f.path);
                current_file.and_then(|cf| {
                    if forward {
                        all.iter().position(|(f, _, _, _, _)| f == cf)
                    } else {
                        all.iter().rposition(|(f, _, _, _, _)| f == cf)
                    }
                })
            });

        let next_idx = match current_pos {
            Some(pos) => {
                if forward {
                    if pos + 1 < all.len() {
                        pos + 1
                    } else {
                        0
                    }
                } else if pos > 0 {
                    pos - 1
                } else {
                    all.len() - 1
                }
            }
            None => {
                if forward {
                    0
                } else {
                    all.len() - 1
                }
            }
        };

        let (ref file, hunk_index, _, ref id, hint_type) = all[next_idx];

        // Set the appropriate focus ID based on hint type
        match hint_type {
            HintType::Question | HintType::Note | HintType::GitHubComment => {
                tab.focused_comment_id = Some(id.clone());
                tab.focused_finding_id = None;
            }
            HintType::Finding => {
                tab.focused_finding_id = Some(id.clone());
                tab.focused_comment_id = None;
            }
        }

        let needs_file_change = tab
            .files
            .get(tab.selected_file)
            .is_none_or(|f| f.path != *file);

        if needs_file_change {
            if let Some(idx) = tab.files.iter().position(|f| f.path == *file) {
                tab.selected_file = idx;
                tab.current_hunk = hunk_index.unwrap_or(0);
                tab.current_line = None;
                tab.selection_anchor = None;
                tab.diff_scroll = 0;
                tab.h_scroll = 0;
                tab.ensure_file_parsed();
                tab.rebuild_hunk_offsets();
            }
        } else if let Some(hi) = hunk_index {
            tab.current_hunk = hi;
            tab.current_line = None;
        }

        tab.scroll_to_current_hunk();
    }

    /// Execute comment deletion after confirmation
    pub fn confirm_delete_comment(&mut self, comment_id: &str) -> Result<()> {
        let er_dir = self.tab().er_dir();
        let repo_root = self.tab().repo_root.clone();

        // Determine which file this comment lives in (by id prefix)
        if comment_id.starts_with("q-") {
            // Delete from questions.json
            let path = format!("{}/questions.json", er_dir);
            if let Ok(content) = std::fs::read_to_string(&path) {
                if let Ok(mut qs) = serde_json::from_str::<ai::ErQuestions>(&content) {
                    qs.questions.retain(|q| {
                        q.id != comment_id && q.in_reply_to.as_deref() != Some(comment_id)
                    });
                    let json = serde_json::to_string_pretty(&qs)?;
                    let tmp_path = format!("{}.tmp", path);
                    std::fs::write(&tmp_path, &json)?;
                    std::fs::rename(&tmp_path, &path)?;
                }
            }
        } else if comment_id.starts_with("n-") {
            // Delete from notes.json (cascade replies)
            let path = format!("{}/notes.json", er_dir);
            if let Ok(content) = std::fs::read_to_string(&path) {
                if let Ok(mut ns) = serde_json::from_str::<ai::ErNotes>(&content) {
                    ns.notes.retain(|n| {
                        n.id != comment_id && n.in_reply_to.as_deref() != Some(comment_id)
                    });
                    let json = serde_json::to_string_pretty(&ns)?;
                    let tmp_path = format!("{}.tmp", path);
                    std::fs::write(&tmp_path, &json)?;
                    std::fs::rename(&tmp_path, &path)?;
                }
            }
        } else {
            // Delete from github-comments.json (uses cache dir in remote mode)
            let path = self.tab().github_comments_path();
            if let Ok(content) = std::fs::read_to_string(&path) {
                if let Ok(mut gc) = serde_json::from_str::<ai::ErGitHubComments>(&content) {
                    // Check if the comment has a github_id for API deletion
                    let github_id = gc
                        .comments
                        .iter()
                        .find(|c| c.id == comment_id)
                        .and_then(|c| c.github_id);

                    let reply_github_ids: Vec<u64> = gc
                        .comments
                        .iter()
                        .filter(|c| {
                            c.in_reply_to.as_deref() == Some(comment_id) && c.github_id.is_some()
                        })
                        .filter_map(|c| c.github_id)
                        .collect();

                    // Delete from GitHub if applicable
                    if let Some(gh_id) = github_id {
                        if let Some(ref gh) = gc.github {
                            let _ = crate::github::gh_pr_delete_comment(
                                &gh.owner, &gh.repo, gh_id, &repo_root,
                            );
                            for reply_id in &reply_github_ids {
                                let _ = crate::github::gh_pr_delete_comment(
                                    &gh.owner, &gh.repo, *reply_id, &repo_root,
                                );
                            }
                        }
                    }

                    // Remove comment and cascade replies
                    gc.comments.retain(|c| {
                        c.id != comment_id && c.in_reply_to.as_deref() != Some(comment_id)
                    });

                    let json = serde_json::to_string_pretty(&gc)?;
                    let tmp_path = format!("{}.tmp", path);
                    std::fs::write(&tmp_path, &json)?;
                    std::fs::rename(&tmp_path, &path)?;
                }
            }
        }

        self.input_mode = InputMode::Normal;
        self.tab_mut().reload_ai_state();
        self.notify("Comment deleted");
        Ok(())
    }

    /// Cancel the confirm dialog
    pub fn cancel_confirm(&mut self) {
        self.input_mode = InputMode::Normal;
        self.clear_ai_selection_override();
    }

    // ── Hunk Comment (Shift-C) ──

    // ── Commit ──

    /// Start commit input (only in Staged mode)
    pub fn start_commit(&mut self) {
        self.tab_mut().commit_input.clear();
        self.input_mode = InputMode::Commit;
    }

    /// Run git commit with the typed message
    pub fn submit_commit(&mut self) -> Result<()> {
        let message = self.tab().commit_input.trim().to_string();
        if message.is_empty() {
            self.input_mode = InputMode::Normal;
            return Ok(());
        }
        let repo_root = self.tab().repo_root.clone();
        git::git_commit(&repo_root, &message)?;
        self.tab_mut().commit_input.clear();
        self.input_mode = InputMode::Normal;
        self.tab_mut().committed_unpushed = true;
        let _ = self.tab_mut().refresh_diff();
        self.notify("Committed! Ctrl+P to push");
        Ok(())
    }

    /// Cancel commit input
    pub fn cancel_commit(&mut self) {
        self.tab_mut().commit_input.clear();
        self.input_mode = InputMode::Normal;
    }

    // ── AiReview Navigation ──

    /// Jump from AiSummary panel to the selected file in FileDetail mode
    pub fn review_jump_to_file(&mut self) {
        let file_path = {
            let tab = self.tab();
            match tab.review_focus {
                ReviewFocus::Files => tab.ai.review_file_at(tab.review_cursor),
                ReviewFocus::Checklist => tab.ai.checklist_file_at(tab.review_cursor),
            }
        };

        if let Some(path) = file_path {
            let file_idx = self.tab().files.iter().position(|f| f.path == path);
            if let Some(idx) = file_idx {
                // Collect first anchored finding before taking mutable borrow
                let first_finding = self
                    .tab()
                    .ai
                    .file_review(&path)
                    .and_then(|fr| {
                        fr.findings
                            .iter()
                            .filter(|f| f.hunk_index.is_some())
                            .min_by_key(|f| (f.hunk_index, f.line_start))
                    })
                    .map(|f| (f.hunk_index.unwrap(), f.id.clone()));

                let tab = self.tab_mut();
                tab.selected_file = idx;
                tab.current_hunk = first_finding.as_ref().map(|(hi, _)| *hi).unwrap_or(0);
                tab.focused_finding_id = first_finding.map(|(_, id)| id);
                tab.current_line = None;
                tab.diff_scroll = 0;
                tab.h_scroll = 0;
                tab.ensure_file_parsed();
                tab.rebuild_hunk_offsets();
                tab.scroll_to_current_hunk();
                if tab.panel.is_none() {
                    tab.panel = Some(PanelContent::FileDetail);
                }
                self.notify(&format!("Jumped to: {}", path));
            } else {
                self.notify(&format!("File not in diff: {}", path));
            }
        } else {
            self.notify("No file associated with this item");
        }
    }

    /// Toggle the checklist item at cursor and persist to .er/checklist.json
    pub fn review_toggle_checklist(&mut self) -> Result<()> {
        let tab = self.tab_mut();
        if tab.review_focus != ReviewFocus::Checklist {
            return Ok(());
        }

        let cursor = tab.review_cursor;
        tab.ai.toggle_checklist_item(cursor);

        // Persist atomically via temp file + rename
        if let Some(ref checklist) = tab.ai.checklist {
            let checklist_path = format!("{}/checklist.json", tab.er_dir());
            let tmp_path = format!("{}.tmp", checklist_path);
            let json = serde_json::to_string_pretty(checklist)?;
            std::fs::write(&tmp_path, json)?;
            std::fs::rename(&tmp_path, &checklist_path)?;
        }

        let checked = tab
            .ai
            .checklist
            .as_ref()
            .and_then(|c| c.items.get(cursor))
            .map(|i| i.checked)
            .unwrap_or(false);

        if checked {
            self.notify("✓ Item checked");
        } else {
            self.notify("○ Item unchecked");
        }
        Ok(())
    }

    // ── Clipboard ──

    /// Copy the current hunk to the system clipboard
    pub fn copy_review_json(&mut self) -> Result<()> {
        let er_dir = self.tab().er_dir();
        let path = std::path::Path::new(&er_dir).join("review.json");
        if !path.exists() {
            self.notify("No review.json found");
            return Ok(());
        }
        let content = std::fs::read_to_string(&path).context("Failed to read review.json")?;
        let bytes = content.len();
        Self::copy_to_clipboard(&content)?;
        self.notify(&format!("Copied review.json ({} bytes)", bytes));
        Ok(())
    }

    /// Copy the current questions.json to the system clipboard
    pub fn copy_questions_json(&mut self) -> Result<()> {
        let er_dir = self.tab().er_dir();
        let path = std::path::Path::new(&er_dir).join("questions.json");
        if !path.exists() {
            self.notify("No questions.json found");
            return Ok(());
        }
        let content = std::fs::read_to_string(&path).context("Failed to read questions.json")?;
        let bytes = content.len();
        Self::copy_to_clipboard(&content)?;
        self.notify(&format!("Copied questions.json ({} bytes)", bytes));
        Ok(())
    }

    pub fn yank_hunk(&mut self) -> Result<()> {
        let si = self.tab().selected_file;
        let hi = self.tab().current_hunk;

        if si >= self.tab().files.len() {
            self.notify("No file selected");
            return Ok(());
        }
        if hi >= self.tab().files[si].hunks.len() {
            self.notify("No hunk selected");
            return Ok(());
        }

        let text = self.tab().files[si].hunks[hi].to_text();
        Self::copy_to_clipboard(&text)?;
        self.notify("Hunk copied to clipboard");
        Ok(())
    }

    /// Copy all hunks for the selected file in unified diff format
    pub fn copy_full_file(&mut self) -> Result<()> {
        let tab = self.tab();
        if let Some(file) = tab.selected_diff_file() {
            let mut text = format!("--- a/{}\n+++ b/{}\n", file.path, file.path);
            for hunk in &file.hunks {
                text.push_str(&hunk.to_text());
                text.push('\n');
            }
            let count = file.hunks.len();
            Self::copy_to_clipboard(&text)?;
            self.notify(&format!("Copied full file diff ({} hunks)", count));
        } else {
            self.notify("No file selected");
        }
        Ok(())
    }

    /// Copy the selected file's path to clipboard
    pub fn copy_file_path(&mut self) -> Result<()> {
        let tab = self.tab();
        if let Some(file) = tab.selected_diff_file() {
            let path = file.path.clone();
            Self::copy_to_clipboard(&path)?;
            self.notify(&format!("Copied: {}", path));
        } else {
            self.notify("No file selected");
        }
        Ok(())
    }

    /// Copy the current line's content to clipboard (requires line-level navigation)
    pub fn copy_line(&mut self) -> Result<()> {
        let tab = self.tab();
        if let Some(file) = tab.selected_diff_file() {
            if let Some(line_idx) = tab.current_line {
                if let Some(hunk) = file.hunks.get(tab.current_hunk) {
                    if let Some(line) = hunk.lines.get(line_idx) {
                        let content = line.content.clone();
                        Self::copy_to_clipboard(&content)?;
                        self.notify("Line copied to clipboard");
                        return Ok(());
                    }
                }
            }
            self.notify("No line selected — use arrow keys to enter line navigation");
        } else {
            self.notify("No file selected");
        }
        Ok(())
    }

    /// Copy rich context to clipboard for pasting into an agent terminal.
    ///
    /// What gets copied depends on navigation state:
    /// - Selection active (shift+arrow): selected lines only
    /// - Line-level nav (arrow keys): current line only
    /// - Hunk-level nav (n/N keys): full hunk
    pub fn copy_context(&mut self) -> Result<()> {
        let tab = self.tab();
        let file = match tab.selected_diff_file() {
            Some(f) => f,
            None => {
                self.notify("No file selected");
                return Ok(());
            }
        };
        let hunk = match file.hunks.get(tab.current_hunk) {
            Some(h) => h,
            None => {
                self.notify("No hunk selected");
                return Ok(());
            }
        };

        let mut text = String::new();

        // Header
        text.push_str(&format!("File: {}\n", file.path));
        text.push_str(&format!(
            "Branch: {} (vs {})\n",
            tab.current_branch, tab.base_branch
        ));

        // Determine what to copy based on navigation state
        let (lines_to_copy, line_label) = if let Some(range) = tab.selected_range() {
            // Shift+arrow selection: copy selected lines
            let selected: Vec<_> = hunk
                .lines
                .iter()
                .enumerate()
                .filter(|(i, _)| range.contains(i))
                .map(|(_, l)| l)
                .collect();
            let start = selected.first().and_then(|l| l.new_num).unwrap_or(0);
            let end = selected.last().and_then(|l| l.new_num).unwrap_or(0);
            let label = if start == end {
                format!("Line {}", start)
            } else {
                format!("Lines {}-{}", start, end)
            };
            (selected, label)
        } else if let Some(line_idx) = tab.current_line {
            // Line-level navigation: copy current line only
            if let Some(line) = hunk.lines.get(line_idx) {
                let ln = line.new_num.unwrap_or(0);
                (vec![line], format!("Line {}", ln))
            } else {
                let all: Vec<_> = hunk.lines.iter().collect();
                (all, format!("Hunk #{}", tab.current_hunk + 1))
            }
        } else {
            // Hunk-level navigation: copy full hunk
            let all: Vec<_> = hunk.lines.iter().collect();
            (all, format!("Hunk #{}", tab.current_hunk + 1))
        };

        text.push_str(&format!("{}:\n\n", line_label));

        // Hunk header
        text.push_str(&format!(" {}\n", hunk.header));

        // Diff lines
        for line in &lines_to_copy {
            let prefix = match line.line_type {
                crate::git::LineType::Add => "+",
                crate::git::LineType::Delete => "-",
                crate::git::LineType::Context => " ",
                crate::git::LineType::Fold(_) => continue,
            };
            text.push_str(&format!("{}{}\n", prefix, line.content));
        }

        // AI finding if present
        let findings = tab
            .ai
            .findings_for_hunk(&file.path, tab.current_hunk, file.hunks.len());
        if let Some(finding) = findings.first() {
            text.push_str(&format!(
                "\nFinding: [{:?}] {}\n",
                finding.severity, finding.title
            ));
            if !finding.suggestion.is_empty() {
                text.push_str(&format!("Suggestion: {}\n", finding.suggestion));
            }
        }

        let line_count = lines_to_copy.len();
        let scope = if tab.selected_range().is_some() {
            "selection"
        } else if tab.current_line.is_some() {
            "line"
        } else {
            "hunk"
        };
        Self::copy_to_clipboard(&text)?;
        self.notify(&format!("Copied {} ({} lines)", scope, line_count));
        Ok(())
    }

    pub(super) fn copy_to_clipboard(text: &str) -> Result<()> {
        let (cmd, args): (&str, Vec<&str>) = if cfg!(target_os = "macos") {
            ("pbcopy", vec![])
        } else if cfg!(target_os = "windows") {
            ("clip", vec![])
        } else {
            // Linux — try xclip, fall back to xsel
            if std::process::Command::new("which")
                .arg("xclip")
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false)
            {
                ("xclip", vec!["-selection", "clipboard"])
            } else {
                ("xsel", vec!["--clipboard", "--input"])
            }
        };

        let mut child = std::process::Command::new(cmd)
            .args(&args)
            .stdin(std::process::Stdio::piped())
            .spawn()
            .context("Failed to open clipboard command")?;

        if let Some(ref mut stdin) = child.stdin {
            stdin.write_all(text.as_bytes())?;
        }

        child.wait().context("Clipboard command failed")?;
        Ok(())
    }

    // ── Notifications ──

    pub fn notify(&mut self, msg: &str) {
        self.watch_message = Some(msg.to_string());
        self.watch_message_ticks = 0;
        self.watch_message_max_ticks = 20; // ~2s
    }

    /// Like notify but persists for ~5 seconds — for important results.
    pub fn notify_long(&mut self, msg: &str) {
        self.watch_message = Some(msg.to_string());
        self.watch_message_ticks = 0;
        self.watch_message_max_ticks = 50; // ~5s
    }

    // ── Background Commands ──

    /// Build a human-readable summary after an agent command completes.
    /// Reads the output files to report what was produced.
    fn agent_completion_summary_for(tab: &TabState, name: &str) -> String {
        let er_dir = std::path::PathBuf::from(tab.er_dir());

        match name {
            "review" => {
                let review_path = er_dir.join("review.json");
                if let Ok(content) = std::fs::read_to_string(&review_path) {
                    if let Ok(review) = serde_json::from_str::<ai::ErReview>(&content) {
                        let file_count = review.files.len();
                        let finding_count: usize =
                            review.files.values().map(|f| f.findings.len()).sum();
                        format!(
                            "Review done — {} file{}, {} finding{}",
                            file_count,
                            if file_count == 1 { "" } else { "s" },
                            finding_count,
                            if finding_count == 1 { "" } else { "s" },
                        )
                    } else {
                        "Review done — review.json written but could not be parsed".into()
                    }
                } else {
                    "Review done — but no review.json found (agent may lack permissions)".into()
                }
            }
            "questions" => {
                let questions_path = er_dir.join("questions.json");
                if let Ok(content) = std::fs::read_to_string(&questions_path) {
                    if let Ok(qs) = serde_json::from_str::<ai::ErQuestions>(&content) {
                        let answered = qs
                            .questions
                            .iter()
                            .filter(|q| q.in_reply_to.is_some())
                            .count();
                        let total = qs
                            .questions
                            .iter()
                            .filter(|q| q.in_reply_to.is_none())
                            .count();
                        format!("Questions done — {} of {} answered", answered, total)
                    } else {
                        "Questions done — questions.json written but could not be parsed".into()
                    }
                } else {
                    "Questions done — but no questions.json found".into()
                }
            }
            _ => format!("{} done", name),
        }
    }

    fn agent_command_writes_ai_artifacts(name: &str) -> bool {
        matches!(
            name,
            "summary"
                | "review"
                | "questions"
                | "triage"
                | "professor"
                | "validate"
                | "validate-comments"
        ) || name.starts_with("expert-")
    }

    /// Spawn a shell command in the background under the given name.
    /// The command string is run via `sh -c` in the repo root.
    /// Placeholders {base}, {branch}, {repo}, {output} are substituted.
    #[allow(clippy::literal_string_with_formatting_args)] // {base}/{branch}/{repo}/{output} are template placeholders, not format args
    pub fn spawn_command(&mut self, name: &str, shell_cmd: &str) -> Result<()> {
        if self.tab().command_status.get(name) == Some(&CommandStatus::Running) {
            self.notify(&format!("{} already running", name));
            return Ok(());
        }

        let tab = self.tab();
        let repo_root = tab.repo_root.clone();
        let base = tab.base_branch.clone();
        let branch = tab.current_branch.clone();
        let er_dir = tab.er_dir();
        let output_path = format!("{}/summary.md", er_dir);

        // Substitute placeholders — sanitize values for safe shell interpolation
        let cmd = shell_cmd
            .replace("{base}", &crate::ai::prompts::sanitize_for_shell(&base))
            .replace("{branch}", &crate::ai::prompts::sanitize_for_shell(&branch))
            .replace(
                "{repo}",
                &crate::ai::prompts::sanitize_for_shell(&repo_root),
            )
            .replace(
                "{output}",
                &crate::ai::prompts::sanitize_for_shell(&output_path),
            );

        std::fs::create_dir_all(&er_dir)?;

        let push_to_pr = name == "summary" && self.config.summary.push_to_pr;
        let name_owned = name.to_string();

        // Send status log entry before spawning
        let _ = self.tab().log_tx.send(AgentLogEntry {
            timestamp: std::time::Instant::now(),
            command_name: name.to_string(),
            source: AgentLogSource::Status,
            text: format!("{} started", name),
        });

        let log_tx = self.tab().log_tx.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let result = (|| -> Result<()> {
                let mut child = std::process::Command::new("sh")
                    .args(["-c", &cmd])
                    .current_dir(&repo_root)
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped())
                    .spawn()
                    .with_context(|| format!("Failed to run {}", name_owned))?;

                let stdout = child.stdout.take();
                let stderr = child.stderr.take();

                let log_tx_out = log_tx.clone();
                let cmd_name_out = name_owned.clone();
                let stdout_handle = std::thread::spawn(move || {
                    if let Some(pipe) = stdout {
                        use std::io::BufRead;
                        let reader = std::io::BufReader::new(pipe);
                        for line in reader.lines().map_while(Result::ok) {
                            let _ = log_tx_out.send(AgentLogEntry {
                                timestamp: std::time::Instant::now(),
                                command_name: cmd_name_out.clone(),
                                source: AgentLogSource::Stdout,
                                text: line,
                            });
                        }
                    }
                });

                let log_tx_err = log_tx.clone();
                let cmd_name_err = name_owned.clone();
                let mut stderr_lines: Vec<String> = Vec::new();
                let stderr_handle = std::thread::spawn(move || -> Vec<String> {
                    if let Some(pipe) = stderr {
                        use std::io::BufRead;
                        let reader = std::io::BufReader::new(pipe);
                        for line in reader.lines().map_while(Result::ok) {
                            let _ = log_tx_err.send(AgentLogEntry {
                                timestamp: std::time::Instant::now(),
                                command_name: cmd_name_err.clone(),
                                source: AgentLogSource::Stderr,
                                text: line.clone(),
                            });
                            stderr_lines.push(line);
                        }
                    }
                    stderr_lines
                });

                let status = child
                    .wait()
                    .with_context(|| format!("Failed to wait for {}", name_owned))?;
                let _ = stdout_handle.join();
                let accumulated_stderr = stderr_handle.join().unwrap_or_default();

                if !status.success() {
                    let stderr_text = accumulated_stderr.join("\n");
                    anyhow::bail!("{} failed: {}", name_owned, stderr_text.trim());
                }

                // Summary-specific: optionally push to PR body
                if push_to_pr {
                    let summary_path = std::path::Path::new(&er_dir).join("summary.md");
                    if let Ok(summary) = std::fs::read_to_string(&summary_path) {
                        if !summary.trim().is_empty() {
                            crate::github::gh_pr_edit_body(&repo_root, &summary)?;
                        }
                    }
                }

                Ok(())
            })();
            let _ = tx.send(result);
        });

        self.tab_mut().command_rx.insert(name.to_string(), rx);
        self.tab_mut()
            .command_status
            .insert(name.to_string(), CommandStatus::Running);
        self.notify(&format!("{} started...", name));
        Ok(())
    }

    /// Drain all pending agent log entries from the channel into `agent_log`.
    /// Called each tick. Auto-scrolls the AgentLog panel when new entries arrive.
    pub fn drain_agent_log(&mut self) {
        for (i, tab) in self.tabs.iter_mut().enumerate() {
            let mut received = false;
            while let Ok(entry) = tab.log_rx.try_recv() {
                tab.agent_log.push_back(entry);
                received = true;
                if tab.agent_log.len() > 5000 {
                    tab.agent_log.pop_front();
                }
            }
            if received && i == self.active_tab && tab.agent_log_auto_scroll {
                if let Some(panel) = tab.panel {
                    if panel == crate::ai::PanelContent::AgentLog {
                        tab.panel_scroll =
                            tab.agent_log.len().saturating_sub(1).min(u16::MAX as usize) as u16;
                    }
                }
            }
        }
    }

    /// Poll all running commands for completion (called from event loop).
    pub fn check_commands(&mut self) {
        // Collect completions per tab to avoid borrow conflicts with notify_long
        let mut notifications: Vec<String> = Vec::new();

        for tab in self.tabs.iter_mut() {
            let names: Vec<String> = tab.command_rx.keys().cloned().collect();
            for name in names {
                let result = if let Some(rx) = tab.command_rx.get(&name) {
                    match rx.try_recv() {
                        Ok(ok_or_err) => Some(ok_or_err),
                        Err(std::sync::mpsc::TryRecvError::Empty) => None,
                        Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                            Some(Err(anyhow::anyhow!("{} thread crashed", name)))
                        }
                    }
                } else {
                    None
                };

                if let Some(result) = result {
                    tab.command_rx.remove(&name);
                    match result {
                        Ok(()) => {
                            tab.command_status.insert(name.clone(), CommandStatus::Done);
                            let _ = tab.log_tx.send(AgentLogEntry {
                                timestamp: std::time::Instant::now(),
                                command_name: name.clone(),
                                source: AgentLogSource::Status,
                                text: format!("{} completed", name),
                            });
                            // Force AI reload for commands that write sidecar artifacts.
                            // reload_ai_state() resets last_ai_check itself.
                            if Self::agent_command_writes_ai_artifacts(&name) {
                                tab.reload_ai_state();
                            }
                            let msg = Self::agent_completion_summary_for(tab, &name);
                            notifications.push(msg);
                        }
                        Err(e) => {
                            let msg = format!("{}", e);
                            tab.command_status
                                .insert(name.clone(), CommandStatus::Failed(msg.clone()));
                            let _ = tab.log_tx.send(AgentLogEntry {
                                timestamp: std::time::Instant::now(),
                                command_name: name.clone(),
                                source: AgentLogSource::Status,
                                text: format!("{} failed: {}", name, msg),
                            });
                            // Truncate long error messages to fit status bar (safe for multi-byte UTF-8)
                            let short = if msg.len() > 80 {
                                let boundary = msg
                                    .char_indices()
                                    .nth(80)
                                    .map(|(i, _)| i)
                                    .unwrap_or(msg.len());
                                format!("{}…", &msg[..boundary])
                            } else {
                                msg
                            };
                            notifications.push(format!("{} failed: {}", name, short));
                        }
                    }
                }
            }
        }

        // Apply notifications after the tab iteration loop (avoids borrow conflict)
        for msg in notifications {
            self.notify_long(&msg);
        }
    }
    /// Spawn the configured agent command with a pre-built prompt.
    ///
    /// Uses `agent.command` from config (default: "claude") with `-p` flag
    /// for non-interactive agentic execution. The agent is expected to read
    /// the diff and write `.er/` files directly.
    pub fn spawn_agent_prompt(&mut self, name: &str, prompt: &str) -> Result<()> {
        if self.tab().command_status.get(name) == Some(&CommandStatus::Running) {
            self.notify(&format!("{} already running", name));
            return Ok(());
        }

        let repo_root = self.tab().repo_root.clone();
        let er_dir_path = self.tab().er_dir();
        let is_remote = self.tab().is_remote();
        let remote_repo = self.tab().remote_repo.clone();
        let selection = if let Some(selection) = self.pending_ai_selection_override.clone() {
            selection
        } else {
            self.sync_ai_selection();
            crate::config::AiSelection {
                provider_id: self.current_ai_provider.clone(),
                model_id: self.current_ai_model.clone(),
                effort: self.current_ai_effort.clone(),
            }
        };

        let (
            agent_cmd,
            mut config_args,
            is_claude_compatible,
            is_codex,
            is_stream_json,
            resolved_provider_id,
            resolved_model_id,
            family,
        ) = if let Some(provider_id) = self
            .config
            .ai_hub
            .resolve_provider_id(selection.provider_id.as_deref())
        {
            let provider = self
                .config
                .ai_hub
                .providers
                .get(&provider_id)
                .ok_or_else(|| anyhow::anyhow!("Unknown AI provider: {}", provider_id))?;
            let mut args = provider.args.clone();
            let resolved_model_id = self
                .config
                .ai_hub
                .resolve_model_id(&provider_id, selection.model_id.as_deref());
            let family = provider.cli_family();
            if let Some(model_id) = &resolved_model_id {
                if let Some(model) = provider.models.iter().find(|m| m.id == *model_id) {
                    crate::config::extend_provider_model_args(family, &mut args, &model.args);
                }
            }
            let is_claude = crate::config::agent_command_is_claude(&provider.command);
            let is_codex = crate::config::agent_command_is_codex(&provider.command);
            (
                provider.command.clone(),
                args,
                is_claude,
                is_codex,
                provider.uses_stream_json_log(),
                Some(provider_id),
                resolved_model_id,
                family,
            )
        } else {
            let cmd = self.config.agent.command.clone();
            let is_claude = crate::config::agent_command_is_claude(&cmd);
            let is_codex = crate::config::agent_command_is_codex(&cmd);
            let family = crate::config::CliFamily::detect(&cmd);
            (
                cmd.clone(),
                self.config.agent.args.clone(),
                is_claude,
                is_codex,
                crate::config::agent_command_uses_stream_json(&cmd),
                None,
                (!self.config.agent.model.is_empty()).then(|| self.config.agent.model.clone()),
                family,
            )
        };
        let effort_override = if name == "triage" { Some("low") } else { None };
        let effort = crate::config::resolve_effort_for_model(
            &self.config.ai_hub,
            &self.config.agent,
            resolved_provider_id.as_deref(),
            resolved_model_id.as_deref(),
            selection.effort.as_deref(),
            effort_override,
        );
        crate::config::inject_provider_effort(
            family,
            &mut config_args,
            resolved_model_id.as_deref(),
            effort.as_deref(),
        );
        if is_codex {
            crate::config::inject_codex_ignore_user_config(&mut config_args);
        }
        crate::config::inject_agent_storage_access(
            family,
            &mut config_args,
            Some(er_dir_path.as_str()),
        );
        let opencode_env = crate::config::apply_opencode_spawn(
            family,
            &mut config_args,
            Some(er_dir_path.as_str()),
        );

        // Ensure .er/ directory exists
        std::fs::create_dir_all(&er_dir_path)?;

        let name_owned = name.to_string();
        let prompt_owned = prompt.to_string();

        // Send status log entry before spawning
        let _ = self.tab().log_tx.send(AgentLogEntry {
            timestamp: std::time::Instant::now(),
            command_name: name.to_string(),
            source: AgentLogSource::Status,
            text: format!("{} started", name),
        });

        let log_tx = self.tab().log_tx.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let result = (|| -> Result<()> {
                let debug_path = std::path::Path::new(&er_dir_path).join("debug-agent.log");

                let mut agent_args: Vec<String> = config_args
                    .iter()
                    .map(|a| a.replace("{prompt}", &prompt_owned))
                    .collect();

                // Auto-inject --output-format stream-json for claude commands so the
                // agent log panel can show real-time tool calls and progress. This is
                // injected here (not in config defaults) so user configs that override
                // agent.args still get streaming without manual changes.
                if is_stream_json {
                    if !agent_args.iter().any(|a| a == "--output-format") {
                        agent_args.push("--output-format".to_string());
                        agent_args.push("stream-json".to_string());
                    }
                    // --verbose is required when combining --print with stream-json (Claude only)
                    let has_print = agent_args.iter().any(|a| a == "--print");
                    let has_stream = agent_args.iter().any(|a| a == "stream-json");
                    let has_verbose = agent_args.iter().any(|a| a == "--verbose");
                    if is_claude_compatible && has_print && has_stream && !has_verbose {
                        agent_args.push("--verbose".to_string());
                    }
                }

                // Grant the agent targeted tool permissions without blanket
                // --dangerously-skip-permissions. The prompt is fully controlled by er.
                if is_claude_compatible {
                    let allowed: &[&str] = &[
                        "Read",
                        "Write",
                        "Edit",
                        "Bash(gh pr *)",
                        "Bash(cp *)",
                        "Bash(grep *)",
                        "Bash(rg *)",
                        "Bash(git grep*)",
                        "Bash(git diff*)",
                        "Bash(git show*)",
                        // Narrow on purpose: a bare `git fetch*` would also permit
                        // `git fetch origin +refs/heads/*:refs/heads/*`, which can
                        // clobber the user's local branches.
                        "Bash(git fetch origin pull/*)",
                        "Bash(shasum*)",
                        "Bash(sha256sum*)",
                        "Bash(mkdir*)",
                        "Bash(awk*)",
                    ];
                    for rule in allowed.iter().rev() {
                        agent_args.insert(0, rule.to_string());
                        agent_args.insert(0, "--allowedTools".to_string());
                    }
                    agent_args.insert(0, CLONE_DENY_RULE.to_string());
                    agent_args.insert(0, "--disallowedTools".to_string());
                }

                // Remote mode has no repo around it, which is what sends the agent hunting
                // the filesystem for a copy of the code. When the local checkout *is* the
                // PR's repo, run there so verification reads hit real files; otherwise fall
                // back to the artifact dir. Output dirs are absolute either way.
                let remote_checkout = if is_remote {
                    remote_repo
                        .as_deref()
                        .and_then(|slug| crate::github::local_checkout_for_repo(&repo_root, slug))
                } else {
                    None
                };
                let work_dir = if let Some(checkout) = &remote_checkout {
                    checkout
                } else if is_remote {
                    &er_dir_path
                } else {
                    &repo_root
                };
                let mut cmd = std::process::Command::new(&agent_cmd);
                cmd.args(&agent_args)
                    .current_dir(work_dir)
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped());
                if let Some((key, value)) = &opencode_env {
                    cmd.env(key, value);
                }
                let mut child = cmd
                    .spawn()
                    .with_context(|| format!("Failed to run {} ({})", name_owned, agent_cmd))?;

                let stdout = child.stdout.take();
                let stderr = child.stderr.take();

                // Accumulate stdout for debug log while also streaming to agent log.
                // When output is stream-json (default for claude), parse events into
                // human-readable log entries. Falls back to raw lines for other formats.
                let log_tx_out = log_tx.clone();
                let cmd_name_out = name_owned.clone();
                let stdout_handle = std::thread::spawn(move || -> Vec<String> {
                    let mut lines: Vec<String> = Vec::new();
                    if let Some(pipe) = stdout {
                        use std::io::BufRead;
                        let reader = std::io::BufReader::new(pipe);
                        for line in reader.lines().map_while(Result::ok) {
                            lines.push(line.clone());
                            // Try to parse as stream-json event
                            let display = if is_stream_json {
                                parse_stream_json_line(&line)
                            } else {
                                Some(line.trim().to_string())
                            };
                            if let Some(text) = display.filter(|text| !text.is_empty()) {
                                let _ = log_tx_out.send(AgentLogEntry {
                                    timestamp: std::time::Instant::now(),
                                    command_name: cmd_name_out.clone(),
                                    source: AgentLogSource::Stdout,
                                    text,
                                });
                            }
                            // Skip lines that parse to None (noise like empty results)
                        }
                    }
                    lines
                });

                // Accumulate stderr for debug log while also streaming to agent log
                let log_tx_err = log_tx.clone();
                let cmd_name_err = name_owned.clone();
                let stderr_handle = std::thread::spawn(move || -> Vec<String> {
                    let mut lines: Vec<String> = Vec::new();
                    if let Some(pipe) = stderr {
                        use std::io::BufRead;
                        let reader = std::io::BufReader::new(pipe);
                        for line in reader.lines().map_while(Result::ok) {
                            let _ = log_tx_err.send(AgentLogEntry {
                                timestamp: std::time::Instant::now(),
                                command_name: cmd_name_err.clone(),
                                source: AgentLogSource::Stderr,
                                text: line.clone(),
                            });
                            lines.push(line);
                        }
                    }
                    lines
                });

                let status = child.wait().with_context(|| {
                    format!("Failed to wait for {} ({})", name_owned, agent_cmd)
                })?;
                let stdout_lines = stdout_handle.join().unwrap_or_default();
                let stderr_lines = stderr_handle.join().unwrap_or_default();

                // Write debug log with accumulated stdout + stderr
                let debug_content = format!(
                    "=== {} agent command ===\ncommand: {} {}\nexit code: {}\n\n--- stdout ---\n{}\n\n--- stderr ---\n{}\n",
                    name_owned,
                    agent_cmd,
                    agent_args.join(" "),
                    status.code().map_or_else(|| "signal".to_string(), |c| c.to_string()),
                    stdout_lines.join("\n"),
                    stderr_lines.join("\n"),
                );
                let _ = std::fs::write(&debug_path, &debug_content);

                if !status.success() {
                    anyhow::bail!(
                        "{} failed (see {}/debug-agent.log)",
                        name_owned,
                        er_dir_path
                    );
                }

                Ok(())
            })();
            let _ = tx.send(result);
        });

        self.tab_mut().command_rx.insert(name.to_string(), rx);
        self.tab_mut()
            .command_status
            .insert(name.to_string(), CommandStatus::Running);
        self.notify(&format!("{} started...", name));
        Ok(())
    }

    pub fn tick(&mut self) {
        if self.watch_message.is_some() {
            self.watch_message_ticks += 1;
            if self.watch_message_ticks > self.watch_message_max_ticks {
                self.watch_message = None;
                self.watch_message_ticks = 0;
            }
        }
    }

    /// Spawn an app-level background general review (`kind` = `review`).
    pub fn spawn_background_review(
        &mut self,
        target: super::background::BackgroundTaskTarget,
        prompt: String,
        prepared_diff: bool,
    ) -> Result<()> {
        self.spawn_background_agent_task(
            "review".to_string(),
            "review",
            target,
            prompt,
            prepared_diff,
            None,
        )
    }

    /// Spawn a specialized expert review (`kind` = `expert:{id}`). General and
    /// multiple experts can run concurrently on the same target.
    pub fn spawn_background_expert_review(
        &mut self,
        expert_id: &str,
        target: super::background::BackgroundTaskTarget,
        prompt: String,
        prepared_diff: bool,
    ) -> Result<()> {
        let def = crate::ai::expert_by_id(expert_id)
            .ok_or_else(|| anyhow::anyhow!("unknown expert: {expert_id}"))?;
        self.spawn_background_agent_task(
            crate::ai::expert_task_kind(expert_id),
            &format!("expert-{}", def.id),
            target,
            prompt,
            prepared_diff,
            None,
        )
    }

    /// Spawn guided-tour generation (`kind` = `tour`). Writes `tour.json` only.
    pub fn spawn_background_tour(
        &mut self,
        target: super::background::BackgroundTaskTarget,
        prompt: String,
        prepared_diff: bool,
    ) -> Result<()> {
        self.spawn_background_agent_task(
            "tour".to_string(),
            "tour",
            target,
            prompt,
            prepared_diff,
            None,
        )
    }

    /// Spawn diagram generation (`kind` = `diagram:<diagram-kind>`). The agent
    /// runs read-only and emits JSON; the harness atomically writes
    /// `diagrams/<file>.json` only (prompt-injection write confinement).
    pub fn spawn_background_diagram(
        &mut self,
        diagram_kind: &str,
        target: super::background::BackgroundTaskTarget,
        prompt: String,
        prepared_diff: bool,
        host_write: super::background::HostWriteDiagram,
    ) -> Result<()> {
        self.spawn_background_agent_task(
            crate::ai::diagram_task_kind(diagram_kind),
            "diagram",
            target,
            prompt,
            prepared_diff,
            Some(host_write),
        )
    }

    /// Spawn the Professor learning agent (`kind` = `professor`).
    pub fn spawn_background_professor_review(
        &mut self,
        target: super::background::BackgroundTaskTarget,
        prompt: String,
        prepared_diff: bool,
    ) -> Result<()> {
        self.spawn_background_agent_task(
            crate::ai::professor_task_kind(),
            "professor",
            target,
            prompt,
            prepared_diff,
            None,
        )
    }

    /// Spawn triage scan (`kind` = `triage`) using the shared default model at low effort.
    pub fn spawn_background_triage_review(
        &mut self,
        target: super::background::BackgroundTaskTarget,
        prompt: String,
        prepared_diff: bool,
    ) -> Result<()> {
        self.spawn_background_agent_task(
            crate::ai::triage_task_kind(),
            "triage",
            target,
            prompt,
            prepared_diff,
            None,
        )
    }

    /// Number of background agent tasks currently running a subprocess.
    pub(crate) fn running_background_task_count(&self) -> usize {
        self.background_tasks
            .values()
            .filter(|h| matches!(h.task.status, CommandStatus::Running))
            .count()
    }

    /// App-level background agent task (review or expert). When the number
    /// of running tasks has reached `ai_hub.max_concurrent_reviews`, the
    /// task is queued and launched later by `poll_background_tasks`.
    fn spawn_background_agent_task(
        &mut self,
        kind: String,
        command_name: &str,
        target: super::background::BackgroundTaskTarget,
        prompt: String,
        prepared_diff: bool,
        host_write_diagram: Option<super::background::HostWriteDiagram>,
    ) -> Result<()> {
        use super::background::{BackgroundTask, PendingBackgroundTask};

        if target.repo_root.is_empty() && target.remote_repo.is_none() {
            anyhow::bail!("Open a repository or PR first — nothing to review yet");
        }

        let already_active = self.background_tasks.values().any(|h| {
            h.task.kind == kind
                && h.task.target == target
                && matches!(h.task.status, CommandStatus::Running)
        }) || self
            .pending_background_tasks
            .iter()
            .any(|p| p.task.kind == kind && p.task.target == target);
        if already_active {
            anyhow::bail!(
                "{command_name} already running for {}",
                target.display_label()
            );
        }

        let pending =
            PendingBackgroundTask {
                task: BackgroundTask::new(kind, target),
                command_name: command_name.to_string(),
                prompt,
                prepared_diff,
                host_write_diagram,
                // Snapshot at enqueue so a mid-queue palette change cannot retarget
                // an already-queued job.
                ai_selection: Some(self.pending_ai_selection_override.clone().unwrap_or_else(
                    || {
                        self.sync_ai_selection();
                        crate::config::AiSelection {
                            provider_id: self.current_ai_provider.clone(),
                            model_id: self.current_ai_model.clone(),
                            effort: self.current_ai_effort.clone(),
                        }
                    },
                )),
            };

        let cap = self.config.ai_hub.effective_max_concurrent_reviews();
        if self.running_background_task_count() >= cap {
            let label = pending.task.target.display_label();
            self.pending_background_tasks.push_back(pending);
            let pos = self.pending_background_tasks.len();
            self.notify(&format!("{command_name} queued (#{pos}, {label})"));
            return Ok(());
        }

        self.launch_background_agent_task(pending)
    }

    /// Actually spawn the agent subprocess for an accepted task. Split from
    /// `spawn_background_agent_task` so the dispatch loop can launch queued
    /// tasks when capacity frees up.
    #[allow(clippy::literal_string_with_formatting_args)] // {prompt} is a template placeholder substituted via .replace()
    fn launch_background_agent_task(
        &mut self,
        pending: super::background::PendingBackgroundTask,
    ) -> Result<()> {
        use super::background::BackgroundTaskHandle;

        let super::background::PendingBackgroundTask {
            mut task,
            command_name,
            prompt,
            prepared_diff,
            host_write_diagram,
            ai_selection,
        } = pending;
        let command_name = command_name.as_str();
        let target = task.target.clone();
        // The task may have waited in the queue; report runtime from launch.
        // The id (assigned at enqueue) stays stable so UI pills don't jump.
        task.started_at_ms = super::background::unix_now_ms();

        let selection = ai_selection.unwrap_or_else(|| {
            self.sync_ai_selection();
            crate::config::AiSelection {
                provider_id: self.current_ai_provider.clone(),
                model_id: self.current_ai_model.clone(),
                effort: self.current_ai_effort.clone(),
            }
        });

        let (
            agent_cmd,
            mut config_args,
            is_claude_compatible,
            is_codex,
            is_stream_json,
            resolved_provider_id,
            resolved_model_id,
            family,
        ) = if let Some(provider_id) = self
            .config
            .ai_hub
            .resolve_provider_id(selection.provider_id.as_deref())
        {
            let provider = self
                .config
                .ai_hub
                .providers
                .get(&provider_id)
                .ok_or_else(|| anyhow::anyhow!("Unknown AI provider: {}", provider_id))?;
            let mut args = provider.args.clone();
            let resolved_model_id = self
                .config
                .ai_hub
                .resolve_model_id(&provider_id, selection.model_id.as_deref());
            let family = provider.cli_family();
            if let Some(model_id) = &resolved_model_id {
                if let Some(model) = provider.models.iter().find(|m| m.id == *model_id) {
                    crate::config::extend_provider_model_args(family, &mut args, &model.args);
                }
            }
            let is_claude = crate::config::agent_command_is_claude(&provider.command);
            let is_codex = crate::config::agent_command_is_codex(&provider.command);
            (
                provider.command.clone(),
                args,
                is_claude,
                is_codex,
                provider.uses_stream_json_log(),
                Some(provider_id),
                resolved_model_id,
                family,
            )
        } else {
            let cmd = self.config.agent.command.clone();
            let is_claude = crate::config::agent_command_is_claude(&cmd);
            let is_codex = crate::config::agent_command_is_codex(&cmd);
            let family = crate::config::CliFamily::detect(&cmd);
            (
                cmd.clone(),
                self.config.agent.args.clone(),
                is_claude,
                is_codex,
                crate::config::agent_command_uses_stream_json(&cmd),
                None,
                (!self.config.agent.model.is_empty()).then(|| self.config.agent.model.clone()),
                family,
            )
        };

        let effort_override = if task.kind == "triage" {
            Some("low")
        } else {
            None
        };
        let effort = crate::config::resolve_effort_for_model(
            &self.config.ai_hub,
            &self.config.agent,
            resolved_provider_id.as_deref(),
            resolved_model_id.as_deref(),
            selection.effort.as_deref(),
            effort_override,
        );
        crate::config::inject_provider_effort(
            family,
            &mut config_args,
            resolved_model_id.as_deref(),
            effort.as_deref(),
        );
        if is_codex {
            crate::config::inject_codex_ignore_user_config(&mut config_args);
        }
        crate::config::inject_agent_storage_access(
            family,
            &mut config_args,
            Some(target.er_dir.as_str()),
        );
        // Diagrams: host writes the sidecar — deny agent edit tools. Still allow
        // reading the managed bucket (diff-tmp) via external_directory allow.
        let opencode_env = if host_write_diagram.is_some() {
            crate::config::apply_opencode_readonly_storage_spawn(
                family,
                &mut config_args,
                Some(target.er_dir.as_str()),
            )
        } else {
            crate::config::apply_opencode_spawn(
                family,
                &mut config_args,
                Some(target.er_dir.as_str()),
            )
        };

        std::fs::create_dir_all(&target.er_dir)?;

        let task_id = task.id.clone();
        let er_dir = target.er_dir.clone();
        let repo_root = target.repo_root.clone();
        let remote_repo = target.remote_repo.clone();
        let is_remote = target.remote_repo.is_some();
        let managed_local = target.managed_local;
        // For local-branch views (cache-dir er_dir) treat like remote — cwd is
        // the cache dir so .er/* writes land where the loader reads them.
        let is_cache_er_dir = !er_dir.starts_with(&repo_root);

        let (log_tx, log_rx) = std::sync::mpsc::channel::<AgentLogEntry>();
        let (result_tx, result_rx) = std::sync::mpsc::channel::<Result<()>>();

        let _ = log_tx.send(AgentLogEntry {
            timestamp: std::time::Instant::now(),
            command_name: command_name.to_string(),
            source: AgentLogSource::Status,
            text: format!("{command_name} started ({})", target.display_label()),
        });

        let log_tx_thread = log_tx;
        let command_name_stdout = command_name.to_string();
        let command_name_stderr = command_name.to_string();
        let command_name_fail = command_name.to_string();
        let slot_cap = self.config.ai_hub.effective_max_concurrent_reviews();
        std::thread::spawn(move || {
            let result = (|| -> Result<()> {
                // Hard process-wide cap shared with arena reviewers. The
                // App-level queue already bounds how many of these workers
                // exist, so this only waits while arena rounds hold slots.
                let _slot = crate::agent_slots::acquire_blocking(slot_cap);
                let debug_path = std::path::Path::new(&er_dir).join("debug-agent.log");

                let mut agent_args: Vec<String> = config_args
                    .iter()
                    .map(|a| a.replace("{prompt}", &prompt))
                    .collect();

                if is_stream_json {
                    if !agent_args.iter().any(|a| a == "--output-format") {
                        agent_args.push("--output-format".to_string());
                        agent_args.push("stream-json".to_string());
                    }
                    let has_print = agent_args.iter().any(|a| a == "--print");
                    let has_stream = agent_args.iter().any(|a| a == "stream-json");
                    let has_verbose = agent_args.iter().any(|a| a == "--verbose");
                    if is_claude_compatible && has_print && has_stream && !has_verbose {
                        agent_args.push("--verbose".to_string());
                    }
                }

                if is_claude_compatible {
                    let allowed: &[&str] = if host_write_diagram.is_some() {
                        // Read-only: harness persists the diagram JSON from stdout.
                        &[
                            "Read",
                            "Bash(grep *)",
                            "Bash(rg *)",
                            "Bash(git grep*)",
                            "Bash(git show*)",
                            "Bash(git log*)",
                        ]
                    } else if prepared_diff {
                        &[
                            "Read",
                            "Write",
                            "Edit",
                            "Bash(grep *)",
                            "Bash(rg *)",
                            "Bash(git grep*)",
                            "Bash(git show*)",
                            "Bash(git fetch origin pull/*)",
                            "Bash(cp .er/*)",
                            "Bash(shasum*)",
                            "Bash(sha256sum*)",
                            "Bash(mkdir*)",
                            "Bash(awk*)",
                        ]
                    } else {
                        &[
                            "Read",
                            "Write",
                            "Edit",
                            "Bash(gh pr *)",
                            "Bash(cp .er/*)",
                            "Bash(git diff*)",
                            "Bash(git grep*)",
                            "Bash(git show*)",
                            "Bash(git fetch origin pull/*)",
                            "Bash(shasum*)",
                            "Bash(sha256sum*)",
                            "Bash(mkdir*)",
                        ]
                    };
                    for rule in allowed.iter().rev() {
                        agent_args.insert(0, rule.to_string());
                        agent_args.insert(0, "--allowedTools".to_string());
                    }
                    agent_args.insert(0, CLONE_DENY_RULE.to_string());
                    agent_args.insert(0, "--disallowedTools".to_string());
                }

                // managed_local: er_dir is outside repo_root but git must run from repo_root.
                // is_remote / is_cache_er_dir without managed_local: cwd = er_dir (cache-dir or remote).
                // A remote PR whose repo *is* the local checkout runs there instead, so the
                // agent can verify against real code rather than hunting the filesystem.
                let remote_checkout = if is_remote {
                    remote_repo
                        .as_deref()
                        .and_then(|slug| crate::github::local_checkout_for_repo(&repo_root, slug))
                } else {
                    None
                };
                let work_dir = if let Some(checkout) = &remote_checkout {
                    checkout
                } else if managed_local || (!is_remote && !is_cache_er_dir) {
                    &repo_root
                } else {
                    &er_dir
                };
                let mut cmd = std::process::Command::new(&agent_cmd);
                cmd.args(&agent_args)
                    .current_dir(work_dir)
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped());
                if let Some((key, value)) = &opencode_env {
                    cmd.env(key, value);
                }
                let mut child = cmd
                    .spawn()
                    .with_context(|| format!("Failed to run review ({})", agent_cmd))?;

                let stdout = child.stdout.take();
                let stderr = child.stderr.take();

                let log_tx_out = log_tx_thread.clone();
                let stdout_handle = std::thread::spawn(move || -> Vec<String> {
                    let mut lines: Vec<String> = Vec::new();
                    if let Some(pipe) = stdout {
                        use std::io::BufRead;
                        let reader = std::io::BufReader::new(pipe);
                        for line in reader.lines().map_while(Result::ok) {
                            lines.push(line.clone());
                            let display = if is_stream_json {
                                parse_stream_json_line(&line)
                            } else {
                                Some(line.trim().to_string())
                            };
                            if let Some(text) = display.filter(|t| !t.is_empty()) {
                                let _ = log_tx_out.send(AgentLogEntry {
                                    timestamp: std::time::Instant::now(),
                                    command_name: command_name_stdout.clone(),
                                    source: AgentLogSource::Stdout,
                                    text,
                                });
                            }
                        }
                    }
                    lines
                });

                let log_tx_err = log_tx_thread.clone();
                let stderr_handle = std::thread::spawn(move || -> Vec<String> {
                    let mut lines: Vec<String> = Vec::new();
                    if let Some(pipe) = stderr {
                        use std::io::BufRead;
                        let reader = std::io::BufReader::new(pipe);
                        for line in reader.lines().map_while(Result::ok) {
                            let _ = log_tx_err.send(AgentLogEntry {
                                timestamp: std::time::Instant::now(),
                                command_name: command_name_stderr.clone(),
                                source: AgentLogSource::Stderr,
                                text: line.clone(),
                            });
                            lines.push(line);
                        }
                    }
                    lines
                });

                let status = child
                    .wait()
                    .with_context(|| format!("Failed to wait for review ({})", agent_cmd))?;
                let stdout_lines = stdout_handle.join().unwrap_or_default();
                let stderr_lines = stderr_handle.join().unwrap_or_default();

                let debug_content = format!(
                    "=== review agent command ===\ncommand: {} {}\nexit code: {}\n\n--- stdout ---\n{}\n\n--- stderr ---\n{}\n",
                    agent_cmd,
                    agent_args.join(" "),
                    status.code().map_or_else(|| "signal".to_string(), |c| c.to_string()),
                    stdout_lines.join("\n"),
                    stderr_lines.join("\n"),
                );
                let _ = std::fs::write(&debug_path, &debug_content);

                if !status.success() {
                    let stderr_snip = {
                        let joined = stderr_lines.join("\n");
                        let trimmed = joined.trim();
                        if trimmed.is_empty() {
                            String::new()
                        } else {
                            // Char-safe: agent stderr often includes multi-byte text.
                            let max_chars = 280usize;
                            if trimmed.chars().count() <= max_chars {
                                trimmed.to_string()
                            } else {
                                let boundary = trimmed
                                    .char_indices()
                                    .nth(max_chars)
                                    .map(|(i, _)| i)
                                    .unwrap_or(trimmed.len());
                                format!("{}…", &trimmed[..boundary])
                            }
                        }
                    };
                    if stderr_snip.to_lowercase().contains("not logged in")
                        || stderr_snip.to_lowercase().contains("authentication")
                        || stderr_snip.to_lowercase().contains("unauthorized")
                        || stderr_snip.to_lowercase().contains("not authenticated")
                    {
                        anyhow::bail!(
                            "{command_name_fail}: `{agent_cmd}` needs authentication. \
                             Run `{agent_cmd}` in a terminal to sign in, then retry. {stderr_snip}"
                        );
                    }
                    if stderr_snip.is_empty() {
                        anyhow::bail!(
                            "{command_name_fail} failed (exit {:?}). See Settings → AI Hub if the \
                             provider CLI is missing or not signed in.",
                            status.code()
                        );
                    }
                    anyhow::bail!("{command_name_fail} failed: {stderr_snip}");
                }
                // Host-owned diagram write: agent had no Write/Edit; persist
                // only the validated diagrams/<id>.json from stdout.
                if let Some(hw) = &host_write_diagram {
                    crate::ai::persist_diagram_from_agent_stdout(
                        &stdout_lines.join("\n"),
                        is_stream_json,
                        &hw.kind,
                        &hw.diff_hash,
                        hw.custom_prompt.as_deref(),
                        &hw.output_path,
                    )
                    .with_context(|| {
                        format!("{command_name_fail}: failed to persist diagram sidecar")
                    })?;
                }
                // Selected-file reviews overwrite sidecars with a subset —
                // merge back into the pre-scoped snapshot when present.
                if let Err(e) = crate::ai::apply_scoped_sidecar_merge(
                    std::path::Path::new(&er_dir),
                    &command_name_fail,
                ) {
                    anyhow::bail!(
                        "{command_name_fail} wrote artifacts but failed to merge with previous review: {e}"
                    );
                }
                Ok(())
            })();
            let _ = result_tx.send(result);
        });

        self.background_tasks.insert(
            task_id.clone(),
            BackgroundTaskHandle {
                task,
                result_rx,
                log_rx,
                recent_log: std::collections::VecDeque::new(),
            },
        );

        if super::background::debug_bg_enabled() {
            eprintln!(
                "[bg] inserted task id={} target={} map_size={}",
                task_id,
                target.display_label(),
                self.background_tasks.len()
            );
        }

        self.notify(&format!(
            "{command_name} started ({})",
            target.display_label()
        ));
        Ok(())
    }

    /// Drain log channels and check for completion across all app-level
    /// background tasks. Should be called once per tick from the event loop
    /// (or per `poll` Tauri call on the desktop).
    pub fn poll_background_tasks(&mut self) {
        // 1. Drain log channels — push entries into matching tabs' agent_log
        //    so existing UI keeps working when viewing a matching tab.
        let task_ids: Vec<String> = self.background_tasks.keys().cloned().collect();
        for id in &task_ids {
            // Pull log entries without holding a long borrow.
            let mut drained: Vec<AgentLogEntry> = Vec::new();
            if let Some(handle) = self.background_tasks.get(id) {
                while let Ok(entry) = handle.log_rx.try_recv() {
                    drained.push(entry);
                }
            }
            if drained.is_empty() {
                continue;
            }
            let target = match self.background_tasks.get(id) {
                Some(h) => h.task.target.clone(),
                None => continue,
            };
            for tab in self.tabs.iter_mut() {
                if !tab.matches_target(&target) {
                    continue;
                }
                for entry in &drained {
                    tab.agent_log.push_back(entry.clone());
                    if tab.agent_log.len() > 5000 {
                        tab.agent_log.pop_front();
                    }
                }
            }
            // Also push into the per-handle ring buffer for app-wide log access.
            if let Some(handle) = self.background_tasks.get_mut(id) {
                for entry in &drained {
                    handle.recent_log.push_back(entry.clone());
                    if handle.recent_log.len() > 500 {
                        handle.recent_log.pop_front();
                    }
                }
            }
        }

        // 2. Check for completion.
        let mut completed: Vec<(String, Result<()>)> = Vec::new();
        for id in &task_ids {
            if let Some(handle) = self.background_tasks.get(id) {
                if !matches!(handle.task.status, CommandStatus::Running) {
                    continue;
                }
                match handle.result_rx.try_recv() {
                    Ok(res) => completed.push((id.clone(), res)),
                    Err(std::sync::mpsc::TryRecvError::Empty) => {}
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        completed.push((id.clone(), Err(anyhow::anyhow!("review thread crashed"))));
                    }
                }
            }
        }

        for (id, result) in completed {
            let now = super::background::unix_now_ms();
            let Some(handle) = self.background_tasks.get_mut(&id) else {
                continue;
            };
            let target = handle.task.target.clone();
            let label = target.display_label();
            let (status, error, status_msg) = match result {
                Ok(()) => (
                    CommandStatus::Done,
                    None,
                    format!("review completed ({})", label),
                ),
                Err(e) => {
                    let msg = e.to_string();
                    (
                        CommandStatus::Failed(msg.clone()),
                        Some(msg.clone()),
                        format!("review failed ({}): {}", label, msg),
                    )
                }
            };
            handle.task.status = status.clone();
            handle.task.finished_at_ms = Some(now);
            handle.task.error = error.clone();

            // Force reload only on matching tabs. No `last_ai_check = None`
            // reset here (O5): the agent's freshly written sidecars have
            // newer mtimes than the previous check, so `check_ai_files_changed`
            // fires the reload naturally — while a tab whose poll already
            // loaded the final files skips the redundant full re-read.
            for tab in self.tabs.iter_mut() {
                if tab.matches_target(&target) {
                    tab.push_synthetic_log("review", status_msg.clone(), AgentLogSource::Status);
                }
            }
            // Mirror completion status into handle's recent_log.
            if let Some(handle) = self.background_tasks.get_mut(&id) {
                handle.recent_log.push_back(AgentLogEntry {
                    timestamp: std::time::Instant::now(),
                    command_name: "review".to_string(),
                    source: AgentLogSource::Status,
                    text: status_msg.clone(),
                });
                if handle.recent_log.len() > 500 {
                    handle.recent_log.pop_front();
                }
            }

            self.notify_long(&status_msg);
        }

        // 3. Retire finished tasks past the 8s display window into
        //    `recent_background_tasks` (snapshot-only).
        let cutoff = super::background::unix_now_ms().saturating_sub(8_000);
        let mut to_remove: Vec<String> = Vec::new();
        for (id, handle) in self.background_tasks.iter() {
            if let Some(finished_at) = handle.task.finished_at_ms {
                if finished_at < cutoff {
                    to_remove.push(id.clone());
                }
            }
        }
        for id in to_remove {
            if let Some(handle) = self.background_tasks.remove(&id) {
                // Keep around in finished form only briefly; bounded.
                self.recent_background_tasks.push(handle.task);
                if self.recent_background_tasks.len() > 32 {
                    self.recent_background_tasks.remove(0);
                }
            }
        }

        // 4. Launch queued tasks while there's capacity.
        self.dispatch_pending_background_tasks();
    }

    /// Pop queued review tasks and launch them while the running count is
    /// below the configured cap. Launch failures are surfaced as failed
    /// tasks so the UI shows them like any other review failure.
    fn dispatch_pending_background_tasks(&mut self) {
        let cap = self.config.ai_hub.effective_max_concurrent_reviews();
        while !self.pending_background_tasks.is_empty()
            && self.running_background_task_count() < cap
        {
            let Some(pending) = self.pending_background_tasks.pop_front() else {
                break;
            };
            let label = pending.task.target.display_label();
            let command_name = pending.command_name.clone();
            let mut failed_task = pending.task.clone();
            if let Err(e) = self.launch_background_agent_task(pending) {
                let msg = format!("{command_name} failed to start ({label}): {e}");
                failed_task.status = CommandStatus::Failed(e.to_string());
                failed_task.error = Some(e.to_string());
                failed_task.finished_at_ms = Some(super::background::unix_now_ms());
                self.recent_background_tasks.push(failed_task);
                if self.recent_background_tasks.len() > 32 {
                    self.recent_background_tasks.remove(0);
                }
                self.notify_long(&msg);
            }
        }
    }

    /// Remove a queued (not yet started) review task. Returns true when a
    /// matching task was found and removed.
    pub fn cancel_queued_background_task(&mut self, id: &str) -> bool {
        let before = self.pending_background_tasks.len();
        self.pending_background_tasks.retain(|p| p.task.id != id);
        let removed = self.pending_background_tasks.len() != before;
        if removed {
            self.notify("review removed from queue");
        }
        removed
    }

    /// Snapshot of in-flight + recently finished background tasks. Includes
    /// Running tasks and Done/Failed within the last 8 seconds.
    pub fn background_task_snapshots(&self) -> Vec<super::background::BackgroundTaskSnapshot> {
        // Only log when there's actually something interesting to report —
        // otherwise this function fires many times per second from every
        // build_snapshot call and floods stderr.
        let debug_bg = super::background::debug_bg_enabled()
            && (!self.background_tasks.is_empty() || !self.recent_background_tasks.is_empty());
        let cutoff = super::background::unix_now_ms().saturating_sub(8_000);
        let mut out: Vec<super::background::BackgroundTaskSnapshot> = Vec::new();
        if debug_bg {
            eprintln!(
                "[bg] snapshots called map_size={} recent_size={} cutoff={}",
                self.background_tasks.len(),
                self.recent_background_tasks.len(),
                cutoff
            );
            for (id, handle) in self.background_tasks.iter() {
                eprintln!(
                    "[bg]   handle id={} status={:?} finished_at_ms={:?}",
                    id, handle.task.status, handle.task.finished_at_ms
                );
            }
        }
        for handle in self.background_tasks.values() {
            let include = match handle.task.status {
                CommandStatus::Running => true,
                _ => handle
                    .task
                    .finished_at_ms
                    .map(|t| t >= cutoff)
                    .unwrap_or(false),
            };
            if include {
                let mut snap = super::background::BackgroundTaskSnapshot::from_task(&handle.task);
                snap.recent_log = handle
                    .recent_log
                    .iter()
                    .rev()
                    .take(40)
                    .rev()
                    .cloned()
                    .collect();
                out.push(snap);
            }
        }
        for task in &self.recent_background_tasks {
            if task.finished_at_ms.map(|t| t >= cutoff).unwrap_or(false) {
                out.push(super::background::BackgroundTaskSnapshot::from_task(task));
            }
        }
        for pending in &self.pending_background_tasks {
            let mut snap = super::background::BackgroundTaskSnapshot::from_task(&pending.task);
            snap.status = "queued".to_string();
            out.push(snap);
        }
        out.sort_by_key(|t| t.started_at_ms);
        if debug_bg {
            eprintln!(
                "[bg] snapshots returning len={} statuses=[{}]",
                out.len(),
                out.iter()
                    .map(|t| format!("{}={}", t.id, t.status))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        out
    }

    /// Background tasks whose target matches the given tab. Used to merge
    /// app-level task entries into the active tab's `agent_commands`
    /// snapshot list.
    pub fn background_tasks_for_tab(
        &self,
        tab: &TabState,
    ) -> Vec<super::background::BackgroundTaskSnapshot> {
        let cutoff = super::background::unix_now_ms().saturating_sub(8_000);
        let mut out = Vec::new();
        for handle in self.background_tasks.values() {
            if !tab.matches_target(&handle.task.target) {
                continue;
            }
            let include = match handle.task.status {
                CommandStatus::Running => true,
                _ => handle
                    .task
                    .finished_at_ms
                    .map(|t| t >= cutoff)
                    .unwrap_or(false),
            };
            if include {
                out.push(super::background::BackgroundTaskSnapshot::from_task(
                    &handle.task,
                ));
            }
        }
        for pending in &self.pending_background_tasks {
            if !tab.matches_target(&pending.task.target) {
                continue;
            }
            let mut snap = super::background::BackgroundTaskSnapshot::from_task(&pending.task);
            snap.status = "queued".to_string();
            out.push(snap);
        }
        out
    }

    /// Return a tail of log entries for a specific background task by ID.
    /// Returns an empty Vec if the task is not found (may have been reaped).
    pub fn background_task_log_tail(&self, task_id: &str) -> Vec<AgentLogEntry> {
        self.background_tasks
            .get(task_id)
            .map(|h| h.recent_log.iter().cloned().collect())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod background_queue_tests {
    use crate::app::{App, BackgroundTaskTarget};
    use crate::config::{AiModelConfig, AiProviderConfig, AiSelection};

    fn target(tmp: &std::path::Path, branch: &str) -> BackgroundTaskTarget {
        BackgroundTaskTarget {
            repo_root: tmp.to_string_lossy().to_string(),
            er_dir: tmp.join(".er").to_string_lossy().to_string(),
            branch_label: branch.to_string(),
            base_branch: "main".to_string(),
            scope: "branch".to_string(),
            pr_number: None,
            remote_repo: None,
            managed_local: false,
        }
    }

    /// Build an App over a throwaway git repo with a slow no-op agent so
    /// spawned "reviews" run long enough to observe queue state. Returns
    /// None when git isn't available (test then silently skips, matching
    /// the pattern in background.rs).
    fn test_app(cap: usize) -> Option<(App, std::path::PathBuf)> {
        let tmp =
            std::env::temp_dir().join(format!("er-bg-queue-test-{}-{}", std::process::id(), cap));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).ok()?;
        std::process::Command::new("git")
            .arg("init")
            .current_dir(&tmp)
            .output()
            .ok()?;
        let mut app = App::new_with_args(&[tmp.to_string_lossy().to_string()]).ok()?;
        app.config.ai_hub.max_concurrent_reviews = cap;
        app.config.agent.command = "sleep".to_string();
        app.config.agent.args = vec!["5".to_string()];
        Some((app, tmp))
    }

    #[test]
    fn excess_reviews_queue_and_dedup() {
        let Some((mut app, tmp)) = test_app(1) else {
            return;
        };

        app.spawn_background_triage_review(target(&tmp, "feat-a"), "p".into(), true)
            .unwrap();
        app.set_ai_selection_override(AiSelection {
            provider_id: Some("codex".into()),
            model_id: Some("gpt-5.3-codex-spark".into()),
            effort: None,
        });
        app.spawn_background_triage_review(target(&tmp, "feat-b"), "p".into(), true)
            .unwrap();
        app.clear_ai_selection_override();

        assert_eq!(app.running_background_task_count(), 1, "cap of 1 enforced");
        assert_eq!(app.pending_background_tasks.len(), 1, "second task queued");
        assert_eq!(
            app.pending_background_tasks[0]
                .ai_selection
                .as_ref()
                .and_then(|selection| selection.model_id.as_deref()),
            Some("gpt-5.3-codex-spark")
        );

        // Same kind+target as the queued task is rejected as a duplicate.
        let dup = app.spawn_background_triage_review(target(&tmp, "feat-b"), "p".into(), true);
        assert!(dup.is_err(), "duplicate of queued task rejected");

        // Queued task is visible in snapshots as "queued".
        let snaps = app.background_task_snapshots();
        assert!(
            snaps.iter().any(|s| s.status == "queued"),
            "queued status surfaced: {:?}",
            snaps.iter().map(|s| s.status.clone()).collect::<Vec<_>>()
        );

        // Polling while the first task still runs must not launch the queued one.
        app.poll_background_tasks();
        assert_eq!(app.running_background_task_count(), 1);
        assert_eq!(app.pending_background_tasks.len(), 1);

        // Queued tasks can be cancelled by id.
        let queued_id = app.pending_background_tasks[0].task.id.clone();
        assert!(app.cancel_queued_background_task(&queued_id));
        assert!(app.pending_background_tasks.is_empty());
        assert!(!app.cancel_queued_background_task(&queued_id));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn queued_task_snapshots_current_selection_without_override() {
        let Some((mut app, tmp)) = test_app(1) else {
            return;
        };
        app.config.ai_hub.providers.insert(
            "codex".into(),
            AiProviderConfig {
                models: vec![AiModelConfig {
                    id: "gpt-5.6-luna".into(),
                    ..Default::default()
                }],
                ..Default::default()
            },
        );
        app.current_ai_provider = Some("codex".into());
        app.current_ai_model = Some("gpt-5.6-luna".into());
        app.current_ai_effort = Some("high".into());

        app.spawn_background_triage_review(target(&tmp, "feat-a"), "p".into(), true)
            .unwrap();
        app.current_ai_model = Some("other".into());
        app.spawn_background_triage_review(target(&tmp, "feat-b"), "p".into(), true)
            .unwrap();

        let queued = &app.pending_background_tasks[0];
        assert_eq!(
            queued
                .ai_selection
                .as_ref()
                .and_then(|s| s.model_id.as_deref()),
            Some("gpt-5.6-luna"),
            "queued task keeps the selection from enqueue time"
        );
        assert_eq!(
            queued
                .ai_selection
                .as_ref()
                .and_then(|s| s.effort.as_deref()),
            Some("high")
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn dispatch_launches_queued_when_slot_frees() {
        let Some((mut app, tmp)) = test_app(2) else {
            return;
        };
        // Fast-exiting agent: completion frees a slot on the next poll.
        app.config.agent.args = vec!["0".to_string()];

        for branch in ["a", "b", "c"] {
            app.spawn_background_triage_review(target(&tmp, branch), "p".into(), true)
                .unwrap();
        }
        assert_eq!(app.running_background_task_count(), 2);
        assert_eq!(app.pending_background_tasks.len(), 1);

        // Wait for the fast agents to exit, then poll: completions are
        // detected and the queued task is dispatched.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            app.poll_background_tasks();
            if app.pending_background_tasks.is_empty() {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "queued task was never dispatched"
            );
            std::thread::sleep(std::time::Duration::from_millis(50));
        }

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    #[cfg(unix)]
    fn background_codex_review_ignores_user_config() {
        use std::os::unix::fs::PermissionsExt;

        let Some((mut app, tmp)) = test_app(1) else {
            return;
        };
        let fake_bin = tmp.join("codex");
        std::fs::write(&fake_bin, "#!/bin/sh\nexit 0\n").unwrap();
        let mut permissions = std::fs::metadata(&fake_bin).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&fake_bin, permissions).unwrap();

        app.config.ai_hub.providers.clear();
        app.config.ai_hub.default_provider = Some("codex".to_string());
        app.config.ai_hub.default_model = Some("gpt-5.5".to_string());
        app.config.ai_hub.providers.insert(
            "codex".to_string(),
            AiProviderConfig {
                command: fake_bin.to_string_lossy().to_string(),
                args: vec!["exec".to_string(), "{prompt}".to_string()],
                models: vec![AiModelConfig {
                    id: "gpt-5.5".to_string(),
                    args: vec!["--model".to_string(), "gpt-5.5".to_string()],
                    ..Default::default()
                }],
                ..Default::default()
            },
        );
        app.current_ai_provider = Some("codex".to_string());
        app.current_ai_model = Some("gpt-5.5".to_string());

        app.spawn_background_triage_review(target(&tmp, "feat-codex"), "p".into(), true)
            .unwrap();

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            app.poll_background_tasks();
            let done = app
                .background_tasks
                .values()
                .any(|handle| matches!(handle.task.status, crate::app::CommandStatus::Done));
            if done {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "fake Codex task did not finish"
            );
            std::thread::sleep(std::time::Duration::from_millis(25));
        }

        let debug_log = std::fs::read_to_string(tmp.join(".er/debug-agent.log")).unwrap();
        assert!(
            debug_log.contains("codex exec --ignore-user-config"),
            "debug log should show isolated Codex invocation:\n{debug_log}"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn agent_command_artifact_reload_classification() {
        for name in [
            "summary",
            "review",
            "questions",
            "triage",
            "professor",
            "expert-security",
            "validate",
            "validate-comments",
        ] {
            assert!(
                App::agent_command_writes_ai_artifacts(name),
                "{name} should trigger AI sidecar reload"
            );
        }

        for name in ["test", "lint", "fmt", "build"] {
            assert!(
                !App::agent_command_writes_ai_artifacts(name),
                "{name} should not trigger AI sidecar reload"
            );
        }
    }
}

#[cfg(test)]
mod comment_state_tests {
    use super::super::background::{
        unix_now_ms, BackgroundTask, BackgroundTaskHandle, BackgroundTaskTarget,
        PendingBackgroundTask,
    };
    use crate::ai;
    use crate::app::{AgentLogEntry, AgentLogSource, App, CommandStatus, InputMode, TabState};
    use crate::git::{DiffFile, DiffHunk, DiffLine, FileStatus, LineType};
    use crate::paths::ErRoot;
    use anyhow::Result;
    use std::collections::HashMap;
    use tui_textarea::TextArea;

    // ── Fixtures ───────────────────────────────────────────────────────────
    // Mirrors of the private helpers in `state/mod.rs` and `ai/review.rs`
    // (both live in `#[cfg(test)] mod tests` and are not reachable from here).

    fn make_line(line_type: LineType, content: &str, new_num: Option<usize>) -> DiffLine {
        DiffLine {
            line_type,
            content: content.to_string(),
            old_num: None,
            new_num,
        }
    }

    /// Hunk of plain added lines numbered `new_nums` on the new side.
    fn make_hunk(new_nums: &[usize]) -> DiffHunk {
        DiffHunk {
            header: "@@ -1,3 +1,4 @@".to_string(),
            old_start: 1,
            old_count: 3,
            new_start: 1,
            new_count: 4,
            lines: new_nums
                .iter()
                .map(|n| make_line(LineType::Add, &format!("line {n}"), Some(*n)))
                .collect(),
        }
    }

    fn make_file(path: &str, hunks: Vec<DiffHunk>) -> DiffFile {
        DiffFile {
            path: path.to_string(),
            status: FileStatus::Modified,
            hunks,
            adds: 1,
            dels: 0,
            compacted: false,
            raw_hunk_count: 0,
        }
    }

    /// One-hunk file whose lines are numbered `new_nums`.
    fn simple_file(path: &str, new_nums: &[usize]) -> DiffFile {
        make_file(path, vec![make_hunk(new_nums)])
    }

    fn question(
        id: &str,
        file: &str,
        hunk_index: Option<usize>,
        line_start: Option<usize>,
    ) -> ai::ReviewQuestion {
        ai::ReviewQuestion {
            id: id.to_string(),
            timestamp: String::new(),
            file: file.to_string(),
            hunk_index,
            line_start,
            line_end: None,
            line_content: String::new(),
            text: format!("text for {id}"),
            resolved: false,
            stale: false,
            context_before: vec![],
            context_after: vec![],
            old_line_start: None,
            side: "RIGHT".to_string(),
            hunk_header: String::new(),
            anchor_status: "original".to_string(),
            relocated_at_hash: String::new(),
            in_reply_to: None,
            author: "You".to_string(),
            promoted_to: None,
            finding_ref: None,
        }
    }

    fn reply_question(id: &str, file: &str, parent: &str) -> ai::ReviewQuestion {
        let mut q = question(id, file, Some(0), Some(1));
        q.in_reply_to = Some(parent.to_string());
        q
    }

    fn gh_comment(
        id: &str,
        file: &str,
        hunk_index: Option<usize>,
        line_start: Option<usize>,
    ) -> ai::GitHubReviewComment {
        ai::GitHubReviewComment {
            id: id.to_string(),
            timestamp: String::new(),
            file: file.to_string(),
            hunk_index,
            line_start,
            line_end: None,
            line_content: String::new(),
            comment: format!("comment for {id}"),
            in_reply_to: None,
            resolved: false,
            source: "local".to_string(),
            github_id: None,
            author: "You".to_string(),
            synced: false,
            outdated: false,
            stale: false,
            context_before: vec![],
            context_after: vec![],
            old_line_start: None,
            hunk_header: String::new(),
            anchor_status: "original".to_string(),
            relocated_at_hash: String::new(),
            finding_ref: None,
            side: "RIGHT".to_string(),
        }
    }

    fn finding_full(
        id: &str,
        hunk_index: Option<usize>,
        line_start: Option<usize>,
        severity: ai::RiskLevel,
        confidence: ai::Confidence,
    ) -> ai::Finding {
        ai::Finding {
            id: id.to_string(),
            severity,
            category: String::new(),
            title: format!("Finding {id}"),
            description: String::new(),
            hunk_index,
            line_start,
            line_end: None,
            suggestion: String::new(),
            related_files: Vec::new(),
            outside_diff: false,
            confidence,
            verification_plan: String::new(),
            evidence: Vec::new(),
            responses: Vec::new(),
            resolved: false,
            resolved_note: String::new(),
            resolved_at: String::new(),
            promoted_to: None,
        }
    }

    fn finding(id: &str, hunk_index: Option<usize>, line_start: Option<usize>) -> ai::Finding {
        finding_full(
            id,
            hunk_index,
            line_start,
            ai::RiskLevel::Medium,
            ai::Confidence::Tentative,
        )
    }

    fn review_with(files: Vec<(&str, ai::RiskLevel, Vec<ai::Finding>)>) -> ai::ErReview {
        let mut map = HashMap::new();
        for (path, risk, findings) in files {
            map.insert(
                path.to_string(),
                ai::ErFileReview {
                    risk,
                    risk_reason: String::new(),
                    summary: String::new(),
                    findings,
                },
            );
        }
        ai::ErReview {
            version: 1,
            diff_hash: "fixture-hash".to_string(),
            created_at: String::new(),
            base_branch: "main".to_string(),
            head_branch: "feature".to_string(),
            files: map,
            file_hashes: HashMap::new(),
        }
    }

    fn questions_doc(questions: Vec<ai::ReviewQuestion>) -> ai::ErQuestions {
        ai::ErQuestions {
            version: 1,
            diff_hash: "fixture-hash".to_string(),
            questions,
        }
    }

    fn notes_doc(notes: Vec<ai::ReviewQuestion>) -> ai::ErNotes {
        ai::ErNotes {
            version: 1,
            diff_hash: "fixture-hash".to_string(),
            notes,
        }
    }

    fn gh_doc(comments: Vec<ai::GitHubReviewComment>) -> ai::ErGitHubComments {
        ai::ErGitHubComments {
            version: 1,
            diff_hash: "fixture-hash".to_string(),
            github: None,
            comments,
        }
    }

    /// A `TabState` whose sidecar directory is a throwaway TempDir. Never the
    /// shared `/tmp/test/.er` that `TabState::new_for_test` defaults to —
    /// that path is global and would cross-contaminate the whole binary.
    fn tab_in_tempdir(files: Vec<DiffFile>) -> (TabState, tempfile::TempDir) {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path().to_string_lossy().to_string();
        let mut tab = TabState::new_for_test(files);
        tab.repo_root = root.clone();
        tab.er_root = ErRoot::RepoLocal(root);
        tab.diff_hash = "fixture-hash".to_string();
        tab.branch_diff_hash = "fixture-hash".to_string();
        std::fs::create_dir_all(tab.er_dir()).unwrap();
        (tab, tmp)
    }

    fn app_in_tempdir(files: Vec<DiffFile>) -> (App, tempfile::TempDir) {
        let (tab, tmp) = tab_in_tempdir(files);
        let mut app = App::new_for_test(vec![]);
        app.tabs = vec![tab];
        (app, tmp)
    }

    fn write_sidecar(er_dir: &str, name: &str, contents: &str) {
        std::fs::write(std::path::Path::new(er_dir).join(name), contents).unwrap();
    }

    fn read_sidecar(er_dir: &str, name: &str) -> String {
        std::fs::read_to_string(std::path::Path::new(er_dir).join(name)).unwrap()
    }

    fn log_entry(name: &str, text: &str) -> AgentLogEntry {
        AgentLogEntry {
            timestamp: std::time::Instant::now(),
            command_name: name.to_string(),
            source: AgentLogSource::Stdout,
            text: text.to_string(),
        }
    }

    // ── App::agent_completion_summary_for ───────────────────────────────────

    #[test]
    fn agent_summary_for_review_counts_files_and_findings() {
        let (tab, _tmp) = tab_in_tempdir(vec![]);
        let review = review_with(vec![
            (
                "a.rs",
                ai::RiskLevel::High,
                vec![
                    finding("f-1", Some(0), Some(1)),
                    finding("f-2", Some(0), None),
                ],
            ),
            (
                "b.rs",
                ai::RiskLevel::Low,
                vec![finding("f-3", Some(0), None)],
            ),
        ]);
        write_sidecar(
            &tab.er_dir(),
            "review.json",
            &serde_json::to_string(&review).unwrap(),
        );

        assert_eq!(
            App::agent_completion_summary_for(&tab, "review"),
            "Review done — 2 files, 3 findings"
        );
    }

    #[test]
    fn agent_summary_for_review_uses_singular_wording_for_one_file_and_finding() {
        let (tab, _tmp) = tab_in_tempdir(vec![]);
        let review = review_with(vec![(
            "a.rs",
            ai::RiskLevel::High,
            vec![finding("f-1", Some(0), Some(1))],
        )]);
        write_sidecar(
            &tab.er_dir(),
            "review.json",
            &serde_json::to_string(&review).unwrap(),
        );

        assert_eq!(
            App::agent_completion_summary_for(&tab, "review"),
            "Review done — 1 file, 1 finding"
        );
    }

    #[test]
    fn agent_summary_for_review_without_a_sidecar_blames_missing_permissions() {
        let (tab, _tmp) = tab_in_tempdir(vec![]);
        assert_eq!(
            App::agent_completion_summary_for(&tab, "review"),
            "Review done — but no review.json found (agent may lack permissions)"
        );
    }

    #[test]
    fn agent_summary_for_review_reports_an_unparseable_sidecar() {
        let (tab, _tmp) = tab_in_tempdir(vec![]);
        write_sidecar(&tab.er_dir(), "review.json", "{not json");

        assert_eq!(
            App::agent_completion_summary_for(&tab, "review"),
            "Review done — review.json written but could not be parsed"
        );
    }

    #[test]
    fn agent_summary_for_questions_counts_replies_against_top_level_questions() {
        let (tab, _tmp) = tab_in_tempdir(vec![]);
        let doc = questions_doc(vec![
            question("q-1", "a.rs", Some(0), Some(1)),
            question("q-2", "a.rs", Some(0), Some(2)),
            reply_question("q-3", "a.rs", "q-1"),
        ]);
        write_sidecar(
            &tab.er_dir(),
            "questions.json",
            &serde_json::to_string(&doc).unwrap(),
        );

        assert_eq!(
            App::agent_completion_summary_for(&tab, "questions"),
            "Questions done — 1 of 2 answered"
        );
    }

    #[test]
    fn agent_summary_for_questions_without_a_sidecar_reports_the_missing_file() {
        let (tab, _tmp) = tab_in_tempdir(vec![]);
        assert_eq!(
            App::agent_completion_summary_for(&tab, "questions"),
            "Questions done — but no questions.json found"
        );
    }

    #[test]
    fn agent_summary_for_questions_reports_an_unparseable_sidecar() {
        let (tab, _tmp) = tab_in_tempdir(vec![]);
        write_sidecar(&tab.er_dir(), "questions.json", "[[[");

        assert_eq!(
            App::agent_completion_summary_for(&tab, "questions"),
            "Questions done — questions.json written but could not be parsed"
        );
    }

    #[test]
    fn agent_summary_for_an_unrecognised_command_falls_back_to_name_done() {
        let (tab, _tmp) = tab_in_tempdir(vec![]);
        assert_eq!(App::agent_completion_summary_for(&tab, "lint"), "lint done");
    }

    // ── App::check_commands ────────────────────────────────────────────────

    /// Queue a finished command result on a tab without running a subprocess.
    fn stage_command_result(tab: &mut TabState, name: &str, result: Result<()>) {
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(result).unwrap();
        tab.command_rx.insert(name.to_string(), rx);
        tab.command_status
            .insert(name.to_string(), CommandStatus::Running);
    }

    #[test]
    fn check_commands_marks_a_finished_command_done_and_logs_completion() {
        let (mut app, _tmp) = app_in_tempdir(vec![]);
        stage_command_result(app.tab_mut(), "lint", Ok(()));

        app.check_commands();

        assert_eq!(
            app.tab().command_status.get("lint"),
            Some(&CommandStatus::Done)
        );
        assert!(
            !app.tab().command_rx.contains_key("lint"),
            "the finished receiver is dropped so it is not polled again"
        );
        assert_eq!(app.watch_message.as_deref(), Some("lint done"));

        app.drain_agent_log();
        assert!(
            app.tab()
                .agent_log
                .iter()
                .any(|e| e.text == "lint completed"),
            "completion is announced on the agent log"
        );
    }

    #[test]
    fn check_commands_reloads_ai_sidecars_for_artifact_writing_commands() {
        let (mut app, _tmp) = app_in_tempdir(vec![]);
        let review = review_with(vec![(
            "a.rs",
            ai::RiskLevel::High,
            vec![finding("f-1", Some(0), Some(1))],
        )]);
        write_sidecar(
            &app.tab().er_dir(),
            "review.json",
            &serde_json::to_string(&review).unwrap(),
        );
        assert!(app.tab().ai.review.is_none(), "not loaded before the poll");

        stage_command_result(app.tab_mut(), "review", Ok(()));
        app.check_commands();

        assert!(
            app.tab().ai.review.is_some(),
            "an artifact-writing command forces a sidecar reload"
        );
        assert_eq!(
            app.watch_message.as_deref(),
            Some("Review done — 1 file, 1 finding")
        );
    }

    #[test]
    fn check_commands_does_not_reload_sidecars_for_non_artifact_commands() {
        let (mut app, _tmp) = app_in_tempdir(vec![]);
        let review = review_with(vec![(
            "a.rs",
            ai::RiskLevel::High,
            vec![finding("f-1", Some(0), Some(1))],
        )]);
        write_sidecar(
            &app.tab().er_dir(),
            "review.json",
            &serde_json::to_string(&review).unwrap(),
        );

        stage_command_result(app.tab_mut(), "test", Ok(()));
        app.check_commands();

        assert!(
            app.tab().ai.review.is_none(),
            "`test` writes no sidecars, so the reload must not fire"
        );
        assert_eq!(app.watch_message.as_deref(), Some("test done"));
    }

    #[test]
    fn check_commands_records_a_failure_message_as_the_command_status() {
        let (mut app, _tmp) = app_in_tempdir(vec![]);
        stage_command_result(app.tab_mut(), "lint", Err(anyhow::anyhow!("boom")));

        app.check_commands();

        assert_eq!(
            app.tab().command_status.get("lint"),
            Some(&CommandStatus::Failed("boom".to_string()))
        );
        assert_eq!(app.watch_message.as_deref(), Some("lint failed: boom"));

        app.drain_agent_log();
        assert!(
            app.tab()
                .agent_log
                .iter()
                .any(|e| e.text == "lint failed: boom"),
            "failure is announced on the agent log"
        );
    }

    #[test]
    fn check_commands_truncates_a_long_failure_on_a_char_boundary() {
        // 100 two-byte chars: `msg.len()` is 200 bytes, so byte slicing at 80
        // would split a char and panic. The status bar text must cut at the
        // 80th *character*.
        let long = "é".repeat(100);
        let (mut app, _tmp) = app_in_tempdir(vec![]);
        stage_command_result(app.tab_mut(), "lint", Err(anyhow::anyhow!("{long}")));

        app.check_commands();

        let expected = format!("lint failed: {}…", "é".repeat(80));
        assert_eq!(app.watch_message.as_deref(), Some(expected.as_str()));
        assert_eq!(
            app.tab().command_status.get("lint"),
            Some(&CommandStatus::Failed(long)),
            "the stored status keeps the untruncated message"
        );
    }

    #[test]
    fn check_commands_treats_a_dropped_sender_as_a_crashed_thread() {
        let (mut app, _tmp) = app_in_tempdir(vec![]);
        let (tx, rx) = std::sync::mpsc::channel::<Result<()>>();
        drop(tx);
        app.tab_mut().command_rx.insert("review".to_string(), rx);
        app.tab_mut()
            .command_status
            .insert("review".to_string(), CommandStatus::Running);

        app.check_commands();

        assert_eq!(
            app.tab().command_status.get("review"),
            Some(&CommandStatus::Failed("review thread crashed".to_string()))
        );
        assert_eq!(
            app.watch_message.as_deref(),
            Some("review failed: review thread crashed")
        );
    }

    #[test]
    fn check_commands_leaves_a_still_running_command_alone() {
        let (mut app, _tmp) = app_in_tempdir(vec![]);
        let (tx, rx) = std::sync::mpsc::channel::<Result<()>>();
        app.tab_mut().command_rx.insert("lint".to_string(), rx);
        app.tab_mut()
            .command_status
            .insert("lint".to_string(), CommandStatus::Running);

        app.check_commands();

        assert_eq!(
            app.tab().command_status.get("lint"),
            Some(&CommandStatus::Running)
        );
        assert!(
            app.tab().command_rx.contains_key("lint"),
            "an unfinished command keeps its receiver for the next tick"
        );
        assert!(app.watch_message.is_none());
        drop(tx);
    }

    // ── App::drain_agent_log ───────────────────────────────────────────────

    #[test]
    fn drain_agent_log_moves_pending_entries_into_the_tab_buffer_in_order() {
        let (mut app, _tmp) = app_in_tempdir(vec![]);
        for text in ["first", "second", "third"] {
            app.tab().log_tx.send(log_entry("review", text)).unwrap();
        }

        app.drain_agent_log();

        let texts: Vec<&str> = app
            .tab()
            .agent_log
            .iter()
            .map(|e| e.text.as_str())
            .collect();
        assert_eq!(texts, vec!["first", "second", "third"]);
    }

    #[test]
    fn drain_agent_log_autoscrolls_the_open_agent_log_panel() {
        let (mut app, _tmp) = app_in_tempdir(vec![]);
        app.tab_mut().panel = Some(ai::PanelContent::AgentLog);
        app.tab_mut().agent_log_auto_scroll = true;
        for text in ["a", "b", "c"] {
            app.tab().log_tx.send(log_entry("review", text)).unwrap();
        }

        app.drain_agent_log();

        assert_eq!(
            app.tab().panel_scroll,
            2,
            "scroll parks on the last of the three entries"
        );
    }

    #[test]
    fn drain_agent_log_leaves_scroll_alone_when_autoscroll_is_off() {
        let (mut app, _tmp) = app_in_tempdir(vec![]);
        app.tab_mut().panel = Some(ai::PanelContent::AgentLog);
        app.tab_mut().agent_log_auto_scroll = false;
        app.tab_mut().panel_scroll = 7;
        app.tab().log_tx.send(log_entry("review", "a")).unwrap();

        app.drain_agent_log();

        assert_eq!(app.tab().agent_log.len(), 1);
        assert_eq!(app.tab().panel_scroll, 7);
    }

    #[test]
    fn drain_agent_log_leaves_scroll_alone_for_a_non_log_panel() {
        let (mut app, _tmp) = app_in_tempdir(vec![]);
        app.tab_mut().panel = Some(ai::PanelContent::FileDetail);
        app.tab_mut().agent_log_auto_scroll = true;
        app.tab_mut().panel_scroll = 3;
        app.tab().log_tx.send(log_entry("review", "a")).unwrap();

        app.drain_agent_log();

        assert_eq!(app.tab().agent_log.len(), 1);
        assert_eq!(
            app.tab().panel_scroll,
            3,
            "only the AgentLog panel follows the tail"
        );
    }

    #[test]
    fn drain_agent_log_caps_the_buffer_at_5000_entries() {
        let (mut app, _tmp) = app_in_tempdir(vec![]);
        for i in 0..5002 {
            app.tab()
                .log_tx
                .send(log_entry("review", &format!("e{i}")))
                .unwrap();
        }

        app.drain_agent_log();

        assert_eq!(app.tab().agent_log.len(), 5000);
        assert_eq!(
            app.tab().agent_log.front().map(|e| e.text.as_str()),
            Some("e2"),
            "the two oldest entries are evicted from the front"
        );
        assert_eq!(
            app.tab().agent_log.back().map(|e| e.text.as_str()),
            Some("e5001")
        );
    }

    #[test]
    fn drain_agent_log_drains_background_tabs_without_touching_their_scroll() {
        let (mut app, _tmp) = app_in_tempdir(vec![]);
        let mut second = TabState::new_for_test(vec![]);
        second.panel = Some(ai::PanelContent::AgentLog);
        second.agent_log_auto_scroll = true;
        second.panel_scroll = 42;
        second.log_tx.send(log_entry("review", "bg")).unwrap();
        app.tabs.push(second);

        app.drain_agent_log();

        assert_eq!(app.tabs[1].agent_log.len(), 1, "inactive tabs still drain");
        assert_eq!(
            app.tabs[1].panel_scroll, 42,
            "only the active tab autoscrolls"
        );
    }

    // ── App::start_reply_comment ───────────────────────────────────────────

    #[test]
    fn start_reply_comment_anchors_the_draft_to_the_parent_question() {
        let mut app = App::new_for_test(vec![simple_file("a.rs", &[1, 2, 3])]);
        app.tab_mut().ai.questions = Some(questions_doc(vec![question(
            "q-1",
            "a.rs",
            Some(2),
            Some(42),
        )]));

        app.start_reply_comment("q-1");

        assert_eq!(app.tab().comment_file, "a.rs");
        assert_eq!(app.tab().comment_hunk, 2);
        assert_eq!(app.tab().comment_line_num, Some(42));
        assert_eq!(app.tab().comment_reply_to.as_deref(), Some("q-1"));
        assert_eq!(app.tab().comment_type, ai::CommentType::Question);
        assert!(app.tab().comment_finding_ref.is_none());
        assert_eq!(app.input_mode, InputMode::Comment);
    }

    #[test]
    fn start_reply_comment_on_a_note_keeps_the_note_draft_type() {
        let mut app = App::new_for_test(vec![simple_file("a.rs", &[1, 2, 3])]);
        app.tab_mut().ai.notes = Some(notes_doc(vec![question("n-1", "a.rs", Some(1), Some(7))]));

        app.start_reply_comment("n-1");

        assert_eq!(app.tab().comment_type, ai::CommentType::Note);
        assert_eq!(app.tab().comment_hunk, 1);
        assert_eq!(app.tab().comment_line_num, Some(7));
        assert_eq!(app.tab().comment_reply_to.as_deref(), Some("n-1"));
    }

    #[test]
    fn start_reply_comment_on_a_github_comment_keeps_the_comment_draft_type() {
        let mut app = App::new_for_test(vec![simple_file("a.rs", &[1, 2, 3])]);
        app.tab_mut().ai.github_comments =
            Some(gh_doc(vec![gh_comment("c-1", "a.rs", None, Some(9))]));

        app.start_reply_comment("c-1");

        assert_eq!(app.tab().comment_type, ai::CommentType::GitHubComment);
        assert_eq!(
            app.tab().comment_hunk,
            0,
            "a hunk-less parent falls back to hunk 0"
        );
        assert_eq!(app.tab().comment_line_num, Some(9));
        assert_eq!(app.input_mode, InputMode::Comment);
    }

    #[test]
    fn start_reply_comment_with_an_unknown_id_does_not_open_the_editor() {
        let mut app = App::new_for_test(vec![simple_file("a.rs", &[1, 2, 3])]);
        app.tab_mut().ai.questions = Some(questions_doc(vec![question(
            "q-1",
            "a.rs",
            Some(0),
            Some(1),
        )]));

        app.start_reply_comment("q-missing");

        assert_eq!(app.input_mode, InputMode::Normal);
        assert!(app.tab().comment_reply_to.is_none());
    }

    #[test]
    fn start_reply_comment_without_any_loaded_sidecar_does_nothing() {
        let mut app = App::new_for_test(vec![simple_file("a.rs", &[1, 2, 3])]);

        app.start_reply_comment("q-1");
        app.start_reply_comment("n-1");
        app.start_reply_comment("c-1");

        assert_eq!(app.input_mode, InputMode::Normal);
        assert!(app.tab().comment_reply_to.is_none());
    }

    // ── App::start_reply_finding ───────────────────────────────────────────

    #[test]
    fn start_reply_finding_opens_a_github_comment_bound_to_the_finding() {
        let mut app = App::new_for_test(vec![
            simple_file("a.rs", &[1, 2, 3]),
            simple_file("b.rs", &[10, 11]),
        ]);
        app.tab_mut().ai.review = Some(review_with(vec![(
            "b.rs",
            ai::RiskLevel::High,
            vec![finding("f-1", Some(3), Some(11))],
        )]));

        app.start_reply_finding("f-1");

        assert_eq!(app.tab().comment_file, "b.rs");
        assert_eq!(app.tab().comment_hunk, 3);
        assert_eq!(app.tab().comment_line_num, Some(11));
        assert_eq!(app.tab().comment_finding_ref.as_deref(), Some("f-1"));
        assert_eq!(app.tab().comment_type, ai::CommentType::GitHubComment);
        assert!(
            app.tab().comment_reply_to.is_none(),
            "a finding reply is a new top-level comment, not a thread reply"
        );
        assert_eq!(app.input_mode, InputMode::Comment);
    }

    #[test]
    fn start_reply_finding_without_a_review_explains_why_it_cannot_reply() {
        let mut app = App::new_for_test(vec![simple_file("a.rs", &[1, 2, 3])]);

        app.start_reply_finding("f-1");

        assert_eq!(
            app.watch_message.as_deref(),
            Some("No AI review loaded — cannot reply to finding")
        );
        assert_eq!(app.input_mode, InputMode::Normal);
    }

    #[test]
    fn start_reply_finding_with_an_unknown_id_warns_about_a_stale_review() {
        let mut app = App::new_for_test(vec![simple_file("a.rs", &[1, 2, 3])]);
        app.tab_mut().ai.review = Some(review_with(vec![(
            "a.rs",
            ai::RiskLevel::Low,
            vec![finding("f-1", Some(0), Some(1))],
        )]));

        app.start_reply_finding("f-gone");

        assert_eq!(
            app.watch_message.as_deref(),
            Some("Finding not found — review may be stale")
        );
        assert_eq!(app.input_mode, InputMode::Normal);
        assert!(app.tab().comment_finding_ref.is_none());
    }

    // ── App::submit_question (via submit_comment) ──────────────────────────

    /// Stage a question draft anchored at `a.rs` line 2.
    fn stage_question_draft(app: &mut App, text: &str) {
        app.tab_mut().comment_textarea = TextArea::new(vec![text.to_string()]);
        app.tab_mut().comment_file = "a.rs".to_string();
        app.tab_mut().comment_hunk = 0;
        app.tab_mut().comment_line_num = Some(2);
        app.tab_mut().comment_type = ai::CommentType::Question;
        app.input_mode = InputMode::Comment;
    }

    fn load_questions(er_dir: &str) -> ai::ErQuestions {
        serde_json::from_str(&read_sidecar(er_dir, "questions.json")).unwrap()
    }

    #[test]
    fn submit_comment_writes_a_new_question_anchored_to_the_current_line() {
        let (mut app, _tmp) = app_in_tempdir(vec![simple_file("a.rs", &[1, 2, 3])]);
        stage_question_draft(&mut app, "why this branch?");

        app.submit_comment().unwrap();

        let er_dir = app.tab().er_dir();
        let doc = load_questions(&er_dir);
        assert_eq!(doc.questions.len(), 1);
        let q = &doc.questions[0];
        assert_eq!(q.text, "why this branch?");
        assert!(
            q.id.starts_with("q-"),
            "minted id is question-scoped: {}",
            q.id
        );
        assert_eq!(q.line_start, Some(2));
        assert_eq!(q.line_content, "line 2");
        assert_eq!(q.hunk_index, Some(0));
        assert_eq!(q.author, "You");
        assert_eq!(q.side, "RIGHT");
        assert!(q.in_reply_to.is_none());
        assert_eq!(app.input_mode, InputMode::Normal);
        assert_eq!(
            app.watch_message.as_deref(),
            Some("Question added: why this branch?")
        );
    }

    #[test]
    fn submit_comment_labels_a_threaded_question_as_a_reply() {
        let (mut app, _tmp) = app_in_tempdir(vec![simple_file("a.rs", &[1, 2, 3])]);
        stage_question_draft(&mut app, "answering");
        app.tab_mut().comment_reply_to = Some("q-parent".to_string());

        app.submit_comment().unwrap();

        let doc = load_questions(&app.tab().er_dir());
        assert_eq!(doc.questions[0].in_reply_to.as_deref(), Some("q-parent"));
        assert_eq!(app.watch_message.as_deref(), Some("Reply added: answering"));
    }

    #[test]
    fn submit_comment_honours_a_question_scoped_id_override() {
        let (mut app, _tmp) = app_in_tempdir(vec![simple_file("a.rs", &[1, 2, 3])]);
        stage_question_draft(&mut app, "pinned id");
        app.tab_mut().comment_id_override = Some("q-fixed-id".to_string());

        app.submit_comment().unwrap();

        let doc = load_questions(&app.tab().er_dir());
        assert_eq!(doc.questions[0].id, "q-fixed-id");
        assert!(
            app.tab().comment_id_override.is_none(),
            "the override is consumed, not reused by the next draft"
        );
    }

    #[test]
    fn submit_comment_ignores_an_id_override_with_the_wrong_prefix() {
        let (mut app, _tmp) = app_in_tempdir(vec![simple_file("a.rs", &[1, 2, 3])]);
        stage_question_draft(&mut app, "wrong prefix");
        app.tab_mut().comment_id_override = Some("c-not-a-question".to_string());

        app.submit_comment().unwrap();

        let doc = load_questions(&app.tab().er_dir());
        assert_ne!(doc.questions[0].id, "c-not-a-question");
        assert!(doc.questions[0].id.starts_with("q-"));
    }

    #[test]
    fn submit_comment_starts_a_fresh_questions_file_when_the_sidecar_is_corrupt() {
        let (mut app, _tmp) = app_in_tempdir(vec![simple_file("a.rs", &[1, 2, 3])]);
        write_sidecar(&app.tab().er_dir(), "questions.json", "}}not json{{");
        stage_question_draft(&mut app, "after corruption");

        app.submit_comment().unwrap();

        let doc = load_questions(&app.tab().er_dir());
        assert_eq!(
            doc.questions.len(),
            1,
            "the unreadable file is replaced, not appended to"
        );
        assert_eq!(doc.questions[0].text, "after corruption");
    }

    #[test]
    fn submit_comment_appends_to_an_existing_questions_file() {
        let (mut app, _tmp) = app_in_tempdir(vec![simple_file("a.rs", &[1, 2, 3])]);
        let er_dir = app.tab().er_dir();
        write_sidecar(
            &er_dir,
            "questions.json",
            &serde_json::to_string(&questions_doc(vec![question(
                "q-old",
                "a.rs",
                Some(0),
                Some(1),
            )]))
            .unwrap(),
        );
        stage_question_draft(&mut app, "second one");

        app.submit_comment().unwrap();

        let doc = load_questions(&er_dir);
        assert_eq!(doc.questions.len(), 2);
        assert_eq!(doc.questions[0].id, "q-old");
        assert_eq!(doc.questions[1].text, "second one");
    }

    #[test]
    fn submit_comment_with_an_empty_draft_closes_the_editor_without_writing() {
        let (mut app, _tmp) = app_in_tempdir(vec![simple_file("a.rs", &[1, 2, 3])]);
        stage_question_draft(&mut app, "   ");
        app.tab_mut().comment_id_override = Some("q-fixed-id".to_string());

        app.submit_comment().unwrap();

        assert_eq!(app.input_mode, InputMode::Normal);
        assert!(
            !std::path::Path::new(&app.tab().er_dir())
                .join("questions.json")
                .exists(),
            "a whitespace-only draft must not create a sidecar"
        );
        assert!(app.tab().comment_id_override.is_none());
    }

    // ── App::update_comment_text ───────────────────────────────────────────

    #[test]
    fn update_comment_text_rewrites_a_question_body_on_disk() {
        let (mut app, _tmp) = app_in_tempdir(vec![simple_file("a.rs", &[1, 2, 3])]);
        let er_dir = app.tab().er_dir();
        write_sidecar(
            &er_dir,
            "questions.json",
            &serde_json::to_string(&questions_doc(vec![
                question("q-1", "a.rs", Some(0), Some(1)),
                question("q-2", "a.rs", Some(0), Some(2)),
            ]))
            .unwrap(),
        );

        app.update_comment_text("q-1", "rewritten").unwrap();

        let doc = load_questions(&er_dir);
        assert_eq!(doc.questions[0].text, "rewritten");
        assert_eq!(
            doc.questions[1].text, "text for q-2",
            "sibling questions are untouched"
        );
    }

    #[test]
    fn update_comment_text_refuses_to_edit_ai_authored_question_text() {
        let (mut app, _tmp) = app_in_tempdir(vec![simple_file("a.rs", &[1, 2, 3])]);
        let er_dir = app.tab().er_dir();
        let mut q = question("q-1", "a.rs", Some(0), Some(1));
        q.author = "ai".to_string();
        write_sidecar(
            &er_dir,
            "questions.json",
            &serde_json::to_string(&questions_doc(vec![q])).unwrap(),
        );

        let err = app.update_comment_text("q-1", "hijacked").unwrap_err();

        assert!(
            format!("{err:#}").contains("Cannot edit AI-generated text"),
            "unexpected error: {err:#}"
        );
        assert_eq!(
            load_questions(&er_dir).questions[0].text,
            "text for q-1",
            "the refused edit leaves the sidecar untouched"
        );
    }

    #[test]
    fn update_comment_text_errors_when_the_question_id_is_unknown() {
        let (mut app, _tmp) = app_in_tempdir(vec![simple_file("a.rs", &[1, 2, 3])]);
        write_sidecar(
            &app.tab().er_dir(),
            "questions.json",
            &serde_json::to_string(&questions_doc(vec![question(
                "q-1",
                "a.rs",
                Some(0),
                Some(1),
            )]))
            .unwrap(),
        );

        let err = app.update_comment_text("q-gone", "nope").unwrap_err();

        assert!(
            format!("{err:#}").contains("Question not found"),
            "unexpected error: {err:#}"
        );
    }

    #[test]
    fn update_comment_text_errors_when_the_sidecar_is_absent() {
        let (mut app, _tmp) = app_in_tempdir(vec![simple_file("a.rs", &[1, 2, 3])]);

        let err = app.update_comment_text("q-1", "nope").unwrap_err();

        assert!(
            format!("{err:#}").contains("Failed to read"),
            "unexpected error: {err:#}"
        );
    }

    #[test]
    fn update_comment_text_rewrites_a_note_body_on_disk() {
        let (mut app, _tmp) = app_in_tempdir(vec![simple_file("a.rs", &[1, 2, 3])]);
        let er_dir = app.tab().er_dir();
        write_sidecar(
            &er_dir,
            "notes.json",
            &serde_json::to_string(&notes_doc(vec![question("n-1", "a.rs", Some(0), Some(1))]))
                .unwrap(),
        );

        app.update_comment_text("n-1", "handed to the agent")
            .unwrap();

        let doc: ai::ErNotes = serde_json::from_str(&read_sidecar(&er_dir, "notes.json")).unwrap();
        assert_eq!(doc.notes[0].text, "handed to the agent");
    }

    #[test]
    fn update_comment_text_errors_when_the_note_id_is_unknown() {
        let (mut app, _tmp) = app_in_tempdir(vec![simple_file("a.rs", &[1, 2, 3])]);
        write_sidecar(
            &app.tab().er_dir(),
            "notes.json",
            &serde_json::to_string(&notes_doc(vec![question("n-1", "a.rs", Some(0), Some(1))]))
                .unwrap(),
        );

        let err = app.update_comment_text("n-gone", "nope").unwrap_err();

        assert!(
            format!("{err:#}").contains("Note not found"),
            "unexpected error: {err:#}"
        );
    }

    #[test]
    fn update_comment_text_rewrites_a_local_github_comment_without_calling_github() {
        let (mut app, _tmp) = app_in_tempdir(vec![simple_file("a.rs", &[1, 2, 3])]);
        let er_dir = app.tab().er_dir();
        // `github_id: None` (from the fixture) is the local-only case: no `gh`
        // round-trip, just a sidecar rewrite.
        write_sidecar(
            &er_dir,
            "github-comments.json",
            &serde_json::to_string(&gh_doc(vec![gh_comment("c-1", "a.rs", Some(0), Some(1))]))
                .unwrap(),
        );

        app.update_comment_text("c-1", "please rename this")
            .unwrap();

        let doc: ai::ErGitHubComments =
            serde_json::from_str(&read_sidecar(&er_dir, "github-comments.json")).unwrap();
        assert_eq!(doc.comments[0].comment, "please rename this");
        assert!(doc.comments[0].github_id.is_none());
    }

    #[test]
    fn update_comment_text_errors_when_the_github_comment_id_is_unknown() {
        let (mut app, _tmp) = app_in_tempdir(vec![simple_file("a.rs", &[1, 2, 3])]);
        write_sidecar(
            &app.tab().er_dir(),
            "github-comments.json",
            &serde_json::to_string(&gh_doc(vec![gh_comment("c-1", "a.rs", Some(0), Some(1))]))
                .unwrap(),
        );

        let err = app.update_comment_text("c-gone", "nope").unwrap_err();

        assert!(
            format!("{err:#}").contains("Comment not found"),
            "unexpected error: {err:#}"
        );
    }

    // ── App::update_comment (via submit_comment in edit mode) ──────────────

    /// Stage an in-place edit of `comment_id`, re-anchored at `a.rs` line 3.
    fn stage_edit_draft(app: &mut App, comment_id: &str, text: &str) {
        app.tab_mut().comment_textarea = TextArea::new(vec![text.to_string()]);
        app.tab_mut().comment_file = "a.rs".to_string();
        app.tab_mut().comment_hunk = 0;
        app.tab_mut().comment_line_num = Some(3);
        app.tab_mut().comment_edit_id = Some(comment_id.to_string());
        app.input_mode = InputMode::Comment;
    }

    #[test]
    fn submit_comment_in_edit_mode_reanchors_a_question_to_the_current_line() {
        let (mut app, _tmp) = app_in_tempdir(vec![simple_file("a.rs", &[1, 2, 3])]);
        let er_dir = app.tab().er_dir();
        let mut q = question("q-1", "a.rs", Some(9), Some(1));
        // `stale` is `#[serde(skip)]`, so the persisted evidence of a drifted
        // anchor is the status + hash pair, not the runtime flag.
        q.anchor_status = "lost".to_string();
        q.relocated_at_hash = "stale-hash".to_string();
        write_sidecar(
            &er_dir,
            "questions.json",
            &serde_json::to_string(&questions_doc(vec![q])).unwrap(),
        );
        stage_edit_draft(&mut app, "q-1", "now with context");

        app.submit_comment().unwrap();

        let stored = &load_questions(&er_dir).questions[0];
        assert_eq!(stored.text, "now with context");
        assert_eq!(stored.line_start, Some(3));
        assert_eq!(stored.line_content, "line 3");
        assert_eq!(stored.hunk_index, Some(0));
        assert_eq!(stored.anchor_status, "original");
        assert_eq!(
            stored.relocated_at_hash, "fixture-hash",
            "the anchor is re-stamped with the current diff hash"
        );
        assert_eq!(app.input_mode, InputMode::Normal);
        assert!(app.tab().comment_edit_id.is_none());
        assert_eq!(
            app.watch_message.as_deref(),
            Some("Comment updated: now with context")
        );
    }

    #[test]
    fn submit_comment_in_edit_mode_reanchors_a_note() {
        let (mut app, _tmp) = app_in_tempdir(vec![simple_file("a.rs", &[1, 2, 3])]);
        let er_dir = app.tab().er_dir();
        write_sidecar(
            &er_dir,
            "notes.json",
            &serde_json::to_string(&notes_doc(vec![question("n-1", "a.rs", Some(9), Some(1))]))
                .unwrap(),
        );
        stage_edit_draft(&mut app, "n-1", "hand this to the agent");

        app.submit_comment().unwrap();

        let doc: ai::ErNotes = serde_json::from_str(&read_sidecar(&er_dir, "notes.json")).unwrap();
        assert_eq!(doc.notes[0].text, "hand this to the agent");
        assert_eq!(doc.notes[0].line_start, Some(3));
        assert_eq!(doc.notes[0].hunk_index, Some(0));
    }

    #[test]
    fn submit_comment_in_edit_mode_reanchors_a_github_comment() {
        let (mut app, _tmp) = app_in_tempdir(vec![simple_file("a.rs", &[1, 2, 3])]);
        let er_dir = app.tab().er_dir();
        write_sidecar(
            &er_dir,
            "github-comments.json",
            &serde_json::to_string(&gh_doc(vec![gh_comment("c-1", "a.rs", Some(9), Some(1))]))
                .unwrap(),
        );
        stage_edit_draft(&mut app, "c-1", "still worth a look");

        app.submit_comment().unwrap();

        let doc: ai::ErGitHubComments =
            serde_json::from_str(&read_sidecar(&er_dir, "github-comments.json")).unwrap();
        assert_eq!(doc.comments[0].comment, "still worth a look");
        assert_eq!(doc.comments[0].line_start, Some(3));
        assert_eq!(doc.comments[0].line_content, "line 3");
        assert_eq!(doc.comments[0].anchor_status, "original");
    }

    // ── App::confirm_delete_comment ────────────────────────────────────────

    #[test]
    fn confirm_delete_comment_removes_a_question_and_cascades_its_replies() {
        let (mut app, _tmp) = app_in_tempdir(vec![simple_file("a.rs", &[1, 2, 3])]);
        let er_dir = app.tab().er_dir();
        write_sidecar(
            &er_dir,
            "questions.json",
            &serde_json::to_string(&questions_doc(vec![
                question("q-1", "a.rs", Some(0), Some(1)),
                reply_question("q-1r", "a.rs", "q-1"),
                question("q-2", "a.rs", Some(0), Some(2)),
            ]))
            .unwrap(),
        );
        app.input_mode = InputMode::Confirm(crate::app::ConfirmAction::DeleteComment {
            comment_id: "q-1".to_string(),
        });

        app.confirm_delete_comment("q-1").unwrap();

        let ids: Vec<String> = load_questions(&er_dir)
            .questions
            .iter()
            .map(|q| q.id.clone())
            .collect();
        assert_eq!(ids, vec!["q-2".to_string()], "parent and reply both go");
        assert_eq!(app.input_mode, InputMode::Normal);
        assert_eq!(app.watch_message.as_deref(), Some("Comment deleted"));
    }

    #[test]
    fn confirm_delete_comment_removes_a_note_and_cascades_its_replies() {
        let (mut app, _tmp) = app_in_tempdir(vec![simple_file("a.rs", &[1, 2, 3])]);
        let er_dir = app.tab().er_dir();
        let mut reply = question("n-1r", "a.rs", Some(0), Some(1));
        reply.in_reply_to = Some("n-1".to_string());
        write_sidecar(
            &er_dir,
            "notes.json",
            &serde_json::to_string(&notes_doc(vec![
                question("n-1", "a.rs", Some(0), Some(1)),
                reply,
                question("n-2", "a.rs", Some(0), Some(2)),
            ]))
            .unwrap(),
        );

        app.confirm_delete_comment("n-1").unwrap();

        let doc: ai::ErNotes = serde_json::from_str(&read_sidecar(&er_dir, "notes.json")).unwrap();
        let ids: Vec<String> = doc.notes.iter().map(|n| n.id.clone()).collect();
        assert_eq!(ids, vec!["n-2".to_string()]);
    }

    #[test]
    fn confirm_delete_comment_removes_a_local_github_comment_and_cascades_its_replies() {
        let (mut app, _tmp) = app_in_tempdir(vec![simple_file("a.rs", &[1, 2, 3])]);
        let er_dir = app.tab().er_dir();
        let mut reply = gh_comment("c-1r", "a.rs", Some(0), Some(1));
        reply.in_reply_to = Some("c-1".to_string());
        write_sidecar(
            &er_dir,
            "github-comments.json",
            &serde_json::to_string(&gh_doc(vec![
                gh_comment("c-1", "a.rs", Some(0), Some(1)),
                reply,
                gh_comment("c-2", "a.rs", Some(0), Some(2)),
            ]))
            .unwrap(),
        );

        app.confirm_delete_comment("c-1").unwrap();

        let doc: ai::ErGitHubComments =
            serde_json::from_str(&read_sidecar(&er_dir, "github-comments.json")).unwrap();
        let ids: Vec<String> = doc.comments.iter().map(|c| c.id.clone()).collect();
        assert_eq!(ids, vec!["c-2".to_string()]);
        assert_eq!(
            app.tab()
                .ai
                .github_comments
                .as_ref()
                .unwrap()
                .comments
                .len(),
            1,
            "the in-memory state is reloaded after the delete"
        );
    }

    #[test]
    fn confirm_delete_comment_returns_to_normal_mode_when_the_sidecar_is_missing() {
        let (mut app, _tmp) = app_in_tempdir(vec![simple_file("a.rs", &[1, 2, 3])]);
        app.input_mode = InputMode::Confirm(crate::app::ConfirmAction::DeleteComment {
            comment_id: "q-1".to_string(),
        });

        app.confirm_delete_comment("q-1").unwrap();

        assert_eq!(app.input_mode, InputMode::Normal);
        assert_eq!(app.watch_message.as_deref(), Some("Comment deleted"));
    }

    // ── App::jump_comment (via next_comment / prev_comment / next_question) ─

    /// `a.rs` carries a question, `b.rs` a GitHub comment.
    fn app_with_two_file_comments() -> App {
        let mut app = App::new_for_test(vec![
            simple_file("a.rs", &[1, 2, 3]),
            simple_file("b.rs", &[10, 11]),
        ]);
        app.tab_mut().ai.questions = Some(questions_doc(vec![question(
            "q-a",
            "a.rs",
            Some(0),
            Some(1),
        )]));
        app.tab_mut().ai.github_comments =
            Some(gh_doc(vec![gh_comment("c-b", "b.rs", Some(0), Some(10))]));
        app
    }

    #[test]
    fn next_comment_moves_to_the_comment_in_the_following_file() {
        let mut app = app_with_two_file_comments();
        app.tab_mut().focused_finding_id = Some("f-stale".to_string());
        app.tab_mut().selection_anchor = Some(2);

        app.next_comment();

        assert_eq!(app.tab().focused_comment_id.as_deref(), Some("c-b"));
        assert_eq!(app.tab().selected_file, 1);
        assert!(
            app.tab().focused_finding_id.is_none(),
            "comment focus takes over from finding focus"
        );
        assert!(app.tab().selection_anchor.is_none());
    }

    #[test]
    fn next_comment_wraps_to_the_first_comment_after_the_last() {
        let mut app = app_with_two_file_comments();
        app.tab_mut().selected_file = 1;
        app.tab_mut().focused_comment_id = Some("c-b".to_string());

        app.next_comment();

        assert_eq!(app.tab().focused_comment_id.as_deref(), Some("q-a"));
        assert_eq!(app.tab().selected_file, 0);
    }

    #[test]
    fn prev_comment_wraps_to_the_last_comment_from_the_first() {
        let mut app = app_with_two_file_comments();
        app.tab_mut().focused_comment_id = Some("q-a".to_string());

        app.prev_comment();

        assert_eq!(app.tab().focused_comment_id.as_deref(), Some("c-b"));
        assert_eq!(app.tab().selected_file, 1);
    }

    #[test]
    fn next_comment_from_a_file_with_no_comments_starts_at_the_first() {
        let mut app = App::new_for_test(vec![
            simple_file("z_unrelated.rs", &[1]),
            simple_file("a.rs", &[1, 2, 3]),
            simple_file("b.rs", &[10, 11]),
        ]);
        app.tab_mut().ai.questions = Some(questions_doc(vec![question(
            "q-a",
            "a.rs",
            Some(0),
            Some(1),
        )]));
        app.tab_mut().ai.github_comments =
            Some(gh_doc(vec![gh_comment("c-b", "b.rs", Some(0), Some(10))]));

        app.next_comment();

        assert_eq!(app.tab().focused_comment_id.as_deref(), Some("q-a"));
        assert_eq!(app.tab().selected_file, 1);
    }

    #[test]
    fn prev_comment_from_a_file_with_no_comments_starts_at_the_last() {
        let mut app = App::new_for_test(vec![
            simple_file("z_unrelated.rs", &[1]),
            simple_file("a.rs", &[1, 2, 3]),
            simple_file("b.rs", &[10, 11]),
        ]);
        app.tab_mut().ai.questions = Some(questions_doc(vec![question(
            "q-a",
            "a.rs",
            Some(0),
            Some(1),
        )]));
        app.tab_mut().ai.github_comments =
            Some(gh_doc(vec![gh_comment("c-b", "b.rs", Some(0), Some(10))]));

        app.prev_comment();

        assert_eq!(app.tab().focused_comment_id.as_deref(), Some("c-b"));
        assert_eq!(app.tab().selected_file, 2);
    }

    #[test]
    fn next_comment_within_one_file_moves_the_hunk_cursor() {
        let mut app = App::new_for_test(vec![make_file(
            "a.rs",
            vec![
                make_hunk(&[1, 2]),
                make_hunk(&[5, 6]),
                make_hunk(&[8, 9]),
                make_hunk(&[20, 21]),
            ],
        )]);
        app.tab_mut().ai.questions = Some(questions_doc(vec![
            question("q-a1", "a.rs", Some(0), Some(1)),
            question("q-a2", "a.rs", Some(3), Some(20)),
        ]));
        app.tab_mut().current_line = Some(1);

        app.next_comment();

        assert_eq!(app.tab().focused_comment_id.as_deref(), Some("q-a2"));
        assert_eq!(app.tab().selected_file, 0, "same file, no reselection");
        assert_eq!(app.tab().current_hunk, 3);
        assert!(app.tab().current_line.is_none());
    }

    #[test]
    fn next_question_skips_github_comments() {
        let mut app = App::new_for_test(vec![
            simple_file("a.rs", &[1, 2, 3]),
            simple_file("b.rs", &[10, 11]),
            simple_file("c.rs", &[20, 21]),
        ]);
        app.tab_mut().ai.questions = Some(questions_doc(vec![
            question("q-a", "a.rs", Some(0), Some(1)),
            question("q-c", "c.rs", Some(0), Some(20)),
        ]));
        app.tab_mut().ai.github_comments =
            Some(gh_doc(vec![gh_comment("c-b", "b.rs", Some(0), Some(10))]));

        app.next_question();

        assert_eq!(
            app.tab().focused_comment_id.as_deref(),
            Some("q-c"),
            "the GitHub comment on b.rs is not part of question navigation"
        );
        assert_eq!(app.tab().selected_file, 2);
    }

    #[test]
    fn next_comment_without_any_comments_is_a_noop() {
        let mut app = App::new_for_test(vec![simple_file("a.rs", &[1, 2, 3])]);
        app.tab_mut().selected_file = 0;

        app.next_comment();

        assert!(app.tab().focused_comment_id.is_none());
        assert_eq!(app.tab().selected_file, 0);
    }

    // ── App::jump_finding (via next_finding / prev_finding) ────────────────

    /// `a.rs` has two line-anchored findings, `b.rs` one.
    fn app_with_findings_across_two_files() -> App {
        let mut app = App::new_for_test(vec![
            simple_file("a.rs", &[1, 2, 3]),
            simple_file("b.rs", &[10, 11, 12]),
        ]);
        app.tab_mut().ai.review = Some(review_with(vec![
            (
                "a.rs",
                ai::RiskLevel::Medium,
                vec![
                    finding("f-a1", Some(0), Some(1)),
                    finding("f-a2", Some(0), Some(3)),
                ],
            ),
            (
                "b.rs",
                ai::RiskLevel::High,
                vec![finding("f-b", Some(0), Some(11))],
            ),
        ]));
        app
    }

    #[test]
    fn next_finding_advances_within_the_file_and_lands_on_the_finding_line() {
        let mut app = app_with_findings_across_two_files();
        app.tab_mut().focused_comment_id = Some("q-stale".to_string());

        app.next_finding();

        assert_eq!(app.tab().focused_finding_id.as_deref(), Some("f-a2"));
        assert_eq!(app.tab().selected_file, 0);
        assert_eq!(
            app.tab().current_line,
            Some(2),
            "line 3 is the third line of the hunk"
        );
        assert!(
            app.tab().focused_comment_id.is_none(),
            "finding focus takes over from comment focus"
        );
    }

    #[test]
    fn next_finding_crosses_into_the_next_file_and_clears_the_selection() {
        let mut app = app_with_findings_across_two_files();
        app.tab_mut().focused_finding_id = Some("f-a2".to_string());
        app.tab_mut().selection_anchor = Some(1);

        app.next_finding();

        assert_eq!(app.tab().focused_finding_id.as_deref(), Some("f-b"));
        assert_eq!(app.tab().selected_file, 1);
        assert_eq!(app.tab().current_line, Some(1), "line 11 is index 1");
        assert!(app.tab().selection_anchor.is_none());
    }

    #[test]
    fn next_finding_wraps_to_the_first_finding_after_the_last() {
        let mut app = app_with_findings_across_two_files();
        app.tab_mut().selected_file = 1;
        app.tab_mut().focused_finding_id = Some("f-b".to_string());

        app.next_finding();

        assert_eq!(app.tab().focused_finding_id.as_deref(), Some("f-a1"));
        assert_eq!(app.tab().selected_file, 0);
        assert_eq!(app.tab().current_line, Some(0));
    }

    #[test]
    fn next_finding_leaves_the_hunk_cursor_alone_for_a_hunkless_finding() {
        let mut app = App::new_for_test(vec![make_file(
            "a.rs",
            vec![make_hunk(&[1, 2, 3]), make_hunk(&[10, 11, 12])],
        )]);
        // A finding with no hunk anchor but a line anchor still navigates
        // (`all_findings_ordered` only drops findings with neither).
        app.tab_mut().ai.review = Some(review_with(vec![(
            "a.rs",
            ai::RiskLevel::Medium,
            vec![
                finding("f-hunkless", None, Some(2)),
                finding("f-anchored", Some(1), Some(11)),
            ],
        )]));
        app.tab_mut().focused_finding_id = Some("f-anchored".to_string());
        app.tab_mut().current_hunk = 1;

        app.next_finding();

        assert_eq!(app.tab().focused_finding_id.as_deref(), Some("f-hunkless"));
        assert_eq!(
            app.tab().current_hunk,
            1,
            "without a hunk anchor the hunk cursor is left where it was"
        );
        assert_eq!(
            app.tab().current_line,
            Some(1),
            "the line cursor is still resolved from line_start against hunk 0"
        );
    }

    #[test]
    fn prev_finding_wraps_to_the_last_finding_from_the_first() {
        let mut app = app_with_findings_across_two_files();
        app.tab_mut().focused_finding_id = Some("f-a1".to_string());

        app.prev_finding();

        assert_eq!(app.tab().focused_finding_id.as_deref(), Some("f-b"));
        assert_eq!(app.tab().selected_file, 1);
    }

    #[test]
    fn prev_finding_from_no_focus_steps_back_from_the_cursor_position() {
        let mut app = app_with_findings_across_two_files();
        app.tab_mut().selected_file = 1;
        app.tab_mut().current_hunk = 0;

        app.prev_finding();

        assert_eq!(app.tab().focused_finding_id.as_deref(), Some("f-a2"));
        assert_eq!(app.tab().selected_file, 0);
    }

    #[test]
    fn next_finding_ignores_a_focused_id_belonging_to_another_file() {
        let mut app = app_with_findings_across_two_files();
        // The user navigated back to a.rs but the panel still remembers b.rs's
        // finding. Honouring it would wrap to f-a1; the stale id must be dropped
        // and the cursor position used instead.
        app.tab_mut().selected_file = 0;
        app.tab_mut().current_hunk = 0;
        app.tab_mut().focused_finding_id = Some("f-b".to_string());

        app.next_finding();

        assert_eq!(app.tab().focused_finding_id.as_deref(), Some("f-a2"));
        assert_eq!(app.tab().selected_file, 0);
    }

    #[test]
    fn next_finding_parks_the_cursor_at_the_end_of_a_hunk_level_finding() {
        let mut app = App::new_for_test(vec![
            simple_file("a.rs", &[1, 2, 3]),
            simple_file("b.rs", &[10, 11, 12]),
        ]);
        app.tab_mut().ai.review = Some(review_with(vec![
            (
                "a.rs",
                ai::RiskLevel::Medium,
                vec![finding("f-a1", Some(0), Some(1))],
            ),
            (
                "b.rs",
                ai::RiskLevel::High,
                vec![finding("f-bh", Some(0), None)],
            ),
        ]));

        app.next_finding();

        assert_eq!(app.tab().focused_finding_id.as_deref(), Some("f-bh"));
        assert_eq!(
            app.tab().current_line,
            Some(2),
            "a hunk-level finding renders after the hunk's last line"
        );
    }

    #[test]
    fn next_finding_skips_findings_for_files_absent_from_the_diff() {
        let mut app = App::new_for_test(vec![simple_file("a.rs", &[1, 2, 3])]);
        app.tab_mut().ai.review = Some(review_with(vec![(
            "deleted-since.rs",
            ai::RiskLevel::High,
            vec![finding("f-gone", Some(0), Some(1))],
        )]));

        app.next_finding();

        assert!(
            app.tab().focused_finding_id.is_none(),
            "a finding whose file is not in the diff is unreachable"
        );
    }

    // ── App::jump_hint (via next_hint / prev_hint) ─────────────────────────

    /// `a.rs` holds a question, its reply, and an AI finding; `b.rs` a second
    /// question. Hint navigation must visit the reply and skip the finding.
    fn app_with_hints_and_a_finding() -> App {
        let mut app = App::new_for_test(vec![
            simple_file("a.rs", &[1, 2, 3]),
            simple_file("b.rs", &[10, 11]),
        ]);
        app.tab_mut().ai.questions = Some(questions_doc(vec![
            question("q-1", "a.rs", Some(0), Some(1)),
            reply_question("q-1r", "a.rs", "q-1"),
            question("q-2", "b.rs", Some(0), Some(10)),
        ]));
        app.tab_mut().ai.review = Some(review_with(vec![(
            "a.rs",
            ai::RiskLevel::High,
            vec![finding("f-a", Some(0), Some(2))],
        )]));
        app
    }

    #[test]
    fn next_hint_visits_replies_that_comment_navigation_skips() {
        let mut app = app_with_hints_and_a_finding();

        app.next_hint();

        assert_eq!(
            app.tab().focused_comment_id.as_deref(),
            Some("q-1r"),
            "hint navigation steps through thread replies"
        );
    }

    #[test]
    fn next_comment_skips_the_replies_that_hint_navigation_visits() {
        let mut app = app_with_hints_and_a_finding();

        app.next_comment();

        assert_eq!(
            app.tab().focused_comment_id.as_deref(),
            Some("q-2"),
            "comment navigation only stops on thread parents"
        );
        assert_eq!(app.tab().selected_file, 1);
    }

    #[test]
    fn hint_navigation_never_focuses_an_ai_finding() {
        let mut app = app_with_hints_and_a_finding();
        let mut visited = Vec::new();

        for _ in 0..3 {
            app.next_hint();
            assert!(
                app.tab().focused_finding_id.is_none(),
                "findings are excluded from Shift+J/K navigation"
            );
            visited.push(app.tab().focused_comment_id.clone().unwrap());
        }

        assert_eq!(visited, vec!["q-1r", "q-2", "q-1"]);
    }

    #[test]
    fn next_hint_switching_files_clears_the_line_cursor() {
        let mut app = app_with_hints_and_a_finding();
        app.tab_mut().current_line = Some(2);

        app.next_hint(); // q-1r, still a.rs
        app.next_hint(); // q-2, now b.rs

        assert_eq!(app.tab().selected_file, 1);
        assert_eq!(app.tab().current_hunk, 0);
        assert!(app.tab().current_line.is_none());
    }

    #[test]
    fn prev_hint_wraps_to_the_last_hint_from_the_first() {
        let mut app = app_with_hints_and_a_finding();
        app.tab_mut().focused_comment_id = Some("q-1".to_string());

        app.prev_hint();

        assert_eq!(app.tab().focused_comment_id.as_deref(), Some("q-2"));
        assert_eq!(app.tab().selected_file, 1);
    }

    #[test]
    fn next_hint_with_only_findings_loaded_is_a_noop() {
        let mut app = App::new_for_test(vec![simple_file("a.rs", &[1, 2, 3])]);
        app.tab_mut().ai.review = Some(review_with(vec![(
            "a.rs",
            ai::RiskLevel::High,
            vec![finding("f-a", Some(0), Some(2))],
        )]));

        app.next_hint();

        assert!(app.tab().focused_comment_id.is_none());
        assert!(app.tab().focused_finding_id.is_none());
    }

    // ── App::navigate_panel_finding ────────────────────────────────────────

    /// Panel order is confidence first, then hunk: `f2` (Confirmed, hunk 2)
    /// outranks `f3` (Tentative, hunk 0) and `f1` (Tentative, hunk 1).
    fn app_with_panel_findings() -> App {
        let mut app = App::new_for_test(vec![simple_file("a.rs", &[1, 2, 3])]);
        app.tab_mut().ai.review = Some(review_with(vec![(
            "a.rs",
            ai::RiskLevel::Medium,
            vec![
                finding_full(
                    "f1",
                    Some(1),
                    None,
                    ai::RiskLevel::Medium,
                    ai::Confidence::Tentative,
                ),
                finding_full(
                    "f2",
                    Some(2),
                    None,
                    ai::RiskLevel::Low,
                    ai::Confidence::Confirmed,
                ),
                finding_full(
                    "f3",
                    Some(0),
                    None,
                    ai::RiskLevel::High,
                    ai::Confidence::Tentative,
                ),
            ],
        )]));
        app
    }

    #[test]
    fn navigate_panel_finding_forward_starts_at_the_confirmed_finding() {
        let mut app = app_with_panel_findings();

        app.navigate_panel_finding(true);

        assert_eq!(
            app.tab().focused_finding_id.as_deref(),
            Some("f2"),
            "confidence outranks hunk order in the panel list"
        );
    }

    #[test]
    fn navigate_panel_finding_backward_starts_at_the_end_of_the_panel_list() {
        let mut app = app_with_panel_findings();

        app.navigate_panel_finding(false);

        assert_eq!(app.tab().focused_finding_id.as_deref(), Some("f1"));
    }

    #[test]
    fn navigate_panel_finding_forward_follows_panel_order_not_hunk_order() {
        let mut app = app_with_panel_findings();
        app.tab_mut().focused_finding_id = Some("f2".to_string());

        app.navigate_panel_finding(true);

        assert_eq!(app.tab().focused_finding_id.as_deref(), Some("f3"));
    }

    #[test]
    fn navigate_panel_finding_forward_wraps_after_the_last_entry() {
        let mut app = app_with_panel_findings();
        app.tab_mut().focused_finding_id = Some("f1".to_string());

        app.navigate_panel_finding(true);

        assert_eq!(app.tab().focused_finding_id.as_deref(), Some("f2"));
    }

    #[test]
    fn navigate_panel_finding_backward_wraps_before_the_first_entry() {
        let mut app = app_with_panel_findings();
        app.tab_mut().focused_finding_id = Some("f2".to_string());

        app.navigate_panel_finding(false);

        assert_eq!(app.tab().focused_finding_id.as_deref(), Some("f1"));
    }

    #[test]
    fn navigate_panel_finding_is_a_noop_when_the_file_has_no_findings() {
        let mut app = App::new_for_test(vec![simple_file("a.rs", &[1, 2, 3])]);
        app.tab_mut().ai.review = Some(review_with(vec![(
            "other.rs",
            ai::RiskLevel::High,
            vec![finding("f-x", Some(0), Some(1))],
        )]));

        app.navigate_panel_finding(true);

        assert!(app.tab().focused_finding_id.is_none());
    }

    #[test]
    fn navigate_panel_finding_is_a_noop_without_a_selected_file() {
        let mut app = App::new_for_test(vec![]);

        app.navigate_panel_finding(true);

        assert!(app.tab().focused_finding_id.is_none());
    }

    // ── App::jump_to_focused_finding ───────────────────────────────────────

    /// `a.rs` has two hunks (lines 1-3 and 10-11) and findings anchored in,
    /// past, and outside them.
    fn app_with_two_hunk_findings() -> App {
        let mut app = App::new_for_test(vec![make_file(
            "a.rs",
            vec![make_hunk(&[1, 2, 3]), make_hunk(&[10, 11])],
        )]);
        app.tab_mut().ai.review = Some(review_with(vec![(
            "a.rs",
            ai::RiskLevel::High,
            vec![
                finding("f-line", Some(1), Some(11)),
                finding("f-hunk", Some(0), None),
                finding("f-outside", Some(0), Some(999)),
                finding("f-nohunk", Some(5), None),
            ],
        )]));
        app.tab_mut().panel_focus = true;
        app
    }

    #[test]
    fn jump_to_focused_finding_moves_the_cursor_onto_the_finding_line() {
        let mut app = app_with_two_hunk_findings();
        app.tab_mut().focused_finding_id = Some("f-line".to_string());

        app.jump_to_focused_finding();

        assert_eq!(app.tab().current_hunk, 1);
        assert_eq!(app.tab().current_line, Some(1), "line 11 is index 1");
        assert!(
            !app.tab().panel_focus,
            "jumping hands focus back to the diff"
        );
    }

    #[test]
    fn jump_to_focused_finding_parks_at_the_end_of_a_hunk_level_finding() {
        let mut app = app_with_two_hunk_findings();
        app.tab_mut().focused_finding_id = Some("f-hunk".to_string());

        app.jump_to_focused_finding();

        assert_eq!(app.tab().current_hunk, 0);
        assert_eq!(app.tab().current_line, Some(2));
        assert!(!app.tab().panel_focus);
    }

    #[test]
    fn jump_to_focused_finding_reports_a_line_that_is_not_in_the_diff() {
        let mut app = app_with_two_hunk_findings();
        app.tab_mut().current_hunk = 1;
        app.tab_mut().focused_finding_id = Some("f-outside".to_string());

        app.jump_to_focused_finding();

        assert_eq!(
            app.watch_message.as_deref(),
            Some("Line 999 is outside the diff — open in editor to view")
        );
        assert_eq!(app.tab().current_hunk, 1, "the cursor does not move");
        assert!(!app.tab().panel_focus);
    }

    #[test]
    fn jump_to_focused_finding_reports_a_hunk_beyond_the_parsed_diff() {
        let mut app = app_with_two_hunk_findings();
        app.tab_mut().focused_finding_id = Some("f-nohunk".to_string());

        app.jump_to_focused_finding();

        assert_eq!(
            app.watch_message.as_deref(),
            Some("Line ? is outside the diff — open in editor to view")
        );
        assert_eq!(app.tab().current_hunk, 0);
    }

    #[test]
    fn jump_to_focused_finding_without_a_focused_finding_is_a_noop() {
        let mut app = app_with_two_hunk_findings();

        app.jump_to_focused_finding();

        assert!(app.tab().panel_focus, "panel focus is left untouched");
        assert!(app.watch_message.is_none());
    }

    #[test]
    fn jump_to_focused_finding_without_a_review_is_a_noop() {
        let mut app = app_with_two_hunk_findings();
        app.tab_mut().ai.review = None;
        app.tab_mut().focused_finding_id = Some("f-line".to_string());

        app.jump_to_focused_finding();

        assert!(app.tab().panel_focus);
        assert!(app.watch_message.is_none());
    }

    #[test]
    fn jump_to_focused_finding_with_an_unknown_id_is_a_noop() {
        let mut app = app_with_two_hunk_findings();
        app.tab_mut().focused_finding_id = Some("f-gone".to_string());

        app.jump_to_focused_finding();

        assert!(app.tab().panel_focus);
        assert!(app.watch_message.is_none());
    }

    // ── App::review_jump_to_file ───────────────────────────────────────────

    #[test]
    fn review_jump_to_file_opens_the_riskiest_file_at_its_first_anchored_finding() {
        let mut app = App::new_for_test(vec![
            simple_file("a.rs", &[1, 2, 3]),
            simple_file("b.rs", &[10, 11]),
        ]);
        app.tab_mut().ai.review = Some(review_with(vec![
            ("a.rs", ai::RiskLevel::Medium, vec![]),
            (
                "b.rs",
                ai::RiskLevel::High,
                vec![
                    finding("f-b1", Some(1), Some(11)),
                    finding("f-b0", Some(0), Some(10)),
                ],
            ),
        ]));
        app.tab_mut().review_focus = ai::ReviewFocus::Files;
        app.tab_mut().review_cursor = 0;

        app.review_jump_to_file();

        assert_eq!(app.tab().selected_file, 1, "High risk sorts first");
        assert_eq!(app.tab().focused_finding_id.as_deref(), Some("f-b0"));
        assert_eq!(app.tab().current_hunk, 0);
        assert_eq!(app.tab().panel, Some(ai::PanelContent::FileDetail));
        assert_eq!(app.watch_message.as_deref(), Some("Jumped to: b.rs"));
    }

    #[test]
    fn review_jump_to_file_reports_a_file_missing_from_the_diff() {
        let mut app = App::new_for_test(vec![simple_file("a.rs", &[1, 2, 3])]);
        app.tab_mut().ai.review = Some(review_with(vec![(
            "removed.rs",
            ai::RiskLevel::High,
            vec![finding("f-x", Some(0), Some(1))],
        )]));
        app.tab_mut().review_focus = ai::ReviewFocus::Files;
        app.tab_mut().review_cursor = 0;

        app.review_jump_to_file();

        assert_eq!(
            app.watch_message.as_deref(),
            Some("File not in diff: removed.rs")
        );
        assert_eq!(app.tab().selected_file, 0);
        assert!(app.tab().panel.is_none());
    }

    #[test]
    fn review_jump_to_file_follows_the_checklist_when_that_column_has_focus() {
        let mut app = App::new_for_test(vec![
            simple_file("a.rs", &[1, 2, 3]),
            simple_file("b.rs", &[10, 11]),
        ]);
        app.tab_mut().ai.checklist = Some(ai::ErChecklist {
            version: 1,
            diff_hash: "fixture-hash".to_string(),
            items: vec![ai::ChecklistItem {
                id: "chk-1".to_string(),
                text: "verify the parser".to_string(),
                category: String::new(),
                checked: false,
                related_findings: vec![],
                related_files: vec!["b.rs".to_string()],
            }],
        });
        app.tab_mut().review_focus = ai::ReviewFocus::Checklist;
        app.tab_mut().review_cursor = 0;

        app.review_jump_to_file();

        assert_eq!(app.tab().selected_file, 1);
        assert_eq!(app.watch_message.as_deref(), Some("Jumped to: b.rs"));
    }

    #[test]
    fn review_jump_to_file_reports_an_item_with_no_associated_file() {
        let mut app = App::new_for_test(vec![simple_file("a.rs", &[1, 2, 3])]);
        app.tab_mut().review_focus = ai::ReviewFocus::Checklist;
        app.tab_mut().review_cursor = 0;

        app.review_jump_to_file();

        assert_eq!(
            app.watch_message.as_deref(),
            Some("No file associated with this item")
        );
    }

    // ── Background task fixtures ───────────────────────────────────────────

    fn bg_target(repo_root: &str, branch: &str) -> BackgroundTaskTarget {
        BackgroundTaskTarget {
            repo_root: repo_root.to_string(),
            er_dir: format!("{repo_root}/.er"),
            branch_label: branch.to_string(),
            base_branch: "main".to_string(),
            scope: "branch".to_string(),
            pr_number: None,
            remote_repo: None,
            managed_local: false,
        }
    }

    /// Register a synthetic in-flight handle. The result/log senders are
    /// dropped immediately — nothing in these tests polls them, and
    /// `poll_background_tasks` (which would reap them) is never called.
    fn insert_bg_handle(app: &mut App, task: BackgroundTask, recent_log: Vec<AgentLogEntry>) {
        let (_result_tx, result_rx) = std::sync::mpsc::channel();
        let (_log_tx, log_rx) = std::sync::mpsc::channel();
        app.background_tasks.insert(
            task.id.clone(),
            BackgroundTaskHandle {
                task,
                result_rx,
                log_rx,
                recent_log: recent_log.into_iter().collect(),
            },
        );
    }

    fn pending_task(task: BackgroundTask) -> PendingBackgroundTask {
        PendingBackgroundTask {
            task,
            command_name: "review".to_string(),
            prompt: "prompt".to_string(),
            prepared_diff: false,
            host_write_diagram: None,
            ai_selection: None,
        }
    }

    // ── App::background_task_snapshots ─────────────────────────────────────

    #[test]
    fn background_task_snapshots_include_running_and_recently_finished_tasks_only() {
        let mut app = App::new_for_test(vec![]);
        let now = unix_now_ms();

        let mut running = BackgroundTask::new("review".to_string(), bg_target("/repo", "running"));
        running.started_at_ms = 10;
        insert_bg_handle(&mut app, running.clone(), vec![]);

        let mut just_done = BackgroundTask::new("triage".to_string(), bg_target("/repo", "fresh"));
        just_done.status = CommandStatus::Done;
        just_done.started_at_ms = 20;
        just_done.finished_at_ms = Some(now);
        insert_bg_handle(&mut app, just_done.clone(), vec![]);

        let mut long_done = BackgroundTask::new("tour".to_string(), bg_target("/repo", "ancient"));
        long_done.status = CommandStatus::Done;
        long_done.started_at_ms = 30;
        long_done.finished_at_ms = Some(1);
        insert_bg_handle(&mut app, long_done.clone(), vec![]);

        let mut no_finish =
            BackgroundTask::new("professor".to_string(), bg_target("/repo", "orphan"));
        no_finish.status = CommandStatus::Failed("crashed".to_string());
        no_finish.started_at_ms = 40;
        no_finish.finished_at_ms = None;
        insert_bg_handle(&mut app, no_finish.clone(), vec![]);

        let mut retired_fresh =
            BackgroundTask::new("review".to_string(), bg_target("/repo", "retired-fresh"));
        retired_fresh.status = CommandStatus::Done;
        retired_fresh.started_at_ms = 50;
        retired_fresh.finished_at_ms = Some(now);
        app.recent_background_tasks.push(retired_fresh.clone());

        let mut retired_old =
            BackgroundTask::new("review".to_string(), bg_target("/repo", "retired-old"));
        retired_old.status = CommandStatus::Done;
        retired_old.started_at_ms = 60;
        retired_old.finished_at_ms = Some(1);
        app.recent_background_tasks.push(retired_old.clone());

        let mut queued = BackgroundTask::new("review".to_string(), bg_target("/repo", "waiting"));
        queued.started_at_ms = 70;
        app.pending_background_tasks
            .push_back(pending_task(queued.clone()));

        let snaps = app.background_task_snapshots();
        let status_of = |id: &str| {
            snaps
                .iter()
                .find(|s| s.id == id)
                .map(|s| s.status.as_str().to_string())
        };

        assert_eq!(status_of(&running.id), Some("running".to_string()));
        assert_eq!(status_of(&just_done.id), Some("done".to_string()));
        assert_eq!(
            status_of(&long_done.id),
            None,
            "a task that finished more than 8s ago is dropped"
        );
        assert_eq!(
            status_of(&no_finish.id),
            None,
            "a non-running task without a finish time is dropped"
        );
        assert_eq!(status_of(&retired_fresh.id), Some("done".to_string()));
        assert_eq!(status_of(&retired_old.id), None);
        assert_eq!(
            status_of(&queued.id),
            Some("queued".to_string()),
            "pending tasks are relabelled as queued"
        );
        assert_eq!(snaps.len(), 4);
    }

    #[test]
    fn background_task_snapshots_are_ordered_by_start_time() {
        let mut app = App::new_for_test(vec![]);
        let mut last = BackgroundTask::new("review".to_string(), bg_target("/repo", "c"));
        last.started_at_ms = 300;
        let mut first = BackgroundTask::new("review".to_string(), bg_target("/repo", "a"));
        first.started_at_ms = 100;
        let mut middle = BackgroundTask::new("review".to_string(), bg_target("/repo", "b"));
        middle.started_at_ms = 200;
        insert_bg_handle(&mut app, last.clone(), vec![]);
        insert_bg_handle(&mut app, first.clone(), vec![]);
        insert_bg_handle(&mut app, middle.clone(), vec![]);

        let ids: Vec<String> = app
            .background_task_snapshots()
            .into_iter()
            .map(|s| s.id)
            .collect();

        assert_eq!(ids, vec![first.id, middle.id, last.id]);
    }

    #[test]
    fn background_task_snapshots_keep_the_last_40_log_lines_in_order() {
        let mut app = App::new_for_test(vec![]);
        let mut task = BackgroundTask::new("review".to_string(), bg_target("/repo", "chatty"));
        task.started_at_ms = 1;
        let log: Vec<AgentLogEntry> = (0..50)
            .map(|i| log_entry("review", &format!("log-{i}")))
            .collect();
        insert_bg_handle(&mut app, task.clone(), log);

        let snaps = app.background_task_snapshots();
        let snap = snaps.iter().find(|s| s.id == task.id).unwrap();

        assert_eq!(snap.recent_log.len(), 40);
        assert_eq!(
            snap.recent_log.first().unwrap().text,
            "log-10",
            "the tail is kept, not the head"
        );
        assert_eq!(snap.recent_log.last().unwrap().text, "log-49");
    }

    #[test]
    fn background_task_snapshots_carry_no_log_for_queued_tasks() {
        let mut app = App::new_for_test(vec![]);
        let mut queued = BackgroundTask::new("review".to_string(), bg_target("/repo", "waiting"));
        queued.started_at_ms = 1;
        app.pending_background_tasks
            .push_back(pending_task(queued.clone()));

        let snaps = app.background_task_snapshots();

        assert_eq!(snaps.len(), 1);
        assert_eq!(snaps[0].status, "queued");
        assert!(snaps[0].recent_log.is_empty());
    }

    // ── App::background_tasks_for_tab ──────────────────────────────────────

    #[test]
    fn background_tasks_for_tab_keeps_only_matching_and_recent_targets() {
        let mut app = App::new_for_test(vec![]);
        // `TabState::new_for_test` uses repo_root "/tmp/test", branch "feature".
        let tab = TabState::new_for_test(vec![]);
        let now = unix_now_ms();

        let mut mine = BackgroundTask::new("review".to_string(), bg_target("/tmp/test", "feature"));
        mine.started_at_ms = 1;
        insert_bg_handle(&mut app, mine.clone(), vec![]);

        let theirs = BackgroundTask::new("review".to_string(), bg_target("/tmp/test", "other"));
        insert_bg_handle(&mut app, theirs.clone(), vec![]);

        let mut stale =
            BackgroundTask::new("triage".to_string(), bg_target("/tmp/test", "feature"));
        stale.status = CommandStatus::Done;
        stale.finished_at_ms = Some(1);
        insert_bg_handle(&mut app, stale.clone(), vec![]);

        let mut done = BackgroundTask::new("tour".to_string(), bg_target("/tmp/test", "feature"));
        done.status = CommandStatus::Done;
        done.finished_at_ms = Some(now);
        insert_bg_handle(&mut app, done.clone(), vec![]);

        let queued_mine =
            BackgroundTask::new("professor".to_string(), bg_target("/tmp/test", "feature"));
        app.pending_background_tasks
            .push_back(pending_task(queued_mine.clone()));
        let queued_theirs =
            BackgroundTask::new("professor".to_string(), bg_target("/tmp/test", "elsewhere"));
        app.pending_background_tasks
            .push_back(pending_task(queued_theirs.clone()));

        let out = app.background_tasks_for_tab(&tab);
        let ids: Vec<&str> = out.iter().map(|s| s.id.as_str()).collect();

        assert!(ids.contains(&mine.id.as_str()));
        assert!(ids.contains(&done.id.as_str()));
        assert!(ids.contains(&queued_mine.id.as_str()));
        assert!(
            !ids.contains(&theirs.id.as_str()),
            "another branch's task belongs to another tab"
        );
        assert!(
            !ids.contains(&stale.id.as_str()),
            "finished more than 8s ago"
        );
        assert!(!ids.contains(&queued_theirs.id.as_str()));
        assert_eq!(
            out.iter()
                .find(|s| s.id == queued_mine.id)
                .map(|s| s.status.as_str()),
            Some("queued")
        );
        assert_eq!(out.len(), 3);
    }

    #[test]
    fn background_tasks_for_tab_is_empty_for_a_tab_on_a_different_repo() {
        let mut app = App::new_for_test(vec![]);
        let tab = TabState::new_for_test(vec![]);
        insert_bg_handle(
            &mut app,
            BackgroundTask::new(
                "review".to_string(),
                bg_target("/somewhere/else", "feature"),
            ),
            vec![],
        );

        assert!(app.background_tasks_for_tab(&tab).is_empty());
    }

    // ── App::spawn_background_agent_task (via spawn_background_review) ─────

    #[test]
    fn spawn_background_review_rejects_a_target_with_no_repo_and_no_remote() {
        let mut app = App::new_for_test(vec![]);

        let err = app
            .spawn_background_review(bg_target("", "feature"), "prompt".to_string(), false)
            .unwrap_err();

        assert!(
            format!("{err:#}").contains("Open a repository or PR first"),
            "unexpected error: {err:#}"
        );
        assert!(app.background_tasks.is_empty());
        assert!(app.pending_background_tasks.is_empty());
    }

    #[test]
    fn spawn_background_review_rejects_a_duplicate_of_a_running_task() {
        let mut app = App::new_for_test(vec![]);
        let target = bg_target("/repo", "feat-a");
        insert_bg_handle(
            &mut app,
            BackgroundTask::new("review".to_string(), target.clone()),
            vec![],
        );

        let err = app
            .spawn_background_review(target, "prompt".to_string(), false)
            .unwrap_err();

        assert!(
            format!("{err:#}").contains("review already running for feat-a"),
            "unexpected error: {err:#}"
        );
        assert_eq!(app.background_tasks.len(), 1);
        assert!(app.pending_background_tasks.is_empty());
    }

    #[test]
    fn spawn_background_review_queues_when_the_concurrency_cap_is_reached() {
        let mut app = App::new_for_test(vec![]);
        app.config.ai_hub.max_concurrent_reviews = 1;
        insert_bg_handle(
            &mut app,
            BackgroundTask::new("review".to_string(), bg_target("/repo", "busy")),
            vec![],
        );

        app.spawn_background_review(bg_target("/repo", "feat-a"), "prompt".to_string(), false)
            .unwrap();

        assert_eq!(
            app.background_tasks.len(),
            1,
            "the cap prevents a second subprocess"
        );
        assert_eq!(app.pending_background_tasks.len(), 1);
        assert_eq!(app.pending_background_tasks[0].task.kind, "review");
        assert!(
            app.pending_background_tasks[0].ai_selection.is_some(),
            "the provider/model choice is snapshotted at enqueue time"
        );
        assert_eq!(
            app.watch_message.as_deref(),
            Some("review queued (#1, feat-a)")
        );
    }

    #[test]
    fn spawn_background_review_rejects_a_duplicate_of_an_already_queued_task() {
        let mut app = App::new_for_test(vec![]);
        app.config.ai_hub.max_concurrent_reviews = 1;
        insert_bg_handle(
            &mut app,
            BackgroundTask::new("review".to_string(), bg_target("/repo", "busy")),
            vec![],
        );
        app.spawn_background_review(bg_target("/repo", "feat-a"), "prompt".to_string(), false)
            .unwrap();

        let err = app
            .spawn_background_review(bg_target("/repo", "feat-a"), "prompt".to_string(), false)
            .unwrap_err();

        assert!(
            format!("{err:#}").contains("review already running for feat-a"),
            "unexpected error: {err:#}"
        );
        assert_eq!(
            app.pending_background_tasks.len(),
            1,
            "the queue does not grow a second copy"
        );
    }

    #[test]
    fn a_tour_and_a_review_queue_independently_for_the_same_target() {
        let mut app = App::new_for_test(vec![]);
        app.config.ai_hub.max_concurrent_reviews = 1;
        insert_bg_handle(
            &mut app,
            BackgroundTask::new("review".to_string(), bg_target("/repo", "busy")),
            vec![],
        );

        app.spawn_background_review(bg_target("/repo", "feat-a"), "prompt".to_string(), false)
            .unwrap();
        app.spawn_background_tour(bg_target("/repo", "feat-a"), "prompt".to_string(), false)
            .unwrap();

        assert_eq!(app.pending_background_tasks.len(), 2);
        assert_eq!(app.pending_background_tasks[1].task.kind, "tour");
        assert_eq!(
            app.watch_message.as_deref(),
            Some("tour queued (#2, feat-a)"),
            "dedup is per kind, not per target"
        );
    }

    // ── App::spawn_command ─────────────────────────────────────────────────

    /// Poll `check_commands` until `name` leaves the Running state.
    fn wait_for_command(app: &mut App, name: &str) -> CommandStatus {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        loop {
            app.check_commands();
            match app.tab().command_status.get(name) {
                Some(CommandStatus::Running) | None => {}
                Some(status) => return status.clone(),
            }
            assert!(
                std::time::Instant::now() < deadline,
                "{name} never finished"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    #[test]
    fn spawn_command_substitutes_placeholders_before_running_the_shell() {
        let (mut app, _tmp) = app_in_tempdir(vec![]);

        app.spawn_command("summary", "printf '%s' {branch} > {output}")
            .unwrap();

        assert_eq!(
            app.tab().command_status.get("summary"),
            Some(&CommandStatus::Running)
        );
        assert_eq!(app.watch_message.as_deref(), Some("summary started..."));

        assert_eq!(wait_for_command(&mut app, "summary"), CommandStatus::Done);
        assert_eq!(
            read_sidecar(&app.tab().er_dir(), "summary.md"),
            "feature",
            "{{branch}} resolves to the tab's branch and {{output}} to the sidecar path"
        );
        assert_eq!(
            app.tab().ai.summary.as_deref(),
            Some("feature"),
            "`summary` is an artifact-writing command, so the sidecar is reloaded"
        );
        assert_eq!(app.watch_message.as_deref(), Some("summary done"));
    }

    #[test]
    fn spawn_command_refuses_a_second_run_of_the_same_name() {
        let (mut app, _tmp) = app_in_tempdir(vec![]);
        app.tab_mut()
            .command_status
            .insert("test".to_string(), CommandStatus::Running);

        app.spawn_command("test", "printf hi").unwrap();

        assert_eq!(app.watch_message.as_deref(), Some("test already running"));
        assert!(
            !app.tab().command_rx.contains_key("test"),
            "no second process is started"
        );
    }

    #[test]
    fn spawn_command_reports_a_failing_shell_command_with_its_stderr() {
        let (mut app, _tmp) = app_in_tempdir(vec![]);

        app.spawn_command("test", "echo 'boom happened' >&2; exit 1")
            .unwrap();

        match wait_for_command(&mut app, "test") {
            CommandStatus::Failed(msg) => {
                assert_eq!(msg, "test failed: boom happened");
            }
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    #[test]
    fn spawn_command_streams_stdout_into_the_agent_log() {
        let (mut app, _tmp) = app_in_tempdir(vec![]);

        app.spawn_command("test", "echo hello-from-shell").unwrap();
        wait_for_command(&mut app, "test");
        app.drain_agent_log();

        let texts: Vec<&str> = app
            .tab()
            .agent_log
            .iter()
            .map(|e| e.text.as_str())
            .collect();
        assert!(texts.contains(&"test started"), "log: {texts:?}");
        assert!(texts.contains(&"hello-from-shell"), "log: {texts:?}");
        assert!(texts.contains(&"test completed"), "log: {texts:?}");
    }

    // ── App::spawn_agent_prompt ────────────────────────────────────────────

    /// Write an executable stub named `claude` (the stem decides which CLI
    /// conventions `spawn_agent_prompt` applies) into `dir`.
    #[cfg(unix)]
    fn write_fake_claude(dir: &std::path::Path, body: &str) -> std::path::PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join("claude");
        std::fs::write(&path, format!("#!/bin/sh\n{body}")).unwrap();
        let mut perms = std::fs::metadata(&path).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&path, perms).unwrap();
        path
    }

    #[test]
    #[cfg(unix)]
    fn spawn_agent_prompt_injects_claude_tool_rules_and_records_the_invocation() {
        let (mut app, tmp) = app_in_tempdir(vec![]);
        let fake = write_fake_claude(
            tmp.path(),
            "echo '{\"type\":\"assistant\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"looking at the diff\"}]}}'\n\
             echo 'plain non-json line'\n\
             echo 'agent warning' >&2\n\
             exit 0\n",
        );
        // No AI Hub providers configured, so the legacy `agent.command` arm runs
        // with the default args (--print --output-format stream-json -p {prompt}).
        assert!(app.config.ai_hub.providers.is_empty());
        app.config.agent.command = fake.to_string_lossy().to_string();

        app.spawn_agent_prompt("review", "review this diff")
            .unwrap();
        assert_eq!(app.watch_message.as_deref(), Some("review started..."));

        assert_eq!(wait_for_command(&mut app, "review"), CommandStatus::Done);

        let debug = read_sidecar(&app.tab().er_dir(), "debug-agent.log");
        assert!(
            debug.contains("--disallowedTools Bash(git clone*)"),
            "cloning stays denied:\n{debug}"
        );
        assert!(
            debug.contains("--allowedTools Read"),
            "read access is granted explicitly, not via skip-permissions:\n{debug}"
        );
        assert!(
            debug.contains("--allowedTools Bash(git fetch origin pull/*)"),
            "the narrow fetch rule survives:\n{debug}"
        );
        assert!(
            debug.contains("--add-dir="),
            "the sidecar directory is handed to the agent:\n{debug}"
        );
        assert!(
            debug.contains("--verbose"),
            "--print + stream-json requires --verbose on Claude:\n{debug}"
        );
        assert!(debug.contains("exit code: 0"), "{debug}");
        assert!(
            debug.contains("agent warning"),
            "stderr is captured:\n{debug}"
        );

        app.drain_agent_log();
        let texts: Vec<&str> = app
            .tab()
            .agent_log
            .iter()
            .map(|e| e.text.as_str())
            .collect();
        assert!(
            texts.iter().any(|t| t.contains("looking at the diff")),
            "stream-json assistant text reaches the agent log: {texts:?}"
        );
        assert!(
            texts.contains(&"agent warning"),
            "stderr reaches the agent log: {texts:?}"
        );
        assert!(
            !texts.contains(&"plain non-json line"),
            "unparseable stream-json lines are dropped, not shown raw: {texts:?}"
        );
    }

    #[test]
    #[cfg(unix)]
    fn spawn_agent_prompt_points_a_failed_run_at_the_debug_log() {
        let (mut app, tmp) = app_in_tempdir(vec![]);
        let fake = write_fake_claude(tmp.path(), "echo 'fatal' >&2\nexit 3\n");
        app.config.agent.command = fake.to_string_lossy().to_string();

        app.spawn_agent_prompt("review", "review this diff")
            .unwrap();

        match wait_for_command(&mut app, "review") {
            CommandStatus::Failed(msg) => {
                assert!(
                    msg.starts_with("review failed (see ") && msg.ends_with("debug-agent.log)"),
                    "unexpected failure message: {msg}"
                );
            }
            other => panic!("expected Failed, got {other:?}"),
        }
        let debug = read_sidecar(&app.tab().er_dir(), "debug-agent.log");
        assert!(debug.contains("exit code: 3"), "{debug}");
    }

    #[test]
    fn spawn_agent_prompt_refuses_a_second_run_of_the_same_name() {
        let (mut app, _tmp) = app_in_tempdir(vec![]);
        app.tab_mut()
            .command_status
            .insert("review".to_string(), CommandStatus::Running);

        app.spawn_agent_prompt("review", "prompt").unwrap();

        assert_eq!(app.watch_message.as_deref(), Some("review already running"));
        assert!(!app.tab().command_rx.contains_key("review"));
    }

    #[test]
    #[cfg(unix)]
    fn spawn_agent_prompt_prefers_the_ai_hub_provider_over_the_legacy_agent_command() {
        let (mut app, tmp) = app_in_tempdir(vec![]);
        let fake = write_fake_claude(tmp.path(), "exit 0\n");
        app.config.ai_hub.providers.insert(
            "hub-claude".to_string(),
            crate::config::AiProviderConfig {
                command: fake.to_string_lossy().to_string(),
                args: vec![
                    "--print".to_string(),
                    "-p".to_string(),
                    "{prompt}".to_string(),
                ],
                models: vec![crate::config::AiModelConfig {
                    id: "sonnet-test".to_string(),
                    args: vec!["--model".to_string(), "sonnet-test".to_string()],
                    ..Default::default()
                }],
                ..Default::default()
            },
        );
        app.config.ai_hub.default_provider = Some("hub-claude".to_string());
        app.config.ai_hub.default_model = Some("sonnet-test".to_string());
        // If the hub arm were skipped this command would be spawned and fail.
        app.config.agent.command = "er-test-no-such-binary".to_string();

        app.spawn_agent_prompt("review", "review this diff")
            .unwrap();

        assert_eq!(wait_for_command(&mut app, "review"), CommandStatus::Done);
        let debug = read_sidecar(&app.tab().er_dir(), "debug-agent.log");
        assert!(
            debug.contains(fake.to_string_lossy().as_ref()),
            "the hub provider's command wins over agent.command:\n{debug}"
        );
        assert!(
            debug.contains("--model sonnet-test"),
            "the selected model's args are merged in:\n{debug}"
        );
        assert!(
            debug.contains("--output-format stream-json"),
            "a Claude-family provider still gets streaming output:\n{debug}"
        );
    }

    #[test]
    #[cfg(unix)]
    fn spawn_agent_prompt_isolates_codex_and_leaves_claude_flags_off() {
        use std::os::unix::fs::PermissionsExt;

        let (mut app, tmp) = app_in_tempdir(vec![]);
        let fake = tmp.path().join("codex");
        std::fs::write(&fake, "#!/bin/sh\necho 'codex plain output'\nexit 0\n").unwrap();
        let mut perms = std::fs::metadata(&fake).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&fake, perms).unwrap();

        app.config.ai_hub.providers.insert(
            "codex".to_string(),
            crate::config::AiProviderConfig {
                command: fake.to_string_lossy().to_string(),
                args: vec!["exec".to_string(), "{prompt}".to_string()],
                models: vec![crate::config::AiModelConfig {
                    id: "gpt-test".to_string(),
                    ..Default::default()
                }],
                ..Default::default()
            },
        );
        app.config.ai_hub.default_provider = Some("codex".to_string());

        app.spawn_agent_prompt("review", "review this diff")
            .unwrap();

        assert_eq!(wait_for_command(&mut app, "review"), CommandStatus::Done);
        let debug = read_sidecar(&app.tab().er_dir(), "debug-agent.log");
        assert!(
            debug.contains("--ignore-user-config"),
            "Codex runs isolated from the user's own config:\n{debug}"
        );
        assert!(
            !debug.contains("--allowedTools"),
            "Claude-only permission flags must not leak onto Codex:\n{debug}"
        );
        assert!(
            !debug.contains("--output-format"),
            "Codex does not emit Claude stream-json:\n{debug}"
        );

        app.drain_agent_log();
        let texts: Vec<&str> = app
            .tab()
            .agent_log
            .iter()
            .map(|e| e.text.as_str())
            .collect();
        assert!(
            texts.contains(&"codex plain output"),
            "non-streaming stdout is logged verbatim: {texts:?}"
        );
    }

    // ── App::copy_context ──────────────────────────────────────────────────
    //
    // The happy paths end in a real `pbcopy`, so they are macOS-only: that is
    // the one platform where the clipboard binary is guaranteed present.

    #[test]
    fn copy_context_reports_when_no_file_is_selected() {
        let mut app = App::new_for_test(vec![]);

        app.copy_context().unwrap();

        assert_eq!(app.watch_message.as_deref(), Some("No file selected"));
    }

    #[test]
    fn copy_context_reports_when_the_file_has_no_hunks() {
        let mut app = App::new_for_test(vec![make_file("a.rs", vec![])]);

        app.copy_context().unwrap();

        assert_eq!(app.watch_message.as_deref(), Some("No hunk selected"));
    }

    /// A hunk with one of each line kind, including a fold marker (which is
    /// counted but never rendered), plus an AI finding on the same hunk.
    #[cfg(target_os = "macos")]
    fn app_for_copy_context() -> App {
        let hunk = DiffHunk {
            header: "@@ -1,3 +1,4 @@".to_string(),
            old_start: 1,
            old_count: 3,
            new_start: 1,
            new_count: 4,
            lines: vec![
                make_line(LineType::Context, "ctx", Some(1)),
                make_line(LineType::Add, "added", Some(2)),
                make_line(LineType::Delete, "removed", None),
                make_line(LineType::Fold(12), "", None),
            ],
        };
        let mut app = App::new_for_test(vec![make_file("a.rs", vec![hunk])]);
        let mut f = finding("f-1", Some(0), Some(2));
        f.suggestion = "guard the unwrap".to_string();
        app.tab_mut().ai.review = Some(review_with(vec![("a.rs", ai::RiskLevel::High, vec![f])]));
        app
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn copy_context_copies_the_whole_hunk_when_navigating_by_hunk() {
        let mut app = app_for_copy_context();
        app.tab_mut().current_line = None;

        app.copy_context().unwrap();

        assert_eq!(
            app.watch_message.as_deref(),
            Some("Copied hunk (4 lines)"),
            "hunk-level navigation copies every line, fold marker included"
        );
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn copy_context_copies_only_the_cursor_line_when_navigating_by_line() {
        let mut app = app_for_copy_context();
        app.tab_mut().current_line = Some(1);

        app.copy_context().unwrap();

        assert_eq!(app.watch_message.as_deref(), Some("Copied line (1 lines)"));
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn copy_context_copies_the_shift_selected_range() {
        let mut app = app_for_copy_context();
        app.tab_mut().selection_anchor = Some(0);
        app.tab_mut().current_line = Some(2);

        app.copy_context().unwrap();

        assert_eq!(
            app.watch_message.as_deref(),
            Some("Copied selection (3 lines)")
        );
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn copy_context_falls_back_to_the_full_hunk_when_the_line_cursor_is_out_of_range() {
        let mut app = app_for_copy_context();
        app.tab_mut().current_line = Some(99);

        app.copy_context().unwrap();

        // The scope word still reads "line" (it is derived from `current_line`
        // alone), but the count shows the whole 4-line hunk was copied.
        assert_eq!(
            app.watch_message.as_deref(),
            Some("Copied line (4 lines)"),
            "an out-of-range line index degrades to the whole hunk"
        );
    }
}
