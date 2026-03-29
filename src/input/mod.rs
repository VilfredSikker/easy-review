use crate::app;
use crate::app::{
    cleanup_question_answers, cleanup_questions, cleanup_reviews, App, ConfirmAction, DiffMode,
    HubAction, InputMode,
};
use crate::{git, github};
use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

pub mod normal;
pub mod quiz;
pub mod wizard;

pub use normal::handle_normal_input;

pub fn handle_overlay_input(app: &mut App, key: KeyEvent) -> Result<()> {
    // Config hub overlay — handles inline string editing
    if matches!(app.overlay, Some(app::OverlayData::ConfigHub { .. })) {
        let is_editing = matches!(
            &app.overlay,
            Some(app::OverlayData::ConfigHub {
                editing: Some(_),
                ..
            })
        );

        if is_editing {
            match key.code {
                KeyCode::Char(c) => {
                    if let Some(app::OverlayData::ConfigHub {
                        editing: Some(ref mut edit),
                        ..
                    }) = &mut app.overlay
                    {
                        edit.buffer.insert(edit.cursor_pos, c);
                        edit.cursor_pos += c.len_utf8();
                    }
                }
                KeyCode::Backspace => {
                    if let Some(app::OverlayData::ConfigHub {
                        editing: Some(ref mut edit),
                        ..
                    }) = &mut app.overlay
                    {
                        if edit.cursor_pos > 0 {
                            // Find the char boundary before cursor_pos
                            let prev_boundary = edit.buffer[..edit.cursor_pos]
                                .char_indices()
                                .last()
                                .map(|(i, _)| i)
                                .unwrap_or(0);
                            edit.cursor_pos = prev_boundary;
                            edit.buffer.remove(edit.cursor_pos);
                        }
                    }
                }
                KeyCode::Left => {
                    if let Some(app::OverlayData::ConfigHub {
                        editing: Some(ref mut edit),
                        ..
                    }) = &mut app.overlay
                    {
                        if edit.cursor_pos > 0 {
                            let prev_boundary = edit.buffer[..edit.cursor_pos]
                                .char_indices()
                                .last()
                                .map(|(i, _)| i)
                                .unwrap_or(0);
                            edit.cursor_pos = prev_boundary;
                        }
                    }
                }
                KeyCode::Right => {
                    if let Some(app::OverlayData::ConfigHub {
                        editing: Some(ref mut edit),
                        ..
                    }) = &mut app.overlay
                    {
                        if edit.cursor_pos < edit.buffer.len() {
                            let next_char = edit.buffer[edit.cursor_pos..]
                                .chars()
                                .next()
                                .unwrap_or('\0');
                            edit.cursor_pos += next_char.len_utf8();
                        }
                    }
                }
                KeyCode::Enter => {
                    app.config_hub_confirm_edit();
                }
                KeyCode::Esc => {
                    app.config_hub_cancel_edit();
                }
                _ => {}
            }
        } else {
            match key.code {
                KeyCode::Char('j') | KeyCode::Down => app.overlay_next(),
                KeyCode::Char('k') | KeyCode::Up => app.overlay_prev(),
                KeyCode::Enter | KeyCode::Char(' ') => app.config_hub_activate(),
                KeyCode::Char('d') => app.config_hub_delete_selected(),
                KeyCode::Char('s') => app.config_hub_save_local(),
                KeyCode::Char('S') => app.config_hub_save_global(),
                KeyCode::Esc | KeyCode::Char('q') => app.config_hub_cancel(),
                _ => {}
            }
        }
        return Ok(());
    }

    match key.code {
        KeyCode::Char('j') | KeyCode::Down => app.overlay_next(),
        KeyCode::Char('k') | KeyCode::Up => app.overlay_prev(),
        KeyCode::Enter => {
            app.overlay_select()?;
            // Dispatch pending hub action if overlay_select set one
            if let Some(action) = app.pending_hub_action.take() {
                app.overlay = None;
                dispatch_hub_action(app, action)?;
            }
        }
        KeyCode::Backspace => app.overlay_go_up(),
        KeyCode::Esc | KeyCode::Char('q') => app.overlay_close(),
        _ => {}
    }
    Ok(())
}

