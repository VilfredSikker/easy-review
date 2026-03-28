use crate::app;
use crate::app::{App, DiffMode};
use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

pub fn handle_quiz_input(app: &mut App, key: KeyEvent) -> Result<()> {
    use app::QuizInputMode;

    // Check if we're in freeform input mode
    let in_freeform = app
        .tab()
        .quiz
        .as_ref()
        .map(|q| q.input_mode == QuizInputMode::AnsweringFreeform)
        .unwrap_or(false);

    if in_freeform {
        match key.code {
            KeyCode::Enter => {
                app.tab_mut().quiz_submit_freeform();
                let _ = app.tab().quiz_save_answers();
            }
            KeyCode::Esc => {
                if let Some(q) = app.tab_mut().quiz.as_mut() {
                    q.input_mode = QuizInputMode::Navigating;
                    q.input_buffer.clear();
                }
            }
            KeyCode::Backspace => {
                if let Some(q) = app.tab_mut().quiz.as_mut() {
                    q.input_buffer.pop();
                }
            }
            KeyCode::Char(c) => {
                if let Some(q) = app.tab_mut().quiz.as_mut() {
                    q.input_buffer.push(c);
                }
            }
            _ => {}
        }
        return Ok(());
    }

    match key.code {
        // Navigation
        KeyCode::Char('j') | KeyCode::Char('n') => {
            app.tab_mut().quiz_next();
        }
        KeyCode::Char('k') | KeyCode::Char('N') => {
            app.tab_mut().quiz_prev();
        }

        // MC answers (A-D, case insensitive)
        KeyCode::Char(c @ 'a'..='d') | KeyCode::Char(c @ 'A'..='D') => {
            let label = c.to_ascii_uppercase();
            let is_mc = app
                .tab()
                .quiz
                .as_ref()
                .and_then(|q| q.questions.get(q.current))
                .map(|q| q.options.is_some() && !q.freeform)
                .unwrap_or(false);
            if is_mc {
                let correct = app.tab_mut().quiz_answer_mc(label);
                let _ = app.tab().quiz_save_answers();
                if correct {
                    app.notify("Correct!");
                } else {
                    app.notify("Incorrect — see explanation below");
                }
            }
        }

        // Enter to start freeform input (or cycle to next if already answered)
        KeyCode::Enter => {
            let is_freeform = app
                .tab()
                .quiz
                .as_ref()
                .and_then(|q| q.questions.get(q.current))
                .map(|q| q.freeform)
                .unwrap_or(false);
            let already_answered = app
                .tab()
                .quiz
                .as_ref()
                .map(|q| {
                    let id = q
                        .questions
                        .get(q.current)
                        .map(|x| x.id.as_str())
                        .unwrap_or("");
                    q.answers.contains_key(id)
                })
                .unwrap_or(false);
            if is_freeform && !already_answered {
                if let Some(q) = app.tab_mut().quiz.as_mut() {
                    q.input_mode = QuizInputMode::AnsweringFreeform;
                }
            }
        }

        // Level filters
        KeyCode::Char('1') if key.modifiers == KeyModifiers::NONE => {
            let current_filter = app.tab().quiz.as_ref().and_then(|q| q.filter_level);
            let new_filter = if current_filter == Some(1) {
                None
            } else {
                Some(1)
            };
            app.tab_mut().quiz_filter_level(new_filter);
        }
        KeyCode::Char('2') if key.modifiers == KeyModifiers::NONE => {
            let current_filter = app.tab().quiz.as_ref().and_then(|q| q.filter_level);
            let new_filter = if current_filter == Some(2) {
                None
            } else {
                Some(2)
            };
            app.tab_mut().quiz_filter_level(new_filter);
        }
        KeyCode::Char('3') if key.modifiers == KeyModifiers::NONE => {
            let current_filter = app.tab().quiz.as_ref().and_then(|q| q.filter_level);
            let new_filter = if current_filter == Some(3) {
                None
            } else {
                Some(3)
            };
            app.tab_mut().quiz_filter_level(new_filter);
        }
        KeyCode::Char('0') => {
            app.tab_mut().quiz_filter_level(None);
        }

        // Toggle explanation
        KeyCode::Char('e') => {
            if let Some(q) = app.tab_mut().quiz.as_mut() {
                q.show_explanation = !q.show_explanation;
            }
        }

        // Exit quiz mode — go back to Branch
        KeyCode::Esc => {
            app.tab_mut().set_mode(DiffMode::Branch);
        }

        _ => {}
    }

    Ok(())
}
