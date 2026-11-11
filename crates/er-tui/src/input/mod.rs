use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use er_engine::app;
use er_engine::app::{
    cleanup_note_replies, cleanup_question_answers, cleanup_questions_and_notes, cleanup_reviews,
    AiActionKind, App, ConfirmAction, DiffMode, HubAction, InputMode,
};
use er_engine::github;

pub mod normal;

pub use normal::handle_normal_input;

/// How a gate level reads in a notification.
///
/// Phrasing for this surface, so it lives here rather than on the engine state:
/// the desktop names the same three levels its own way, and both are sentences
/// about a number neither front end owns.
fn min_trust_label(level: er_engine::ai::Confidence) -> &'static str {
    use er_engine::ai::Confidence;
    match level {
        Confidence::Informational => "all findings",
        Confidence::Tentative => "informational hidden",
        Confidence::Confirmed | Confidence::Dropped => "confirmed only",
    }
}

/// Byte index of the char boundary immediately before `pos` (0 if at start).
fn prev_char_boundary(s: &str, pos: usize) -> usize {
    (0..pos).rev().find(|&i| s.is_char_boundary(i)).unwrap_or(0)
}

/// Byte index of the char boundary immediately after `pos` (`s.len()` if at end).
fn next_char_boundary(s: &str, pos: usize) -> usize {
    (pos + 1..s.len())
        .find(|&i| s.is_char_boundary(i))
        .unwrap_or(s.len())
}