pub(super) fn dispatch_hub_action(app: &mut App, action: HubAction) -> Result<()> {
    match action {
        HubAction::Noop => {}
        HubAction::PushToRemote => {
            if app.tab().mode == DiffMode::Staged {
                app.input_mode = InputMode::Confirm(ConfirmAction::Push);
            }
        }
        HubAction::PullGitHubComments => {
            sync_github_comments(app)?;
        }
        HubAction::PushCommentsToGitHub => {
            app.input_mode = InputMode::Confirm(ConfirmAction::PushComments);
        }
        HubAction::CommentOnPR => {
            app.start_general_comment();
        }
        HubAction::RefreshDiff => {
            app.tab_mut().refresh_diff()?;
            app.notify("Refreshed");
        }
        HubAction::StageFile => {
            app.toggle_stage_file()?;
        }
        HubAction::StageAll => {
            app.stage_all()?;
        }
        HubAction::CopyContext => {
            app.copy_context()?;
        }
        HubAction::ToggleAiFindings => {
            app.tab_mut().toggle_layer_ai();
            let on = app.tab().layers.show_ai_findings;
            app.notify(if on {
                "AI findings: ON"
            } else {
                "AI findings: OFF"
            });
        }
        HubAction::ToggleComments => {
            app.tab_mut().toggle_layer_comments();
            let on = app.tab().layers.show_github_comments;
            app.notify(if on {
                "Comments: visible"
            } else {
                "Comments: hidden"
            });
        }
        HubAction::ToggleQuestions => {
            app.tab_mut().toggle_layer_questions();
            let on = app.tab().layers.show_questions;
            app.notify(if on {
                "Questions: visible"
            } else {
                "Questions: hidden"
            });
        }
        HubAction::CleanupQuestions => {
            let count = app
                .tab()
                .ai
                .questions
                .as_ref()
                .map_or(0, |q| q.questions.len());
            app.input_mode = InputMode::Confirm(ConfirmAction::CleanupQuestions { count });
        }
        HubAction::CleanupReviews => {
            let count = app.tab().ai.review.as_ref().map_or(0, |r| r.files.len());
            app.input_mode = InputMode::Confirm(ConfirmAction::CleanupReviews { count });
        }
        HubAction::RunCommand(name) => {
            if let Some(cmd) = app.config.resolve_command(&name) {
                app.spawn_command(&name, &cmd)?;
            } else {
                app.notify(&format!("{}: not configured", name));
            }
        }
        HubAction::PromptReview => {
            let has_review = app.tab().ai.review.is_some();
            if has_review {
                // Ask to clear previous review first
                app.input_mode = InputMode::Confirm(ConfirmAction::RunAgentReview {
                    clear_previous: true,
                });
            } else {
                // No previous review, run directly
                if let Some(prompt) = build_agent_review_prompt(app) {
                    app.spawn_agent_prompt("review", &prompt)?;
                }
            }
        }
        HubAction::ApprovePR => {
            app.input_mode = InputMode::Confirm(ConfirmAction::ApprovePR);
        }
        HubAction::PromptQuestions => {
            let has_answers = app.tab().ai.questions.as_ref().is_some_and(|q| {
                q.questions
                    .iter()
                    .any(|q| q.in_reply_to.is_some() && q.author == "Claude")
            });
            if has_answers {
                // Ask to clear previous answers first
                app.input_mode = InputMode::Confirm(ConfirmAction::RunAgentQuestions {
                    clear_previous: true,
                });
            } else {
                // No previous answers, run directly
                if let Some(prompt) = build_agent_questions_prompt(app) {
                    app.spawn_agent_prompt("questions", &prompt)?;
                }
            }
        }
        HubAction::PromptQuiz => {
            if let Some(prompt) = build_agent_quiz_prompt(app) {
                app.spawn_agent_prompt("quiz", &prompt)?;
            }
        }
        HubAction::PromptQuizReview => {
            if let Some(prompt) = build_agent_quiz_review_prompt(app) {
                app.spawn_agent_prompt("quiz-review", &prompt)?;
            }
        }
        HubAction::PromptWizard => {
            if let Some(prompt) = build_agent_wizard_prompt(app) {
                app.spawn_agent_prompt("wizard", &prompt)?;
            }
        }
        HubAction::OpenDirectory => {
            app.open_directory_browser();
        }
        HubAction::OpenWorktree => {
            app.open_worktree_picker()?;
        }
        HubAction::OpenRemoteUrl => {
            app.remote_url_input.clear();
            app.input_mode = InputMode::RemoteUrl;
        }
        HubAction::OpenPrInBrowser => {
            let repo_root = app.tab().repo_root.clone();
            if let Some(pr_number) = app.tab().pr_number {
                let mut args = vec![
                    "pr".to_string(),
                    "view".to_string(),
                    pr_number.to_string(),
                    "--web".to_string(),
                ];
                if let Some(ref slug) = app.tab().remote_repo {
                    args.push("-R".to_string());
                    args.push(slug.clone());
                }
                if let Ok(mut child) = std::process::Command::new("gh")
                    .args(&args)
                    .current_dir(&repo_root)
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn()
                {
                    std::thread::spawn(move || {
                        let _ = child.wait();
                    });
                }
                app.notify("Opening PR in browser...");
            }
        }
        HubAction::CopyFullFile => {
            app.copy_full_file()?;
        }
        HubAction::CopyFilePath => {
            app.copy_file_path()?;
        }
        HubAction::CopyHunk => {
            app.yank_hunk()?;
        }
        HubAction::CopyLine => {
            app.copy_line()?;
        }
    }
    Ok(())
}

pub fn handle_search_input(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Enter | KeyCode::Esc => {
            app.input_mode = InputMode::Normal;
            if key.code == KeyCode::Esc {
                let tab = app.tab_mut();
                tab.search_query.clear();
                tab.search_query_lower.clear();
            } else {
                // Search confirmed — snap selection to a visible file
                app.tab_mut().snap_to_visible();
            }
        }
        KeyCode::Char(c) => {
            let tab = app.tab_mut();
            tab.search_query.push(c);
            tab.search_query_lower = tab.search_query.to_lowercase();
        }
        KeyCode::Backspace => {
            let tab = app.tab_mut();
            tab.search_query.pop();
            tab.search_query_lower = tab.search_query.to_lowercase();
        }
        _ => {}
    }
}

