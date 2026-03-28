use super::*;

impl TabState {
    pub fn enter_wizard_mode(&mut self) {
        // Build ordered list from ErOrder if available, else use diff file order
        let ordered: Vec<String> = if let Some(ref order) = self.ai.order {
            order.order.iter().map(|e| e.path.clone()).collect()
        } else {
            self.files.iter().map(|f| f.path.clone()).collect()
        };

        // Filter: exclude Info-risk files with no findings (hide them)
        let mut visible_files: Vec<String> = Vec::new();
        let mut hidden_count = 0usize;

        for path in &ordered {
            let should_hide = if let Some(ref review) = self.ai.review {
                if let Some(fr) = review.files.get(path) {
                    fr.risk == crate::ai::RiskLevel::Info && fr.findings.is_empty()
                } else {
                    false
                }
            } else {
                false
            };
            if should_hide {
                hidden_count += 1;
            } else {
                visible_files.push(path.clone());
            }
        }

        // Also include files in diff that weren't in the order list
        for file in &self.files {
            if !ordered.contains(&file.path) {
                visible_files.push(file.path.clone());
            }
        }

        // Set selected_file to first wizard file
        if let Some(first) = visible_files.first() {
            if let Some(idx) = self.files.iter().position(|f| &f.path == first) {
                self.selected_file = idx;
            }
        }

        self.wizard = Some(WizardState {
            ordered_files: visible_files,
            current_step: 0,
            completed: HashSet::new(),
            hidden_count,
            show_hidden: false,
        });
    }

    /// Mark the current wizard file as reviewed and advance to the next unreviewed file.
    pub fn wizard_mark_reviewed(&mut self) {
        let wizard = match self.wizard.as_mut() {
            Some(w) => w,
            None => return,
        };

        if wizard.current_step < wizard.ordered_files.len() {
            let path = wizard.ordered_files[wizard.current_step].clone();
            wizard.completed.insert(path.clone());
            // Also mark in the main reviewed map
            let hash = self
                .current_per_file_hashes
                .get(&path)
                .cloned()
                .unwrap_or_default();
            self.reviewed.insert(path, hash);
        }

        self.wizard_next_unreviewed();
    }

    /// Advance to the next unreviewed file in wizard order.
    pub fn wizard_next_unreviewed(&mut self) {
        let wizard = match self.wizard.as_mut() {
            Some(w) => w,
            None => return,
        };

        let len = wizard.ordered_files.len();
        if len == 0 {
            return;
        }

        // Find next unreviewed file after current_step
        let start = (wizard.current_step + 1) % len;
        for i in 0..len {
            let idx = (start + i) % len;
            let path = &wizard.ordered_files[idx];
            if !wizard.completed.contains(path) {
                wizard.current_step = idx;
                // Update selected_file to match
                if let Some(file_idx) = self.files.iter().position(|f| &f.path == path) {
                    self.selected_file = file_idx;
                    self.current_hunk = 0;
                    self.diff_scroll = 0;
                }
                return;
            }
        }
        // All reviewed — stay at current
    }

    // ── Quiz Mode ──
}
