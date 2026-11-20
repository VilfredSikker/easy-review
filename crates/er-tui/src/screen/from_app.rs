//! The one function that builds the flag-off `Screen` from the App.

use super::view::{self, ConfirmAction, HubKind, InputMode, OverlayData};
use super::{AppData, Chrome, HistoryView, MemoryBudget, Screen, TabData, TabView};
use er_engine::app::{self, App};

/// The App's lists whose rows are App types, converted to view rows and kept
/// between frames. A list is converted again only when its rows change, so a
/// frame copies none.
#[derive(Default)]
pub struct ViewCache {
    hub_items: Vec<view::HubItem>,
    dir_entries: Vec<view::DirEntry>,
}

impl ViewCache {
    fn sync(&mut self, app: &App) {
        match &app.overlay {
            Some(app::OverlayData::ModalHub { items, .. }) if self.hub_items[..] != items[..] => {
                self.hub_items = items.iter().map(hub_item).collect();
            }
            Some(app::OverlayData::DirectoryBrowser { entries, .. })
                if self.dir_entries[..] != entries[..] =>
            {
                self.dir_entries = entries.iter().map(dir_entry).collect();
            }
            _ => {}
        }
    }
}

/// Builds the frame's read model. Converts a cached list only when the App's
/// copy has changed; everything else is borrowed or computed as the
/// renderers computed it.
pub fn from_app<'a>(app: &'a App, cache: &'a mut ViewCache) -> Screen<'a> {
    cache.sync(app);
    let cache: &'a ViewCache = cache;
    let tab = app.tab();
    Screen {
        chrome: Chrome {
            tab_names: app.tabs.iter().map(app::TabState::tab_name).collect(),
            active_tab: app.active_tab,
            theme: &app.config.display.theme,
            config: &app.config,
            available_update: app.available_update.as_deref(),
            notification: app.notification.as_ref().map(|n| n.message.as_str()),
            config_load_error: app.config_load_error.as_deref(),
            watching: app.watching,
            input_mode: input_mode(&app.input_mode),
            overlay: app
                .overlay
                .as_ref()
                .map(|overlay| overlay_data(overlay, cache)),
            remote_url_input: &app.remote_url_input,
            ai_selection_label: app.active_ai_selection_label(),
            has_comment_draft: app.has_comment_draft(),
            can_toggle_comment_type: app.can_toggle_comment_type(),
            split_diff: app.split_diff_active(&app.config),
        },
        tab: TabView {
            mode: tab.mode,
            name: tab.tab_name(),
            remote_repo: tab.remote_repo.as_deref(),
            is_remote: tab.is_remote(),
            current_branch: &tab.current_branch,
            base_branch: &tab.base_branch,
            pr_number: tab.pr_number,
            merge_active: tab.merge_active,
            unresolved_count: tab.unresolved_count,
            committed_unpushed: tab.committed_unpushed,
            sort_by_mtime: tab.sort_by_mtime,
            modes: tab.visible_modes(&app.config),
            line_stats: tab.diff_line_stats(),
            layers: &tab.layers,
            panel: tab.panel,
            panel_focus: tab.panel_focus,
            file_tree_width: tab.file_tree_width,
            panel_width: tab.panel_width,
            review_focus: tab.review_focus,
            selected_file: tab.selected_file,
            focused_comment_id: tab.focused_comment_id.as_deref(),
            focused_finding_id: tab.focused_finding_id.as_deref(),
            filter_expr: &tab.filter_expr,
            search_query: &tab.search_query,
            show_unreviewed_only: tab.show_unreviewed_only,
            show_watched: tab.show_watched,
            watched_count: tab.watched_files.len(),
            lazy_mode: tab.lazy_mode,
            comment_type: tab.comment_type,
            comment_reply_to: tab.comment_reply_to.as_deref(),
            comment_finding_ref: tab.comment_finding_ref.as_deref(),
            comment_file: &tab.comment_file,
            comment_line_num: tab.comment_line_num,
            comment_hunk: tab.comment_hunk,
            comment_textarea: &tab.comment_textarea,
            filter_input: &tab.filter_input,
            commit_input: &tab.commit_input,
        },
        data: TabData::App(AppData {
            files: &tab.files,
            ai: &tab.ai,
            pr_data: tab.pr_data.as_ref(),
            history: tab.history.as_ref().map(|history| HistoryView {
                commits: &history.commits,
                selected_commit: history.selected_commit,
                commit_files: &history.commit_files,
                selected_file: history.selected_file,
            }),
            reviewed_count: tab.reviewed_count(),
            filtered_reviewed_count: tab.filtered_reviewed_count(),
            running_commands: tab
                .agent_statuses()
                .into_iter()
                .map(|(name, _, _)| name)
                .collect(),
            mem_budget: MemoryBudget {
                parsed_files: tab.mem_budget.parsed_files,
                total_lines: tab.mem_budget.total_lines,
                compacted_files: tab.mem_budget.compacted_files,
            },
        }),
        app,
    }
}

