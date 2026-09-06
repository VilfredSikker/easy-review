use super::*;

impl TabState {
    pub fn next_file(&mut self) {
        self.focused_comment_id = None;
        self.focused_finding_id = None;
        if self.mode == DiffMode::History {
            self.history_next_file();
            return;
        }
        if let Some(idx) = self.selected_watched {
            // In watched section — move down within watched files
            let visible_watched = self.visible_watched_files();
            if let Some(pos) = visible_watched.iter().position(|(i, _)| *i == idx) {
                if pos + 1 < visible_watched.len() {
                    self.selected_watched = Some(visible_watched[pos + 1].0);
                    self.diff_scroll = 0;
                    self.h_scroll = 0;
                } else {
                    // At last watched file — wrap to first diff file
                    self.selected_watched = None;
                    let visible = self.visible_files();
                    if !visible.is_empty() {
                        self.selected_file = visible[0].0;
                        self.current_hunk = 0;
                        self.current_line = None;
                        self.selection_anchor = None;
                        self.diff_scroll = 0;
                        self.h_scroll = 0;
                        self.panel_scroll = 0;
                        self.ensure_file_parsed();
                        self.rebuild_hunk_offsets();
                    }
                }
            }
        } else {
            // In diff section
            let visible = self.visible_files();
            if visible.is_empty() {
                // No diff files — jump to watched if available
                let visible_watched = self.visible_watched_files();
                if !visible_watched.is_empty() {
                    self.selected_watched = Some(visible_watched[0].0);
                    self.diff_scroll = 0;
                    self.h_scroll = 0;
                }
                return;
            }
            if let Some(pos) = visible.iter().position(|(i, _)| *i == self.selected_file) {
                if pos + 1 < visible.len() {
                    self.selected_file = visible[pos + 1].0;
                    self.current_hunk = 0;
                    self.current_line = None;
                    self.selection_anchor = None;
                    self.diff_scroll = 0;
                    self.h_scroll = 0;
                    self.panel_scroll = 0;
                    self.ensure_file_parsed();
                    self.rebuild_hunk_offsets();
                } else {
                    // At last diff file
                    let visible_watched = self.visible_watched_files();
                    if !visible_watched.is_empty() {
                        // Transition to watched section
                        self.selected_watched = Some(visible_watched[0].0);
                        self.diff_scroll = 0;
                        self.h_scroll = 0;
                    } else {
                        // Wrap to first diff file
                        self.selected_file = visible[0].0;
                        self.current_hunk = 0;
                        self.current_line = None;
                        self.selection_anchor = None;
                        self.diff_scroll = 0;
                        self.h_scroll = 0;
                        self.panel_scroll = 0;
                        self.ensure_file_parsed();
                        self.rebuild_hunk_offsets();
                    }
                }
            } else {
                // Current selection not in visible set — snap to first
                self.selected_file = visible[0].0;
                self.current_hunk = 0;
                self.diff_scroll = 0;
                self.h_scroll = 0;
                self.panel_scroll = 0;
                self.ensure_file_parsed();
                self.rebuild_hunk_offsets();
            }
        }
    }

    pub fn prev_file(&mut self) {
        self.focused_comment_id = None;
        self.focused_finding_id = None;
        if self.mode == DiffMode::History {
            self.history_prev_file();
            return;
        }
        if let Some(idx) = self.selected_watched {
            // In watched section — move up within watched files
            let visible_watched = self.visible_watched_files();
            if let Some(pos) = visible_watched.iter().position(|(i, _)| *i == idx) {
                if pos > 0 {
                    self.selected_watched = Some(visible_watched[pos - 1].0);
                    self.diff_scroll = 0;
                    self.h_scroll = 0;
                } else {
                    // At first watched file — transition back to diff section
                    self.selected_watched = None;
                    let visible = self.visible_files();
                    if !visible.is_empty() {
                        self.selected_file = visible.last().unwrap().0;
                        self.current_hunk = 0;
                        self.current_line = None;
                        self.selection_anchor = None;
                        self.diff_scroll = 0;
                        self.h_scroll = 0;
                        self.panel_scroll = 0;
                        self.ensure_file_parsed();
                        self.rebuild_hunk_offsets();
                    }
                }
            }
        } else {
            // In diff section — normal navigation
            let visible = self.visible_files();
            if visible.is_empty() {
                return;
            }
            if let Some(pos) = visible.iter().position(|(i, _)| *i == self.selected_file) {
                if pos > 0 {
                    self.selected_file = visible[pos - 1].0;
                    self.current_hunk = 0;
                    self.current_line = None;
                    self.selection_anchor = None;
                    self.diff_scroll = 0;
                    self.h_scroll = 0;
                    self.panel_scroll = 0;
                    self.ensure_file_parsed();
                    self.rebuild_hunk_offsets();
                } else {
                    // At first diff file — wrap to last item
                    let visible_watched = self.visible_watched_files();
                    if !visible_watched.is_empty() {
                        self.selected_watched = Some(visible_watched.last().unwrap().0);
                        self.diff_scroll = 0;
                        self.h_scroll = 0;
                    } else {
                        // Wrap to last diff file
                        self.selected_file = visible.last().unwrap().0;
                        self.current_hunk = 0;
                        self.current_line = None;
                        self.selection_anchor = None;
                        self.diff_scroll = 0;
                        self.h_scroll = 0;
                        self.panel_scroll = 0;
                        self.ensure_file_parsed();
                        self.rebuild_hunk_offsets();
                    }
                }
            } else {
                // Current selection not in visible set — snap to first
                self.selected_file = visible[0].0;
                self.current_hunk = 0;
                self.diff_scroll = 0;
                self.h_scroll = 0;
                self.panel_scroll = 0;
                self.ensure_file_parsed();
                self.rebuild_hunk_offsets();
            }
        }
    }

    pub fn next_hunk(&mut self) {
        self.focused_comment_id = None;
        self.focused_finding_id = None;
        let total = self.total_hunks();
        if total > 0 && self.current_hunk < total - 1 {
            self.current_hunk += 1;
            self.current_line = None;
            self.selection_anchor = None;
            self.scroll_to_current_hunk();
        }
    }

    pub fn prev_hunk(&mut self) {
        self.focused_comment_id = None;
        self.focused_finding_id = None;
        if self.current_hunk > 0 {
            self.current_hunk -= 1;
            self.current_line = None;
            self.selection_anchor = None;
            self.scroll_to_current_hunk();
        }
    }

    /// Returns true if the line at `idx` in the current hunk is a Fold marker.
    fn is_fold_line(&self, idx: usize) -> bool {
        self.selected_diff_file()
            .and_then(|f| f.hunks.get(self.current_hunk))
            .and_then(|h| h.lines.get(idx))
            .map(|l| matches!(l.line_type, crate::git::LineType::Fold(_)))
            .unwrap_or(false)
    }

    /// Move to the next line within the current hunk (arrow down)
    pub fn next_line(&mut self) {
        self.selection_anchor = None;
        let total_lines = self.current_hunk_line_count();
        if total_lines == 0 {
            return;
        }
        match self.current_line {
            None => {
                // Find the first non-Fold line
                let mut idx = 0;
                while idx < total_lines && self.is_fold_line(idx) {
                    idx += 1;
                }
                self.current_line = if idx < total_lines { Some(idx) } else { None };
                self.scroll_to_current_hunk();
            }
            Some(line) => {
                if line + 1 < total_lines {
                    // Skip Fold lines forward
                    let mut next = line + 1;
                    while next < total_lines && self.is_fold_line(next) {
                        next += 1;
                    }
                    if next < total_lines {
                        self.current_line = Some(next);
                        self.scroll_to_current_hunk();
                    } else {
                        // Reached end of hunk — move to next hunk
                        let total_hunks = self.total_hunks();
                        if self.current_hunk + 1 < total_hunks {
                            self.current_hunk += 1;
                            self.current_line = Some(0);
                            self.scroll_to_current_hunk();
                        }
                    }
                } else {
                    let total_hunks = self.total_hunks();
                    if self.current_hunk + 1 < total_hunks {
                        self.current_hunk += 1;
                        self.current_line = Some(0);
                        self.scroll_to_current_hunk();
                    }
                }
            }
        }
    }

    /// Move to the previous line within the current hunk (arrow up)
    pub fn prev_line(&mut self) {
        self.selection_anchor = None;
        match self.current_line {
            None => {
                // Enter line mode at the last non-Fold line of the current hunk
                let count = self.current_hunk_line_count();
                if count > 0 {
                    let mut idx = count - 1;
                    while idx > 0 && self.is_fold_line(idx) {
                        idx -= 1;
                    }
                    self.current_line = if !self.is_fold_line(idx) {
                        Some(idx)
                    } else {
                        None
                    };
                    self.scroll_to_current_hunk();
                }
            }
            Some(0) => {
                if self.current_hunk > 0 {
                    self.current_hunk -= 1;
                    let count = self.current_hunk_line_count();
                    if count > 0 {
                        let mut idx = count - 1;
                        while idx > 0 && self.is_fold_line(idx) {
                            idx -= 1;
                        }
                        self.current_line = if !self.is_fold_line(idx) {
                            Some(idx)
                        } else {
                            None
                        };
                    } else {
                        self.current_line = None;
                    }
                    self.scroll_to_current_hunk();
                } else {
                    self.current_line = None;
                }
            }
            Some(line) => {
                // Skip Fold lines backward
                let mut prev = line - 1;
                while prev > 0 && self.is_fold_line(prev) {
                    prev -= 1;
                }
                self.current_line = if !self.is_fold_line(prev) {
                    Some(prev)
                } else {
                    // All lines above are Folds — exit line mode
                    None
                };
                self.scroll_to_current_hunk();
            }
        }
    }

    /// Get the number of lines in the current hunk
    pub fn current_hunk_line_count(&self) -> usize {
        self.selected_diff_file()
            .and_then(|f| f.hunks.get(self.current_hunk))
            .map(|h| h.lines.len())
            .unwrap_or(0)
    }

    /// Get the line number for the currently selected diff line.
    /// Prefers `new_num`; falls back to `old_num` for delete-only lines so comments
    /// anchor inline on removed code (e.g. full-file deletes).
    pub fn current_line_number(&self) -> Option<usize> {
        let file = self.selected_diff_file()?;
        let hunk = file.hunks.get(self.current_hunk)?;
        let line_idx = self.current_line?;
        let diff_line = hunk.lines.get(line_idx)?;
        diff_line.new_num.or(diff_line.old_num)
    }

    /// GitHub/question/note `side` for the current cursor line.
    ///
    /// Deleted lines (no new number) and split-view old-side focus store `"LEFT"`.
    /// Everything else is `"RIGHT"` (GitHub default).
    pub fn comment_side_for_cursor(&self, split_active: bool) -> String {
        if split_active && self.split_focus == SplitSide::Old {
            return "LEFT".to_string();
        }
        let Some(file) = self.selected_diff_file() else {
            return "RIGHT".to_string();
        };
        let Some(hunk) = file.hunks.get(self.current_hunk) else {
            return "RIGHT".to_string();
        };
        let Some(line_idx) = self.current_line else {
            return "RIGHT".to_string();
        };
        let Some(diff_line) = hunk.lines.get(line_idx) else {
            return "RIGHT".to_string();
        };
        if diff_line.new_num.is_none() {
            "LEFT".to_string()
        } else {
            "RIGHT".to_string()
        }
    }

    /// Get the line number for the focused side in split diff view
    pub fn current_line_number_for_split(&self, side: SplitSide) -> Option<usize> {
        let file = self.selected_diff_file()?;
        let hunk = file.hunks.get(self.current_hunk)?;
        let line_idx = self.current_line?;
        let diff_line = hunk.lines.get(line_idx)?;
        match side {
            SplitSide::Old => diff_line.old_num.or(diff_line.new_num),
            SplitSide::New => diff_line.new_num.or(diff_line.old_num),
        }
    }

