use super::*;

impl TabState {
    pub fn next_file(&mut self) {
        self.focused_comment_id = None;
        self.focused_finding_id = None;
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

    /// Get the new-side line number for the currently selected line
    pub fn current_line_number(&self) -> Option<usize> {
        let file = self.selected_diff_file()?;
        let hunk = file.hunks.get(self.current_hunk)?;
        let line_idx = self.current_line?;
        let diff_line = hunk.lines.get(line_idx)?;
        diff_line.new_num
    }

    /// Get the line number for the focused side in split diff view
    pub fn current_line_number_for_split(&self, side: SplitSide) -> Option<usize> {
        let file = self.selected_diff_file()?;
        let hunk = file.hunks.get(self.current_hunk)?;
        let line_idx = self.current_line?;
        let diff_line = hunk.lines.get(line_idx)?;
        match side {
            SplitSide::Old => diff_line.old_num,
            SplitSide::New => diff_line.new_num,
        }
    }

    /// Increment the focused pane's horizontal scroll in split diff view
    pub fn scroll_right_split(&mut self) {
        match self.split_focus {
            SplitSide::Old => self.h_scroll_old = self.h_scroll_old.saturating_add(1),
            SplitSide::New => self.h_scroll_new = self.h_scroll_new.saturating_add(1),
        }
    }

    /// Decrement the focused pane's horizontal scroll in split diff view
    pub fn scroll_left_split(&mut self) {
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
                // TODO(risk:medium): base + current_line can overflow usize on pathological inputs
                // (e.g., a hunk with usize::MAX lines). saturating_sub then .min(u16::MAX) masks the overflow
                // rather than preventing it. Add a bounds check on current_line before the addition.
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

    pub fn panel_scroll_down(&mut self, amount: u16) {
        self.panel_scroll = self.panel_scroll.saturating_add(amount);
    }

    pub fn panel_scroll_up(&mut self, amount: u16) {
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

            // TODO(risk:high): file.hunks.len() - 1 panics if hunks is empty. The early return above (line ~1699)
            // guards against this, but only for the case where hunks.is_empty() at the top of the function.
            // If a refactor moves or removes that guard, this becomes an OOB panic. Use saturating_sub(1) here.
            found.unwrap_or_else(|| {
                // Past the end — clamp to last line of last hunk
                let last = file.hunks.len() - 1;
                (last, file.hunks[last].lines.len().saturating_sub(1))
            })
        };

        self.current_hunk = result.0;
        self.current_line = Some(result.1);
    }

    pub fn scroll_right(&mut self, amount: u16) {
        self.h_scroll = self.h_scroll.saturating_add(amount);
    }

    pub fn scroll_left(&mut self, amount: u16) {
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
        if !self.lazy_mode {
            return;
        }
        if let Some(file) = self.files.get(self.selected_file) {
            // Already parsed (has hunks) or is compacted — skip
            if !file.hunks.is_empty() || file.compacted {
                return;
            }
        }
        // Parse on demand from raw diff — look up header by path (not index) to handle mtime sort
        let path = self.files.get(self.selected_file).map(|f| f.path.clone());
        let header_idx = path
            .as_ref()
            .and_then(|p| self.file_headers.iter().position(|h| h.path == *p));
        if let (Some(ref raw), Some(idx)) = (&self.raw_diff, header_idx) {
            let header = &self.file_headers[idx];
            let parsed = git::parse_file_at_offset(raw, header);
            if !parsed.hunks.is_empty() {
                if let Some(file) = self.files.get_mut(self.selected_file) {
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
            if let Some(file) = self.files.get(self.selected_file) {
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
                            if let Some(file) = self.files.get_mut(self.selected_file) {
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
            if is_remote {
                // Remote mode: re-parse from raw_diff
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
            // TODO(risk:minor): any file can be re-compacted via Enter regardless of whether it originally
            // matched a compaction pattern. A file that was never auto-compacted (user navigated to it
            // in eager mode) still gets compacted on the second Enter press, which may be surprising.
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

    /// Context level progression for expand/collapse
    const CONTEXT_STEPS: &'static [usize] = &[3, 10, 25, 50, 99999];

    /// Expand context lines for the currently selected file.
    /// Steps through increasing context levels: 3 → 10 → 25 → 50 → full.
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
        let current = self.context_overrides.get(&path).copied().unwrap_or(3);

        // Find next step above current
        let next = Self::CONTEXT_STEPS
            .iter()
            .copied()
            .find(|&s| s > current)
            .unwrap_or(99999);
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
    /// Steps back through context levels: full → 50 → 25 → 10 → 3.
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
        let current = self.context_overrides.get(&path).copied().unwrap_or(3);

        if current <= 3 {
            return Ok(());
        }

        // Find previous step below current
        let prev = Self::CONTEXT_STEPS
            .iter()
            .rev()
            .copied()
            .find(|&s| s < current)
            .unwrap_or(3);

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

        if prev == 3 {
            self.context_overrides.remove(&path);
        } else {
            self.context_overrides.insert(path, prev);
        }
        self.rebuild_hunk_offsets();
        self.update_mem_budget();
        Ok(())
    }

    /// Auto-expand context for small files on selection.
    /// Files with total diff lines ≤ threshold get full context automatically.
    pub fn maybe_auto_expand_context(&mut self, threshold: usize) {
        if threshold == 0 || self.mode == DiffMode::History {
            return;
        }

        let file = match self.files.get(self.selected_file) {
            Some(f) => f,
            None => return,
        };

        // Skip compacted, already-overridden, or untracked files
        if file.compacted || self.context_overrides.contains_key(&file.path) {
            return;
        }
        if file.status == git::FileStatus::Added && file.hunks.len() <= 1 {
            return;
        }

        let total_lines: usize = file.hunks.iter().map(|h| h.lines.len()).sum();
        if total_lines > threshold || total_lines == 0 {
            return;
        }

        let path = file.path.clone();
        if let Some(file) = self.files.get_mut(self.selected_file) {
            if git::refetch_file_with_context(
                file,
                &self.repo_root,
                self.mode.git_mode(),
                &self.base_branch,
                99999,
                self.pr_head_ref.as_deref(),
            )
            .is_ok()
            {
                self.context_overrides.insert(path, 99999);
                self.rebuild_hunk_offsets();
                self.update_mem_budget();
            }
        }
    }

    /// Get cached visible files, rebuilding cache if needed
    #[allow(dead_code)]
    pub fn visible_files_cached(&mut self) -> Vec<usize> {
        let reviewed_count = self.reviewed.len();
        let needs_rebuild = match &self.file_tree_cache {
            Some(cache) => {
                cache.search_query != self.search_query
                    || cache.show_unreviewed_only != self.show_unreviewed_only
                    || cache.file_count != self.files.len()
                    || cache.reviewed_count != reviewed_count
            }
            None => true,
        };

        if needs_rebuild {
            let visible = self
                .visible_files()
                .iter()
                .map(|(i, _)| *i)
                .collect::<Vec<_>>();
            self.file_tree_cache = Some(FileTreeCache {
                visible: visible.clone(),
                search_query: self.search_query.clone(),
                show_unreviewed_only: self.show_unreviewed_only,
                file_count: self.files.len(),
                reviewed_count,
            });
            visible
        } else {
            self.file_tree_cache.as_ref().unwrap().visible.clone()
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
    fn history_load_selected_diff(&mut self) {
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
            // TODO(risk:medium): cached.clone() copies the entire Vec<DiffFile> including all hunk lines.
            // For a commit with thousands of changed lines this is an expensive allocation on every
            // back-navigation to a cached commit. Consider storing Arcs or indices instead of cloning.
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

        let files = match git::git_diff_commit(&hash, &repo_root) {
            Ok(raw) => git::parse_diff(&raw),
            Err(_) => vec![],
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

        // TODO(risk:medium): git_log_branch is called synchronously on the event loop thread. Loading 50
        // commits on a slow filesystem or network-mounted repo blocks the UI for the full duration of the
        // git log call. This should be moved to a background thread like the PR hint check.
        let new_commits =
            git::git_log_branch(&self.base_branch, &self.repo_root, 50, skip).unwrap_or_default();

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
    pub fn history_scroll_down(&mut self, amount: u16) {
        if let Some(ref mut h) = self.history {
            h.diff_scroll = h.diff_scroll.saturating_add(amount);
        }
    }

    /// Scroll up in history mode
    pub fn history_scroll_up(&mut self, amount: u16) {
        if let Some(ref mut h) = self.history {
            h.diff_scroll = h.diff_scroll.saturating_sub(amount);
        }
    }

    /// Scroll right in history mode
    pub fn history_scroll_right(&mut self, amount: u16) {
        if let Some(ref mut h) = self.history {
            h.h_scroll = h.h_scroll.saturating_add(amount);
        }
    }

    /// Scroll left in history mode
    pub fn history_scroll_left(&mut self, amount: u16) {
        if let Some(ref mut h) = self.history {
            h.h_scroll = h.h_scroll.saturating_sub(amount);
        }
    }
}