const fn input_mode(mode: &app::InputMode) -> InputMode<'_> {
    match mode {
        app::InputMode::Normal => InputMode::Normal,
        app::InputMode::Search => InputMode::Search,
        app::InputMode::Comment => InputMode::Comment,
        app::InputMode::Confirm(action) => InputMode::Confirm(confirm_action(action)),
        app::InputMode::Filter => InputMode::Filter,
        app::InputMode::Commit => InputMode::Commit,
        app::InputMode::RemoteUrl => InputMode::RemoteUrl,
        app::InputMode::BaseBranch => InputMode::BaseBranch,
    }
}

const fn confirm_action(action: &app::ConfirmAction) -> ConfirmAction<'_> {
    match action {
        app::ConfirmAction::DeleteComment { .. } => ConfirmAction::DeleteComment,
        app::ConfirmAction::DeleteWatchedFile { path } => ConfirmAction::DeleteWatchedFile {
            path: path.as_str(),
        },
        app::ConfirmAction::Push => ConfirmAction::Push,
        app::ConfirmAction::CleanupQuestions { count } => {
            ConfirmAction::CleanupQuestions { count: *count }
        }
        app::ConfirmAction::CleanupReviews { count } => {
            ConfirmAction::CleanupReviews { count: *count }
        }
        app::ConfirmAction::RunAgentReview { .. } => ConfirmAction::RunAgentReview,
        app::ConfirmAction::RunAgentQuestions { .. } => ConfirmAction::RunAgentQuestions,
        app::ConfirmAction::RunAgentNotes { .. } => ConfirmAction::RunAgentNotes,
        app::ConfirmAction::ApprovePR => ConfirmAction::ApprovePR,
    }
}

fn overlay_data<'a>(overlay: &'a app::OverlayData, cache: &'a ViewCache) -> OverlayData<'a> {
    match overlay {
        app::OverlayData::WorktreePicker {
            worktrees,
            selected,
        } => OverlayData::WorktreePicker {
            worktrees,
            selected: *selected,
        },
        app::OverlayData::DirectoryBrowser {
            current_path,
            selected,
            ..
        } => OverlayData::DirectoryBrowser {
            current_path,
            entries: &cache.dir_entries,
            selected: *selected,
        },
        app::OverlayData::FilterHistory {
            history,
            selected,
            preset_count,
        } => OverlayData::FilterHistory {
            history,
            selected: *selected,
            preset_count: *preset_count,
        },
        app::OverlayData::ModalHub {
            kind,
            title,
            selected,
            ..
        } => OverlayData::ModalHub {
            kind: hub_kind(*kind),
            title: title.as_deref(),
            items: &cache.hub_items,
            selected: *selected,
        },
        app::OverlayData::ConfigHub {
            tab,
            items,
            selected,
            editing,
            ..
        } => OverlayData::ConfigHub {
            tab: *tab,
            items,
            selected: *selected,
            editing: editing.as_ref().map(|edit| view::ConfigEditState {
                item_index: edit.item_index,
                buffer: &edit.buffer,
            }),
        },
        app::OverlayData::ExportPicker {
            include_comments,
            include_findings,
            include_questions,
            include_notes,
            selected,
        } => OverlayData::ExportPicker {
            include_comments: *include_comments,
            include_findings: *include_findings,
            include_questions: *include_questions,
            include_notes: *include_notes,
            selected: *selected,
        },
        app::OverlayData::ReviewFlowProgress {
            er_dir,
            flow,
            selected,
            detail_scroll,
        } => OverlayData::ReviewFlowProgress {
            er_dir,
            flow,
            selected: *selected,
            detail_scroll: *detail_scroll,
        },
        app::OverlayData::ReviewFlowResults {
            results,
            selected,
            selected_ids,
            detail_scroll,
        } => OverlayData::ReviewFlowResults {
            results,
            selected: *selected,
            selected_ids,
            detail_scroll: *detail_scroll,
        },
    }
}