    /// Increment the focused pane's horizontal scroll in split diff view
    pub const fn scroll_right_split(&mut self) {
        match self.split_focus {
            SplitSide::Old => self.h_scroll_old = self.h_scroll_old.saturating_add(1),
            SplitSide::New => self.h_scroll_new = self.h_scroll_new.saturating_add(1),
        }
    }

    /// Decrement the focused pane's horizontal scroll in split diff view
    pub const fn scroll_left_split(&mut self) {
        match self.split_focus {
            SplitSide::Old => self.h_scroll_old = self.h_scroll_old.saturating_sub(1),
            SplitSide::New => self.h_scroll_new = self.h_scroll_new.saturating_sub(1),
        }
    }

    /// Get the selected line range within the current hunk (from shift+arrow selection)
    pub fn selected_range(&self) -> Option<std::ops::RangeInclusive<usize>> {
        let anchor = self.selection_anchor?;
        let current = self.current_line?;
        Some(anchor.min(current)..=anchor.max(current))
    }

    pub fn scroll_to_current_hunk(&mut self) {
        // Use precomputed hunk offsets if available (O(1) lookup)
        if let Some(ref offsets) = self.hunk_offsets {
            if let Some(&base) = offsets.offsets.get(self.current_hunk) {
                let line_offset = base + self.current_line.unwrap_or(0);
                self.diff_scroll = line_offset.saturating_sub(1).min(u16::MAX as usize) as u16;
                return;
            }
        }
        // Fallback: compute from hunks (for Overlay mode where offsets are approximate)
        if let Some(file) = self.selected_diff_file() {
            let mut line_offset: usize = 2;
            for (i, hunk) in file.hunks.iter().enumerate() {
                if i == self.current_hunk {
                    line_offset += self.current_line.unwrap_or(0);
                    self.diff_scroll = line_offset.saturating_sub(1).min(u16::MAX as usize) as u16;
                    return;
                }
                line_offset += 1 + hunk.lines.len() + 1;
            }
        }
    }

    pub fn scroll_down(&mut self, amount: u16) {
        self.diff_scroll = self.diff_scroll.saturating_add(amount);
        self.sync_cursor_to_scroll();
    }

    pub fn scroll_up(&mut self, amount: u16) {
        self.diff_scroll = self.diff_scroll.saturating_sub(amount);
        self.sync_cursor_to_scroll();
    }

    pub const fn panel_scroll_down(&mut self, amount: u16) {
        self.panel_scroll = self.panel_scroll.saturating_add(amount);
    }

    pub const fn panel_scroll_up(&mut self, amount: u16) {
        self.panel_scroll = self.panel_scroll.saturating_sub(amount);
    }

    /// Move the cursor (current_hunk + current_line) to match the current
    /// diff_scroll position.  Uses the same layout model as the renderer:
    /// 2 header lines, then per hunk: 1 header + N content lines + 1 blank.
    fn sync_cursor_to_scroll(&mut self) {
        // Compute target (hunk, line) from the scroll offset without
        // holding a borrow across the mutation.
        let result = {
            let file = match self.selected_diff_file() {
                Some(f) => f,
                None => return,
            };
            if file.hunks.is_empty() {
                return;
            }

            let target = self.diff_scroll as usize;
            let mut offset: usize = 2; // file header + blank

            let mut found: Option<(usize, usize)> = None;
            for (i, hunk) in file.hunks.iter().enumerate() {
                offset += 1; // hunk header line
                let content_start = offset;
                let content_end = offset + hunk.lines.len();

                if target < content_end {
                    let line_idx = target.saturating_sub(content_start);
                    found = Some((i, line_idx));
                    break;
                }

                offset = content_end + 1; // blank line after hunk
            }

            found.unwrap_or_else(|| {
                // Past the end — clamp to last line of last hunk
                match file.hunks.last() {
                    Some(hunk) => (file.hunks.len() - 1, hunk.lines.len().saturating_sub(1)),
                    None => (0, 0),
                }
            })
        };

        self.current_hunk = result.0;
        self.current_line = Some(result.1);
    }

    pub const fn scroll_right(&mut self, amount: u16) {
        self.h_scroll = self.h_scroll.saturating_add(amount);
    }

    pub const fn scroll_left(&mut self, amount: u16) {
        self.h_scroll = self.h_scroll.saturating_sub(amount);
    }

    // ── Performance helpers ──

    /// Rebuild hunk offsets for the currently selected file
    pub fn rebuild_hunk_offsets(&mut self) {
        self.hunk_offsets = self
            .selected_diff_file()
            .map(|f| HunkOffsets::build(&f.hunks));
    }

    /// Update memory budget counters
    pub fn update_mem_budget(&mut self) {
        let mut total_lines = 0usize;
        let mut compacted = 0usize;
        let mut parsed = 0usize;
        for file in &self.files {
            if file.compacted {
                compacted += 1;
            } else {
                parsed += 1;
                total_lines += file.hunks.iter().map(|h| h.lines.len()).sum::<usize>();
            }
        }
        self.mem_budget = MemoryBudget {
            parsed_files: parsed,
            total_lines,
            compacted_files: compacted,
        };
    }

    /// In lazy mode, ensure the currently selected file has its hunks parsed.
    /// No-op in eager mode or if already parsed.
    pub fn ensure_file_parsed(&mut self) {
        let idx = self.selected_file;
        self.ensure_file_parsed_at(idx);
    }

    /// Parse a specific file by its index in `self.files` without changing navigation state.
    /// Used to pre-load in-viewport lazy stubs without disturbing the selection.
    pub fn ensure_file_parsed_at(&mut self, index: usize) {
        if !self.lazy_mode {
            return;
        }
        if let Some(file) = self.files.get(index) {
            // Already parsed (has hunks) or is compacted — skip
            if !file.hunks.is_empty() || file.compacted {
                return;
            }
        }
        // Parse on demand from raw diff — look up header by path (not index) to handle mtime sort
        let path = self.files.get(index).map(|f| f.path.clone());
        let header_idx = path
            .as_ref()
            .and_then(|p| self.file_headers.iter().position(|h| h.path == *p));
        if let (Some(ref raw), Some(idx)) = (&self.raw_diff, header_idx) {
            let header = &self.file_headers[idx];
            let parsed = git::parse_file_at_offset(raw, header);
            if !parsed.hunks.is_empty() {
                if let Some(file) = self.files.get_mut(index) {
                    file.hunks = parsed.hunks;
                    file.adds = parsed.adds;
                    file.dels = parsed.dels;
                }
                self.rebuild_hunk_offsets();
                self.update_mem_budget();
                return;
            }
        }
        // Fallback: offset parse returned no hunks but file has changes — fetch from git directly
        // Skip git fallback in remote mode — raw_diff is our only source
        if !self.is_remote() {
            if let Some(file) = self.files.get(index) {
                if file.adds + file.dels > 0 {
                    let path = file.path.clone();
                    let repo_root = self.repo_root.clone();
                    let mode = self.mode.git_mode().to_string();
                    let base = self.base_branch.clone();
                    let head_ref_owned = self.pr_head_ref.clone();
                    if let Ok(raw) = git::git_diff_raw_file(
                        &mode,
                        &base,
                        &repo_root,
                        &path,
                        None,
                        head_ref_owned.as_deref(),
                    ) {
                        let parsed = git::parse_diff(&raw);
                        if let Some(p) = parsed.into_iter().next() {
                            if let Some(file) = self.files.get_mut(index) {
                                file.hunks = p.hunks;
                                file.adds = p.adds;
                                file.dels = p.dels;
                            }
                        }
                    }
                }
            }
        }
        self.rebuild_hunk_offsets();
        self.update_mem_budget();
    }

    /// Toggle expand/compact for the currently selected file.
    /// If compacted, expand by re-fetching from git.
    /// If expanded (and was compacted), re-compact it.
    pub fn toggle_compacted(&mut self) -> Result<()> {
        let is_remote = self.is_remote();
        let is_compacted = self
            .files
            .get(self.selected_file)
            .is_some_and(|f| f.compacted);
        if is_compacted {
            let path = self.files[self.selected_file].path.clone();
            // Local PR mode uses gh pr diff for the full load, so pr_head_ref is never
            // fetched into the local clone. Treat it like remote when raw_diff is available;
            // fetch the ref on demand when it isn't (e.g. pattern-compacted file in a small diff).
            let use_raw_diff = is_remote || (self.pr_number.is_some() && self.raw_diff.is_some());
            if use_raw_diff {
                // Remote / local-PR with cached diff: re-parse from raw_diff
                let header_idx = self.file_headers.iter().position(|h| h.path == path);
                if let Some(raw) = self.raw_diff.clone() {
                    if let Some(idx) = header_idx {
                        let header = self.file_headers[idx].clone();
                        let parsed = git::parse_file_at_offset(&raw, &header);
                        if let Some(file) = self.files.get_mut(self.selected_file) {
                            file.hunks = parsed.hunks;
                            file.adds = parsed.adds;
                            file.dels = parsed.dels;
                            file.compacted = false;
                        }
                    }
                }
            } else {
                // Extract values before mutable borrow of files
                let repo_root = self.repo_root.clone();
                let git_mode = self.mode.git_mode().to_string();
                let base_branch = self.base_branch.clone();
                let head_ref_owned = self.pr_head_ref.clone();
                git::expand_compacted_file(
                    &mut self.files[self.selected_file],
                    &repo_root,
                    &git_mode,
                    &base_branch,
                    head_ref_owned.as_deref(),
                )?;
            }
            self.user_expanded.insert(path);
            self.rebuild_hunk_offsets();
            self.update_mem_budget();
        } else if let Some(file) = self.files.get_mut(self.selected_file) {
            // Re-compact: only if it matched a pattern or was large
            // Enter re-compacts any expanded file, even one that never matched a
            // compaction pattern in the first place.
            let path = file.path.clone();
            file.compacted = true;
            file.raw_hunk_count = file.hunks.len();
            file.hunks.clear();
            file.hunks.shrink_to_fit();
            self.user_expanded.remove(&path);
            self.current_hunk = 0;
            self.current_line = None;
            self.diff_scroll = 0;
            self.hunk_offsets = None;
            self.update_mem_budget();
        }
        Ok(())
    }

    /// Expand context lines for the currently selected file.
    /// Steps through increasing levels per `git::CONTEXT_STEPS`.
    /// If the file is compacted, expands it first.
    pub fn expand_context(&mut self) -> Result<()> {
        // History mode not supported (would need per-file commit diff)
        if self.mode == DiffMode::History {
            return Ok(());
        }

        let file = match self.files.get(self.selected_file) {
            Some(f) => f,
            None => return Ok(()),
        };

        // If compacted, expand it first (same as Enter)
        if file.compacted {
            return self.toggle_compacted();
        }

        // Untracked files (Added status with synthetic diff) are already full-file
        if file.status == git::FileStatus::Added && file.hunks.len() <= 1 {
            return Ok(());
        }

        let path = file.path.clone();
        let current = self
            .context_overrides
            .get(&path)
            .copied()
            .unwrap_or(git::DEFAULT_CONTEXT_LINES);

        // Find next step above current
        let next = git::CONTEXT_STEPS
            .iter()
            .copied()
            .find(|&s| s > current)
            .unwrap_or(git::FULL_CONTEXT);
        if next == current {
            return Ok(());
        }

        // Re-fetch the file diff with new context
        if let Some(file) = self.files.get_mut(self.selected_file) {
            git::refetch_file_with_context(
                file,
                &self.repo_root,
                self.mode.git_mode(),
                &self.base_branch,
                next,
                self.pr_head_ref.as_deref(),
            )?;
        }
        self.context_overrides.insert(path, next);
        self.rebuild_hunk_offsets();
        self.update_mem_budget();
        Ok(())
    }

