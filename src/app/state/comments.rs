use super::*;

impl App {
    // ── Comment System ──

    /// Enter comment mode for the current file + hunk (and optionally line)
    pub fn start_comment(&mut self, comment_type: CommentType) {
        let split_active = self.split_diff_active(&self.config.clone());
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
        self.input_mode = InputMode::Comment;
    }

    /// Start typing a general PR comment (not attached to any file/line)
    pub fn start_general_comment(&mut self) {
        let tab = self.tab_mut();
        tab.comment_textarea = TextArea::default();
        tab.comment_file = String::new();
        tab.comment_hunk = 0;
        tab.comment_line_num = None;
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
        let (text, is_question) = if comment_id.starts_with("q-") {
            if let Some(qs) = &tab.ai.questions {
                if let Some(q) = qs.questions.iter().find(|q| q.id == comment_id) {
                    (q.text.clone(), true)
                } else {
                    return;
                }
            } else {
                return;
            }
        } else if let Some(gc) = &tab.ai.github_comments {
            if let Some(c) = gc.comments.iter().find(|c| c.id == comment_id) {
                (c.comment.clone(), false)
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
        tab.comment_type = if is_question {
            CommentType::Question
        } else {
            CommentType::GitHubComment
        };
        tab.comment_edit_id = Some(comment_id.to_string());
        self.input_mode = InputMode::Comment;
    }

    /// Start replying to a comment or question — creates a threaded reply
    pub fn start_reply_comment(&mut self, comment_id: &str) {
        let tab = self.tab();
        // Determine type from ID prefix and find the parent comment's location
        let (file, hunk_index, line_start, is_question) = if comment_id.starts_with("q-") {
            if let Some(qs) = &tab.ai.questions {
                if let Some(q) = qs.questions.iter().find(|q| q.id == comment_id) {
                    (
                        q.file.clone(),
                        q.hunk_index.unwrap_or(0),
                        q.line_start,
                        true,
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
                    false,
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
        tab.comment_type = if is_question {
            CommentType::Question
        } else {
            CommentType::GitHubComment
        };
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
        let reply_to = tab.comment_reply_to.clone();
        let pr_head_ref_owned = tab.pr_head_ref.clone();
        let change_id = tab.jj_stack.get(tab.jj_selected).map(|e| e.change_id.clone());

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

        let seq = COMMENT_SEQ.fetch_add(1, Ordering::Relaxed);
        let id = format!(
            "q-{}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0),
            seq
        );

        let is_reply = reply_to.is_some();
        questions.questions.push(ai::ReviewQuestion {
            id,
            timestamp: chrono_now(),
            file: file_path,
            hunk_index: Some(hunk_index),
            line_start: anchor.line_start,
            line_content: anchor.line_content,
            text: text.clone(),
            resolved: false,
            stale: false,
            context_before: anchor.context_before,
            context_after: anchor.context_after,
            old_line_start: anchor.old_line_start,
            hunk_header: anchor.hunk_header,
            anchor_status: "original".to_string(),
            relocated_at_hash: self.tab().branch_diff_hash.clone(),
            in_reply_to: reply_to,
            author: "You".to_string(),
            change_id,
        });

        // Write atomically
        std::fs::create_dir_all(&er_dir)?;
        let json = serde_json::to_string_pretty(&questions)?;
        let tmp_path = format!("{}.tmp", questions_path);
        std::fs::write(&tmp_path, json)?;
        std::fs::rename(&tmp_path, &questions_path)?;

        self.tab_mut().comment_textarea = TextArea::default();
        self.input_mode = InputMode::Normal;
        self.tab_mut().reload_ai_state();
        let label = if is_reply { "Reply" } else { "Question" };
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
        let change_id = tab.jj_stack.get(tab.jj_selected).map(|e| e.change_id.clone());

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

        let seq = COMMENT_SEQ.fetch_add(1, Ordering::Relaxed);
        let id = format!(
            "c-{}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0),
            seq
        );

        let is_reply = reply_to.is_some();
        gh_comments.comments.push(ai::GitHubReviewComment {
            id,
            timestamp: chrono_now(),
            file: file_path,
            hunk_index: Some(hunk_index),
            line_start: anchor.line_start,
            line_end: None,
            line_content: anchor.line_content,
            comment: text.clone(),
            in_reply_to: reply_to,
            resolved: false,
            source: "local".to_string(),
            github_id: None,
            author: "You".to_string(),
            synced: false,
            stale: false,
            context_before: anchor.context_before,
            context_after: anchor.context_after,
            old_line_start: anchor.old_line_start,
            hunk_header: anchor.hunk_header,
            anchor_status: "original".to_string(),
            relocated_at_hash: self.tab().branch_diff_hash.clone(),
            finding_ref,
            change_id,
        });

        // Write atomically
        let comments_dir = self.tab().comments_dir();
        std::fs::create_dir_all(&comments_dir)?;
        let json = serde_json::to_string_pretty(&gh_comments)?;
        let tmp_path = format!("{}.tmp", comments_path);
        std::fs::write(&tmp_path, json)?;
        std::fs::rename(&tmp_path, &comments_path)?;

        self.tab_mut().comment_textarea = TextArea::default();
        self.input_mode = InputMode::Normal;
        let is_remote = self.tab().is_remote();
        if !is_remote {
            self.tab_mut().reload_ai_state();
        } else {
            // In remote mode, manually reload github comments from the cache file
            self.tab_mut().reload_remote_comments();
        }
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
        if let Some(df) = tab.selected_diff_file() {
            if let Some(hunk) = df.hunks.get(hunk_index) {
                if let Some(ln) = comment_line_num {
                    // Find the target line index within the hunk
                    let target_idx = hunk
                        .lines
                        .iter()
                        .position(|l| l.new_num == Some(ln))
                        .or_else(|| hunk.lines.iter().position(|l| l.old_num == Some(ln)));
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
                        q.hunk_header = anchor.hunk_header.clone();
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
                        c.hunk_header = anchor.hunk_header.clone();
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
            HintType::Question | HintType::GitHubComment => {
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

        // Determine which file this comment lives in
        let is_question = comment_id.starts_with("q-");

        if is_question {
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
            let checklist_path = format!("{}/.er/checklist.json", tab.repo_root);
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

    fn copy_to_clipboard(text: &str) -> Result<()> {
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
                    "Review done — but no .er/review.json found (agent may lack permissions)".into()
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
                    "Questions done — but no .er/questions.json found".into()
                }
            }
            _ => format!("{} done", name),
        }
    }

    /// Spawn a shell command in the background under the given name.
    /// The command string is run via `sh -c` in the repo root.
    /// Placeholders {base}, {branch}, {repo}, {output} are substituted.
    pub fn spawn_command(&mut self, name: &str, shell_cmd: &str) -> Result<()> {
        if self.tab().command_status.get(name) == Some(&CommandStatus::Running) {
            self.notify(&format!("{} already running", name));
            return Ok(());
        }

        let tab = self.tab();
        let repo_root = tab.repo_root.clone();
        let base = tab.base_branch.clone();
        let branch = tab.current_branch.clone();
        let output_path = format!("{}/.er/summary.md", repo_root);

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

        // Ensure .er/ directory exists
        let er_dir = std::path::Path::new(&repo_root).join(".er");
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
                    let summary_path = std::path::Path::new(&repo_root).join(".er/summary.md");
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
                            // Force AI reload for commands that write .er/ files
                            if name == "summary"
                                || name == "review"
                                || name == "questions"
                                || name == "quiz"
                                || name == "wizard"
                            {
                                tab.last_ai_check = None;
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
        self.sync_ai_selection();

        let (agent_cmd, config_args, is_claude_compatible) = if let Some(provider_id) = self
            .config
            .ai_hub
            .resolve_provider_id(self.current_ai_provider.as_deref())
        {
            let provider = self
                .config
                .ai_hub
                .providers
                .get(&provider_id)
                .ok_or_else(|| anyhow::anyhow!("Unknown AI provider: {}", provider_id))?;
            let mut args = provider.args.clone();
            if let Some(model_id) = self
                .config
                .ai_hub
                .resolve_model_id(&provider_id, self.current_ai_model.as_deref())
            {
                if let Some(model) = provider.models.iter().find(|m| m.id == model_id) {
                    args.extend(model.args.clone());
                }
            }
            let is_claude = provider.command.ends_with("claude") || provider.command == "claude";
            (provider.command.clone(), args, is_claude)
        } else {
            let cmd = self.config.agent.command.clone();
            let is_claude = cmd.ends_with("claude") || cmd == "claude";
            (cmd, self.config.agent.args.clone(), is_claude)
        };

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
                let debug_path =
                    std::path::Path::new(&er_dir_path).join(format!("debug-{}.log", name_owned));

                let mut agent_args: Vec<String> = config_args
                    .iter()
                    .map(|a| a.replace("{prompt}", &prompt_owned))
                    .collect();

                // Auto-inject --output-format stream-json for claude commands so the
                // agent log panel can show real-time tool calls and progress. This is
                // injected here (not in config defaults) so user configs that override
                // agent.args still get streaming without manual changes.
                if is_claude_compatible {
                    if !agent_args.iter().any(|a| a == "--output-format") {
                        agent_args.push("--output-format".to_string());
                        agent_args.push("stream-json".to_string());
                    }
                    // --verbose is required when combining --print with stream-json
                    let has_print = agent_args.iter().any(|a| a == "--print");
                    let has_stream = agent_args.iter().any(|a| a == "stream-json");
                    let has_verbose = agent_args.iter().any(|a| a == "--verbose");
                    if has_print && has_stream && !has_verbose {
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
                        "Bash(cp .er/*)",
                        "Bash(git diff*)",
                        "Bash(shasum*)",
                        "Bash(sha256sum*)",
                        "Bash(mkdir*)",
                    ];
                    for rule in allowed.iter().rev() {
                        agent_args.insert(0, rule.to_string());
                        agent_args.insert(0, "--allowedTools".to_string());
                    }
                }

                // In remote mode, run from the cache dir so relative paths resolve there.
                // The agent fetches the diff via `gh` — no local repo access needed.
                let work_dir = if is_remote { &er_dir_path } else { &repo_root };
                let mut child = std::process::Command::new(&agent_cmd)
                    .args(&agent_args)
                    .current_dir(work_dir)
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped())
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
                            let display = if is_claude_compatible {
                                parse_stream_json_line(&line)
                            } else {
                                Some(truncate_str(line.trim(), 120))
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
                    status.code().map_or("signal".to_string(), |c| c.to_string()),
                    stdout_lines.join("\n"),
                    stderr_lines.join("\n"),
                );
                let _ = std::fs::write(&debug_path, &debug_content);

                if !status.success() {
                    anyhow::bail!("{} failed (see .er/debug-{}.log)", name_owned, name_owned);
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
}