pub fn handle_filter_input(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Enter => {
            let expr = app.tab().filter_input.clone();
            app.tab_mut().apply_filter_expr(&expr);
            app.input_mode = InputMode::Normal;
            if expr.trim().is_empty() {
                app.notify("Filter cleared");
            } else {
                let visible = app.tab().visible_files().len();
                let total = app.tab().files.len();
                app.notify(&format!("Filter: {} ({}/{})", expr.trim(), visible, total));
            }
        }
        KeyCode::Esc => {
            app.tab_mut().filter_input.clear();
            app.input_mode = InputMode::Normal;
        }
        KeyCode::Char(c) => {
            app.tab_mut().filter_input.push(c);
        }
        KeyCode::Backspace => {
            app.tab_mut().filter_input.pop();
        }
        _ => {}
    }
}

pub fn handle_remote_url_input(app: &mut App, key: KeyEvent) -> Result<()> {
    match key.code {
        KeyCode::Enter => {
            let url = app.remote_url_input.clone();
            app.input_mode = InputMode::Normal;
            if url.trim().is_empty() {
                return Ok(());
            }
            if let Err(e) = app.open_remote_url(url.trim()) {
                app.notify(&format!("Failed: {}", e));
            }
            app.remote_url_input.clear();
        }
        KeyCode::Esc => {
            app.remote_url_input.clear();
            app.input_mode = InputMode::Normal;
        }
        KeyCode::Char(c) => {
            app.remote_url_input.push(c);
        }
        KeyCode::Backspace => {
            app.remote_url_input.pop();
        }
        _ => {}
    }
    Ok(())
}

pub fn handle_comment_input(app: &mut App, key: KeyEvent) -> Result<()> {
    match key.code {
        KeyCode::Enter => {
            app.submit_comment()?;
        }
        KeyCode::Esc => {
            app.cancel_comment();
        }
        // Scroll the diff view while composing a comment
        KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.tab_mut().scroll_down(10);
        }
        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.tab_mut().scroll_up(10);
        }
        KeyCode::PageDown => app.tab_mut().scroll_down(20),
        KeyCode::PageUp => app.tab_mut().scroll_up(20),
        KeyCode::Char(c) => {
            app.tab_mut().comment_input.push(c);
        }
        KeyCode::Backspace => {
            app.tab_mut().comment_input.pop();
        }
        _ => {}
    }
    Ok(())
}

pub fn handle_confirm_input(app: &mut App, key: KeyEvent) -> Result<()> {
    match key.code {
        KeyCode::Char('y') => {
            let action = app.input_mode.clone();
            if let InputMode::Confirm(ConfirmAction::DeleteComment { comment_id }) = action {
                app.confirm_delete_comment(&comment_id)?;
            } else if let InputMode::Confirm(ConfirmAction::Push) = action {
                app.input_mode = InputMode::Normal;
                let repo_root = app.tab().repo_root.clone();
                match git::git_push(&repo_root) {
                    Ok(_) => {
                        app.tab_mut().committed_unpushed = false;
                        let _ = app.tab_mut().refresh_diff();
                        app.notify("Pushed!");
                    }
                    Err(e) => {
                        app.notify(&format!("Push failed: {}", e));
                    }
                }
            } else if let InputMode::Confirm(ConfirmAction::CleanupQuestions { .. }) = action {
                app.input_mode = InputMode::Normal;
                let er_dir = app.tab().er_dir();
                cleanup_questions(&er_dir);
                app.tab_mut().reload_ai_state();
                app.notify("Questions cleared");
            } else if let InputMode::Confirm(ConfirmAction::DeleteWatchedFile { ref path }) = action
            {
                let full_path = format!("{}/{}", app.tab().repo_root, path);
                app.input_mode = InputMode::Normal;
                match std::fs::remove_file(&full_path) {
                    Ok(_) => {
                        app.tab_mut().refresh_watched_files();
                        // Clamp selection after removal
                        let count = app.tab().watched_files.len();
                        if count == 0 {
                            app.tab_mut().selected_watched = None;
                        } else if let Some(idx) = app.tab().selected_watched {
                            if idx >= count {
                                app.tab_mut().selected_watched = Some(count - 1);
                            }
                        }
                        app.notify(&format!("Deleted: {}", path));
                    }
                    Err(e) => {
                        app.notify(&format!("Delete failed: {}", e));
                    }
                }
            } else if let InputMode::Confirm(ConfirmAction::CleanupReviews { .. }) = action {
                app.input_mode = InputMode::Normal;
                let er_dir = app.tab().er_dir();
                cleanup_reviews(&er_dir);
                app.tab_mut().reload_ai_state();
                app.notify("Review cleared");
            } else if let InputMode::Confirm(ConfirmAction::RunAgentReview { .. }) = action {
                // User said "yes" to clearing previous review — clear, then run
                app.input_mode = InputMode::Normal;
                let er_dir = app.tab().er_dir();
                cleanup_reviews(&er_dir);
                app.tab_mut().reload_ai_state();
                if let Some(prompt) = build_agent_review_prompt(app) {
                    app.spawn_agent_prompt("review", &prompt)?;
                }
            } else if let InputMode::Confirm(ConfirmAction::RunAgentQuestions { .. }) = action {
                // User said "yes" to clearing previous answers — clear answers, then run
                app.input_mode = InputMode::Normal;
                let er_dir = app.tab().er_dir();
                cleanup_question_answers(&er_dir);
                app.tab_mut().reload_ai_state();
                if let Some(prompt) = build_agent_questions_prompt(app) {
                    app.spawn_agent_prompt("questions", &prompt)?;
                }
            } else if let InputMode::Confirm(ConfirmAction::ApprovePR) = action {
                app.input_mode = InputMode::Normal;
                let repo_root = app.tab().repo_root.clone();
                let remote = app.tab().remote_repo.clone();
                let pr = app.tab().pr_number;
                match crate::github::gh_pr_approve(&repo_root, remote.as_deref(), pr) {
                    Ok(()) => app.notify("PR approved"),
                    Err(e) => app.notify(&format!("Approve failed: {}", e)),
                }
            }
        }
        KeyCode::Char('r') => {
            if let InputMode::Confirm(ConfirmAction::PushComments) = &app.input_mode {
                app.input_mode = InputMode::Normal;
                push_comments_as_review(app)?;
            }
        }
        KeyCode::Char('i') => {
            if let InputMode::Confirm(ConfirmAction::PushComments) = &app.input_mode {
                app.input_mode = InputMode::Normal;
                push_all_comments_to_github(app)?;
            }
        }
        KeyCode::Char('n') => {
            app.cancel_confirm();
        }
        KeyCode::Char('k') => {
            // For agent prompts, 'k' = keep previous data but still run
            if let InputMode::Confirm(ConfirmAction::RunAgentReview { .. }) = &app.input_mode {
                app.input_mode = InputMode::Normal;
                if let Some(prompt) = build_agent_review_prompt(app) {
                    app.spawn_agent_prompt("review", &prompt)?;
                }
            } else if let InputMode::Confirm(ConfirmAction::RunAgentQuestions { .. }) =
                &app.input_mode
            {
                app.input_mode = InputMode::Normal;
                if let Some(prompt) = build_agent_questions_prompt(app) {
                    app.spawn_agent_prompt("questions", &prompt)?;
                }
            }
        }
        KeyCode::Esc => {
            app.cancel_confirm();
        }
        _ => {} // Ignore all other keys in confirm mode
    }
    Ok(())
}