    /// Collapse context lines for the currently selected file.
    /// Steps back through context levels: full → 80 → 40 → 20 → 10.
    pub fn collapse_context(&mut self) -> Result<()> {
        if self.mode == DiffMode::History {
            return Ok(());
        }

        let file = match self.files.get(self.selected_file) {
            Some(f) => f,
            None => return Ok(()),
        };

        if file.compacted {
            return Ok(());
        }

        let path = file.path.clone();
        let current = self
            .context_overrides
            .get(&path)
            .copied()
            .unwrap_or(git::DEFAULT_CONTEXT_LINES);

        if current <= git::DEFAULT_CONTEXT_LINES {
            return Ok(());
        }

        // Find previous step below current
        let prev = git::CONTEXT_STEPS
            .iter()
            .rev()
            .copied()
            .find(|&s| s < current)
            .unwrap_or(git::DEFAULT_CONTEXT_LINES);

        if let Some(file) = self.files.get_mut(self.selected_file) {
            git::refetch_file_with_context(
                file,
                &self.repo_root,
                self.mode.git_mode(),
                &self.base_branch,
                prev,
                self.pr_head_ref.as_deref(),
            )?;
        }

        if prev == git::DEFAULT_CONTEXT_LINES {
            self.context_overrides.remove(&path);
        } else {
            self.context_overrides.insert(path, prev);
        }
        self.rebuild_hunk_offsets();
        self.update_mem_budget();
        Ok(())
    }

    /// Auto-pick unified context for the selected file based on its size.
    /// Smaller files get more context, larger files stay lean. Bigger files
    /// keep the default `-U=3` so memory and parse time stay bounded.
    /// Files with many small hunks are bumped one tier higher (extra context
    /// helps scattered hunks merge into a readable block).
    /// Pass `enabled = 0` to disable; any non-zero value enables the ladder.
    pub fn maybe_auto_expand_context(&mut self, enabled: usize) {
        if enabled == 0 || self.mode == DiffMode::History {
            return;
        }

        let file = match self.files.get(self.selected_file) {
            Some(f) => f,
            None => return,
        };

        if file.compacted || self.context_overrides.contains_key(&file.path) {
            return;
        }
        if file.status == git::FileStatus::Added && file.hunks.len() <= 1 {
            return;
        }

        let total_lines: usize = file.hunks.iter().map(|h| h.lines.len()).sum();
        if total_lines == 0 {
            return;
        }

        let hunks = file.hunks.len();
        let picked = pick_context_for_size(total_lines, hunks);
        if picked <= git::DEFAULT_CONTEXT_LINES {
            return;
        }

        let path = file.path.clone();
        if let Some(file) = self.files.get_mut(self.selected_file) {
            if git::refetch_file_with_context(
                file,
                &self.repo_root,
                self.mode.git_mode(),
                &self.base_branch,
                picked,
                self.pr_head_ref.as_deref(),
            )
            .is_ok()
            {
                self.context_overrides.insert(path, picked);
                self.rebuild_hunk_offsets();
                self.update_mem_budget();
            }
        }
    }

    // ── Editor ──

    pub fn open_in_editor(&self) -> Result<()> {
        let file = match self.selected_diff_file() {
            Some(f) => f,
            None => return Ok(()),
        };

        let editor = std::env::var("EDITOR").unwrap_or_else(|_| "code".to_string());
        let file_path = std::path::Path::new(&self.repo_root).join(&file.path);
        let line_num = file
            .hunks
            .get(self.current_hunk)
            .map(|h| h.new_start)
            .unwrap_or(1);

        let mut cmd = std::process::Command::new(&editor);
        if editor.contains("code") || editor.contains("cursor") {
            cmd.arg(&self.repo_root)
                .arg("-g")
                .arg(format!("{}:{}", file_path.display(), line_num));
        } else if editor.contains("zed") {
            cmd.arg(&self.repo_root)
                .arg(format!("{}:{}", file_path.display(), line_num));
        } else {
            cmd.arg(format!("+{}", line_num)).arg(&file_path);
        }

        cmd.spawn().context("Failed to open editor")?;
        Ok(())
    }

    // ── History Mode Navigation ──

    /// Move to the next commit in history (older)
    pub fn history_next_commit(&mut self) {
        let history = match self.history.as_mut() {
            Some(h) => h,
            None => return,
        };
        if history.selected_commit + 1 < history.commits.len() {
            history.selected_commit += 1;
            self.history_load_selected_diff();
        }
    }

    /// Move to the previous commit in history (newer)
    pub fn history_prev_commit(&mut self) {
        let history = match self.history.as_mut() {
            Some(h) => h,
            None => return,
        };
        if history.selected_commit > 0 {
            history.selected_commit -= 1;
            self.history_load_selected_diff();
        }
    }

    /// Load the diff for the currently selected commit
    pub fn history_load_selected_diff(&mut self) {
        let (hash, repo_root) = {
            let history = match self.history.as_mut() {
                Some(h) => h,
                None => return,
            };
            let commit_hash = match history.commits.get(history.selected_commit) {
                Some(c) => c.hash.clone(),
                None => return,
            };
            // Check cache first (promotes to MRU on access)
            if let Some(cached) = history.diff_cache.get(&commit_hash) {
                let files = cached.clone();
                history.commit_files = files;
                history.selected_file = 0;
                history.current_hunk = 0;
                history.current_line = None;
                history.diff_scroll = 0;
                history.h_scroll = 0;
                return;
            }
            (commit_hash, self.repo_root.clone())
        };

        let raw_diff = match git::git_diff_commit(&hash, &repo_root) {
            Ok(raw) => Ok(raw),
            Err(first_err)
                if self.pr_number.is_some()
                    && self.local_branch_view.is_some()
                    && !self.is_remote() =>
            {
                if let Some(pr_number) = self.pr_number {
                    if let Ok(head_ref) = crate::github::fetch_pr_head(pr_number, &self.repo_root) {
                        self.pr_head_ref = Some(head_ref);
                        self.pr_refs_fetched = true;
                    }
                }
                git::git_diff_commit(&hash, &repo_root).map_err(|_| first_err)
            }
            Err(err) => Err(err),
        };
        let files = match raw_diff {
            Ok(raw) => git::parse_diff(&raw),
            Err(err) => {
                eprintln!("Failed to load commit diff for {hash}: {err}");
                Vec::new()
            }
        };

        let history = match self.history.as_mut() {
            Some(h) => h,
            None => return,
        };
        history.diff_cache.insert(hash, files.clone());
        history.commit_files = files;
        history.selected_file = 0;
        history.current_hunk = 0;
        history.current_line = None;
        history.diff_scroll = 0;
        history.h_scroll = 0;
    }

    /// Select a file within the current commit diff (History mode).
    pub fn history_select_file(&mut self, idx: usize) {
        let history = match self.history.as_mut() {
            Some(h) => h,
            None => return,
        };
        if idx >= history.commit_files.len() {
            return;
        }
        history.selected_file = idx;
        history.current_hunk = 0;
        history.current_line = None;
        Self::history_scroll_to_file(history);
    }

    /// Move to next file within the selected commit's diff
    pub fn history_next_file(&mut self) {
        let history = match self.history.as_mut() {
            Some(h) => h,
            None => return,
        };
        if history.commit_files.is_empty() {
            return;
        }
        if history.selected_file + 1 < history.commit_files.len() {
            history.selected_file += 1;
            history.current_hunk = 0;
            history.current_line = None;
            Self::history_scroll_to_file(history);
        }
    }

    /// Move to previous file within the selected commit's diff
    pub fn history_prev_file(&mut self) {
        let history = match self.history.as_mut() {
            Some(h) => h,
            None => return,
        };
        if history.selected_file > 0 {
            history.selected_file -= 1;
            history.current_hunk = 0;
            history.current_line = None;
            Self::history_scroll_to_file(history);
        }
    }

    /// Move to next line within the commit diff
    pub fn history_next_line(&mut self) {
        let history = match self.history.as_mut() {
            Some(h) => h,
            None => return,
        };
        let file = match history.commit_files.get(history.selected_file) {
            Some(f) => f,
            None => return,
        };
        let hunk_count = file.hunks.len();
        let line_count = file
            .hunks
            .get(history.current_hunk)
            .map(|h| h.lines.len())
            .unwrap_or(0);

        match history.current_line {
            None => {
                if line_count > 0 {
                    history.current_line = Some(0);
                    Self::history_scroll_to_current(history);
                }
            }
            Some(line) => {
                if line + 1 < line_count {
                    history.current_line = Some(line + 1);
                    Self::history_scroll_to_current(history);
                } else if history.current_hunk + 1 < hunk_count {
                    // Move to next hunk's first line
                    history.current_hunk += 1;
                    history.current_line = Some(0);
                    Self::history_scroll_to_current(history);
                } else if history.selected_file + 1 < history.commit_files.len() {
                    // Move to next file's first hunk's first line
                    history.selected_file += 1;
                    history.current_hunk = 0;
                    history.current_line = Some(0);
                    Self::history_scroll_to_current(history);
                }
            }
        }
    }

    /// Move to previous line within the commit diff
    pub fn history_prev_line(&mut self) {
        let history = match self.history.as_mut() {
            Some(h) => h,
            None => return,
        };
        let file = match history.commit_files.get(history.selected_file) {
            Some(f) => f,
            None => return,
        };

        match history.current_line {
            None => {
                let count = file
                    .hunks
                    .get(history.current_hunk)
                    .map(|h| h.lines.len())
                    .unwrap_or(0);
                if count > 0 {
                    history.current_line = Some(count - 1);
                    Self::history_scroll_to_current(history);
                }
            }
            Some(0) => {
                if history.current_hunk > 0 {
                    history.current_hunk -= 1;
                    let count = file
                        .hunks
                        .get(history.current_hunk)
                        .map(|h| h.lines.len())
                        .unwrap_or(0);
                    history.current_line = if count > 0 { Some(count - 1) } else { None };
                    Self::history_scroll_to_current(history);
                } else if history.selected_file > 0 {
                    // Move to prev file's last hunk's last line
                    history.selected_file -= 1;
                    let prev_file = &history.commit_files[history.selected_file];
                    if let Some(last_hunk) = prev_file.hunks.last() {
                        history.current_hunk = prev_file.hunks.len() - 1;
                        history.current_line = if last_hunk.lines.is_empty() {
                            None
                        } else {
                            Some(last_hunk.lines.len() - 1)
                        };
                    } else {
                        history.current_hunk = 0;
                        history.current_line = None;
                    }
                    Self::history_scroll_to_current(history);
                } else {
                    history.current_line = None;
                }
            }
            Some(line) => {
                history.current_line = Some(line - 1);
                Self::history_scroll_to_current(history);
            }
        }
    }

    /// Scroll to the current file header in history mode
    fn history_scroll_to_file(history: &mut HistoryState) {
        let mut line_offset: usize = 0;
        for (file_idx, file) in history.commit_files.iter().enumerate() {
            if file_idx == history.selected_file {
                history.diff_scroll = line_offset.min(u16::MAX as usize) as u16;
                return;
            }
            // File header (1) + blank line (1) + per-hunk (header + lines + blank)
            line_offset += 2; // header + blank
            for hunk in &file.hunks {
                line_offset += 1 + hunk.lines.len() + 1; // header + lines + blank
            }
        }
    }

    /// Scroll to the current line position in history mode
    fn history_scroll_to_current(history: &mut HistoryState) {
        let mut line_offset: usize = 0;
        for (file_idx, file) in history.commit_files.iter().enumerate() {
            line_offset += 2; // file header + blank
            for (hunk_idx, hunk) in file.hunks.iter().enumerate() {
                if file_idx == history.selected_file && hunk_idx == history.current_hunk {
                    line_offset += history.current_line.unwrap_or(0);
                    history.diff_scroll =
                        line_offset.saturating_sub(1).min(u16::MAX as usize) as u16;
                    return;
                }
                line_offset += 1 + hunk.lines.len() + 1;
            }
        }
    }

