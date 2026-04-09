use super::*;

impl TabState {
    pub fn enter_quiz_mode(&mut self) {
        let questions = match &self.ai.quiz {
            Some(q) => q.questions.clone(),
            None => return,
        };

        // Select the first question's related file in the diff view
        if let Some(first) = questions.first() {
            if !first.related_file.is_empty() {
                if let Some(idx) = self.files.iter().position(|f| f.path == first.related_file) {
                    self.selected_file = idx;
                    if let Some(hunk) = first.related_hunk {
                        if hunk < self.total_hunks() {
                            self.current_hunk = hunk;
                        }
                    }
                    self.diff_scroll = 0;
                    self.scroll_to_current_hunk();
                }
            }
        }

        self.quiz = Some(QuizState {
            questions,
            current: 0,
            answers: HashMap::new(),
            score: (0, 0),
            filter_level: None,
            filter_category: None,
            input_mode: QuizInputMode::Navigating,
            input_buffer: String::new(),
            show_explanation: false,
        });
    }

    /// Get the filtered question indices based on current filter settings.
    pub fn quiz_visible_indices(&self) -> Vec<usize> {
        let quiz = match &self.quiz {
            Some(q) => q,
            None => return Vec::new(),
        };
        quiz.questions
            .iter()
            .enumerate()
            .filter(|(_, q)| {
                if let Some(level) = quiz.filter_level {
                    if q.level != level {
                        return false;
                    }
                }
                if let Some(ref cat) = quiz.filter_category {
                    if &q.category != cat {
                        return false;
                    }
                }
                true
            })
            .map(|(i, _)| i)
            .collect()
    }

    /// Navigate to next question (within filtered list).
    pub fn quiz_next(&mut self) {
        let visible = self.quiz_visible_indices();
        if visible.is_empty() {
            return;
        }
        let quiz = match &self.quiz {
            Some(q) => q,
            None => return,
        };
        let current_pos = visible.iter().position(|&i| i == quiz.current);
        let next_pos = match current_pos {
            Some(p) if p + 1 < visible.len() => p + 1,
            _ => 0,
        };
        let next_idx = visible[next_pos];
        if let Some(q) = self.quiz.as_mut() {
            q.current = next_idx;
            q.show_explanation = false;
        }
        self.quiz_sync_diff_view();
    }

    /// Navigate to previous question (within filtered list).
    pub fn quiz_prev(&mut self) {
        let visible = self.quiz_visible_indices();
        if visible.is_empty() {
            return;
        }
        let quiz = match &self.quiz {
            Some(q) => q,
            None => return,
        };
        let current_pos = visible.iter().position(|&i| i == quiz.current);
        let prev_pos = match current_pos {
            Some(0) | None => visible.len().saturating_sub(1),
            Some(p) => p - 1,
        };
        let prev_idx = visible[prev_pos];
        if let Some(q) = self.quiz.as_mut() {
            q.current = prev_idx;
            q.show_explanation = false;
        }
        self.quiz_sync_diff_view();
    }

    /// Answer a multiple-choice question. Returns true if correct.
    pub fn quiz_answer_mc(&mut self, label: char) -> bool {
        let quiz = match self.quiz.as_mut() {
            Some(q) => q,
            None => return false,
        };
        let current_idx = quiz.current;
        // Only answer if not already answered
        if quiz.answers.contains_key(&quiz.questions[current_idx].id) {
            return false;
        }
        let question = &quiz.questions[current_idx];
        let is_correct = question
            .options
            .as_ref()
            .and_then(|opts| opts.iter().find(|o| o.label == label))
            .map(|o| o.is_correct)
            .unwrap_or(false);

        let id = question.id.clone();
        quiz.answers.insert(id, QuizAnswer::Choice(label));
        quiz.score.1 += 1;
        if is_correct {
            quiz.score.0 += 1;
        }
        quiz.show_explanation = true;
        is_correct
    }

    /// Submit the freeform answer from input_buffer.
    pub fn quiz_submit_freeform(&mut self) {
        let quiz = match self.quiz.as_mut() {
            Some(q) => q,
            None => return,
        };
        let current_idx = quiz.current;
        if quiz.answers.contains_key(&quiz.questions[current_idx].id) {
            return;
        }
        let text = quiz.input_buffer.trim().to_string();
        if text.is_empty() {
            return;
        }
        let id = quiz.questions[current_idx].id.clone();
        quiz.answers.insert(id, QuizAnswer::Freeform(text));
        quiz.score.1 += 1;
        quiz.input_buffer.clear();
        quiz.input_mode = QuizInputMode::Navigating;
        quiz.show_explanation = true;
    }

    /// Set level filter (None = all levels).
    pub fn quiz_filter_level(&mut self, level: Option<u8>) {
        if let Some(q) = self.quiz.as_mut() {
            q.filter_level = level;
            // Re-snap current to first visible
            let visible = self.quiz_visible_indices();
            if let Some(q) = self.quiz.as_mut() {
                if !visible.is_empty() && !visible.contains(&q.current) {
                    q.current = visible[0];
                    q.show_explanation = false;
                }
            }
            self.quiz_sync_diff_view();
        }
    }

    /// Sync the diff view to the related file/hunk of the current question.
    fn quiz_sync_diff_view(&mut self) {
        let (file, hunk) = match &self.quiz {
            Some(q) => {
                if let Some(question) = q.questions.get(q.current) {
                    (question.related_file.clone(), question.related_hunk)
                } else {
                    return;
                }
            }
            None => return,
        };
        if file.is_empty() {
            return;
        }
        if let Some(idx) = self.files.iter().position(|f| f.path == file) {
            self.selected_file = idx;
            if let Some(h) = hunk {
                if h < self.total_hunks() {
                    self.current_hunk = h;
                } else {
                    self.current_hunk = 0;
                }
            } else {
                self.current_hunk = 0;
            }
            self.diff_scroll = 0;
            self.scroll_to_current_hunk();
        }
    }

    /// Write answers to .er/quiz-answers.json atomically.
    pub fn quiz_save_answers(&self) -> Result<()> {
        let quiz = match &self.quiz {
            Some(q) => q,
            None => return Ok(()),
        };
        let er_dir = self.er_dir();
        std::fs::create_dir_all(&er_dir)?;

        let answers_json: Vec<serde_json::Value> = quiz
            .answers
            .iter()
            .map(|(id, answer)| match answer {
                QuizAnswer::Choice(c) => serde_json::json!({
                    "question_id": id,
                    "answer_type": "choice",
                    "value": c.to_string()
                }),
                QuizAnswer::Freeform(text) => serde_json::json!({
                    "question_id": id,
                    "answer_type": "freeform",
                    "value": text
                }),
            })
            .collect();

        let payload = serde_json::json!({
            "version": 1,
            "diff_hash": self.branch_diff_hash,
            "answers": answers_json
        });

        let json = serde_json::to_string_pretty(&payload)?;
        let path = format!("{}/quiz-answers.json", er_dir);
        let tmp_path = format!("{}.tmp", path);
        std::fs::write(&tmp_path, &json)?;
        std::fs::rename(&tmp_path, &path)?;
        Ok(())
    }
}