pub fn handle_commit_input(app: &mut App, key: KeyEvent) -> Result<()> {
    match key.code {
        KeyCode::Enter => {
            app.submit_commit()?;
        }
        KeyCode::Esc => {
            app.cancel_commit();
        }
        KeyCode::Char(c) => {
            app.tab_mut().commit_input.push(c);
        }
        KeyCode::Backspace => {
            app.tab_mut().commit_input.pop();
        }
        _ => {}
    }
    Ok(())
}

/// Build the review agent prompt, using remote mode if applicable.
pub(super) fn build_agent_review_prompt(app: &mut App) -> Option<String> {
    let tab = app.tab();
    if tab.is_remote() {
        let (slug, pr_number) = match (&tab.remote_repo, tab.pr_number) {
            (Some(ref s), Some(n)) => (s.clone(), n),
            _ => {
                app.notify("Remote mode missing repo or PR number");
                return None;
            }
        };
        let parts: Vec<&str> = slug.split('/').collect();
        if parts.len() != 2 {
            app.notify(&format!("Invalid remote repo slug: {}", slug));
            return None;
        }
        let output_dir = app.tab().er_dir();
        return Some(crate::ai::prompts::build_review_prompt_remote(
            parts[0],
            parts[1],
            pr_number,
            &output_dir,
        ));
    }
    let mode = tab.mode;
    let base = tab.base_branch.clone();
    match mode {
        DiffMode::Branch | DiffMode::Unstaged | DiffMode::Staged => Some(
            crate::ai::prompts::build_review_prompt(&base, mode.git_mode()),
        ),
        _ => {
            app.notify("AI review not available in this mode");
            None
        }
    }
}

/// Build the questions agent prompt, using remote mode if applicable.
pub(super) fn build_agent_questions_prompt(app: &mut App) -> Option<String> {
    let tab = app.tab();
    if tab.is_remote() {
        let (slug, pr_number) = match (&tab.remote_repo, tab.pr_number) {
            (Some(ref s), Some(n)) => (s.clone(), n),
            _ => {
                app.notify("Remote mode missing repo or PR number");
                return None;
            }
        };
        let parts: Vec<&str> = slug.split('/').collect();
        if parts.len() != 2 {
            app.notify(&format!("Invalid remote repo slug: {}", slug));
            return None;
        }
        let output_dir = app.tab().er_dir();
        return Some(crate::ai::prompts::build_questions_prompt_remote(
            parts[0],
            parts[1],
            pr_number,
            &output_dir,
        ));
    }
    let mode = tab.mode;
    let base = tab.base_branch.clone();
    match mode {
        DiffMode::Branch | DiffMode::Unstaged | DiffMode::Staged => Some(
            crate::ai::prompts::build_questions_prompt(&base, mode.git_mode()),
        ),
        _ => {
            app.notify("AI questions not available in this mode");
            None
        }
    }
}

/// Build the quiz generation agent prompt.
pub(super) fn build_agent_quiz_prompt(app: &mut App) -> Option<String> {
    let tab = app.tab();
    let mode = tab.mode;
    let base = tab.base_branch.clone();
    let er_dir = tab.er_dir();
    match mode {
        DiffMode::Branch | DiffMode::Unstaged | DiffMode::Staged | DiffMode::Wizard => Some(
            format!("/er-quiz {} {} --output {}", mode.git_mode(), base, er_dir),
        ),
        _ => {
            app.notify("Quiz generation not available in this mode");
            None
        }
    }
}