    /// Load more commits when scrolling past the end
    pub fn history_load_more(&mut self) {
        let (skip, all_loaded) = match self.history.as_ref() {
            Some(h) => (h.commits.len(), h.all_loaded),
            None => return,
        };
        if all_loaded {
            return;
        }
        if self.pr_number.is_some() && self.local_branch_view.is_some() {
            if let Some(history) = self.history.as_mut() {
                history.all_loaded = true;
            }
            return;
        }

        // Runs synchronously on the event loop thread — a slow `git log` blocks the UI
        // for its full duration.
        let log_root = self.commit_log_root().to_string();
        let head_ref = self.commit_head_ref().to_string();
        let new_commits = git::git_log_range(&self.base_branch, &head_ref, &log_root, 50, skip)
            .unwrap_or_default();

        let history = match self.history.as_mut() {
            Some(h) => h,
            None => return,
        };
        if new_commits.is_empty() {
            history.all_loaded = true;
        } else {
            history.commits.extend(new_commits);
        }
    }

    /// Get visible commits (filtered by search query)
    pub fn visible_commits(&self) -> Vec<(usize, &CommitInfo)> {
        let history = match self.history.as_ref() {
            Some(h) => h,
            None => return vec![],
        };

        if self.search_query.is_empty() {
            history.commits.iter().enumerate().collect()
        } else {
            let q = self.search_query.to_lowercase();
            history
                .commits
                .iter()
                .enumerate()
                .filter(|(_, c)| {
                    c.subject.to_lowercase().contains(&q)
                        || c.short_hash.contains(&q)
                        || c.author.to_lowercase().contains(&q)
                })
                .collect()
        }
    }

    /// Scroll down in history mode
    pub const fn history_scroll_down(&mut self, amount: u16) {
        if let Some(ref mut h) = self.history {
            h.diff_scroll = h.diff_scroll.saturating_add(amount);
        }
    }

    /// Scroll up in history mode
    pub const fn history_scroll_up(&mut self, amount: u16) {
        if let Some(ref mut h) = self.history {
            h.diff_scroll = h.diff_scroll.saturating_sub(amount);
        }
    }

    /// Scroll right in history mode
    pub const fn history_scroll_right(&mut self, amount: u16) {
        if let Some(ref mut h) = self.history {
            h.h_scroll = h.h_scroll.saturating_add(amount);
        }
    }

    /// Scroll left in history mode
    pub const fn history_scroll_left(&mut self, amount: u16) {
        if let Some(ref mut h) = self.history {
            h.h_scroll = h.h_scroll.saturating_sub(amount);
        }
    }

    // ── Tour mode navigation ──
    //
    // The diff pane concatenates `TourState.files` exactly like History
    // concatenates a commit's files, so the scroll-offset math mirrors History.
    // `j`/`k` move between pillars, `n`/`N` between files, arrows between lines,
    // `u`/`d` scroll. Pillar membership is kept in sync via `pillar_of_file`.

    /// Move to the next pillar (jumps to its first file).
    pub fn tour_next_pillar(&mut self) {
        let tour = match self.tour.as_mut() {
            Some(t) => t,
            None => return,
        };
        if tour.selected_pillar + 1 < tour.pillars.len() {
            tour.selected_pillar += 1;
            let start = tour
                .pillar_file_ranges
                .get(tour.selected_pillar)
                .map(|&(s, _)| s)
                .unwrap_or(0);
            tour.selected_file = start;
            tour.current_hunk = 0;
            tour.current_line = None;
            Self::tour_scroll_to_file(tour);
        }
    }

    /// Move to the previous pillar (jumps to its first file).
    pub fn tour_prev_pillar(&mut self) {
        let tour = match self.tour.as_mut() {
            Some(t) => t,
            None => return,
        };
        if tour.selected_pillar > 0 {
            tour.selected_pillar -= 1;
            let start = tour
                .pillar_file_ranges
                .get(tour.selected_pillar)
                .map(|&(s, _)| s)
                .unwrap_or(0);
            tour.selected_file = start;
            tour.current_hunk = 0;
            tour.current_line = None;
            Self::tour_scroll_to_file(tour);
        }
    }

    /// Move to the next file in the tour (crossing pillar boundaries).
    pub fn tour_next_file(&mut self) {
        let tour = match self.tour.as_mut() {
            Some(t) => t,
            None => return,
        };
        if tour.selected_file + 1 < tour.files.len() {
            tour.selected_file += 1;
            tour.current_hunk = 0;
            tour.current_line = None;
            if let Some(p) = tour.pillar_of_file(tour.selected_file) {
                tour.selected_pillar = p;
            }
            Self::tour_scroll_to_file(tour);
        }
    }

    /// Move to the previous file in the tour (crossing pillar boundaries).
    pub fn tour_prev_file(&mut self) {
        let tour = match self.tour.as_mut() {
            Some(t) => t,
            None => return,
        };
        if tour.selected_file > 0 {
            tour.selected_file -= 1;
            tour.current_hunk = 0;
            tour.current_line = None;
            if let Some(p) = tour.pillar_of_file(tour.selected_file) {
                tour.selected_pillar = p;
            }
            Self::tour_scroll_to_file(tour);
        }
    }

    /// Move to the next line within the tour diff.
    pub fn tour_next_line(&mut self) {
        let tour = match self.tour.as_mut() {
            Some(t) => t,
            None => return,
        };
        let file = match tour.files.get(tour.selected_file) {
            Some(f) => f,
            None => return,
        };
        let hunk_count = file.hunks.len();
        let line_count = file
            .hunks
            .get(tour.current_hunk)
            .map(|h| h.lines.len())
            .unwrap_or(0);

        match tour.current_line {
            None => {
                if line_count > 0 {
                    tour.current_line = Some(0);
                    Self::tour_scroll_to_current(tour);
                }
            }
            Some(line) => {
                if line + 1 < line_count {
                    tour.current_line = Some(line + 1);
                    Self::tour_scroll_to_current(tour);
                } else if tour.current_hunk + 1 < hunk_count {
                    tour.current_hunk += 1;
                    tour.current_line = Some(0);
                    Self::tour_scroll_to_current(tour);
                } else if tour.selected_file + 1 < tour.files.len() {
                    tour.selected_file += 1;
                    tour.current_hunk = 0;
                    tour.current_line = Some(0);
                    if let Some(p) = tour.pillar_of_file(tour.selected_file) {
                        tour.selected_pillar = p;
                    }
                    Self::tour_scroll_to_current(tour);
                }
            }
        }
    }

    /// Move to the previous line within the tour diff.
    pub fn tour_prev_line(&mut self) {
        let tour = match self.tour.as_mut() {
            Some(t) => t,
            None => return,
        };
        let file = match tour.files.get(tour.selected_file) {
            Some(f) => f,
            None => return,
        };

        match tour.current_line {
            None => {
                let count = file
                    .hunks
                    .get(tour.current_hunk)
                    .map(|h| h.lines.len())
                    .unwrap_or(0);
                if count > 0 {
                    tour.current_line = Some(count - 1);
                    Self::tour_scroll_to_current(tour);
                }
            }
            Some(0) => {
                if tour.current_hunk > 0 {
                    tour.current_hunk -= 1;
                    let count = file
                        .hunks
                        .get(tour.current_hunk)
                        .map(|h| h.lines.len())
                        .unwrap_or(0);
                    tour.current_line = if count > 0 { Some(count - 1) } else { None };
                    Self::tour_scroll_to_current(tour);
                } else if tour.selected_file > 0 {
                    tour.selected_file -= 1;
                    let prev_file = &tour.files[tour.selected_file];
                    if let Some(last_hunk) = prev_file.hunks.last() {
                        tour.current_hunk = prev_file.hunks.len() - 1;
                        tour.current_line = if last_hunk.lines.is_empty() {
                            None
                        } else {
                            Some(last_hunk.lines.len() - 1)
                        };
                    } else {
                        tour.current_hunk = 0;
                        tour.current_line = None;
                    }
                    if let Some(p) = tour.pillar_of_file(tour.selected_file) {
                        tour.selected_pillar = p;
                    }
                    Self::tour_scroll_to_current(tour);
                } else {
                    tour.current_line = None;
                }
            }
            Some(line) => {
                tour.current_line = Some(line - 1);
                Self::tour_scroll_to_current(tour);
            }
        }
    }

    /// Scroll to the current file header in tour mode.
    fn tour_scroll_to_file(tour: &mut TourState) {
        let mut line_offset: usize = 0;
        for (file_idx, file) in tour.files.iter().enumerate() {
            if file_idx == tour.selected_file {
                tour.diff_scroll = line_offset.min(u16::MAX as usize) as u16;
                return;
            }
            line_offset += 2; // header + blank
            for hunk in &file.hunks {
                line_offset += 1 + hunk.lines.len() + 1; // header + lines + blank
            }
        }
    }

    /// Scroll to the current line position in tour mode.
    fn tour_scroll_to_current(tour: &mut TourState) {
        let mut line_offset: usize = 0;
        for (file_idx, file) in tour.files.iter().enumerate() {
            line_offset += 2; // file header + blank
            for (hunk_idx, hunk) in file.hunks.iter().enumerate() {
                if file_idx == tour.selected_file && hunk_idx == tour.current_hunk {
                    line_offset += tour.current_line.unwrap_or(0);
                    tour.diff_scroll = line_offset.saturating_sub(1).min(u16::MAX as usize) as u16;
                    return;
                }
                line_offset += 1 + hunk.lines.len() + 1;
            }
        }
    }

    /// Scroll the tour diff down.
    pub fn tour_scroll_down(&mut self, amount: u16) {
        if let Some(ref mut t) = self.tour {
            t.diff_scroll = t.diff_scroll.saturating_add(amount);
            // Keep the selected pillar in sync with what's now at the top.
            if let Some(p) = t.pillar_at_scroll() {
                t.selected_pillar = p;
            }
        }
    }

    /// Scroll the tour diff up.
    pub fn tour_scroll_up(&mut self, amount: u16) {
        if let Some(ref mut t) = self.tour {
            t.diff_scroll = t.diff_scroll.saturating_sub(amount);
            if let Some(p) = t.pillar_at_scroll() {
                t.selected_pillar = p;
            }
        }
    }

    /// Scroll the tour diff right.
    pub const fn tour_scroll_right(&mut self, amount: u16) {
        if let Some(ref mut t) = self.tour {
            t.h_scroll = t.h_scroll.saturating_add(amount);
        }
    }

    /// Scroll the tour diff left.
    pub const fn tour_scroll_left(&mut self, amount: u16) {
        if let Some(ref mut t) = self.tour {
            t.h_scroll = t.h_scroll.saturating_sub(amount);
        }
    }

    /// Toggle reviewed state for the tour's currently selected file. Shares the
    /// branch `reviewed` set.
    pub fn tour_toggle_reviewed(&mut self) {
        let path = match self
            .tour
            .as_ref()
            .and_then(|t| t.files.get(t.selected_file))
        {
            Some(f) => f.path.clone(),
            None => return,
        };
        if self.reviewed.contains_key(&path) {
            self.reviewed.remove(&path);
        } else {
            let hash = self
                .current_per_file_hashes
                .get(&path)
                .cloned()
                .unwrap_or_default();
            self.reviewed.insert(path, hash);
        }
        let _ = self.save_reviewed_files();
    }

    /// Mark every file in the selected pillar as reviewed (bulk review). Shares
    /// the branch `reviewed` set, so these also show reviewed in the Diff view.
    pub fn tour_bulk_review_pillar(&mut self) {
        let paths: Vec<String> = {
            let tour = match self.tour.as_ref() {
                Some(t) => t,
                None => return,
            };
            let (start, end) = match tour.pillar_file_ranges.get(tour.selected_pillar) {
                Some(&r) => r,
                None => return,
            };
            tour.files
                .get(start..end)
                .map(|fs| fs.iter().map(|f| f.path.clone()).collect())
                .unwrap_or_default()
        };
        for path in paths {
            let hash = self
                .current_per_file_hashes
                .get(&path)
                .cloned()
                .unwrap_or_default();
            self.reviewed.insert(path, hash);
        }
        let _ = self.save_reviewed_files();
    }
}

