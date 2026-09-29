use super::*;

impl TabState {
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
            let hash = self.per_file_hash(&path);
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
            let hash = self.per_file_hash(&path);
            self.reviewed.insert(path, hash);
        }
        let _ = self.save_reviewed_files();
    }
}