/// Build the quiz answer review agent prompt.
pub(super) fn build_agent_quiz_review_prompt(app: &mut App) -> Option<String> {
    let tab = app.tab();
    let mode = tab.mode;
    let base = tab.base_branch.clone();
    let er_dir = tab.er_dir();
    let answers_path = format!("{}/quiz-answers.json", er_dir);
    if !std::path::Path::new(&answers_path).exists() {
        app.notify("No quiz answers found — take the quiz first (key 8)");
        return None;
    }
    match mode {
        DiffMode::Branch
        | DiffMode::Unstaged
        | DiffMode::Staged
        | DiffMode::Wizard
        | DiffMode::Quiz => Some(format!(
            "/er-quiz-review {} {} --output {}",
            mode.git_mode(),
            base,
            er_dir
        )),
        _ => {
            app.notify("Quiz review not available in this mode");
            None
        }
    }
}

/// Build the wizard tour generation agent prompt.
pub(super) fn build_agent_wizard_prompt(app: &mut App) -> Option<String> {
    let tab = app.tab();
    let mode = tab.mode;
    let base = tab.base_branch.clone();
    let er_dir = tab.er_dir();
    match mode {
        DiffMode::Branch | DiffMode::Unstaged | DiffMode::Staged | DiffMode::Wizard => Some(
            format!("/er-wizard {} {} --output {}", mode.git_mode(), base, er_dir),
        ),
        _ => {
            app.notify("Wizard generation not available in this mode");
            None
        }
    }
}