/// Tiered ladder via `git::SIZE_LADDER`: bigger context for smaller files,
/// lean context for big ones. `total_lines` is the file's diff line count at
/// the default `--unified` value; `hunks` is the hunk count. Returns the
/// `--unified=N` value to use, falling back to `DEFAULT_CONTEXT_LINES` for
/// files larger than the last tier.
fn pick_context_for_size(total_lines: usize, hunks: usize) -> usize {
    let ladder = git::SIZE_LADDER;
    let base_tier = ladder.iter().position(|(limit, _)| total_lines <= *limit);
    let tier = match base_tier {
        Some(i) => i,
        None => return git::DEFAULT_CONTEXT_LINES,
    };

    // Hunk-count nudge: many scattered small hunks benefit from more context.
    let nudged = if hunks >= 4 && tier > 0 {
        tier - 1
    } else {
        tier
    };
    ladder[nudged].1
}

#[cfg(test)]
mod tests {
    use super::pick_context_for_size;

    #[test]
    fn tiny_files_get_full_context() {
        assert_eq!(pick_context_for_size(10, 1), 99999);
        assert_eq!(pick_context_for_size(60, 1), 99999);
    }

    #[test]
    fn small_files_get_80() {
        assert_eq!(pick_context_for_size(100, 1), 80);
        assert_eq!(pick_context_for_size(180, 1), 80);
    }

    #[test]
    fn medium_files_get_40() {
        assert_eq!(pick_context_for_size(250, 1), 40);
        assert_eq!(pick_context_for_size(500, 2), 40);
    }

    #[test]
    fn larger_files_get_20() {
        assert_eq!(pick_context_for_size(800, 1), 20);
        assert_eq!(pick_context_for_size(1500, 2), 20);
    }

    #[test]
    fn big_files_get_10_floor() {
        assert_eq!(pick_context_for_size(1501, 1), 10);
        assert_eq!(pick_context_for_size(5000, 20), 10);
    }

    #[test]
    fn hunk_nudge_bumps_one_tier() {
        // 250 lines normally → 40; with 4+ hunks → 80
        assert_eq!(pick_context_for_size(250, 4), 80);
        // 800 lines normally → 20; with 5 hunks → 40
        assert_eq!(pick_context_for_size(800, 5), 40);
        // 100 lines normally → 80; nudge → full
        assert_eq!(pick_context_for_size(100, 4), 99999);
    }

    #[test]
    fn hunk_nudge_does_not_promote_top_tier() {
        // already at top tier (full); nudge can't go higher
        assert_eq!(pick_context_for_size(20, 10), 99999);
    }

    #[test]
    fn hunk_nudge_does_not_rescue_huge_files() {
        // > 1500 stays at 10 regardless of hunk count
        assert_eq!(pick_context_for_size(2000, 20), 10);
    }
}

#[cfg(test)]
mod nav_state_tests {
    use super::*;
    use crate::git::{DiffHunk, DiffLine, FileStatus, LineType};

    /// `open_in_editor` reads the process-wide `EDITOR`, so the two tests that
    /// set it are serialized against each other.
    static EDITOR_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// A two-file raw diff with LF endings, so `parse_diff_headers` byte offsets
    /// line up with `parse_file_at_offset`.
    const TWO_FILE_DIFF: &str = r#"diff --git a/a.rs b/a.rs
index 1111111..2222222 100644
--- a/a.rs
+++ b/a.rs
@@ -1,3 +1,3 @@
 fn a() {
-    old_a();
+    new_a();
 }
diff --git a/b.rs b/b.rs
index 3333333..4444444 100644
--- a/b.rs
+++ b/b.rs
@@ -1,3 +1,3 @@
 fn b() {
-    old_b();
+    new_b();
 }
"#;

    // ── fixtures ──

    fn diff_line(line_type: LineType, content: &str, new_num: Option<usize>) -> DiffLine {
        DiffLine {
            line_type,
            content: content.to_string(),
            old_num: None,
            new_num,
        }
    }

    fn hunk_with(new_start: usize, count: usize) -> DiffHunk {
        DiffHunk {
            header: String::new(),
            old_start: new_start,
            old_count: count,
            new_start,
            new_count: count,
            lines: (0..count)
                .map(|i| {
                    diff_line(
                        LineType::Context,
                        &format!("line {}", new_start + i),
                        Some(new_start + i),
                    )
                })
                .collect(),
        }
    }

    fn diff_file(path: &str, hunks: Vec<DiffHunk>) -> DiffFile {
        DiffFile {
            path: path.to_string(),
            status: FileStatus::Modified,
            hunks,
            adds: 1,
            dels: 1,
            compacted: false,
            raw_hunk_count: 0,
        }
    }

    fn commit_info(hash: &str, subject: &str, author: &str) -> CommitInfo {
        CommitInfo {
            hash: hash.to_string(),
            short_hash: hash.chars().take(7).collect(),
            subject: subject.to_string(),
            author: author.to_string(),
            date: "2026-01-01T00:00:00Z".to_string(),
            relative_date: "1 day ago".to_string(),
            file_count: 1,
            adds: 1,
            dels: 0,
            is_merge: false,
        }
    }

    fn history_with(commits: Vec<CommitInfo>, commit_files: Vec<DiffFile>) -> HistoryState {
        HistoryState {
            commits,
            selected_commit: 0,
            commit_files,
            selected_file: 0,
            current_hunk: 0,
            current_line: None,
            diff_scroll: 0,
            h_scroll: 0,
            all_loaded: false,
            diff_cache: DiffCache::new(5),
        }
    }

    fn pillar_view(id: &str) -> TourPillarView {
        TourPillarView {
            id: id.to_string(),
            title: id.to_string(),
            description: String::new(),
            importance: 1,
            foundation: false,
        }
    }

    fn tour_with(files: Vec<DiffFile>, ranges: Vec<(usize, usize)>) -> TourState {
        let file_is_related = vec![false; files.len()];
        let pillars: Vec<TourPillarView> = (0..ranges.len())
            .map(|i| pillar_view(&format!("p{i}")))
            .collect();
        TourState {
            pillars,
            selected_pillar: 0,
            files,
            file_is_related,
            pillar_file_ranges: ranges,
            selected_file: 0,
            current_hunk: 0,
            current_line: None,
            diff_scroll: 0,
            h_scroll: 0,
        }
    }

    /// f0: hunks of 3 and 2 lines. f1: one hunk of 2 lines.
    fn two_file_history() -> HistoryState {
        history_with(
            vec![commit_info("aaa1111", "c", "Ada")],
            vec![
                diff_file("f0.rs", vec![hunk_with(1, 3), hunk_with(20, 2)]),
                diff_file("f1.rs", vec![hunk_with(1, 2)]),
            ],
        )
    }

    /// Same shape as `two_file_history`, but the two files sit in *different*
    /// pillars so a cross-file move has to move `selected_pillar` too.
    fn two_pillar_tour() -> TourState {
        tour_with(
            vec![
                diff_file("p0a.rs", vec![hunk_with(1, 3), hunk_with(20, 2)]),
                diff_file("p1a.rs", vec![hunk_with(1, 2)]),
            ],
            vec![(0, 1), (1, 2)],
        )
    }

    // ── real-git fixtures ──

    fn run_git(dir: &std::path::Path, args: &[&str]) -> String {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap_or_else(|e| panic!("failed to run git {args:?}: {e}"));
        assert!(
            output.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    fn init_repo(root: &std::path::Path) {
        run_git(root, &["init", "-b", "main"]);
        run_git(root, &["config", "user.email", "test@example.com"]);
        run_git(root, &["config", "user.name", "Test User"]);
        run_git(root, &["config", "commit.gpgsign", "false"]);
    }

    /// `main` plus a `feature` branch whose single commit edits line 30 of a
    /// 60-line file. Wide enough that -U10 / -U20 / -U40 give distinguishable
    /// hunk sizes (21 / 41 / 60 lines).
    fn temp_branch_repo() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        init_repo(root);
        let base: String = (1..=60).map(|i| format!("line {i}\n")).collect();
        std::fs::write(root.join("src.txt"), base).unwrap();
        run_git(root, &["add", "src.txt"]);
        run_git(root, &["commit", "-m", "base"]);
        run_git(root, &["checkout", "-b", "feature"]);
        let changed: String = (1..=60)
            .map(|i| {
                if i == 30 {
                    "line 30 CHANGED\n".to_string()
                } else {
                    format!("line {i}\n")
                }
            })
            .collect();
        std::fs::write(root.join("src.txt"), changed).unwrap();
        run_git(root, &["commit", "-am", "change line 30"]);
        tmp
    }

    /// `main` plus a `feature` branch carrying three commits (first, second,
    /// third) so `git log main..feature` can be paginated.
    fn temp_history_repo() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        init_repo(root);
        std::fs::write(root.join("base.txt"), "base\n").unwrap();
        run_git(root, &["add", "base.txt"]);
        run_git(root, &["commit", "-m", "base"]);
        run_git(root, &["checkout", "-b", "feature"]);
        for subject in ["first", "second", "third"] {
            std::fs::write(root.join(format!("{subject}.txt")), "x\n").unwrap();
            run_git(root, &["add", "."]);
            run_git(root, &["commit", "-m", subject]);
        }
        tmp
    }

    /// A Branch-mode tab whose diff is the real `main...feature` diff of
    /// `src.txt`, parsed at `context` unified lines.
    fn branch_tab(root: &std::path::Path, context: Option<usize>) -> TabState {
        let root_str = root.to_string_lossy().to_string();
        let raw = git::git_diff_raw_file("branch", "main", &root_str, "src.txt", context, None)
            .expect("git diff for src.txt");
        let mut tab = TabState::new_for_test(git::parse_diff(&raw));
        tab.repo_root = root_str.clone();
        tab.er_root = ErRoot::RepoLocal(root_str);
        tab
    }

    /// `new_count` from the first hunk's `@@` header — unaffected by context
    /// folding, so it is a stable measure of how wide the fetched hunk is.
    fn first_hunk_new_count(tab: &TabState) -> usize {
        tab.files[0].hunks.first().map_or(0, |h| h.new_count)
    }

    fn added_contents(file: &DiffFile) -> Vec<String> {
        file.hunks
            .iter()
            .flat_map(|h| h.lines.iter())
            .filter(|l| matches!(l.line_type, LineType::Add))
            .map(|l| l.content.trim().to_string())
            .collect()
    }

    // ── ensure_file_parsed_at ──

    #[test]
    fn ensure_file_parsed_at_is_a_noop_when_lazy_mode_is_off() {
        let mut tab = TabState::new_for_test(vec![diff_file("a.rs", vec![])]);
        tab.raw_diff = Some(TWO_FILE_DIFF.to_string());
        tab.file_headers = git::parse_diff_headers(TWO_FILE_DIFF);
        tab.lazy_mode = false;

        tab.ensure_file_parsed_at(0);

        assert!(
            tab.files[0].hunks.is_empty(),
            "eager mode already holds every hunk it will ever have; on-demand parsing must not run"
        );
    }

    #[test]
    fn ensure_file_parsed_at_leaves_a_compacted_file_collapsed() {
        let mut file = diff_file("a.rs", vec![]);
        file.compacted = true;
        let mut tab = TabState::new_for_test(vec![file]);
        tab.raw_diff = Some(TWO_FILE_DIFF.to_string());
        tab.file_headers = git::parse_diff_headers(TWO_FILE_DIFF);
        tab.lazy_mode = true;

        tab.ensure_file_parsed_at(0);

        assert!(tab.files[0].compacted);
        assert!(
            tab.files[0].hunks.is_empty(),
            "a compacted stub stays collapsed until expanded, even with its section in raw_diff"
        );
    }