pub fn handle_overlay_input(app: &mut App, key: KeyEvent) -> Result<()> {
    if matches!(
        app.overlay,
        Some(app::OverlayData::ReviewFlowProgress { .. })
    ) {
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => app.overlay_next(),
            KeyCode::Char('k') | KeyCode::Up => app.overlay_prev(),
            KeyCode::PageDown => scroll_review_flow_detail(app, 8),
            KeyCode::PageUp => scroll_review_flow_detail(app, -8),
            KeyCode::Esc | KeyCode::Char('q') => app.overlay_close(),
            _ => {}
        }
        return Ok(());
    }
    if matches!(
        app.overlay,
        Some(app::OverlayData::ReviewFlowResults { .. })
    ) {
        match review_flow_results_key_action(key.code) {
            ReviewFlowResultsKeyAction::Next => app.overlay_next(),
            ReviewFlowResultsKeyAction::Previous => app.overlay_prev(),
            ReviewFlowResultsKeyAction::PageDown => scroll_review_flow_detail(app, 8),
            ReviewFlowResultsKeyAction::PageUp => scroll_review_flow_detail(app, -8),
            ReviewFlowResultsKeyAction::Toggle => toggle_review_flow_result_selection(app),
            ReviewFlowResultsKeyAction::Accept => accept_selected_review_flow_results(app)?,
            ReviewFlowResultsKeyAction::Close => app.overlay_close(),
            ReviewFlowResultsKeyAction::Ignore => {}
        }
        return Ok(());
    }

    // Export picker — multiselect + Enter copies markdown to clipboard
    if matches!(app.overlay, Some(app::OverlayData::ExportPicker { .. })) {
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => app.overlay_next(),
            KeyCode::Char('k') | KeyCode::Up => app.overlay_prev(),
            KeyCode::Char(' ') => app.export_picker_toggle_selected(),
            KeyCode::Enter => app.export_picker_confirm()?,
            KeyCode::Esc | KeyCode::Char('q') => app.overlay_close(),
            _ => {}
        }
        return Ok(());
    }

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
                KeyCode::Enter => app.config_hub_confirm_edit(),
                KeyCode::Esc => app.config_hub_cancel_edit(),
                code => {
                    if let Some(app::OverlayData::ConfigHub {
                        editing: Some(ref mut edit),
                        ..
                    }) = &mut app.overlay
                    {
                        match code {
                            KeyCode::Char(c) => {
                                edit.buffer.insert(edit.cursor_pos, c);
                                edit.cursor_pos += c.len_utf8();
                            }
                            KeyCode::Backspace if edit.cursor_pos > 0 => {
                                edit.cursor_pos = prev_char_boundary(&edit.buffer, edit.cursor_pos);
                                edit.buffer.remove(edit.cursor_pos);
                            }
                            KeyCode::Left if edit.cursor_pos > 0 => {
                                edit.cursor_pos = prev_char_boundary(&edit.buffer, edit.cursor_pos);
                            }
                            KeyCode::Right if edit.cursor_pos < edit.buffer.len() => {
                                edit.cursor_pos = next_char_boundary(&edit.buffer, edit.cursor_pos);
                            }
                            _ => {}
                        }
                    }
                }
            }
        } else {
            match key.code {
                KeyCode::Char('j') | KeyCode::Down => app.overlay_next(),
                KeyCode::Char('k') | KeyCode::Up => app.overlay_prev(),
                KeyCode::Enter | KeyCode::Char(' ') | KeyCode::Right => app.config_hub_activate(),
                KeyCode::Left => app.config_hub_activate_prev(),
                KeyCode::Char('d') => app.config_hub_delete_selected(),
                KeyCode::Char('1') => {
                    app.config_hub_switch_tab(er_engine::config::SettingsScope::General);
                }
                KeyCode::Char('2') => {
                    app.config_hub_switch_tab(er_engine::config::SettingsScope::Terminal);
                }
                KeyCode::Char('[') => {
                    let tab = match &app.overlay {
                        Some(er_engine::app::OverlayData::ConfigHub { tab, .. }) => *tab,
                        _ => return Ok(()),
                    };
                    let prev = match tab.tab_index() {
                        0 => er_engine::config::SettingsScope::Terminal,
                        _ => er_engine::config::SettingsScope::General,
                    };
                    app.config_hub_switch_tab(prev);
                }
                KeyCode::Char(']') => {
                    let tab = match &app.overlay {
                        Some(er_engine::app::OverlayData::ConfigHub { tab, .. }) => *tab,
                        _ => return Ok(()),
                    };
                    let next = match tab.tab_index() {
                        0 => er_engine::config::SettingsScope::Terminal,
                        _ => er_engine::config::SettingsScope::General,
                    };
                    app.config_hub_switch_tab(next);
                }
                KeyCode::Char('s' | 'S') => app.config_hub_close(),
                KeyCode::Esc | KeyCode::Char('q') => app.config_hub_close(),
                _ => {}
            }
        }
        return Ok(());
    }

    match key.code {
        KeyCode::Char('j') | KeyCode::Down => app.overlay_next(),
        KeyCode::Char('k') | KeyCode::Up => app.overlay_prev(),
        KeyCode::Char('b') => {
            // Secondary action for the selected row. Only stacked-PR rows define
            // one (open on GitHub); everywhere else this is a no-op.
            if let Some(action) = app.overlay_secondary_hub_action() {
                app.overlay = None;
                dispatch_hub_action(app, action)?;
            }
        }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReviewFlowResultsKeyAction {
    Next,
    Previous,
    PageDown,
    PageUp,
    Toggle,
    Accept,
    Close,
    Ignore,
}

fn review_flow_results_key_action(key: KeyCode) -> ReviewFlowResultsKeyAction {
    match key {
        KeyCode::Char('j') | KeyCode::Down => ReviewFlowResultsKeyAction::Next,
        KeyCode::Char('k') | KeyCode::Up => ReviewFlowResultsKeyAction::Previous,
        KeyCode::PageDown => ReviewFlowResultsKeyAction::PageDown,
        KeyCode::PageUp => ReviewFlowResultsKeyAction::PageUp,
        KeyCode::Char(' ') | KeyCode::Enter => ReviewFlowResultsKeyAction::Toggle,
        KeyCode::Char('a') => ReviewFlowResultsKeyAction::Accept,
        KeyCode::Esc | KeyCode::Char('q') => ReviewFlowResultsKeyAction::Close,
        _ => ReviewFlowResultsKeyAction::Ignore,
    }
}

fn scroll_review_flow_detail(app: &mut App, delta: isize) {
    if let Some(
        app::OverlayData::ReviewFlowResults { detail_scroll, .. }
        | app::OverlayData::ReviewFlowProgress { detail_scroll, .. },
    ) = app.overlay.as_mut()
    {
        *detail_scroll = if delta < 0 {
            detail_scroll.saturating_sub(delta.unsigned_abs())
        } else {
            detail_scroll.saturating_add(delta as usize).min(10_000)
        };
    }
}

fn toggle_review_flow_result_selection(app: &mut App) {
    let message = match app.overlay.as_mut() {
        Some(app::OverlayData::ReviewFlowResults {
            results,
            selected,
            selected_ids,
            ..
        }) => match results.findings.get(*selected) {
            Some(finding) if finding.selectable => {
                toggle_selected_finding_id(selected_ids, &finding.id, true);
                None
            }
            Some(_) => Some("This finding cannot be accepted"),
            None => Some("There are no findings to select"),
        },
        _ => None,
    };
    if let Some(message) = message {
        app.notify(message);
    }
}

fn toggle_selected_finding_id(
    selected_ids: &mut std::collections::HashSet<String>,
    finding_id: &str,
    selectable: bool,
) -> bool {
    if !selectable {
        return false;
    }
    if !selected_ids.remove(finding_id) {
        selected_ids.insert(finding_id.to_owned());
    }
    true
}

fn accept_selected_review_flow_results(app: &mut App) -> Result<()> {
    let action = match app.overlay.as_ref() {
        Some(app::OverlayData::ReviewFlowResults {
            results,
            selected_ids,
            ..
        }) => build_accept_review_flow_action(&results.er_dir, &results.run_id, selected_ids),
        _ => return Ok(()),
    };
    if let Some(action) = action {
        dispatch_hub_action(app, action)?;
    } else {
        app.notify("Select at least one finding to accept");
    }
    Ok(())
}

fn build_accept_review_flow_action(
    er_dir: &str,
    run_id: &str,
    selected_ids: &std::collections::HashSet<String>,
) -> Option<HubAction> {
    if selected_ids.is_empty() {
        return None;
    }
    let mut finding_ids: Vec<String> = selected_ids.iter().cloned().collect();
    finding_ids.sort();
    Some(HubAction::AcceptReviewFlowFindings {
        er_dir: er_dir.to_owned(),
        run_id: run_id.to_owned(),
        finding_ids,
    })
}

fn open_review_flow_results(app: &mut App, er_dir: &str, run_id: &str, selected: usize) {
    match app.review_flow_results(er_dir, run_id) {
        Ok(results) => {
            let selected = selected.min(results.findings.len().saturating_sub(1));
            app.overlay = Some(app::OverlayData::ReviewFlowResults {
                results,
                selected,
                selected_ids: std::collections::HashSet::new(),
                detail_scroll: 0,
            });
        }
        Err(error) => app.notify(&format!("Review results: {error}")),
    }
}

pub fn dispatch_hub_action(app: &mut App, action: HubAction) -> Result<()> {
    match action {
        HubAction::Noop => {}
        HubAction::PushToRemote => {
            if app.tab().mode == DiffMode::Staged {
                app.input_mode = InputMode::Confirm(ConfirmAction::Push);
            }
        }
        HubAction::PullGitHubComments => {
            app.sync_github_comments()?;
        }
        HubAction::CommentOnPR => {
            if app.tab().pr_absent {
                app.notify(er_engine::github_post::NO_PR_FOR_COMMENT);
            } else {
                app.start_general_comment();
            }
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
        HubAction::CopyReviewJson => {
            app.copy_review_json()?;
        }
        HubAction::CopyQuestionsJson => {
            app.copy_questions_json()?;
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
        HubAction::ToggleHideResolved => {
            app.tab_mut().toggle_hide_resolved();
            let on = app.tab().layers.hide_resolved;
            app.notify(if on {
                "Resolved: hidden"
            } else {
                "Resolved: visible"
            });
        }
        HubAction::CycleMinTrust => {
            let next = app.tab_mut().cycle_min_trust();
            app.notify(&format!("Confidence: {}", min_trust_label(next)));
        }
        HubAction::ToggleDroppedFindings => {
            app.tab_mut().toggle_show_dropped();
            let on = app.tab().layers.show_dropped;
            app.notify(if on {
                "Arbiter's drops: listed"
            } else {
                "Arbiter's drops: counted only"
            });
        }
        HubAction::StartReviewFlow { scope } => match app.start_review_flow(&scope) {
            Ok(()) => app.notify("Review started"),
            Err(error) => app.notify(&format!("Review: {error}")),
        },
        HubAction::ConfirmReviewFlow { er_dir } => match app.confirm_review_flow(&er_dir) {
            Ok(()) => app.notify("Deep review started"),
            Err(error) => app.notify(&format!("Deep review: {error}")),
        },
        HubAction::DeclineReviewFlow { er_dir } => match app.decline_review_flow(&er_dir) {
            Ok(()) => app.notify("Deep review declined"),
            Err(error) => app.notify(&format!("Deep review: {error}")),
        },
        HubAction::CancelReviewFlow { er_dir } => {
            if app.cancel_review_flow(&er_dir) {
                app.notify("Review stopping");
            } else {
                app.notify("No running review to cancel");
            }
        }
        HubAction::OpenReviewFlowProgress { er_dir } => app.open_review_flow_progress(&er_dir),
        HubAction::OpenReviewFlowResults { er_dir, run_id } => {
            open_review_flow_results(app, &er_dir, &run_id, 0);
        }
        HubAction::AcceptReviewFlowFindings {
            er_dir,
            run_id,
            finding_ids,
        } => match app.accept_review_flow_findings(&er_dir, &run_id, finding_ids) {
            Ok(count) => {
                app.notify(&format!("Accepted {count} finding(s)"));
                open_review_flow_results(app, &er_dir, &run_id, 0);
            }
            Err(error) => app.notify(&format!("Could not accept findings: {error}")),
        },
        HubAction::RunImportanceAgent => match app.spawn_background_importance() {
            Ok(()) => app.notify("Importance agent started"),
            Err(e) => app.notify(&format!("Importance: {e}")),
        },
        HubAction::CleanupQuestions => {
            let count = app.tab().ai.local_draft_count();
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
        HubAction::ConfigureAiSelection => {
            app.open_ai_provider_picker(None);
        }
        HubAction::RunAiAction(action) => {
            execute_ai_action(app, action)?;
        }
        HubAction::SelectAiProvider {
            action,
            provider_id,
        } => {
            let (model_count, model_id) = match app.config.ai_hub.providers.get(&provider_id) {
                Some(provider) => (
                    provider.models.len(),
                    provider.models.first().map(|m| m.id.clone()),
                ),
                None => {
                    app.notify("Unknown AI provider");
                    return Ok(());
                }
            };
            if model_count > 1 {
                app.open_ai_model_picker(provider_id, action);
            } else if model_id
                .as_ref()
                .is_some_and(|id| app.model_has_effort_levels(&provider_id, id))
            {
                if let Some(model_id) = model_id {
                    app.open_ai_effort_picker(provider_id, model_id, action);
                }
            } else if let Some(action) = action {
                let selection =
                    app.resolve_ai_selection_override(&provider_id, model_id.as_deref())?;
                execute_ai_action_with_selection(app, action, selection)?;
            } else {
                app.set_ai_default_selection(&provider_id, model_id.as_deref())?;
                app.notify(&format!("AI target: {}", app.active_ai_selection_label()));
            }
        }
        HubAction::SelectAiModel {
            action,
            provider_id,
            model_id,
        } => {
            if app.model_has_effort_levels(&provider_id, &model_id) {
                app.open_ai_effort_picker(provider_id, model_id, action);
            } else if let Some(action) = action {
                let selection = app.resolve_ai_selection_override(&provider_id, Some(&model_id))?;
                execute_ai_action_with_selection(app, action, selection)?;
            } else {
                app.set_ai_default_selection(&provider_id, Some(&model_id))?;
                app.notify(&format!("AI target: {}", app.active_ai_selection_label()));
            }
        }
        HubAction::SelectAiEffort {
            action,
            provider_id,
            model_id,
            effort,
        } => {
            if let Some(action) = action {
                let selection = app.resolve_ai_selection_override_with_effort(
                    &provider_id,
                    Some(&model_id),
                    Some(&effort),
                )?;
                execute_ai_action_with_selection(app, action, selection)?;
            } else {
                app.set_ai_default_selection_with_effort(
                    &provider_id,
                    Some(&model_id),
                    Some(&effort),
                )?;
                app.notify(&format!("AI target: {}", app.active_ai_selection_label()));
            }
        }
        HubAction::RefreshAiModels { provider_id } => {
            app.request_model_discovery(provider_id);
        }
        HubAction::OpenAiExpertPicker => {
            app.open_ai_expert_picker();
        }
        HubAction::RunExpertReview { expert_id } => {
            if let Some((er_dir, diff_hash)) = ensure_prepared_diff_for_action(app) {
                if let Some(prompt) =
                    build_agent_expert_prompt(app, &expert_id, &er_dir, &diff_hash)
                {
                    app.spawn_agent_prompt(&format!("expert-{expert_id}"), &prompt)?;
                }
            }
        }
        HubAction::RunProfessorReview => {
            if let Some((er_dir, diff_hash)) = ensure_prepared_diff_for_action(app) {
                if let Some(prompt) = build_agent_professor_prompt(app, &er_dir, &diff_hash) {
                    app.spawn_agent_prompt("professor", &prompt)?;
                }
            }
        }
        HubAction::RunTriageReview => {
            if let Some((er_dir, diff_hash)) = ensure_prepared_diff_for_action(app) {
                if let Some(prompt) = build_agent_triage_prompt(app, &er_dir, &diff_hash) {
                    app.spawn_agent_prompt("triage", &prompt)?;
                }
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
                // No previous review — prepare the in-memory diff (including
                // remote `--remote` tabs) so the sandboxed agent reads
                // diff-tmp instead of calling `gh`.
                start_agent_review(app)?;
            }
        }
        HubAction::PromptValidate => {
            let has_review = app.tab().ai.review.is_some();
            let comment_count = app
                .tab()
                .ai
                .github_comments
                .as_ref()
                .map(er_engine::ai::count_eligible_github_comments)
                .unwrap_or(0);
            if !has_review && comment_count == 0 {
                app.notify("Nothing to validate — run review or add GitHub comments");
            } else if app.tab().is_remote() {
                app.notify(
                    "validate is local-only — checkout the PR first, then re-run review locally",
                );
            } else if let Some((er_dir, diff_hash)) = ensure_prepared_diff_for_action(app) {
                if comment_count > 0 {
                    app.tab_mut().relocate_all_comments();
                }
                if has_review {
                    if let Some(prompt) = build_agent_validate_prompt(app, &er_dir, &diff_hash) {
                        app.spawn_agent_prompt("validate", &prompt)?;
                    }
                }
                if comment_count > 0 {
                    let comments_prompt =
                        er_engine::ai::prompts::build_validate_github_comments_prompt_prepared_diff(
                            &er_dir, &diff_hash,
                        );
                    app.spawn_agent_prompt("validate-comments", &comments_prompt)?;
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
        HubAction::PromptNotes => {
            let has_replies = app.tab().ai.notes.as_ref().is_some_and(|ns| {
                ns.notes
                    .iter()
                    .any(|n| n.in_reply_to.is_some() && n.author == "Claude")
            });
            if has_replies {
                app.input_mode = InputMode::Confirm(ConfirmAction::RunAgentNotes {
                    clear_previous: true,
                });
            } else if let Some(prompt) = build_agent_notes_prompt(app) {
                app.spawn_agent_prompt("notes", &prompt)?;
            }
        }
        HubAction::PromptTour => match app.spawn_tour_for_active_tab() {
            Ok(()) => app.notify_long("Generating guided tour…"),
            Err(e) => app.notify(&format!("Tour generation failed: {}", e)),
        },
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
        HubAction::SetBaseBranch => {
            if app.tab().is_remote() {
                app.notify("Cannot change base — remote PR diff comes from GitHub");
            } else {
                app.remote_url_input.clear();
                app.input_mode = InputMode::BaseBranch;
            }
        }
        HubAction::OpenPrInBrowser => {
            if let Some(pr_number) = app.tab().pr_number {
                let repo_root = app.tab().repo_root.clone();
                let remote_repo = app.tab().remote_repo.clone();
                app.ports
                    .github
                    .open_pr_in_browser(&repo_root, pr_number, remote_repo.as_deref());
                app.notify("Opening PR in browser...");
            }
        }
        HubAction::OpenStackPr {
            pr_number,
            pr_url,
            branch,
        } => {
            // Reuse the remote-PR review path: it focuses an already-open tab for
            // this PR or opens a new one backed by `gh pr view`. A stacked layer
            // is reviewed as its own PR, so the diff is that layer's, not the
            // whole stack's.
            if let Err(e) = app.open_remote_url(&pr_url) {
                app.notify(&format!("Failed to open {branch} (#{pr_number}): {e}"));
            }
        }
        HubAction::OpenStackPrInBrowser {
            pr_number,
            pr_url,
            branch,
        } => {
            let repo_root = app.tab().repo_root.clone();
            // A layer can live in another repo than the tab (when the tab is a
            // remote PR), so prefer the URL's slug over the tab's remote.
            let slug =
                github::parse_github_pr_url(&pr_url).map(|r| format!("{}/{}", r.owner, r.repo));
            if app
                .ports
                .github
                .open_pr_in_browser(&repo_root, pr_number, slug.as_deref())
            {
                app.notify(&format!("Opening {branch} (#{pr_number}) in browser..."));
            }
        }
        HubAction::RefreshStack => app.refresh_stack(),
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
        HubAction::SelectVerifyPackage { package_id } => {
            app.open_package_commands_hub(package_id);
        }
        HubAction::RunPackageCommand {
            command,
            package_id,
        } => match app.config.resolve_package_command(&package_id, &command) {
            Some(cmd) => app.spawn_command(&format!("{}/{}", package_id, command), &cmd)?,
            None => app.notify(&format!("{}/{}: not configured", package_id, command)),
        },
    }
    Ok(())
}

fn execute_ai_action(app: &mut App, action: AiActionKind) -> Result<()> {
    app.sync_ai_selection();

    match action {
        AiActionKind::Review => dispatch_hub_action(app, HubAction::PromptReview),
        AiActionKind::ExpertReview { expert_id } => {
            dispatch_hub_action(app, HubAction::RunExpertReview { expert_id })
        }
        AiActionKind::Professor => dispatch_hub_action(app, HubAction::RunProfessorReview),
        AiActionKind::Triage => dispatch_hub_action(app, HubAction::RunTriageReview),
        AiActionKind::Validate => dispatch_hub_action(app, HubAction::PromptValidate),
        AiActionKind::Questions => dispatch_hub_action(app, HubAction::PromptQuestions),
        AiActionKind::Notes => dispatch_hub_action(app, HubAction::PromptNotes),
        AiActionKind::Tour => dispatch_hub_action(app, HubAction::PromptTour),
        AiActionKind::Summary => {
            if let Some(prompt) = build_agent_summary_prompt(app) {
                app.spawn_agent_prompt("summary", &prompt)?;
            }
            Ok(())
        }
    }
}

fn execute_ai_action_with_selection(
    app: &mut App,
    action: AiActionKind,
    selection: er_engine::config::AiSelection,
) -> Result<()> {
    app.set_ai_selection_override(selection);
    let result = execute_ai_action(app, action);
    let waiting_for_confirmation = matches!(
        app.input_mode,
        er_engine::app::InputMode::Confirm(
            er_engine::app::ConfirmAction::RunAgentReview { .. }
                | er_engine::app::ConfirmAction::RunAgentQuestions { .. }
                | er_engine::app::ConfirmAction::RunAgentNotes { .. }
        )
    );
    if !waiting_for_confirmation {
        app.clear_ai_selection_override();
    }
    result
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

/// Text entry for changing the compare/base branch of the active tab.
pub fn handle_base_branch_input(app: &mut App, key: KeyEvent) -> Result<()> {
    match key.code {
        KeyCode::Enter => {
            let input = app.remote_url_input.clone();
            app.input_mode = InputMode::Normal;
            app.remote_url_input.clear();
            let branch = input.trim();
            if branch.is_empty() {
                return Ok(());
            }
            match app.tab_mut().set_base_branch(branch) {
                Ok(()) => app.notify(&format!("Base set to {}", branch)),
                Err(e) => app.notify(&format!("Could not change base: {}", e)),
            }
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
        KeyCode::Enter if key.modifiers.contains(KeyModifiers::SHIFT) => {
            // Shift+Enter inserts a newline
            app.tab_mut().comment_textarea.insert_newline();
        }
        KeyCode::Enter => {
            // A GitHub-kind create posts now; a failure leaves the composer
            // open with its text so the user can retry.
            if app.github_comment_intent().is_some() {
                if let Err(e) = app.post_github_comment_now() {
                    app.notify(&e.to_string());
                }
            } else if let Err(e) = app.submit_comment() {
                app.notify(&format!("Comment not saved: {e}"));
            }
        }
        KeyCode::Esc => {
            app.cancel_comment();
        }
        KeyCode::Tab | KeyCode::BackTab => {
            app.pause_comment();
        }
        KeyCode::Char('t') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.toggle_comment_type();
        }
        KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.tab_mut().scroll_down(10);
        }
        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.tab_mut().scroll_up(10);
        }
        KeyCode::PageDown => app.tab_mut().scroll_down(20),
        KeyCode::PageUp => app.tab_mut().scroll_up(20),
        _ => {
            // Delegate to textarea (arrows, chars, backspace, etc.)
            // But intercept Enter so it doesn't insert newlines via the default handler
            if key.code != KeyCode::Enter {
                app.tab_mut().comment_textarea.input(key);
            }
        }
    }
    Ok(())
}

pub fn handle_confirm_input(app: &mut App, key: KeyEvent) -> Result<()> {
    match key.code {
        KeyCode::Char('y') => {
            let action = app.input_mode.clone();
            if let InputMode::Confirm(ConfirmAction::DeleteComment { comment_id }) = action {
                app.confirm_delete_comment(&comment_id)?;
            } else if action == InputMode::Confirm(ConfirmAction::Push) {
                app.input_mode = InputMode::Normal;
                let repo_root = app.tab().repo_root.clone();
                match app.ports.git.push(&repo_root) {
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
                cleanup_questions_and_notes(&er_dir);
                app.tab_mut().reload_ai_state();
                app.notify("Questions & notes cleared");
            } else if let InputMode::Confirm(ConfirmAction::DeleteWatchedFile { ref path }) = action
            {
                let full_path = format!("{}/{}", app.tab().repo_root, path);
                app.input_mode = InputMode::Normal;
                match std::fs::remove_file(&full_path) {
                    Ok(()) => {
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
                if let Err(error) = app.check_review_bucket_admission(&er_dir) {
                    app.notify(&format!("Review clear skipped: {error}"));
                    return Ok(());
                }
                if let Err(error) = cleanup_reviews(&er_dir) {
                    app.notify(&format!("Review clear failed: {error}"));
                    return Ok(());
                }
                app.tab_mut().reload_ai_state();
                app.notify("Review cleared");
            } else if let InputMode::Confirm(ConfirmAction::RunAgentReview { .. }) = action {
                // Prepare before clearing so an ignored-only diff leaves the old review intact.
                app.input_mode = InputMode::Normal;
                if let Some((er_dir, diff_hash)) = ensure_prepared_diff_for_action(app) {
                    if let Err(error) = cleanup_reviews(&er_dir) {
                        app.notify(&format!("Review not started: {error}"));
                        return Ok(());
                    }
                    app.tab_mut().reload_ai_state();
                    start_agent_review_with_prepared_diff(app, &er_dir, &diff_hash)?;
                }
                app.clear_ai_selection_override();
            } else if let InputMode::Confirm(ConfirmAction::RunAgentQuestions { .. }) = action {
                // User said "yes" to clearing previous answers — clear answers, then run
                app.input_mode = InputMode::Normal;
                let er_dir = app.tab().er_dir();
                cleanup_question_answers(&er_dir);
                app.tab_mut().reload_ai_state();
                if let Some(prompt) = build_agent_questions_prompt(app) {
                    app.spawn_agent_prompt("questions", &prompt)?;
                }
                app.clear_ai_selection_override();
            } else if let InputMode::Confirm(ConfirmAction::RunAgentNotes { .. }) = action {
                app.input_mode = InputMode::Normal;
                let er_dir = app.tab().er_dir();
                cleanup_note_replies(&er_dir);
                app.tab_mut().reload_ai_state();
                if let Some(prompt) = build_agent_notes_prompt(app) {
                    app.spawn_agent_prompt("notes", &prompt)?;
                }
                app.clear_ai_selection_override();
            } else if action == InputMode::Confirm(ConfirmAction::ApprovePR) {
                app.input_mode = InputMode::Normal;
                let repo_root = app.tab().repo_root.clone();
                let remote = app.tab().remote_repo.clone();
                let pr = app.tab().pr_number;
                match er_engine::github::gh_pr_approve(&repo_root, remote.as_deref(), pr) {
                    Ok(()) => app.notify("PR approved"),
                    Err(e) => app.notify(&format!("Approve failed: {}", e)),
                }
            }
        }
        KeyCode::Char('n') => {
            app.cancel_confirm();
        }
        KeyCode::Char('k') => {
            // For agent prompts, 'k' = keep previous data but still run
            if let InputMode::Confirm(ConfirmAction::RunAgentReview { .. }) = &app.input_mode {
                app.input_mode = InputMode::Normal;
                start_agent_review(app)?;
                app.clear_ai_selection_override();
            } else if let InputMode::Confirm(ConfirmAction::RunAgentQuestions { .. }) =
                &app.input_mode
            {
                app.input_mode = InputMode::Normal;
                if let Some(prompt) = build_agent_questions_prompt(app) {
                    app.spawn_agent_prompt("questions", &prompt)?;
                }
                app.clear_ai_selection_override();
            } else if let InputMode::Confirm(ConfirmAction::RunAgentNotes { .. }) = &app.input_mode
            {
                app.input_mode = InputMode::Normal;
                if let Some(prompt) = build_agent_notes_prompt(app) {
                    app.spawn_agent_prompt("notes", &prompt)?;
                }
                app.clear_ai_selection_override();
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

/// Build the triage scan prompt (local diff modes only).
pub fn build_agent_triage_prompt(app: &mut App, er_dir: &str, diff_hash: &str) -> Option<String> {
    let tab = app.tab();
    if tab.is_remote() {
        app.notify("Triage is local-only in v1 — checkout the PR first");
        return None;
    }
    Some(
        er_engine::ai::prompts::build_triage_review_prompt_prepared_diff(
            tab.mode.git_mode(),
            er_dir,
            diff_hash,
        ),
    )
}

/// Build the Professor learning prompt (local diff modes only).
pub fn build_agent_professor_prompt(
    app: &mut App,
    er_dir: &str,
    diff_hash: &str,
) -> Option<String> {
    let tab = app.tab();
    if tab.is_remote() {
        app.notify("Professor is local-only in v1 — checkout the PR first");
        return None;
    }
    let mode = tab.mode;
    match mode {
        DiffMode::Branch | DiffMode::Unstaged | DiffMode::Staged => Some(
            er_engine::ai::prompts::build_professor_review_prompt_prepared_diff(
                mode.git_mode(),
                er_dir,
                None,
                None,
                diff_hash,
            ),
        ),
        _ => {
            app.notify("Professor not available in this mode");
            None
        }
    }
}

/// Build a specialized expert review prompt (local diff modes only).
pub fn build_agent_expert_prompt(
    app: &mut App,
    expert_id: &str,
    er_dir: &str,
    diff_hash: &str,
) -> Option<String> {
    if er_engine::ai::expert_by_id(expert_id).is_none() {
        app.notify(&format!("Unknown expert: {expert_id}"));
        return None;
    }
    let tab = app.tab();
    if tab.is_remote() {
        app.notify("Specialized expert review is local-only in v1 — checkout the PR first");
        return None;
    }
    let mode = tab.mode;
    match mode {
        DiffMode::Branch | DiffMode::Unstaged | DiffMode::Staged => Some(
            er_engine::ai::prompts::build_expert_review_prompt_prepared_diff(
                mode.git_mode(),
                er_dir,
                expert_id,
                diff_hash,
                None,
            ),
        ),
        _ => {
            app.notify("Expert review not available in this mode");
            None
        }
    }
}

/// Prepare the harness diff artifacts (diff-tmp + diff-annotated + markers)
/// for an agent action, aborting with a notification when there is no diff
/// to prepare. Review/validate/expert/professor/triage must anchor against
/// the SAME prepared bytes: an agent-side git or `gh pr diff` would use
/// different `--unified` flags and mis-anchor findings. Remote tabs
/// (`er --remote`) already hold the PR diff in memory
/// (or can re-fetch it from this unsandboxed process); they use the same
/// prepared-artifact path so the sandboxed agent does not call `gh`.
pub fn ensure_prepared_diff_for_action(app: &mut App) -> Option<(String, String)> {
    let scope = app.tab().mode.fetch_scope();
    let er_dir = app.tab().er_dir();
    if let Err(error) = app.check_review_bucket_admission(&er_dir) {
        app.notify(&format!("action skipped: {error}"));
        return None;
    }
    let mut raw = match app.tab().raw_diff_for_review(scope) {
        Ok(raw) => raw,
        Err(_) => {
            app.notify("No diff to review against");
            return None;
        }
    };
    let (repo_root, remote_repo) = {
        let tab = app.tab();
        (tab.repo_root.clone(), tab.remote_repo.clone())
    };
    let ignore =
        er_engine::projects_pins::review_ignore_globs_for_repo(&repo_root, remote_repo.as_deref());
    if !ignore.is_empty() {
        raw = er_engine::git::filter_raw_diff_exclude_globs(&raw, &ignore);
    }
    if raw.trim().is_empty() {
        app.notify("Nothing to review");
        return None;
    }
    let review_md =
        er_engine::ai::review_md::collect(&app.ports, &app.tab().review_md_source(), &raw);
    match er_engine::ai::prepared_diff::ensure_review_inputs(
        &er_dir,
        &raw,
        app.tab().repo_rules(),
        review_md.as_deref(),
    ) {
        Ok(hash) => Some((er_dir, hash)),
        Err(e) => {
            app.notify(&format!("action skipped: {e}"));
            None
        }
    }
}

/// Write prepared artifacts and spawn the review agent. Shared by the hub
/// action and the confirm-dialog keep/clear paths so remote and local tabs
/// cannot drift (desktop `run_ai_review` already uses this same prompt).
fn start_agent_review(app: &mut App) -> anyhow::Result<()> {
    if let Some((er_dir, diff_hash)) = ensure_prepared_diff_for_action(app) {
        start_agent_review_with_prepared_diff(app, &er_dir, &diff_hash)?;
    }
    Ok(())
}

fn start_agent_review_with_prepared_diff(
    app: &mut App,
    er_dir: &str,
    diff_hash: &str,
) -> anyhow::Result<()> {
    if let Some(prompt) = build_agent_review_prompt(app, er_dir, diff_hash) {
        app.spawn_agent_prompt("review", &prompt)?;
    }
    Ok(())
}

/// Build the review agent prompt from prepared-diff artifacts. Remote
/// (`--remote`) and local tabs share this prompt — the caller pre-writes
/// `diff-tmp`/`diff-annotated` so the agent never shells out to `gh`/`git`.
pub fn build_agent_review_prompt(app: &mut App, er_dir: &str, diff_hash: &str) -> Option<String> {
    let tab = app.tab();
    let mode = tab.mode;
    let base = tab.base_branch.clone();
    // Desktop convention (commands.rs:3194): the PR head is the local branch
    // view, not the checked-out branch — a wrong head_branch makes the
    // loader's artifacts_branch_mismatch silently discard the fresh review.
    // Remote tabs have no local_branch_view; `current_branch` is the PR head.
    let head = tab
        .local_branch_view
        .clone()
        .unwrap_or_else(|| tab.current_branch.clone());
    let scope = match mode {
        DiffMode::PrDiff => "branch".to_string(),
        m => m.git_mode().to_string(),
    };
    match mode {
        DiffMode::Branch | DiffMode::Unstaged | DiffMode::Staged | DiffMode::PrDiff => {
            Some(er_engine::ai::prompts::build_review_prompt_prepared_diff(
                &scope, er_dir, &base, &head, diff_hash, None,
            ))
        }
        _ => {
            app.notify("AI review not available in this mode");
            None
        }
    }
}

/// Build the validate agent prompt. Local mode only — remote validation needs a working
/// tree to read, which is a separate plumbing job (defer). Uses the prepared-diff prompt:
/// the caller pre-writes `diff-tmp`/`diff-annotated` via `ensure_diff_artifacts`, so the
/// agent anchors against the harness-computed hash instead of re-running `git diff` +
/// `sha256sum` + awk.
pub fn build_agent_validate_prompt(app: &mut App, er_dir: &str, diff_hash: &str) -> Option<String> {
    let tab = app.tab();
    if tab.is_remote() {
        app.notify("validate is local-only — checkout the PR first, then re-run review locally");
        return None;
    }
    let mode = tab.mode;
    let scope = if mode == DiffMode::PrDiff {
        "branch"
    } else {
        mode.git_mode()
    };
    match mode {
        DiffMode::Branch | DiffMode::Unstaged | DiffMode::Staged | DiffMode::PrDiff => Some(
            er_engine::ai::prompts::build_validate_prompt_prepared_diff(scope, er_dir, diff_hash),
        ),
        _ => {
            app.notify("validate not available in this mode");
            None
        }
    }
}

/// Build the questions agent prompt, using remote mode if applicable.
pub fn build_agent_questions_prompt(app: &mut App) -> Option<String> {
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
        return Some(er_engine::ai::prompts::build_questions_prompt_remote(
            parts[0],
            parts[1],
            pr_number,
            &output_dir,
        ));
    }
    let mode = tab.mode;
    let base = tab.base_branch.clone();
    let output_dir = tab.er_dir();
    match mode {
        DiffMode::Branch | DiffMode::Unstaged | DiffMode::Staged => Some(
            er_engine::ai::prompts::build_questions_prompt_local_managed(
                &base,
                mode.git_mode(),
                &output_dir,
            ),
        ),
        _ => {
            app.notify("AI questions not available in this mode");
            None
        }
    }
}

/// Build the notes-addressing agent prompt, using remote mode if applicable.
pub fn build_agent_notes_prompt(app: &mut App) -> Option<String> {
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
        return Some(er_engine::ai::prompts::build_notes_prompt_remote(
            parts[0],
            parts[1],
            pr_number,
            &output_dir,
        ));
    }
    let mode = tab.mode;
    let base = tab.base_branch.clone();
    let output_dir = tab.er_dir();
    match mode {
        DiffMode::Branch | DiffMode::Unstaged | DiffMode::Staged => {
            Some(er_engine::ai::prompts::build_notes_prompt_local_managed(
                &base,
                mode.git_mode(),
                &output_dir,
            ))
        }
        _ => {
            app.notify("AI notes not available in this mode");
            None
        }
    }
}

/// Build the summary generation agent prompt, using remote mode if applicable.
pub fn build_agent_summary_prompt(app: &mut App) -> Option<String> {
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
        return Some(er_engine::ai::prompts::build_summary_prompt_remote(
            parts[0],
            parts[1],
            pr_number,
            &output_dir,
        ));
    }
    let mode = tab.mode;
    let base = tab.base_branch.clone();
    let output_dir = tab.er_dir();
    match mode {
        DiffMode::Branch | DiffMode::Unstaged | DiffMode::Staged => {
            Some(er_engine::ai::prompts::build_summary_prompt_local_managed(
                &base,
                mode.git_mode(),
                &output_dir,
            ))
        }
        _ => {
            app.notify("Summary generation not available in this mode");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app_with_managed_dir(output_dir: &str) -> App {
        let mut app = App::new_for_test(vec![]);
        app.tab_mut().er_root = er_engine::ErRoot::Managed {
            agent_dir: output_dir.to_string(),
            session_dir: output_dir.to_string(),
        };
        app
    }

    fn assert_managed_prompt(prompt: &str, output_dir: &str, artifact: &str) {
        assert!(
            prompt.contains(output_dir),
            "prompt should mention managed output dir {output_dir}:\n{prompt}"
        );
        assert!(
            prompt.contains(artifact),
            "prompt should mention target artifact {artifact}:\n{prompt}"
        );
        assert!(
            !prompt.contains("> .er/diff-tmp"),
            "prompt must not write repo-local diff-tmp:\n{prompt}"
        );
    }

    fn review_ignore_diff() -> &'static str {
        "diff --git a/keep.rs b/keep.rs\n--- a/keep.rs\n+++ b/keep.rs\n@@ -0,0 +1 @@\n+keep\n\ndiff --git a/ignored.rs b/ignored.rs\n--- a/ignored.rs\n+++ b/ignored.rs\n@@ -0,0 +1 @@\n+ignore\n"
    }

    /// Write one project with these ignore globs through the projects record.
    fn install_review_ignore_store(globs: &[&str]) {
        er_engine::projects_pins::update(|file| {
            file.projects.push(er_engine::projects_pins::ProjectRecord {
                id: "local".into(),
                name: "widgets".into(),
                root_path: "/repo".into(),
                remote: Some("acme/widgets".into()),
                review_ignore_globs: globs.iter().map(|glob| glob.to_string()).collect(),
                ..Default::default()
            });
            Ok(true)
        })
        .unwrap();
    }

    #[test]
    fn review_progress_dispatch_is_read_only_and_scrolls() {
        let mut app = App::new_for_test(Vec::new());
        let bucket = app.tab().er_dir();
        dispatch_hub_action(
            &mut app,
            HubAction::OpenReviewFlowProgress {
                er_dir: bucket.clone(),
            },
        )
        .unwrap();
        assert!(app.overlay.is_none());
        app.overlay = Some(app::OverlayData::ReviewFlowProgress {
            er_dir: bucket,
            flow: Box::new(er_engine::ai::review_flow::ReviewFlow::new("captured")),
            selected: 0,
            detail_scroll: 0,
        });
        for code in [KeyCode::Enter, KeyCode::Char('a'), KeyCode::Char(' ')] {
            handle_overlay_input(&mut app, KeyEvent::new(code, KeyModifiers::NONE)).unwrap();
        }
        handle_overlay_input(
            &mut app,
            KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE),
        )
        .unwrap();
        assert!(matches!(
            app.overlay,
            Some(app::OverlayData::ReviewFlowProgress {
                detail_scroll: 8,
                ..
            })
        ));
        if let Some(app::OverlayData::ReviewFlowProgress { flow, .. }) = &mut app.overlay {
            use er_engine::ai::review_flow::*;
            let task = FlowTaskProgress {
                id: "one".into(),
                stage: FlowTaskStage::Finding,
                lens: None,
                label: "Finder".into(),
                purpose: "Captured purpose".into(),
                provider_id: "provider".into(),
                provider_label: "Provider".into(),
                model_id: "model".into(),
                model_label: "Model".into(),
                status: FlowTaskStatus::Queued,
                error: None,
                background_task_id: None,
                round: None,
                total_rounds: None,
            };
            flow.tasks = vec![
                task.clone(),
                FlowTaskProgress {
                    id: "two".into(),
                    ..task
                },
            ];
        }
        handle_overlay_input(&mut app, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)).unwrap();
        assert!(matches!(
            app.overlay,
            Some(app::OverlayData::ReviewFlowProgress {
                selected: 1,
                detail_scroll: 0,
                ..
            })
        ));
        handle_overlay_input(&mut app, KeyEvent::new(KeyCode::Up, KeyModifiers::NONE)).unwrap();
        assert!(matches!(
            app.overlay,
            Some(app::OverlayData::ReviewFlowProgress { selected: 0, .. })
        ));
        assert!(app.pending_hub_action.is_none());
        handle_overlay_input(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)).unwrap();
        assert!(app.overlay.is_none());
    }

    #[test]
    fn review_ignore_preparation_filters_before_hashing() {
        let tmp = tempfile::tempdir().unwrap();
        er_engine::storage::with_test_storage_root(tmp.path(), || {
            install_review_ignore_store(&["ignored.rs"]);

            let output_dir = tmp.path().join("bucket");
            std::fs::create_dir_all(&output_dir).unwrap();
            let mut app = app_with_managed_dir(output_dir.to_str().unwrap());
            app.tab_mut().repo_root = "/repo".into();
            app.tab_mut().remote_repo = Some("acme/widgets".into());
            app.tab_mut().set_raw_diff_for_test(review_ignore_diff());

            let (er_dir, hash) = ensure_prepared_diff_for_action(&mut app).expect("prepared diff");
            let prepared = std::fs::read_to_string(format!("{er_dir}/diff-tmp")).unwrap();
            assert!(prepared.contains("keep.rs"));
            assert!(!prepared.contains("ignored.rs"));
            assert_eq!(
                er_engine::ai::prepared_diff::diff_tmp_hash(&er_dir).as_deref(),
                Some(hash.as_str())
            );
        });
    }

    #[test]
    fn review_ignore_all_files_returns_before_preparation_or_spawn() {
        let tmp = tempfile::tempdir().unwrap();
        er_engine::storage::with_test_storage_root(tmp.path(), || {
            // The matcher supports filename extension globs, not a bare `**` glob.
            install_review_ignore_store(&["*.rs"]);

            let output_dir = tmp.path().join("bucket");
            std::fs::create_dir_all(&output_dir).unwrap();
            let mut app = app_with_managed_dir(output_dir.to_str().unwrap());
            app.tab_mut().repo_root = "/repo".into();
            app.tab_mut().set_raw_diff_for_test(review_ignore_diff());

            assert!(ensure_prepared_diff_for_action(&mut app).is_none());
            assert!(!output_dir.join("diff-tmp").exists());
            assert!(app.background_task_snapshots().is_empty());
            assert!(app
                .notification
                .as_ref()
                .is_some_and(|notification| notification.message.contains("Nothing to review")));
        });
    }

    #[test]
    fn review_ignore_empty_globs_preserve_raw_bytes_and_lookup_root_remote_parity() {
        let tmp = tempfile::tempdir().unwrap();
        er_engine::storage::with_test_storage_root(tmp.path(), || {
            install_review_ignore_store(&[]);

            let output_dir = tmp.path().join("bucket");
            std::fs::create_dir_all(&output_dir).unwrap();
            let mut app = app_with_managed_dir(output_dir.to_str().unwrap());
            app.tab_mut().repo_root = "/repo".into();
            app.tab_mut().remote_repo = Some("acme/widgets".into());
            app.tab_mut().set_raw_diff_for_test(review_ignore_diff());

            let (er_dir, _) = ensure_prepared_diff_for_action(&mut app).expect("prepared diff");
            assert_eq!(
                std::fs::read(format!("{er_dir}/diff-tmp")).unwrap(),
                review_ignore_diff().as_bytes()
            );
            assert_eq!(
                er_engine::projects_pins::review_ignore_globs_for_repo("/repo", None),
                Vec::<String>::new()
            );
            assert_eq!(
                er_engine::projects_pins::review_ignore_globs_for_repo(
                    "/other",
                    Some("acme/widgets")
                ),
                Vec::<String>::new()
            );
        });
    }

    #[test]
    fn confirmed_general_review_preserves_bucket_while_review_flow_holds_lease() {
        let tmp = tempfile::tempdir().unwrap();
        er_engine::storage::with_test_storage_root(tmp.path(), || {
            install_review_ignore_store(&[]);

            let repo = tmp.path().join("repo");
            let bucket = tmp.path().join("bucket");
            std::fs::create_dir_all(&repo).unwrap();
            std::fs::create_dir_all(&bucket).unwrap();
            let old_review = br#"{"version":1,"diff_hash":"previous-generation","files":{}}"#;
            std::fs::write(bucket.join("review.json"), old_review).unwrap();

            let mut app = app_with_managed_dir(bucket.to_str().unwrap());
            app.tab_mut().repo_root = repo.to_string_lossy().into_owned();
            app.tab_mut().set_raw_diff_for_test(review_ignore_diff());
            app.tab_mut().reload_ai_state();
            assert!(app.tab().ai.review.is_some(), "fixture review must load");
            app.config.ai_hub = er_engine::config::AiHubConfig {
                default_provider: Some("fake".into()),
                review_models: {
                    let models = vec![er_engine::config::ReviewModelRef {
                        provider_id: "fake".into(),
                        model_id: "fake-model".into(),
                    }];
                    er_engine::config::ReviewModelGroups {
                        fast: models.clone(),
                        standard: models.clone(),
                        advanced: models,
                    }
                },
                providers: [(
                    "fake".into(),
                    er_engine::config::AiProviderConfig {
                        command: "/bin/sh".into(),
                        args: vec![
                            "-c".into(),
                            "exit 1".into(),
                            "fake".into(),
                            "{prompt}".into(),
                        ],
                        models: vec![er_engine::config::AiModelConfig {
                            id: "fake-model".into(),
                            ..Default::default()
                        }],
                        ..Default::default()
                    },
                )]
                .into(),
                max_concurrent_reviews: 16,
                max_concurrent_agents: 16,
                ..Default::default()
            };

            // This public entry point acquires the real bucket lease and starts
            // the fake triage provider. The failed command remains registered
            // until App drains its completion event.
            app.start_review_flow("branch").unwrap();
            let er_dir = app.tab().er_dir();
            assert!(app.check_review_bucket_admission(&er_dir).is_err());
            let files = [
                "review.json",
                "diff-tmp",
                ".diff-tmp.sha256",
                "diff-annotated",
                ".diff-annotated.sha256",
                "change-facts.md",
            ];
            let before: Vec<_> = files
                .iter()
                .map(|name| std::fs::read(bucket.join(name)).unwrap())
                .collect();
            let task_ids_before: std::collections::HashSet<_> = app
                .background_task_snapshots()
                .into_iter()
                .map(|task| task.id)
                .collect();
            assert_eq!(task_ids_before.len(), 1, "only triage should be queued");

            // A different in-memory diff makes any missed admission check rewrite
            // the pinned artifacts before cleanup runs.
            app.tab_mut()
                .set_raw_diff_for_test(&review_ignore_diff().replace("keep", "changed"));
            dispatch_hub_action(&mut app, HubAction::PromptReview).unwrap();
            assert!(matches!(
                app.input_mode,
                InputMode::Confirm(ConfirmAction::RunAgentReview {
                    clear_previous: true
                })
            ));
            handle_confirm_input(
                &mut app,
                KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE),
            )
            .unwrap();

            for (name, bytes) in files.iter().zip(before) {
                assert_eq!(
                    std::fs::read(bucket.join(name)).unwrap(),
                    bytes,
                    "confirmation must preserve {name} while the flow owns the bucket"
                );
            }
            let task_ids_after: std::collections::HashSet<_> = app
                .background_task_snapshots()
                .into_iter()
                .map(|task| task.id)
                .collect();
            assert_eq!(
                task_ids_after, task_ids_before,
                "confirmation spawned a producer"
            );
            assert!(app.notification.as_ref().is_some_and(|notification| {
                notification.message.contains("action skipped")
                    && notification.message.contains("review flow")
            }));
        });
    }

    #[test]
    fn validate_action_ensures_artifacts_for_review_only_tabs() {
        // With a review but zero GitHub comments, PromptValidate must still
        // prepare the diff artifacts: gating the ensure call on comments makes
        // validate silently no-op.
        let dir = tempfile::tempdir().unwrap();
        let output_dir = dir.path().to_str().unwrap();
        er_engine::storage::with_test_storage_root(dir.path(), || {
            let mut app = app_with_managed_dir(output_dir);
            let raw = "diff --git a/x b/x\n@@ -1 +1 @@\n-old\n+new\n";
            app.tab_mut().set_raw_diff_for_test(raw);
            app.tab_mut().ai.review = Some(er_engine::ai::ErReview {
                version: 1,
                diff_hash: String::new(),
                created_at: String::new(),
                base_branch: "main".into(),
                head_branch: "feature".into(),
                files: std::collections::HashMap::new(),
                file_hashes: std::collections::HashMap::new(),
            });
            // The handler fires a real agent subprocess on success — keep the
            // child inert so the test has no external side effect (ratchet-6).
            app.config.agent.command = "/bin/true".to_string();

            dispatch_hub_action(&mut app, HubAction::PromptValidate).expect("validate dispatch");

            assert!(
                std::fs::metadata(format!("{output_dir}/.diff-tmp.sha256")).is_ok(),
                "artifacts ensured even with zero comments"
            );
            assert!(
                std::fs::metadata(format!("{output_dir}/diff-annotated")).is_ok(),
                "annotated diff ensured"
            );
        });
    }

    #[test]
    fn local_ai_prompt_builders_target_tab_er_dir() {
        let root = tempfile::tempdir().unwrap();
        er_engine::storage::with_test_storage_root(root.path(), || {
            let output_dir = "/tmp/er-tui-managed";
            let mut app = app_with_managed_dir(output_dir);
            let er_dir = app.tab().er_dir();
            let hash = "abc123abc123abc123abc123abc123abc123abc123abc123abc123abc123abc1";

            let review = build_agent_review_prompt(&mut app, &er_dir, hash).expect("review prompt");
            assert_managed_prompt(&review, output_dir, "review.json");

            let validate =
                build_agent_validate_prompt(&mut app, &er_dir, hash).expect("validate prompt");
            assert_managed_prompt(&validate, output_dir, "review.json");

            let questions = build_agent_questions_prompt(&mut app).expect("questions prompt");
            assert_managed_prompt(&questions, output_dir, "questions.json");

            let summary = build_agent_summary_prompt(&mut app).expect("summary prompt");
            assert_managed_prompt(&summary, output_dir, "summary.md");

            let triage = build_agent_triage_prompt(&mut app, &er_dir, hash).expect("triage prompt");
            assert_managed_prompt(&triage, output_dir, "triage.json");

            let professor =
                build_agent_professor_prompt(&mut app, &er_dir, hash).expect("professor prompt");
            assert_managed_prompt(&professor, output_dir, "professor.json");

            let expert = build_agent_expert_prompt(&mut app, "security", &er_dir, hash)
                .expect("expert prompt");
            assert_managed_prompt(&expert, output_dir, "experts/security.json");
        });
    }

    #[test]
    fn remote_review_uses_prepared_diff_not_gh_pr_diff() {
        // The sandboxed agent cannot reach GitHub, so a remote review must not
        // be told to `gh pr diff` from the artifact directory — it uses the
        // prepared-diff prompt, as desktop does.
        let dir = tempfile::tempdir().unwrap();
        let output_dir = dir.path().to_str().unwrap();
        er_engine::storage::with_test_storage_root(dir.path(), || {
            let mut app = app_with_managed_dir(output_dir);
            app.tab_mut().remote_repo = Some("reshapebiotech/discovery".into());
            app.tab_mut().pr_number = Some(1473);
            app.tab_mut().mode = DiffMode::PrDiff;
            app.tab_mut().base_branch = "main".into();
            app.tab_mut().current_branch = "agent/refresh-all-experiment-metrics".into();
            let raw = "diff --git a/x.rs b/x.rs\nindex 0000000..1111111 100644\n--- a/x.rs\n+++ b/x.rs\n@@ -1 +1,2 @@\n fn x() {}\n+fn y() {}\n";
            app.tab_mut().set_raw_diff_for_test(raw);
            app.config.agent.command = "/bin/true".to_string();

            let er_dir = app.tab().er_dir();
            let hash = "abc123abc123abc123abc123abc123abc123abc123abc123abc123abc123abc1";
            let prompt =
                build_agent_review_prompt(&mut app, &er_dir, hash).expect("remote review prompt");
            assert_managed_prompt(&prompt, output_dir, "review.json");
            assert!(
                prompt.contains("prepared diff"),
                "remote review must use the prepared-diff prompt:\n{prompt}"
            );
            assert!(
                !prompt.contains("gh pr diff"),
                "sandboxed remote agent must not be told to fetch via gh:\n{prompt}"
            );

            dispatch_hub_action(&mut app, HubAction::PromptReview).expect("review dispatch");
            assert!(
                std::fs::metadata(format!("{output_dir}/diff-tmp")).is_ok(),
                "parent must write diff-tmp before spawning the agent"
            );
            assert!(
                std::fs::metadata(format!("{output_dir}/diff-annotated")).is_ok(),
                "parent must write diff-annotated before spawning the agent"
            );
        });
    }

    #[test]
    fn action_bound_model_selection_does_not_change_the_next_default_action() {
        let mut app = App::new_for_test(vec![]);
        app.config.ai_hub.default_provider = Some("codex".into());
        app.config.ai_hub.default_model = Some("gpt-5.6-luna".into());
        app.config.ai_hub.providers.insert(
            "codex".into(),
            er_engine::config::AiProviderConfig {
                models: vec![
                    er_engine::config::AiModelConfig {
                        id: "gpt-5.6-luna".into(),
                        ..Default::default()
                    },
                    er_engine::config::AiModelConfig {
                        id: "gpt-5.3-codex-spark".into(),
                        ..Default::default()
                    },
                ],
                ..Default::default()
            },
        );
        app.sync_ai_selection_from_defaults();

        let selected_for_run = app
            .resolve_ai_selection_override("codex", Some("gpt-5.3-codex-spark"))
            .unwrap();
        execute_ai_action_with_selection(&mut app, AiActionKind::Validate, selected_for_run)
            .unwrap();

        assert_eq!(app.current_ai_model.as_deref(), Some("gpt-5.6-luna"));
        assert_eq!(
            app.config.ai_hub.default_model.as_deref(),
            Some("gpt-5.6-luna")
        );
    }

    fn app_with_effort_models() -> App {
        let mut app = App::new_for_test(vec![]);
        app.config.ai_hub.providers.insert(
            "codex".into(),
            er_engine::config::AiProviderConfig {
                label: Some("Codex".into()),
                models: vec![
                    er_engine::config::AiModelConfig {
                        id: "gpt-5.6-luna".into(),
                        label: Some("GPT-5.6 Luna".into()),
                        effort_levels: vec!["low".into(), "medium".into(), "high".into()],
                        ..Default::default()
                    },
                    er_engine::config::AiModelConfig {
                        id: "no-effort".into(),
                        label: Some("No Effort".into()),
                        ..Default::default()
                    },
                ],
                ..Default::default()
            },
        );
        app.current_ai_provider = Some("codex".into());
        app.current_ai_model = Some("no-effort".into());
        app
    }

    #[test]
    fn selecting_effort_capable_model_opens_effort_picker_without_activating() {
        let mut app = app_with_effort_models();
        dispatch_hub_action(
            &mut app,
            HubAction::SelectAiModel {
                action: None,
                provider_id: "codex".into(),
                model_id: "gpt-5.6-luna".into(),
            },
        )
        .unwrap();
        assert_eq!(app.current_ai_model.as_deref(), Some("no-effort"));
        assert!(
            matches!(
                app.overlay,
                Some(er_engine::app::OverlayData::ModalHub {
                    kind: er_engine::app::HubKind::AiEffort,
                    ..
                })
            ),
            "expected effort picker overlay, got {:?}",
            app.overlay
        );
    }

    #[test]
    fn selecting_effort_activates_the_model() {
        let mut app = app_with_effort_models();
        dispatch_hub_action(
            &mut app,
            HubAction::SelectAiEffort {
                action: None,
                provider_id: "codex".into(),
                model_id: "gpt-5.6-luna".into(),
                effort: "high".into(),
            },
        )
        .unwrap();
        assert_eq!(app.current_ai_model.as_deref(), Some("gpt-5.6-luna"));
        assert_eq!(app.current_ai_effort.as_deref(), Some("high"));
        assert!(app.overlay.is_none());
    }

    #[test]
    fn selecting_model_without_effort_activates_immediately() {
        let mut app = app_with_effort_models();
        app.current_ai_model = Some("gpt-5.6-luna".into());
        dispatch_hub_action(
            &mut app,
            HubAction::SelectAiModel {
                action: None,
                provider_id: "codex".into(),
                model_id: "no-effort".into(),
            },
        )
        .unwrap();
        assert_eq!(app.current_ai_model.as_deref(), Some("no-effort"));
        assert!(app.overlay.is_none());
    }

    fn overlay_kind(app: &App) -> Option<er_engine::app::HubKind> {
        match &app.overlay {
            Some(er_engine::app::OverlayData::ModalHub { kind, .. }) => Some(*kind),
            _ => None,
        }
    }

    #[test]
    fn selecting_single_effort_model_provider_opens_effort_picker() {
        let mut app = app_with_effort_models();
        app.config
            .ai_hub
            .providers
            .get_mut("codex")
            .unwrap()
            .models
            .truncate(1);
        dispatch_hub_action(
            &mut app,
            HubAction::SelectAiProvider {
                action: None,
                provider_id: "codex".into(),
            },
        )
        .unwrap();
        assert_eq!(app.current_ai_model.as_deref(), Some("no-effort"));
        assert_eq!(overlay_kind(&app), Some(er_engine::app::HubKind::AiEffort));
    }

    #[test]
    fn selecting_effort_for_an_action_does_not_change_the_default() {
        let mut app = app_with_effort_models();
        app.config.ai_hub.default_provider = Some("codex".into());
        app.config.ai_hub.default_model = Some("no-effort".into());
        dispatch_hub_action(
            &mut app,
            HubAction::SelectAiEffort {
                action: Some(AiActionKind::Validate),
                provider_id: "codex".into(),
                model_id: "gpt-5.6-luna".into(),
                effort: "high".into(),
            },
        )
        .unwrap();
        assert_eq!(app.current_ai_model.as_deref(), Some("no-effort"));
        assert_eq!(app.current_ai_effort, None);
        assert_eq!(
            app.config.ai_hub.default_model.as_deref(),
            Some("no-effort")
        );
        assert!(app.overlay.is_none());
    }

    #[test]
    fn escape_from_effort_picker_returns_to_model_list() {
        let mut app = app_with_effort_models();
        dispatch_hub_action(
            &mut app,
            HubAction::SelectAiModel {
                action: None,
                provider_id: "codex".into(),
                model_id: "gpt-5.6-luna".into(),
            },
        )
        .unwrap();
        handle_overlay_input(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)).unwrap();
        assert_eq!(app.current_ai_model.as_deref(), Some("no-effort"));
        assert_eq!(overlay_kind(&app), Some(er_engine::app::HubKind::AiModel));
    }

    #[test]
    fn backspace_from_effort_picker_returns_to_model_list() {
        let mut app = app_with_effort_models();
        dispatch_hub_action(
            &mut app,
            HubAction::SelectAiModel {
                action: None,
                provider_id: "codex".into(),
                model_id: "gpt-5.6-luna".into(),
            },
        )
        .unwrap();
        handle_overlay_input(
            &mut app,
            KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE),
        )
        .unwrap();
        assert_eq!(app.current_ai_model.as_deref(), Some("no-effort"));
        assert_eq!(overlay_kind(&app), Some(er_engine::app::HubKind::AiModel));
    }

    #[test]
    fn char_boundaries_step_over_multibyte_chars() {
        let s = "aé€b";
        // a=0, é=1..3, €=3..6, b=6..7
        assert_eq!(prev_char_boundary(s, 0), 0);
        assert_eq!(prev_char_boundary(s, 1), 0);
        assert_eq!(prev_char_boundary(s, 3), 1);
        assert_eq!(prev_char_boundary(s, 6), 3);
        assert_eq!(prev_char_boundary(s, 7), 6);
        assert_eq!(next_char_boundary(s, 0), 1);
        assert_eq!(next_char_boundary(s, 1), 3);
        assert_eq!(next_char_boundary(s, 3), 6);
        assert_eq!(next_char_boundary(s, 6), 7);
    }

    #[test]
    fn review_flow_result_selection_toggles_only_eligible_rows() {
        let mut selected = std::collections::HashSet::new();
        assert!(!toggle_selected_finding_id(&mut selected, "dropped", false));
        assert!(selected.is_empty());
        assert!(toggle_selected_finding_id(&mut selected, "kept", true));
        assert!(selected.contains("kept"));
        assert!(toggle_selected_finding_id(&mut selected, "kept", true));
        assert!(selected.is_empty());
    }

    #[test]
    fn review_flow_results_keys_keep_close_separate_from_accept() {
        assert_eq!(
            review_flow_results_key_action(KeyCode::Char('j')),
            ReviewFlowResultsKeyAction::Next
        );
        assert_eq!(
            review_flow_results_key_action(KeyCode::PageDown),
            ReviewFlowResultsKeyAction::PageDown
        );
        assert_eq!(
            review_flow_results_key_action(KeyCode::PageUp),
            ReviewFlowResultsKeyAction::PageUp
        );
        assert_eq!(
            review_flow_results_key_action(KeyCode::Char(' ')),
            ReviewFlowResultsKeyAction::Toggle
        );
        assert_eq!(
            review_flow_results_key_action(KeyCode::Char('a')),
            ReviewFlowResultsKeyAction::Accept
        );
        assert_eq!(
            review_flow_results_key_action(KeyCode::Esc),
            ReviewFlowResultsKeyAction::Close
        );
        assert_ne!(
            review_flow_results_key_action(KeyCode::Esc),
            ReviewFlowResultsKeyAction::Accept
        );
    }

    #[test]
    fn accept_action_uses_captured_bucket_and_stable_finding_ids() {
        let selected = std::collections::HashSet::from(["finding-b".into(), "finding-a".into()]);
        assert_eq!(
            build_accept_review_flow_action("/captured/bucket", "run-captured", &selected),
            Some(HubAction::AcceptReviewFlowFindings {
                er_dir: "/captured/bucket".into(),
                run_id: "run-captured".into(),
                finding_ids: vec!["finding-a".into(), "finding-b".into()],
            })
        );
        assert!(build_accept_review_flow_action("/bucket", "run", &Default::default()).is_none());
    }

    struct FailingCommentPoster;

    impl er_engine::github_post::GitHubCommentPoster for FailingCommentPoster {
        fn post(
            &self,
            _request: &er_engine::github_post::GitHubPostRequest,
        ) -> er_engine::github_post::GitHubPostOutcome {
            er_engine::github_post::GitHubPostOutcome::NotPosted {
                error: "network down".to_string(),
            }
        }

        fn list(
            &self,
            _request: &er_engine::github_post::GitHubPostRequest,
        ) -> anyhow::Result<Vec<er_engine::github_post::ListedGitHubComment>> {
            Ok(Vec::new())
        }

        fn viewer_login(
            &self,
            _request: &er_engine::github_post::GitHubPostRequest,
        ) -> anyhow::Result<String> {
            Ok("viewer".to_string())
        }
    }

    #[test]
    fn a_failed_post_keeps_comment_mode_and_the_text() {
        use crate::app::InputMode;
        use er_engine::ai::CommentType;

        let dir = tempfile::tempdir().unwrap();
        er_engine::storage::with_test_storage_root(dir.path(), || {
            let mut app = er_engine::app::App::new_for_test(Vec::new());
            {
                let tab = app.tab_mut();
                tab.remote_repo = Some("owner/repo".to_string());
                tab.pr_number = Some(7);
                tab.comment_file = "src/a.rs".to_string();
                tab.comment_hunk = 0;
                tab.comment_line_num = Some(3);
                tab.comment_type = CommentType::GitHubComment;
                tab.comment_textarea = tui_textarea::TextArea::new(vec!["keep me".to_string()]);
            }
            app.input_mode = InputMode::Comment;
            app.set_github_comment_poster(std::sync::Arc::new(FailingCommentPoster));

            handle_comment_input(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
                .unwrap();

            assert_eq!(app.input_mode, InputMode::Comment);
            assert_eq!(app.tab().comment_textarea.lines().join("\n"), "keep me");
            assert!(app
                .notification
                .as_ref()
                .is_some_and(|n| n.message.contains("Comment not posted")));
        });
    }

    #[test]
    fn c_on_a_tab_known_to_have_no_pr_does_not_open_the_github_composer() {
        use crate::app::InputMode;

        let mut app = er_engine::app::App::new_for_test(Vec::new());
        app.tab_mut().pr_absent = true;
        app.input_mode = InputMode::Normal;

        let (watch_tx, _watch_rx) = std::sync::mpsc::channel();
        let mut watcher = None;
        handle_normal_input(
            &mut app,
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE),
            &watch_tx,
            &mut watcher,
        )
        .unwrap();

        assert_ne!(app.input_mode, InputMode::Comment);
        assert!(app
            .notification
            .as_ref()
            .is_some_and(|n| n.message.contains("No pull request for this branch")));
    }

    #[test]
    fn a_post_whose_pr_lookup_fails_keeps_the_text() {
        use crate::app::InputMode;
        use er_engine::ai::CommentType;

        let mut app = er_engine::app::App::new_for_test(Vec::new());
        {
            let tab = app.tab_mut();
            // `gh pr view` fails to spawn here, so target resolution reports no PR.
            tab.repo_root = "/nonexistent-er-no-pr-repo".to_string();
            tab.pr_number = None;
            tab.remote_repo = None;
            tab.comment_file = "src/a.rs".to_string();
            tab.comment_hunk = 0;
            tab.comment_line_num = Some(3);
            tab.comment_type = CommentType::GitHubComment;
            tab.comment_textarea = tui_textarea::TextArea::new(vec!["keep me too".to_string()]);
        }
        app.input_mode = InputMode::Comment;

        handle_comment_input(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)).unwrap();

        assert_eq!(app.input_mode, InputMode::Comment);
        assert_eq!(app.tab().comment_textarea.lines().join("\n"), "keep me too");
        assert!(app
            .notification
            .as_ref()
            .is_some_and(|n| n.message.contains("Comment not posted")));
    }

    #[test]
    #[cfg(unix)]
    fn the_summary_hub_action_submits_a_summary_job() {
        let dir = tempfile::tempdir().unwrap();
        er_engine::storage::with_test_storage_root(dir.path(), || {
            // The recorder is process-wide; start from an empty slate.
            er_engine::app::take_submitted_tab_job_kinds();
            let output_dir = dir.path().join("er").to_string_lossy().into_owned();
            std::fs::create_dir_all(&output_dir).unwrap();
            let mut app = app_with_managed_dir(&output_dir);

            let argv_file = dir.path().join("summary-argv.txt");
            let bin = dir.path().join("fake-agent");
            let out = argv_file.display();
            er_engine::test_support::write_executable_script(
                &bin,
                &format!(
                    "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"{out}.tmp\" && mv \"{out}.tmp\" \"{out}\"\nprintf 'summary\\n' > \"$ER_JOB_STAGING/summary.md\"\n"
                ),
            )
            .unwrap();
            app.config.agent.command = bin.to_string_lossy().into_owned();
            app.config.agent.args = vec!["{prompt}".into()];

            dispatch_hub_action(&mut app, HubAction::RunAiAction(AiActionKind::Summary)).unwrap();

            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            loop {
                app.check_commands();
                match app.tab().command_status.get("summary") {
                    Some(er_engine::app::CommandStatus::Done) => break,
                    Some(er_engine::app::CommandStatus::Failed(reason)) => {
                        panic!("the summary job failed: {reason}")
                    }
                    _ => {}
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "the summary job did not finish"
                );
                std::thread::sleep(std::time::Duration::from_millis(10));
            }

            // The prompt reaches the CLI through argv; a shell-routed summary
            // would never run the provider at all.
            let argv = std::fs::read_to_string(&argv_file).expect("the provider ran");
            assert!(
                argv.contains("summary.md"),
                "the prompt should reach the CLI through argv: {argv}"
            );
            // Other tests in this binary submit jobs too; the summary must be
            // among them, and not fall back to a catch-all kind.
            let kinds = er_engine::app::take_submitted_tab_job_kinds();
            assert!(
                kinds.contains(&er_engine::job::JobKind::Summary),
                "{kinds:?}"
            );
            assert!(
                !kinds.contains(&er_engine::job::JobKind::Other("summary".into())),
                "{kinds:?}"
            );
        });
    }
}