/// Sync GitHub PR comments (pull)
pub(super) fn sync_github_comments(app: &mut App) -> Result<()> {
    let tab = app.tab();
    let repo_root = tab.repo_root.clone();
    let explicit_pr_number = tab.pr_number;
    let is_remote = tab.is_remote();
    let remote_repo = tab.remote_repo.clone();

    let (owner, repo_name, pr_number) = if is_remote {
        if let (Some(ref slug), Some(n)) = (&remote_repo, explicit_pr_number) {
            let parts: Vec<&str> = slug.split('/').collect();
            if parts.len() == 2 {
                (parts[0].to_string(), parts[1].to_string(), n)
            } else {
                app.notify("Invalid remote repo slug");
                return Ok(());
            }
        } else {
            app.notify("No PR info for remote mode");
            return Ok(());
        }
    } else {
        let pr_info = github::get_pr_info(&repo_root);
        let pr_info = match pr_info {
            Ok(info) => info,
            Err(_) => {
                app.notify("No PR found for current branch");
                return Ok(());
            }
        };
        // If we're in no-checkout PR review mode, override the PR number
        if let Some(n) = explicit_pr_number {
            (pr_info.0, pr_info.1, n)
        } else {
            pr_info
        }
    };

    let gh_comments = if is_remote {
        match github::gh_pr_comments_remote(&owner, &repo_name, pr_number) {
            Ok(c) => c,
            Err(e) => {
                app.notify(&format!("GitHub sync error: {}", e));
                return Ok(());
            }
        }
    } else {
        match github::gh_pr_comments(&owner, &repo_name, pr_number, &repo_root) {
            Ok(c) => c,
            Err(e) => {
                app.notify(&format!("GitHub sync error: {}", e));
                return Ok(());
            }
        }
    };

    // Load existing github-comments.json (uses cache dir in remote mode)
    let comments_dir = app.tab().comments_dir();
    let _ = std::fs::create_dir_all(&comments_dir);
    let comments_path = app.tab().github_comments_path();
    let diff_hash = tab.branch_diff_hash.clone();
    let mut gc: crate::ai::ErGitHubComments = match std::fs::read_to_string(&comments_path) {
        Ok(content) => {
            serde_json::from_str(&content).unwrap_or_else(|_| crate::ai::ErGitHubComments {
                version: 1,
                diff_hash: diff_hash.clone(),
                github: None,
                comments: Vec::new(),
            })
        }
        Err(_) => crate::ai::ErGitHubComments {
            version: 1,
            diff_hash: diff_hash.clone(),
            github: None,
            comments: Vec::new(),
        },
    };

    gc.github = Some(crate::ai::GitHubSyncState {
        pr_number: Some(pr_number),
        owner: owner.clone(),
        repo: repo_name.clone(),
        last_synced: chrono_now(),
    });

    // Keep only truly local unpushed comments
    let local_unpushed: Vec<_> = gc
        .comments
        .into_iter()
        .filter(|c| c.source == "local" && !c.synced)
        .collect();

    // Build fresh GitHub entries from API response
    let tab_files = &app.tab().files;
    let diff_hash_for_anchor = app.tab().diff_hash.clone();
    let mut github_entries = Vec::new();

    for gh in &gh_comments {
        let file_path = gh.path.clone().unwrap_or_default();

        let (
            hunk_index,
            anchor_line_content,
            anchor_ctx_before,
            anchor_ctx_after,
            anchor_old_line,
            anchor_hunk_header,
        ) = if let Some(line) = gh.line {
            if let Some(f) = tab_files.iter().find(|f| f.path == file_path) {
                if let Some((i, hunk)) = f
                    .hunks
                    .iter()
                    .enumerate()
                    .find(|(_, h)| line >= h.new_start && line < h.new_start + h.new_count)
                {
                    let target_idx = hunk.lines.iter().position(|l| l.new_num == Some(line));
                    let (lc, old_ln) = if let Some(idx) = target_idx {
                        (hunk.lines[idx].content.clone(), hunk.lines[idx].old_num)
                    } else {
                        (String::new(), None)
                    };
                    let ctx_before: Vec<String> = if let Some(idx) = target_idx {
                        let start = idx.saturating_sub(3);
                        hunk.lines[start..idx]
                            .iter()
                            .map(|l| l.content.clone())
                            .collect()
                    } else {
                        Vec::new()
                    };
                    let ctx_after: Vec<String> = if let Some(idx) = target_idx {
                        let end = (idx + 4).min(hunk.lines.len());
                        hunk.lines[(idx + 1)..end]
                            .iter()
                            .map(|l| l.content.clone())
                            .collect()
                    } else {
                        Vec::new()
                    };
                    (
                        Some(i),
                        lc,
                        ctx_before,
                        ctx_after,
                        old_ln,
                        hunk.header.clone(),
                    )
                } else {
                    (
                        None,
                        String::new(),
                        Vec::new(),
                        Vec::new(),
                        None,
                        String::new(),
                    )
                }
            } else {
                (
                    None,
                    String::new(),
                    Vec::new(),
                    Vec::new(),
                    None,
                    String::new(),
                )
            }
        } else {
            (
                None,
                String::new(),
                Vec::new(),
                Vec::new(),
                None,
                String::new(),
            )
        };

        let in_reply_to = gh.in_reply_to_id.map(|pid| format!("gh-{}", pid));

        github_entries.push(crate::ai::GitHubReviewComment {
            id: format!("gh-{}", gh.id),
            timestamp: gh.created_at.clone(),
            file: file_path,
            hunk_index,
            line_start: gh.line,
            line_end: None,
            line_content: anchor_line_content,
            comment: gh.body.clone(),
            in_reply_to,
            resolved: false,
            source: "github".to_string(),
            github_id: Some(gh.id),
            author: gh.user.login.clone(),
            synced: true,
            stale: false,
            context_before: anchor_ctx_before,
            context_after: anchor_ctx_after,
            old_line_start: anchor_old_line,
            hunk_header: anchor_hunk_header,
            anchor_status: "original".to_string(),
            relocated_at_hash: diff_hash_for_anchor.clone(),
            finding_ref: None,
        });
    }

    let github_count = github_entries.len();
    let local_count = local_unpushed.len();
    gc.comments = local_unpushed;
    gc.comments.extend(github_entries);

    if !is_remote {
        std::fs::create_dir_all(format!("{}/.er", repo_root))?;
    }
    let json = serde_json::to_string_pretty(&gc)?;
    let tmp_path = format!("{}.tmp", comments_path);
    // TODO(risk:medium): if fs::write succeeds but fs::rename fails (e.g. permissions error),
    // the .tmp file is left behind and the comments file is not updated. The orphaned .tmp
    // will be picked up or confused on the next sync. Clean up the tmp file on rename failure.
    std::fs::write(&tmp_path, &json)?;
    std::fs::rename(&tmp_path, &comments_path)?;

    if is_remote {
        app.tab_mut().reload_remote_comments();
    } else {
        app.tab_mut().reload_ai_state();
    }

    // Refresh PR overview data (CI checks + reviewer status)
    let pr_number_for_overview = app.tab().pr_number;
    if is_remote {
        if let Some(pr_data) = github::gh_pr_overview_remote(
            &owner,
            &repo_name,
            pr_number_for_overview.unwrap_or(pr_number),
        ) {
            app.tab_mut().pr_data = Some(pr_data);
        }
    } else if let Some(pr_data) = github::gh_pr_overview(&repo_root, pr_number_for_overview) {
        app.tab_mut().pr_data = Some(pr_data);
    }

    app.notify(&format!(
        "GitHub sync: {} from GitHub, {} local kept, PR status refreshed",
        github_count, local_count
    ));
    Ok(())
}