    #[test]
    fn ensure_file_parsed_at_resolves_the_header_by_path_not_by_index() {
        // Mtime sort reorders `files` without reordering `file_headers`, so an
        // index-based header lookup parses the wrong file's section.
        let mut tab =
            TabState::new_for_test(vec![diff_file("b.rs", vec![]), diff_file("a.rs", vec![])]);
        tab.raw_diff = Some(TWO_FILE_DIFF.to_string());
        tab.file_headers = git::parse_diff_headers(TWO_FILE_DIFF);
        tab.lazy_mode = true;

        tab.ensure_file_parsed_at(0);

        assert_eq!(
            added_contents(&tab.files[0]),
            vec!["new_b();".to_string()],
            "files[0] is b.rs, so b.rs's section must be parsed even though header[0] is a.rs"
        );
        assert!(
            tab.files[1].hunks.is_empty(),
            "only the requested index is parsed"
        );
    }

    #[test]
    fn ensure_file_parsed_at_skips_the_git_fallback_for_remote_tabs() {
        // The repo really does contain src.txt in `main...feature`, so a git
        // fallback would succeed here — the remote guard is what must stop it.
        let tmp = temp_branch_repo();
        let mut tab = TabState::new_for_test(vec![diff_file("src.txt", vec![])]);
        tab.repo_root = tmp.path().to_string_lossy().to_string();
        tab.lazy_mode = true;
        tab.raw_diff = None;
        tab.remote_repo = Some("owner/repo".to_string());

        tab.ensure_file_parsed_at(0);

        assert!(
            tab.files[0].hunks.is_empty(),
            "a remote tab has no local checkout to diff — raw_diff is its only source"
        );
    }

    #[test]
    fn ensure_file_parsed_at_falls_back_to_git_when_the_raw_diff_has_no_section() {
        let tmp = temp_branch_repo();
        let mut tab = TabState::new_for_test(vec![diff_file("src.txt", vec![])]);
        tab.repo_root = tmp.path().to_string_lossy().to_string();
        tab.lazy_mode = true;
        tab.raw_diff = None;

        tab.ensure_file_parsed_at(0);

        assert!(
            tab.files[0]
                .hunks
                .iter()
                .flat_map(|h| h.lines.iter())
                .any(|l| l.content.contains("line 30 CHANGED")),
            "with no raw_diff section the stub must be filled from a real `git diff` of that path"
        );
        assert_eq!(
            tab.mem_budget.parsed_files, 1,
            "the memory budget is refreshed after an on-demand parse"
        );
    }

    // ── toggle_compacted ──

    #[test]
    fn toggle_compacted_recompacts_an_expanded_file_and_resets_navigation() {
        let mut tab = TabState::new_for_test(vec![diff_file(
            "a.rs",
            vec![hunk_with(1, 3), hunk_with(20, 2)],
        )]);
        tab.user_expanded.insert("a.rs".to_string());
        tab.current_hunk = 1;
        tab.current_line = Some(1);
        tab.diff_scroll = 40;
        // Populate the offsets so clearing them is observable rather than a
        // no-op against the `None` this tab starts with.
        tab.rebuild_hunk_offsets();
        assert!(tab.hunk_offsets.is_some());

        tab.toggle_compacted().unwrap();

        let file = &tab.files[0];
        assert!(file.compacted);
        assert!(file.hunks.is_empty(), "compacting frees the parsed hunks");
        assert_eq!(
            file.raw_hunk_count, 2,
            "the hunk count survives so the collapsed row can still say how big the file is"
        );
        assert!(
            !tab.user_expanded.contains("a.rs"),
            "re-compacting forgets the manual expansion so refreshes keep it collapsed"
        );
        assert_eq!(tab.current_hunk, 0);
        assert_eq!(tab.current_line, None);
        assert_eq!(tab.diff_scroll, 0);
        assert!(tab.hunk_offsets.is_none());
    }

    #[test]
    fn toggle_compacted_expands_a_remote_file_from_the_cached_raw_diff() {
        let mut file = diff_file("b.rs", vec![]);
        file.compacted = true;
        file.raw_hunk_count = 1;
        let mut tab = TabState::new_for_test(vec![file]);
        tab.remote_repo = Some("owner/repo".to_string());
        // A path that is not a git repo: reaching git at all would be the bug.
        tab.repo_root = "/nonexistent/er-test-remote-expand".to_string();
        tab.raw_diff = Some(TWO_FILE_DIFF.to_string());
        tab.file_headers = git::parse_diff_headers(TWO_FILE_DIFF);

        tab.toggle_compacted().unwrap();

        assert!(!tab.files[0].compacted);
        assert_eq!(
            added_contents(&tab.files[0]),
            vec!["new_b();".to_string()],
            "a remote expand re-parses the file's section out of raw_diff instead of shelling out"
        );
        assert!(
            tab.user_expanded.contains("b.rs"),
            "the expansion is remembered so a refresh does not re-collapse it"
        );
    }

    #[test]
    fn toggle_compacted_expands_a_local_file_by_refetching_from_git() {
        let tmp = temp_branch_repo();
        let mut tab = branch_tab(tmp.path(), None);
        tab.files[0].hunks.clear();
        tab.files[0].compacted = true;

        tab.toggle_compacted().unwrap();

        assert!(!tab.files[0].compacted);
        assert!(
            tab.files[0]
                .hunks
                .iter()
                .flat_map(|h| h.lines.iter())
                .any(|l| l.content.contains("line 30 CHANGED")),
            "a local compacted file is re-fetched from git, not from raw_diff"
        );
        assert!(tab.user_expanded.contains("src.txt"));
    }

    // ── expand_context ──

    #[test]
    fn expand_context_is_a_noop_in_history_mode() {
        let mut tab = TabState::new_for_test(vec![diff_file("a.rs", vec![hunk_with(1, 3)])]);
        tab.mode = DiffMode::History;
        tab.repo_root = "/nonexistent/er-test-history-context".to_string();

        tab.expand_context().unwrap();

        assert!(
            tab.context_overrides.is_empty(),
            "History mode renders a commit diff, which has no per-file context override"
        );
    }

    #[test]
    fn expand_context_leaves_a_single_hunk_added_file_alone() {
        let mut file = diff_file("new.rs", vec![hunk_with(1, 3)]);
        file.status = FileStatus::Added;
        let mut tab = TabState::new_for_test(vec![file]);
        tab.repo_root = "/nonexistent/er-test-added-context".to_string();

        tab.expand_context().unwrap();

        assert!(
            tab.context_overrides.is_empty(),
            "an added file's diff is already the whole file; there is no context to widen"
        );
    }

    #[test]
    fn expand_context_on_a_compacted_file_expands_it_instead_of_widening() {
        let mut file = diff_file("b.rs", vec![]);
        file.compacted = true;
        let mut tab = TabState::new_for_test(vec![file]);
        tab.remote_repo = Some("owner/repo".to_string());
        tab.raw_diff = Some(TWO_FILE_DIFF.to_string());
        tab.file_headers = git::parse_diff_headers(TWO_FILE_DIFF);

        tab.expand_context().unwrap();

        assert!(
            !tab.files[0].compacted,
            "`+` on a collapsed file behaves like Enter"
        );
        assert!(!tab.files[0].hunks.is_empty());
        assert!(
            tab.context_overrides.is_empty(),
            "expanding a stub must not also record a context override"
        );
    }

    #[test]
    fn expand_context_steps_to_the_next_level_and_widens_the_hunk() {
        let tmp = temp_branch_repo();
        let mut tab = branch_tab(tmp.path(), None);
        assert_eq!(
            first_hunk_new_count(&tab),
            21,
            "-U10 around line 30 of a 60-line file"
        );

        tab.expand_context().unwrap();

        assert_eq!(tab.context_overrides.get("src.txt").copied(), Some(20));
        assert_eq!(
            first_hunk_new_count(&tab),
            41,
            "the file is refetched at -U20, doubling the context on each side"
        );
    }

    // ── collapse_context ──

    #[test]
    fn collapse_context_is_a_noop_in_history_mode() {
        let mut tab = TabState::new_for_test(vec![diff_file("a.rs", vec![hunk_with(1, 3)])]);
        tab.mode = DiffMode::History;
        tab.context_overrides.insert("a.rs".to_string(), 40);
        tab.repo_root = "/nonexistent/er-test-history-collapse".to_string();

        tab.collapse_context().unwrap();

        assert_eq!(tab.context_overrides.get("a.rs").copied(), Some(40));
    }

    #[test]
    fn collapse_context_is_a_noop_for_a_compacted_file() {
        let mut file = diff_file("a.rs", vec![]);
        file.compacted = true;
        let mut tab = TabState::new_for_test(vec![file]);
        tab.context_overrides.insert("a.rs".to_string(), 40);
        tab.repo_root = "/nonexistent/er-test-collapse-compacted".to_string();

        tab.collapse_context().unwrap();

        assert_eq!(
            tab.context_overrides.get("a.rs").copied(),
            Some(40),
            "a collapsed file renders no context, so there is nothing to narrow"
        );
    }

    #[test]
    fn collapse_context_is_a_noop_at_the_default_context() {
        let tmp = temp_branch_repo();
        let mut tab = branch_tab(tmp.path(), None);
        assert_eq!(first_hunk_new_count(&tab), 21, "-U10 is the default");
        // Re-point at a non-repo so the early return is load-bearing: without it
        // `prev` falls back to the default and the refetch would error through `?`.
        tab.repo_root = "/nonexistent/er-test-collapse-default".to_string();

        tab.collapse_context().unwrap();

        assert!(tab.context_overrides.is_empty());
        assert_eq!(
            first_hunk_new_count(&tab),
            21,
            "already at the default -U10; `-` cannot go below it"
        );
    }

    #[test]
    fn collapse_context_steps_down_one_level_and_keeps_the_override() {
        let tmp = temp_branch_repo();
        let mut tab = branch_tab(tmp.path(), Some(40));
        tab.context_overrides.insert("src.txt".to_string(), 40);
        assert_eq!(first_hunk_new_count(&tab), 60, "-U40 covers the whole file");

        tab.collapse_context().unwrap();

        assert_eq!(tab.context_overrides.get("src.txt").copied(), Some(20));
        assert_eq!(first_hunk_new_count(&tab), 41);
    }

    #[test]
    fn collapse_context_drops_the_override_when_it_reaches_the_default() {
        let tmp = temp_branch_repo();
        let mut tab = branch_tab(tmp.path(), Some(20));
        tab.context_overrides.insert("src.txt".to_string(), 20);

        tab.collapse_context().unwrap();

        assert!(
            tab.context_overrides.is_empty(),
            "back at the default the override is removed, not pinned to 10"
        );
        assert_eq!(first_hunk_new_count(&tab), 21);
    }

    // ── open_in_editor ──

    #[test]
    fn open_in_editor_is_a_noop_when_no_file_is_selected() {
        // EDITOR points at a binary that cannot spawn, so `Ok` here means the
        // no-file guard returned *before* building a command — not merely that
        // the call happened to succeed.
        let _guard = EDITOR_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("EDITOR", "/nonexistent/er-test-unspawnable-editor");
        let tab = TabState::new_for_test(vec![]);

        assert!(
            tab.open_in_editor().is_ok(),
            "with no selected file nothing may be spawned"
        );
    }

    #[test]
    fn open_in_editor_surfaces_a_missing_editor_binary_as_an_error() {
        let _guard = EDITOR_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("EDITOR", "/nonexistent/er-test-missing-editor-bin");
        let tab = TabState::new_for_test(vec![diff_file("src/lib.rs", vec![hunk_with(42, 3)])]);

        let err = tab
            .open_in_editor()
            .expect_err("spawning a missing binary must fail");

        assert!(
            err.to_string().contains("Failed to open editor"),
            "the spawn failure must be reported, not swallowed; got: {err}"
        );
    }