const fn hub_kind(kind: app::HubKind) -> HubKind {
    match kind {
        app::HubKind::Git => HubKind::Git,
        app::HubKind::Ai => HubKind::Ai,
        app::HubKind::AiProvider => HubKind::AiProvider,
        app::HubKind::AiModel => HubKind::AiModel,
        app::HubKind::AiEffort => HubKind::AiEffort,
        app::HubKind::AiExpert => HubKind::AiExpert,
        app::HubKind::Verify => HubKind::Verify,
        app::HubKind::VerifyPackage => HubKind::VerifyPackage,
        app::HubKind::Help => HubKind::Help,
        app::HubKind::Open => HubKind::Open,
        app::HubKind::Copy => HubKind::Copy,
    }
}

const fn opens_stack_pr(item: &app::HubItem) -> bool {
    matches!(item.action, app::HubAction::OpenStackPr { .. })
}

fn hub_item(item: &app::HubItem) -> view::HubItem {
    view::HubItem {
        label: item.label.clone(),
        hint: item.hint.clone(),
        description: item.description.clone(),
        is_header: item.is_header,
        enabled: item.enabled,
        opens_stack_pr: opens_stack_pr(item),
    }
}

impl PartialEq<app::HubItem> for view::HubItem {
    fn eq(&self, item: &app::HubItem) -> bool {
        self.label == item.label
            && self.hint == item.hint
            && self.description == item.description
            && self.is_header == item.is_header
            && self.enabled == item.enabled
            && self.opens_stack_pr == opens_stack_pr(item)
    }
}

fn dir_entry(entry: &app::DirEntry) -> view::DirEntry {
    view::DirEntry {
        name: entry.name.clone(),
        is_dir: entry.is_dir,
        is_git_repo: entry.is_git_repo,
    }
}

impl PartialEq<app::DirEntry> for view::DirEntry {
    fn eq(&self, entry: &app::DirEntry) -> bool {
        self.name == entry.name
            && self.is_dir == entry.is_dir
            && self.is_git_repo == entry.is_git_repo
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hub_rows(app: &App) -> usize {
        match &app.overlay {
            Some(app::OverlayData::ModalHub { items, .. }) => items.len(),
            _ => 0,
        }
    }

    /// A new allocation is the sign of a conversion: the cache builds the new
    /// rows while the old ones are still alive, so the two never share one.
    #[test]
    fn lists_convert_again_only_when_their_rows_change() {
        let mut app = App::new_for_test(Vec::new());
        let mut cache = ViewCache::default();
        app.open_git_hub();
        from_app(&app, &mut cache);
        let hub = cache.hub_items.as_ptr();
        from_app(&app, &mut cache);
        if let Some(app::OverlayData::ModalHub { selected, .. }) = &mut app.overlay {
            *selected += 1;
        }
        from_app(&app, &mut cache);
        assert_eq!(cache.hub_items.as_ptr(), hub, "same rows, converted again");

        app.open_help_hub();
        from_app(&app, &mut cache);
        assert_ne!(cache.hub_items.as_ptr(), hub, "new rows, not converted");
        assert_eq!(cache.hub_items.len(), hub_rows(&app));

        let entry = |name: &str| app::DirEntry {
            name: name.into(),
            is_dir: true,
            is_git_repo: false,
        };
        app.overlay = Some(app::OverlayData::DirectoryBrowser {
            current_path: "/src".into(),
            entries: vec![entry("er")],
            selected: 0,
        });
        from_app(&app, &mut cache);
        let dirs = cache.dir_entries.as_ptr();
        from_app(&app, &mut cache);
        assert_eq!(
            cache.dir_entries.as_ptr(),
            dirs,
            "same entries, converted again"
        );
        if let Some(app::OverlayData::DirectoryBrowser { entries, .. }) = &mut app.overlay {
            entries.push(entry("notes"));
        }
        let screen = from_app(&app, &mut cache);
        assert!(matches!(
            screen.chrome.overlay,
            Some(OverlayData::DirectoryBrowser { entries, .. }) if entries.len() == 2
        ));
    }
}