/// Push all unpushed local comments to GitHub
fn push_all_comments_to_github(app: &mut App) -> Result<()> {
    let tab = app.tab();
    let repo_root = tab.repo_root.clone();
    let explicit_pr_number = tab.pr_number;
    let is_remote = tab.is_remote();
    let remote_repo = tab.remote_repo.clone();

    let (owner, repo_name, pr_number) = if is_remote {
        if let (Some(ref slug), Some(n)) = (&remote_repo, explicit_pr_number) {
            let parts: Vec<&str> = slug.split('/').collect();
            if parts.len() == 2 {
                (parts[0].to_string(), parts[1].to_string(), n)
            } else {
                app.notify("Invalid remote repo slug");
                return Ok(());
            }
        } else {
            app.notify("No PR info for remote mode");
            return Ok(());
        }
    } else {
        let pr_info = match github::get_pr_info(&repo_root) {
            Ok(info) => info,
            Err(_) => {
                app.notify("No PR found for current branch");
                return Ok(());
            }
        };
        // If we're in no-checkout PR review mode, override the PR number
        if let Some(n) = explicit_pr_number {
            (pr_info.0, pr_info.1, n)
        } else {
            pr_info
        }
    };

    let comments_path = app.tab().github_comments_path();
    let mut gc: crate::ai::ErGitHubComments = match std::fs::read_to_string(&comments_path) {
        Ok(content) => match serde_json::from_str(&content) {
            Ok(gc) => gc,
            Err(_) => return Ok(()),
        },
        Err(_) => return Ok(()),
    };

    let mut pushed = 0u32;
    let mut failed = 0u32;

    // Push parents first
    let comment_ids: Vec<String> = gc
        .comments
        .iter()
        .filter(|c| c.source == "local" && !c.synced && c.in_reply_to.is_none())
        .map(|c| c.id.clone())
        .collect();

    for cid in &comment_ids {
        let comment = gc.comments.iter().find(|c| c.id == *cid).cloned();
        if let Some(comment) = comment {
            // General comments (empty file) route to the issues API
            if comment.file.is_empty() {
                match if is_remote {
                    github::gh_pr_general_comment_remote(
                        &owner,
                        &repo_name,
                        pr_number,
                        &comment.comment,
                    )
                } else {
                    github::gh_pr_general_comment(
                        &owner,
                        &repo_name,
                        pr_number,
                        &comment.comment,
                        &repo_root,
                    )
                } {
                    Ok(github_id) => {
                        if let Some(c) = gc.comments.iter_mut().find(|c| c.id == *cid) {
                            c.github_id = Some(github_id);
                            c.synced = true;
                        }
                        pushed += 1;
                    }
                    Err(_) => {
                        failed += 1;
                    }
                }
                continue;
            }

            let path = &comment.file;
            // TODO(risk:medium): a comment with no line_start (hunk-level comment) falls back
            // to line 1, silently attributing the GitHub comment to the wrong location. GitHub
            // will accept it but reviewers will see it anchored to the wrong line. Use the
            // GitHub PR review API for hunk/file-level comments instead of line-level push.
            let line = comment.line_start.unwrap_or(1);
            // TODO(risk:minor): push errors are counted but the error message is discarded.
            // The user sees "N failed" with no indication of which comments failed or why
            // (e.g. outdated commit SHA, deleted file, rate limit). Retain errors for display.
            match if is_remote {
                github::gh_pr_push_comment_remote(
                    &owner,
                    &repo_name,
                    pr_number,
                    path,
                    line,
                    &comment.comment,
                )
            } else {
                github::gh_pr_push_comment(
                    &owner,
                    &repo_name,
                    pr_number,
                    path,
                    line,
                    &comment.comment,
                    &repo_root,
                )
            } {
                Ok(github_id) => {
                    if let Some(c) = gc.comments.iter_mut().find(|c| c.id == *cid) {
                        c.github_id = Some(github_id);
                        c.synced = true;
                    }
                    pushed += 1;
                }
                Err(_) => {
                    failed += 1;
                }
            }
        }
    }

    // Then push replies
    let reply_ids: Vec<String> = gc
        .comments
        .iter()
        .filter(|c| c.source == "local" && !c.synced && c.in_reply_to.is_some())
        .map(|c| c.id.clone())
        .collect();

    for cid in &reply_ids {
        let comment = gc.comments.iter().find(|c| c.id == *cid).cloned();
        if let Some(comment) = comment {
            let parent_gh_id = comment
                .in_reply_to
                .as_ref()
                .and_then(|rt| gc.comments.iter().find(|c| c.id == *rt))
                .and_then(|c| c.github_id);

            if let Some(parent_gh_id) = parent_gh_id {
                match if is_remote {
                    github::gh_pr_reply_comment_remote(
                        &owner,
                        &repo_name,
                        pr_number,
                        parent_gh_id,
                        &comment.comment,
                    )
                } else {
                    github::gh_pr_reply_comment(
                        &owner,
                        &repo_name,
                        pr_number,
                        parent_gh_id,
                        &comment.comment,
                        &repo_root,
                    )
                } {
                    Ok(github_id) => {
                        if let Some(c) = gc.comments.iter_mut().find(|c| c.id == *cid) {
                            c.github_id = Some(github_id);
                            c.synced = true;
                        }
                        pushed += 1;
                    }
                    Err(_) => {
                        failed += 1;
                    }
                }
            } else {
                failed += 1;
            }
        }
    }

    let json = serde_json::to_string_pretty(&gc)?;
    let tmp_path = format!("{}.tmp", comments_path);
    std::fs::write(&tmp_path, &json)?;
    std::fs::rename(&tmp_path, &comments_path)?;
    if is_remote {
        app.tab_mut().reload_remote_comments();
    } else {
        app.tab_mut().reload_ai_state();
    }

    if failed > 0 {
        app.notify(&format!("Pushed {} comments ({} failed)", pushed, failed));
    } else {
        app.notify(&format!("Pushed {} comments", pushed));
    }
    Ok(())
}

