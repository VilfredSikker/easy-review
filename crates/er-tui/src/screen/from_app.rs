//! The one function that builds the flag-off `Screen` from the App.

use super::view::{self, ConfirmAction, HubKind, InputMode, OverlayData};
use super::watched::WatchedSnapshot;
use super::{
    AppData, Chrome, HistoryView, MemoryBudget, Screen, SymbolRefsView, TabData, TabView, TourView,
};
use er_engine::app::{self, App};

/// The App's lists whose rows are App types, converted to view rows and kept
/// between frames, and the watched snapshot diff. A list is converted again
/// only when its rows change, and the agent log converts only its new tail, so
/// a frame copies none.
#[derive(Default)]
pub struct ViewCache {
    hub_items: Vec<view::HubItem>,
    dir_entries: Vec<view::DirEntry>,
    tour_pillars: Vec<view::TourPillar>,
    refs_in_diff: Vec<view::SymbolRef>,
    refs_external: Vec<view::SymbolRef>,
    log_tab: Option<er_engine::TabKey>,
    agent_log: Vec<view::AgentLogEntry>,
    pub watched: WatchedSnapshot,
}

impl ViewCache {
    fn sync(&mut self, app: &App) {
        let tab = app.tab();
        // The App's row types here are not exported, so the rows are read
        // field by field.
        if let Some(tour) = &tab.tour {
            let same = self.tour_pillars.len() == tour.pillars.len()
                && self.tour_pillars.iter().zip(&tour.pillars).all(|(row, p)| {
                    row.id == p.id
                        && row.title == p.title
                        && row.description == p.description
                        && row.importance == p.importance
                        && row.foundation == p.foundation
                });
            if !same {
                self.tour_pillars = tour
                    .pillars
                    .iter()
                    .map(|p| view::TourPillar {
                        id: p.id.clone(),
                        title: p.title.clone(),
                        description: p.description.clone(),
                        importance: p.importance,
                        foundation: p.foundation,
                    })
                    .collect();
            }
        }
        if let Some(refs) = &tab.symbol_refs {
            for (rows, entries) in [
                (&mut self.refs_in_diff, &refs.in_diff),
                (&mut self.refs_external, &refs.external),
            ] {
                let same = rows.len() == entries.len()
                    && rows.iter().zip(entries).all(|(row, e)| {
                        row.file == e.file
                            && row.line_num == e.line_num
                            && row.line_content == e.line_content
                    });
                if !same {
                    *rows = entries
                        .iter()
                        .map(|e| view::SymbolRef {
                            file: e.file.clone(),
                            line_num: e.line_num,
                            line_content: e.line_content.clone(),
                        })
                        .collect();
                }
            }
        }
        self.sync_agent_log(tab);
        self.watched.sync(app);
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

    /// The App appends to the log and evicts from its front, so the rows
    /// already converted stay valid: drop the evicted ones, convert the new
    /// tail. Anything else (another tab, a cleared log) converts it whole.
    fn sync_agent_log(&mut self, tab: &app::TabState) {
        let log = &tab.agent_log;
        let same_tab = self.log_tab == Some(tab.key());
        let evicted = log
            .front()
            .and_then(|front| self.agent_log.iter().position(|row| same_entry(row, front)));
        let kept = evicted.map_or(0, |n| self.agent_log.len() - n);
        let still_valid = same_tab
            && kept <= log.len()
            && (kept == 0
                || self
                    .agent_log
                    .last()
                    .is_some_and(|row| same_entry(row, &log[kept - 1])));
        if still_valid && (kept > 0 || self.agent_log.is_empty()) {
            self.agent_log.drain(..evicted.unwrap_or(0));
            self.agent_log
                .extend(log.range(kept..).map(agent_log_entry));
        } else {
            self.log_tab = Some(tab.key());
            self.agent_log = log.iter().map(agent_log_entry).collect();
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
    let selected_watched = tab.selected_watched_file();
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
            comment_review: false,
            comment_reply_to: tab.comment_reply_to.as_deref(),
            comment_finding_ref: tab.comment_finding_ref.as_deref(),
            comment_file: &tab.comment_file,
            comment_line_num: tab.comment_line_num,
            comment_hunk: tab.comment_hunk,
            comment_textarea: &tab.comment_textarea,
            filter_input: &tab.filter_input,
            commit_input: &tab.commit_input,
            visible_files: tab.visible_files(),
            selected_diff_file: tab.selected_diff_file(),
            diff_scroll: tab.active_diff_scroll(),
            current_hunk: tab.active_current_hunk(),
            current_line: tab.active_current_line(),
            h_scroll: tab.h_scroll,
            h_scroll_old: tab.h_scroll_old,
            h_scroll_new: tab.h_scroll_new,
            split_focus: match tab.split_focus {
                app::SplitSide::Old => view::SplitSide::Old,
                app::SplitSide::New => view::SplitSide::New,
            },
            context_overrides: &tab.context_overrides,
            mtime_cache: &tab.mtime_cache,
            panel_scroll: tab.panel_scroll,
            review_cursor: tab.review_cursor,
            branch_diff_hash: &tab.branch_diff_hash,
            repo_root: &tab.repo_root,
            visible_watched: tab.visible_watched_files(),
            selected_watched,
            selected_watched_index: tab.selected_watched,
            watched_not_ignored: &tab.watched_not_ignored,
            watched_snapshot: selected_watched
                .and_then(|file| cache.watched.diff(&tab.repo_root, &file.path)),
        },
        data: TabData::App(AppData {
            files: &tab.files,
            reviewed: &tab.reviewed,
            ai: &tab.ai,
            pr_data: tab.pr_data.as_ref(),
            history: tab.history.as_ref().map(|history| HistoryView {
                commits: &history.commits,
                visible_commits: tab.visible_commits(),
                selected_commit: history.selected_commit,
                commit_files: history.commit_files.iter().collect(),
                selected_file: history.selected_file,
                current_hunk: history.current_hunk,
                current_line: history.current_line,
                diff_scroll: history.diff_scroll,
                h_scroll: history.h_scroll,
            }),
            tour: tab.tour.as_ref().map(|tour| TourView {
                pillars: &cache.tour_pillars,
                selected_pillar: tour.selected_pillar,
                files: &tour.files,
                file_is_related: &tour.file_is_related,
                pillar_file_ranges: &tour.pillar_file_ranges,
                selected_file: tour.selected_file,
                current_hunk: tour.current_hunk,
                current_line: tour.current_line,
                diff_scroll: tour.diff_scroll,
                h_scroll: tour.h_scroll,
            }),
            symbol_refs: tab.symbol_refs.as_ref().map(|refs| SymbolRefsView {
                symbol: &refs.symbol,
                in_diff: &cache.refs_in_diff,
                external: &cache.refs_external,
                cursor: refs.cursor,
            }),
            agent_log: &cache.agent_log,
            agent_log_auto_scroll: tab.agent_log_auto_scroll,
            running_command_count: tab
                .command_status
                .values()
                .filter(|status| matches!(status, app::CommandStatus::Running))
                .count(),
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

fn agent_log_entry(entry: &app::AgentLogEntry) -> view::AgentLogEntry {
    view::AgentLogEntry {
        timestamp: entry.timestamp,
        command_name: entry.command_name.clone(),
        source: match entry.source {
            app::AgentLogSource::Stdout => view::AgentLogSource::Stdout,
            app::AgentLogSource::Stderr => view::AgentLogSource::Stderr,
            app::AgentLogSource::Status => view::AgentLogSource::Status,
        },
        text: entry.text.clone(),
    }
}

fn same_entry(row: &view::AgentLogEntry, entry: &app::AgentLogEntry) -> bool {
    row.timestamp == entry.timestamp
        && std::sync::Arc::ptr_eq(&row.command_name, &entry.command_name)
        && row.text == entry.text
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
        assert_eq!(cache.hub_items.as_ptr(), hub, "same rows, converted again");
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

    /// A frame with the same log converts no row; an appended line converts
    /// only itself, and a line the App evicts leaves the rest as they were.
    #[test]
    fn the_agent_log_converts_only_its_new_tail() {
        let mut app = App::new_for_test(Vec::new());
        let name: std::sync::Arc<str> = "review".into();
        let now = std::time::Instant::now();
        let push = |app: &mut App, text: &str| {
            app.tab_mut().agent_log.push_back(app::AgentLogEntry {
                timestamp: now,
                command_name: name.clone(),
                source: app::AgentLogSource::Stdout,
                text: text.into(),
            });
        };
        push(&mut app, "one");
        push(&mut app, "two");
        let mut cache = ViewCache::default();
        from_app(&app, &mut cache);
        let texts = |cache: &ViewCache| -> Vec<*const u8> {
            cache
                .agent_log
                .iter()
                .map(|row| row.text.as_ptr())
                .collect()
        };
        let first = texts(&cache);
        from_app(&app, &mut cache);
        assert_eq!(texts(&cache), first, "same log, converted again");

        push(&mut app, "three");
        from_app(&app, &mut cache);
        assert_eq!(texts(&cache)[..2], first[..], "old rows converted again");
        assert_eq!(cache.agent_log[2].text, "three");

        app.tab_mut().agent_log.pop_front();
        from_app(&app, &mut cache);
        assert_eq!(texts(&cache)[..1], first[1..], "kept rows converted again");
        let rows: Vec<_> = cache
            .agent_log
            .iter()
            .map(|row| row.text.as_str())
            .collect();
        assert_eq!(rows, ["two", "three"]);
    }
}