    // NOTE: a second copy of the test above, with EDITOR containing "code" to
    // "cover" the `-g <file>:<line>` arm, was removed as coverage theater: a
    // spawn failure is argument-independent, so it passed identically whichever
    // arm ran and would still pass with the vscode branch deleted. Pinning the
    // three argument forms needs a recorder binary that logs its argv (the
    // editor is spawned, never waited on), which is left for a follow-up.

    // ── history_load_selected_diff ──

    #[test]
    fn history_load_selected_diff_leaves_the_branch_viewport_alone_without_history_state() {
        let mut tab = TabState::new_for_test(vec![diff_file("branch.rs", vec![hunk_with(1, 2)])]);
        tab.diff_scroll = 12;
        tab.current_line = Some(1);

        tab.history_load_selected_diff();

        assert!(tab.history.is_none());
        assert_eq!(
            tab.diff_scroll, 12,
            "the History pane's scroll reset must not touch the tab's own viewport"
        );
        assert_eq!(tab.current_line, Some(1));
    }

    #[test]
    fn history_load_selected_diff_keeps_the_pane_when_no_commit_is_selected() {
        let mut tab = TabState::new_for_test(vec![]);
        tab.history = Some(history_with(
            vec![],
            vec![diff_file("kept.rs", vec![hunk_with(1, 2)])],
        ));

        tab.history_load_selected_diff();

        let history = tab.history.as_ref().unwrap();
        assert_eq!(
            history.commit_files.len(),
            1,
            "an empty commit list returns before touching the diff pane"
        );
        assert_eq!(history.commit_files[0].path, "kept.rs");
    }

    #[test]
    fn history_load_selected_diff_serves_the_cache_and_resets_navigation() {
        let mut tab = TabState::new_for_test(vec![]);
        // Not a git repo: a cache miss could not produce any files here.
        tab.repo_root = "/nonexistent/er-test-history-cache".to_string();
        let mut history = history_with(vec![commit_info("cafe1234", "cached", "Ada")], vec![]);
        history.diff_cache.insert(
            "cafe1234".to_string(),
            vec![diff_file("cached.rs", vec![hunk_with(1, 2)])],
        );
        history.selected_file = 3;
        history.current_hunk = 2;
        history.current_line = Some(5);
        history.diff_scroll = 90;
        history.h_scroll = 7;
        tab.history = Some(history);

        tab.history_load_selected_diff();

        let history = tab.history.as_ref().unwrap();
        assert_eq!(history.commit_files.len(), 1);
        assert_eq!(history.commit_files[0].path, "cached.rs");
        assert_eq!(history.selected_file, 0);
        assert_eq!(history.current_hunk, 0);
        assert_eq!(history.current_line, None);
        assert_eq!(history.diff_scroll, 0);
        assert_eq!(history.h_scroll, 0);
    }

    #[test]
    fn history_load_selected_diff_clears_the_pane_when_git_fails() {
        // A temp dir that is deliberately not a git repo.
        let tmp = tempfile::tempdir().unwrap();
        let mut tab = TabState::new_for_test(vec![]);
        tab.repo_root = tmp.path().to_string_lossy().to_string();
        let mut history = history_with(
            vec![commit_info(&"0".repeat(40), "missing", "Ada")],
            vec![diff_file("stale.rs", vec![hunk_with(1, 2)])],
        );
        history.selected_file = 1;
        tab.history = Some(history);

        tab.history_load_selected_diff();

        let history = tab.history.as_ref().unwrap();
        assert!(
            history.commit_files.is_empty(),
            "a failed load must not leave the previous commit's files on screen"
        );
        assert_eq!(history.selected_file, 0);
    }

    #[test]
    fn history_load_selected_diff_caches_the_commit_it_loaded() {
        let tmp = temp_branch_repo();
        let hash = run_git(tmp.path(), &["rev-parse", "HEAD"]);
        let mut tab = TabState::new_for_test(vec![]);
        tab.repo_root = tmp.path().to_string_lossy().to_string();
        tab.history = Some(history_with(
            vec![commit_info(&hash, "change line 30", "Test User")],
            vec![],
        ));

        tab.history_load_selected_diff();
        assert!(tab
            .history
            .as_ref()
            .unwrap()
            .commit_files
            .iter()
            .any(|f| f.path == "src.txt"));

        // Re-point the tab at a non-repo and clear the pane: only the cache can
        // satisfy the second load.
        tab.repo_root = "/nonexistent/er-test-history-cache-proof".to_string();
        tab.history.as_mut().unwrap().commit_files.clear();

        tab.history_load_selected_diff();

        assert!(
            tab.history
                .as_ref()
                .unwrap()
                .commit_files
                .iter()
                .any(|f| f.path == "src.txt"),
            "the first load must populate diff_cache so re-selecting the commit costs no git call"
        );
    }

    // ── history_load_more ──

    #[test]
    fn history_load_more_does_nothing_once_every_commit_is_loaded() {
        // A real repo with two *unloaded* commits behind the one in the list, so
        // a `git log` that actually ran would visibly append a page.
        let tmp = temp_history_repo();
        let mut tab = TabState::new_for_test(vec![]);
        tab.repo_root = tmp.path().to_string_lossy().to_string();
        let mut history = history_with(vec![commit_info("abc1234", "third", "Test User")], vec![]);
        history.all_loaded = true;
        tab.history = Some(history);

        tab.history_load_more();

        assert_eq!(
            tab.history.as_ref().unwrap().commits.len(),
            1,
            "all_loaded short-circuits before `git log`, though `main..feature` has two more"
        );
    }

    #[test]
    fn history_load_more_ends_pagination_for_pr_history_without_running_git_log() {
        // A real repo whose `main..feature` still holds two unloaded commits, so
        // a stray `git log` would show up as an appended page.
        let tmp = temp_history_repo();
        let mut tab = TabState::new_for_test(vec![]);
        tab.repo_root = tmp.path().to_string_lossy().to_string();
        tab.pr_number = Some(42);
        tab.local_branch_view = Some("feature".to_string());
        tab.history = Some(history_with(
            vec![commit_info("abc1234", "third", "Test User")],
            vec![],
        ));

        tab.history_load_more();

        let history = tab.history.as_ref().unwrap();
        assert!(
            history.all_loaded,
            "PR history is the whole pr_commits list; there is no second page to fetch"
        );
        assert_eq!(
            history.commits.len(),
            1,
            "the PR guard returns before `git log`, which would otherwise append two more"
        );
    }

    #[test]
    fn history_load_more_appends_the_next_page_of_branch_commits() {
        let tmp = temp_history_repo();
        let mut tab = TabState::new_for_test(vec![]);
        tab.repo_root = tmp.path().to_string_lossy().to_string();
        // Page one already holds the newest commit, so the next page skips it.
        tab.history = Some(history_with(
            vec![commit_info("deadbee", "third", "Test User")],
            vec![],
        ));

        tab.history_load_more();

        let subjects: Vec<&str> = tab
            .history
            .as_ref()
            .unwrap()
            .commits
            .iter()
            .map(|c| c.subject.as_str())
            .collect();
        assert_eq!(
            subjects,
            vec!["third", "second", "first"],
            "the page is `main..feature` minus the one already loaded; `base` belongs to main"
        );
        assert!(
            !tab.history.as_ref().unwrap().all_loaded,
            "a non-empty page leaves pagination open"
        );
    }

    #[test]
    fn history_load_more_marks_all_loaded_when_the_page_comes_back_empty() {
        let tmp = temp_history_repo();
        let mut tab = TabState::new_for_test(vec![]);
        tab.repo_root = tmp.path().to_string_lossy().to_string();
        tab.history = Some(history_with(
            vec![
                commit_info("aaaaaa1", "third", "Test User"),
                commit_info("aaaaaa2", "second", "Test User"),
                commit_info("aaaaaa3", "first", "Test User"),
            ],
            vec![],
        ));

        tab.history_load_more();

        let history = tab.history.as_ref().unwrap();
        assert_eq!(history.commits.len(), 3, "there was nothing left to fetch");
        assert!(history.all_loaded, "an empty page ends the pagination");
    }

    // ── visible_commits ──

    #[test]
    fn visible_commits_is_empty_without_history_state() {
        let tab = TabState::new_for_test(vec![]);
        assert!(tab.visible_commits().is_empty());
    }

    #[test]
    fn visible_commits_returns_every_commit_when_the_search_is_empty() {
        let mut tab = TabState::new_for_test(vec![]);
        tab.history = Some(history_with(
            vec![
                commit_info("aaa1111", "first", "Ada"),
                commit_info("bbb2222", "second", "Bob"),
            ],
            vec![],
        ));

        let indices: Vec<usize> = tab.visible_commits().iter().map(|(i, _)| *i).collect();

        assert_eq!(indices, vec![0, 1]);
    }

    #[test]
    fn visible_commits_filters_by_subject_hash_or_author_and_keeps_source_indices() {
        let mut tab = TabState::new_for_test(vec![]);
        tab.history = Some(history_with(
            vec![
                commit_info("aaa1111", "add parser", "Ada Lovelace"),
                commit_info("bbb2222", "fix crash", "Bob Martin"),
                commit_info("ccc3333", "docs", "Ada Lovelace"),
            ],
            vec![],
        ));

        tab.search_query = "CRASH".to_string();
        let indices: Vec<usize> = tab.visible_commits().iter().map(|(i, _)| *i).collect();
        assert_eq!(indices, vec![1], "subject matching is case-insensitive");

        tab.search_query = "ccc3".to_string();
        let indices: Vec<usize> = tab.visible_commits().iter().map(|(i, _)| *i).collect();
        assert_eq!(indices, vec![2], "a short-hash prefix matches");

        tab.search_query = "ada".to_string();
        let indices: Vec<usize> = tab.visible_commits().iter().map(|(i, _)| *i).collect();
        assert_eq!(
            indices,
            vec![0, 2],
            "author matches keep the original commit indices, not filtered positions"
        );
    }

    // ── history_next_line ──

    #[test]
    fn history_next_line_selects_the_first_line_when_nothing_is_selected() {
        let mut tab = TabState::new_for_test(vec![]);
        tab.history = Some(two_file_history());

        tab.history_next_line();

        assert_eq!(tab.history.as_ref().unwrap().current_line, Some(0));
    }

    #[test]
    fn history_next_line_advances_within_the_current_hunk() {
        let mut tab = TabState::new_for_test(vec![]);
        let mut history = two_file_history();
        history.current_line = Some(0);
        tab.history = Some(history);

        tab.history_next_line();

        let history = tab.history.as_ref().unwrap();
        assert_eq!(history.current_line, Some(1));
        assert_eq!(history.current_hunk, 0);
    }

    #[test]
    fn history_next_line_rolls_over_into_the_next_hunk() {
        let mut tab = TabState::new_for_test(vec![]);
        let mut history = two_file_history();
        history.current_line = Some(2); // last line of hunk 0
        tab.history = Some(history);

        tab.history_next_line();

        let history = tab.history.as_ref().unwrap();
        assert_eq!(history.current_hunk, 1);
        assert_eq!(history.current_line, Some(0));
    }

    #[test]
    fn history_next_line_rolls_over_into_the_next_file() {
        let mut tab = TabState::new_for_test(vec![]);
        let mut history = two_file_history();
        history.current_hunk = 1;
        history.current_line = Some(1); // last line of the last hunk of f0
        tab.history = Some(history);

        tab.history_next_line();

        let history = tab.history.as_ref().unwrap();
        assert_eq!(history.selected_file, 1);
        assert_eq!(history.current_hunk, 0);
        assert_eq!(history.current_line, Some(0));
    }