fn push_comments_as_review(app: &mut App) -> Result<()> {
    let tab = app.tab();
    let repo_root = tab.repo_root.clone();
    let explicit_pr_number = tab.pr_number;
    let is_remote = tab.is_remote();
    let remote_repo = tab.remote_repo.clone();

    let (owner, repo_name, pr_number) = if is_remote {
        if let (Some(ref slug), Some(n)) = (&remote_repo, explicit_pr_number) {
            let parts: Vec<&str> = slug.split('/').collect();
            if parts.len() == 2 {
                (parts[0].to_string(), parts[1].to_string(), n)
            } else {
                app.notify("Invalid remote repo slug");
                return Ok(());
            }
        } else {
            app.notify("No PR info for remote mode");
            return Ok(());
        }
    } else {
        let pr_info = match github::get_pr_info(&repo_root) {
            Ok(info) => info,
            Err(_) => {
                app.notify("No PR found for current branch");
                return Ok(());
            }
        };
        if let Some(n) = explicit_pr_number {
            (pr_info.0, pr_info.1, n)
        } else {
            pr_info
        }
    };

    let comments_path = app.tab().github_comments_path();
    let mut gc: crate::ai::ErGitHubComments = match std::fs::read_to_string(&comments_path) {
        Ok(content) => match serde_json::from_str(&content) {
            Ok(gc) => gc,
            Err(_) => return Ok(()),
        },
        Err(_) => return Ok(()),
    };

    let mut pushed = 0u32;
    let mut failed = 0u32;

    // Collect unsynced parent line comments (non-empty file) for the review batch
    let line_comment_ids: Vec<String> = gc
        .comments
        .iter()
        .filter(|c| {
            c.source == "local" && !c.synced && c.in_reply_to.is_none() && !c.file.is_empty()
        })
        .map(|c| c.id.clone())
        .collect();

    // Collect unsynced general comments (empty file) for individual posting
    let general_comment_ids: Vec<String> = gc
        .comments
        .iter()
        .filter(|c| {
            c.source == "local" && !c.synced && c.in_reply_to.is_none() && c.file.is_empty()
        })
        .map(|c| c.id.clone())
        .collect();

    // Submit line comments as a single review batch
    if !line_comment_ids.is_empty() {
        let batch: Vec<(String, usize, String)> = line_comment_ids
            .iter()
            .filter_map(|cid| gc.comments.iter().find(|c| c.id == *cid))
            .map(|c| (c.file.clone(), c.line_start.unwrap_or(1), c.comment.clone()))
            .collect();

        let result = if is_remote {
            github::gh_pr_submit_review_remote(&owner, &repo_name, pr_number, &batch)
        } else {
            github::gh_pr_submit_review(&owner, &repo_name, pr_number, &batch, &repo_root)
        };

        match result {
            Ok(()) => {
                for cid in &line_comment_ids {
                    if let Some(c) = gc.comments.iter_mut().find(|c| c.id == *cid) {
                        c.synced = true;
                        // Review API doesn't return individual comment IDs
                    }
                }
                pushed += line_comment_ids.len() as u32;
            }
            Err(e) => {
                app.notify(&format!("Review submit failed: {}", e));
                failed += line_comment_ids.len() as u32;
            }
        }
    }

    // Post general comments individually (review API doesn't support them)
    for cid in &general_comment_ids {
        let comment = gc.comments.iter().find(|c| c.id == *cid).cloned();
        if let Some(comment) = comment {
            match if is_remote {
                github::gh_pr_general_comment_remote(
                    &owner,
                    &repo_name,
                    pr_number,
                    &comment.comment,
                )
            } else {
                github::gh_pr_general_comment(
                    &owner,
                    &repo_name,
                    pr_number,
                    &comment.comment,
                    &repo_root,
                )
            } {
                Ok(github_id) => {
                    if let Some(c) = gc.comments.iter_mut().find(|c| c.id == *cid) {
                        c.github_id = Some(github_id);
                        c.synced = true;
                    }
                    pushed += 1;
                }
                Err(_) => {
                    failed += 1;
                }
            }
        }
    }

    // Push replies individually (review API doesn't support threading)
    let reply_ids: Vec<String> = gc
        .comments
        .iter()
        .filter(|c| c.source == "local" && !c.synced && c.in_reply_to.is_some())
        .map(|c| c.id.clone())
        .collect();

    for cid in &reply_ids {
        let comment = gc.comments.iter().find(|c| c.id == *cid).cloned();
        if let Some(comment) = comment {
            let parent_gh_id = comment
                .in_reply_to
                .as_ref()
                .and_then(|rt| gc.comments.iter().find(|c| c.id == *rt))
                .and_then(|c| c.github_id);

            if let Some(parent_gh_id) = parent_gh_id {
                match if is_remote {
                    github::gh_pr_reply_comment_remote(
                        &owner,
                        &repo_name,
                        pr_number,
                        parent_gh_id,
                        &comment.comment,
                    )
                } else {
                    github::gh_pr_reply_comment(
                        &owner,
                        &repo_name,
                        pr_number,
                        parent_gh_id,
                        &comment.comment,
                        &repo_root,
                    )
                } {
                    Ok(github_id) => {
                        if let Some(c) = gc.comments.iter_mut().find(|c| c.id == *cid) {
                            c.github_id = Some(github_id);
                            c.synced = true;
                        }
                        pushed += 1;
                    }
                    Err(_) => {
                        failed += 1;
                    }
                }
            } else {
                failed += 1;
            }
        }
    }

    let json = serde_json::to_string_pretty(&gc)?;
    let tmp_path = format!("{}.tmp", comments_path);
    std::fs::write(&tmp_path, &json)?;
    std::fs::rename(&tmp_path, &comments_path)?;
    if is_remote {
        app.tab_mut().reload_remote_comments();
    } else {
        app.tab_mut().reload_ai_state();
    }

    if failed > 0 {
        app.notify(&format!("Review: pushed {} ({} failed)", pushed, failed));
    } else {
        app.notify(&format!("Review: pushed {}", pushed));
    }
    Ok(())
}

fn chrono_now() -> String {
    app::chrono_now()
}