    #[test]
    fn history_next_line_stops_at_the_end_of_the_last_file() {
        let mut tab = TabState::new_for_test(vec![]);
        let mut history = two_file_history();
        history.selected_file = 1;
        history.current_line = Some(1); // last line of the last file
        tab.history = Some(history);

        tab.history_next_line();

        let history = tab.history.as_ref().unwrap();
        assert_eq!(history.selected_file, 1);
        assert_eq!(
            history.current_line,
            Some(1),
            "the end of the commit diff is a hard stop, not a wrap"
        );
    }

    #[test]
    fn history_next_line_is_a_noop_when_the_selected_file_is_out_of_range() {
        let mut tab = TabState::new_for_test(vec![]);
        let mut history = two_file_history();
        history.selected_file = 9;
        tab.history = Some(history);

        tab.history_next_line();

        assert_eq!(tab.history.as_ref().unwrap().current_line, None);
    }

    // ── history_prev_line ──

    #[test]
    fn history_prev_line_selects_the_last_line_when_nothing_is_selected() {
        let mut tab = TabState::new_for_test(vec![]);
        tab.history = Some(two_file_history());

        tab.history_prev_line();

        assert_eq!(tab.history.as_ref().unwrap().current_line, Some(2));
    }

    #[test]
    fn history_prev_line_moves_back_within_the_current_hunk() {
        let mut tab = TabState::new_for_test(vec![]);
        let mut history = two_file_history();
        history.current_line = Some(2);
        tab.history = Some(history);

        tab.history_prev_line();

        assert_eq!(tab.history.as_ref().unwrap().current_line, Some(1));
    }

    #[test]
    fn history_prev_line_steps_back_into_the_previous_hunks_last_line() {
        let mut tab = TabState::new_for_test(vec![]);
        let mut history = two_file_history();
        history.current_hunk = 1;
        history.current_line = Some(0);
        tab.history = Some(history);

        tab.history_prev_line();

        let history = tab.history.as_ref().unwrap();
        assert_eq!(history.current_hunk, 0);
        assert_eq!(history.current_line, Some(2));
    }

    #[test]
    fn history_prev_line_steps_back_into_the_previous_files_last_line() {
        let mut tab = TabState::new_for_test(vec![]);
        let mut history = two_file_history();
        history.selected_file = 1;
        history.current_line = Some(0);
        tab.history = Some(history);

        tab.history_prev_line();

        let history = tab.history.as_ref().unwrap();
        assert_eq!(history.selected_file, 0);
        assert_eq!(
            history.current_hunk, 1,
            "landing in the previous file lands on its last hunk"
        );
        assert_eq!(history.current_line, Some(1));
    }

    #[test]
    fn history_prev_line_clears_the_selection_at_the_very_first_line() {
        let mut tab = TabState::new_for_test(vec![]);
        let mut history = two_file_history();
        history.current_line = Some(0);
        tab.history = Some(history);

        tab.history_prev_line();

        let history = tab.history.as_ref().unwrap();
        assert_eq!(
            history.current_line, None,
            "moving up off the first line drops line focus rather than wrapping"
        );
        assert_eq!(history.selected_file, 0);
    }

    #[test]
    fn history_prev_line_clears_the_line_when_the_previous_file_has_no_hunks() {
        let mut tab = TabState::new_for_test(vec![]);
        let mut history = history_with(
            vec![commit_info("aaa1111", "c", "Ada")],
            vec![
                diff_file("empty.rs", vec![]),
                diff_file("f1.rs", vec![hunk_with(1, 2)]),
            ],
        );
        history.selected_file = 1;
        history.current_line = Some(0);
        tab.history = Some(history);

        tab.history_prev_line();

        let history = tab.history.as_ref().unwrap();
        assert_eq!(history.selected_file, 0);
        assert_eq!(history.current_hunk, 0);
        assert_eq!(
            history.current_line, None,
            "a hunk-less file (binary / mode change) has no line to land on"
        );
    }

    // ── tour_next_line ──

    #[test]
    fn tour_next_line_is_a_noop_when_the_selected_file_is_out_of_range() {
        let mut tab = TabState::new_for_test(vec![]);
        let mut tour = two_pillar_tour();
        tour.selected_file = 9;
        tab.tour = Some(tour);

        tab.tour_next_line();

        let tour = tab.tour.as_ref().unwrap();
        assert_eq!(tour.current_line, None);
        assert_eq!(
            tour.selected_pillar, 0,
            "an out-of-range file must not move the pillar selection"
        );
    }

    #[test]
    fn tour_next_line_selects_the_first_line_when_nothing_is_selected() {
        let mut tab = TabState::new_for_test(vec![]);
        tab.tour = Some(two_pillar_tour());

        tab.tour_next_line();

        assert_eq!(tab.tour.as_ref().unwrap().current_line, Some(0));
    }

    #[test]
    fn tour_next_line_rolls_over_into_the_next_hunk() {
        let mut tab = TabState::new_for_test(vec![]);
        let mut tour = two_pillar_tour();
        tour.current_line = Some(2); // last line of hunk 0
        tab.tour = Some(tour);

        tab.tour_next_line();

        let tour = tab.tour.as_ref().unwrap();
        assert_eq!(tour.current_hunk, 1);
        assert_eq!(tour.current_line, Some(0));
        assert_eq!(tour.selected_pillar, 0, "staying inside a file stays in its pillar");
    }

    #[test]
    fn tour_next_line_crossing_into_the_next_file_follows_the_pillar() {
        let mut tab = TabState::new_for_test(vec![]);
        let mut tour = two_pillar_tour();
        tour.current_hunk = 1;
        tour.current_line = Some(1); // last line of the last hunk of file 0
        tab.tour = Some(tour);

        tab.tour_next_line();

        let tour = tab.tour.as_ref().unwrap();
        assert_eq!(tour.selected_file, 1);
        assert_eq!(tour.current_line, Some(0));
        assert_eq!(
            tour.selected_pillar, 1,
            "the left pillar list must follow the cursor across a pillar boundary"
        );
    }

    #[test]
    fn tour_next_line_stops_at_the_end_of_the_tour() {
        let mut tab = TabState::new_for_test(vec![]);
        let mut tour = two_pillar_tour();
        tour.selected_file = 1;
        tour.selected_pillar = 1;
        tour.current_line = Some(1); // last line of the last file
        tab.tour = Some(tour);

        tab.tour_next_line();

        let tour = tab.tour.as_ref().unwrap();
        assert_eq!(tour.selected_file, 1);
        assert_eq!(tour.current_line, Some(1));
    }

    // ── tour_prev_line ──

    #[test]
    fn tour_prev_line_selects_the_last_line_when_nothing_is_selected() {
        let mut tab = TabState::new_for_test(vec![]);
        tab.tour = Some(two_pillar_tour());

        tab.tour_prev_line();

        assert_eq!(tab.tour.as_ref().unwrap().current_line, Some(2));
    }

    #[test]
    fn tour_prev_line_moves_back_within_the_current_hunk() {
        let mut tab = TabState::new_for_test(vec![]);
        let mut tour = two_pillar_tour();
        tour.current_line = Some(2);
        tab.tour = Some(tour);

        tab.tour_prev_line();

        assert_eq!(tab.tour.as_ref().unwrap().current_line, Some(1));
    }

    #[test]
    fn tour_prev_line_steps_back_into_the_previous_hunks_last_line() {
        let mut tab = TabState::new_for_test(vec![]);
        let mut tour = two_pillar_tour();
        tour.current_hunk = 1;
        tour.current_line = Some(0);
        tab.tour = Some(tour);

        tab.tour_prev_line();

        let tour = tab.tour.as_ref().unwrap();
        assert_eq!(tour.current_hunk, 0);
        assert_eq!(tour.current_line, Some(2));
    }

    #[test]
    fn tour_prev_line_crossing_back_into_the_previous_file_follows_the_pillar() {
        let mut tab = TabState::new_for_test(vec![]);
        let mut tour = two_pillar_tour();
        tour.selected_file = 1;
        tour.selected_pillar = 1;
        tour.current_line = Some(0);
        tab.tour = Some(tour);

        tab.tour_prev_line();

        let tour = tab.tour.as_ref().unwrap();
        assert_eq!(tour.selected_file, 0);
        assert_eq!(tour.current_hunk, 1);
        assert_eq!(tour.current_line, Some(1));
        assert_eq!(
            tour.selected_pillar, 0,
            "stepping back across a pillar boundary must reselect the owning pillar"
        );
    }

    #[test]
    fn tour_prev_line_clears_the_selection_at_the_very_first_line() {
        let mut tab = TabState::new_for_test(vec![]);
        let mut tour = two_pillar_tour();
        tour.current_line = Some(0);
        tab.tour = Some(tour);

        tab.tour_prev_line();

        let tour = tab.tour.as_ref().unwrap();
        assert_eq!(tour.current_line, None);
        assert_eq!(tour.selected_file, 0);
    }

    #[test]
    fn tour_prev_line_clears_the_line_when_the_previous_file_has_no_hunks() {
        let mut tab = TabState::new_for_test(vec![]);
        let mut tour = tour_with(
            vec![
                diff_file("empty.rs", vec![]),
                diff_file("f1.rs", vec![hunk_with(1, 2)]),
            ],
            vec![(0, 1), (1, 2)],
        );
        tour.selected_file = 1;
        tour.selected_pillar = 1;
        tour.current_line = Some(0);
        tab.tour = Some(tour);

        tab.tour_prev_line();

        let tour = tab.tour.as_ref().unwrap();
        assert_eq!(tour.selected_file, 0);
        assert_eq!(tour.current_hunk, 0);
        assert_eq!(tour.current_line, None);
        assert_eq!(tour.selected_pillar, 0);
    }

    // ── tour_bulk_review_pillar ──

    #[test]
    fn tour_bulk_review_pillar_is_a_noop_without_tour_state() {
        let mut tab = TabState::new_for_test(vec![]);

        tab.tour_bulk_review_pillar();

        assert!(tab.reviewed.is_empty());
    }

    #[test]
    fn tour_bulk_review_pillar_is_a_noop_when_the_pillar_has_no_file_range() {
        let mut tab = TabState::new_for_test(vec![]);
        let mut tour = two_pillar_tour();
        tour.selected_pillar = 9;
        tab.tour = Some(tour);

        tab.tour_bulk_review_pillar();

        assert!(tab.reviewed.is_empty());
    }

    #[test]
    fn tour_bulk_review_pillar_marks_only_the_selected_pillars_files() {
        let tmp = tempfile::tempdir().unwrap();
        let mut tab = TabState::new_for_test(vec![]);
        tab.er_root = ErRoot::RepoLocal(tmp.path().to_string_lossy().to_string());
        let mut tour = tour_with(
            vec![
                diff_file("a.rs", vec![]),
                diff_file("b.rs", vec![]),
                diff_file("c.rs", vec![]),
            ],
            vec![(0, 1), (1, 3)],
        );
        tour.selected_pillar = 1;
        tab.tour = Some(tour);
        tab.current_per_file_hashes
            .insert("b.rs".to_string(), "hash-b".to_string());

        tab.tour_bulk_review_pillar();

        let mut marked: Vec<&str> = tab.reviewed.keys().map(String::as_str).collect();
        marked.sort_unstable();
        assert_eq!(
            marked,
            vec!["b.rs", "c.rs"],
            "files outside the selected pillar stay unreviewed"
        );
        assert_eq!(
            tab.reviewed.get("b.rs").map(String::as_str),
            Some("hash-b"),
            "the file's current diff hash is stored, so a later edit un-reviews it"
        );
        assert_eq!(
            tab.reviewed.get("c.rs").map(String::as_str),
            Some(""),
            "a file with no known hash is still marked, with an empty hash"
        );
        assert!(
            tmp.path().join(".er").join("reviewed").exists(),
            "the bulk mark is persisted to the shared reviewed sidecar"
        );
    }
}
