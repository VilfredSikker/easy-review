//! The flag-on `session` and `panels` resources, the AI picks through
//! `er_send`, the update check, the panel mirror and the App's
//! notifications as notices.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use er_engine::config::{AiModelConfig, AiProviderConfig};
use er_protocol::{
    AppUpdate, Command, NoticeLevel, Panel, Resource, ResourceChanged, ResourceKey,
    ResourceVersion, Session, Settings,
};

use super::{review_flow_test_state, while_the_app_is_locked, ReviewFlowTestDir};
use crate::ai_selection_service::SessionPick;
use crate::commands::AppState;
use crate::resources::send::{apply, Applied, Effects};
use crate::resources::{self, tick};
use crate::service::ServiceState;
use crate::snapshot::{build_snapshot_with_delta, SnapshotSources, WatchStatusSnapshot};

const WAIT: Duration = Duration::from_secs(20);

fn with_storage(test: impl FnOnce(&ReviewFlowTestDir)) {
    let dir = ReviewFlowTestDir::new();
    let storage = dir.path().join("storage");
    er_engine::storage::with_process_test_storage_root(storage, || test(&dir));
}

fn model(id: &str) -> AiModelConfig {
    AiModelConfig {
        id: id.into(),
        label: Some(format!("{id} label")),
        // The provider prints its arguments, so the reply names the model.
        args: vec![format!("mark-{id}")],
        effort_levels: vec!["low".into(), "high".into()],
        ..Default::default()
    }
}

/// A provider that answers with its own arguments, two models it tells
/// apart, and a listing command; saved as the managed config pair, which the
/// settings actor owns and the ask runs read. The App holds the same config,
/// as it would have loaded it, and follows its default selection.
fn echo_state(dir: &ReviewFlowTestDir) -> AppState {
    let mut state = review_flow_test_state(dir);
    let path = er_engine::config::managed_config_path();
    {
        let mut app = state.app.lock().unwrap_or_else(|e| e.into_inner());
        let provider = AiProviderConfig {
            label: Some("Echo".into()),
            command: "/bin/sh".into(),
            args: vec![
                "-c".into(),
                "printf '%s\\n' \"$@\"".into(),
                "echo-agent".into(),
                "{prompt}".into(),
            ],
            models: vec![model("first"), model("second")],
            models_command: vec![
                "/bin/sh".into(),
                "-c".into(),
                "printf 'listed-a\\nlisted-b\\n'".into(),
            ],
            ..Default::default()
        };
        let config = &mut app.config;
        config.ai_hub.providers.insert("echo".into(), provider);
        config.ai_hub.default_provider = Some("echo".into());
        config.ai_hub.default_model = Some("first".into());
        config.ai_hub.default_effort = Some("high".into());
        config.display.theme = "paper".into();
        config.display.split_diff = true;
        config.display.tab_width = 2;
        config.features.view_tour = false;
        config.features.model_discovery = true;
        assert!(er_engine::config::save_config_pair_to(&path, config).is_ok());
        app.sync_ai_selection_from_defaults();
    }
    state.service = Arc::new(ServiceState::with_settings_path(
        er_engine::ports::Ports::for_test(),
        path,
    ));
    let _ = state.service.settings().view();
    state
}

fn effects<'a>(
    notify: &'a dyn Fn(NoticeLevel, String),
    follow: &'a dyn Fn(&er_engine::config::AiSelection),
) -> Effects<'a> {
    Effects {
        notify,
        follow_selection: follow,
        check_update: || panic!("no update check in this test"),
        webviews: &crate::browser_webview::NoWebviews,
        pick_folder: &|| None,
    }
}

fn send(state: &AppState, command: Command) -> Applied {
    apply(&state.desktop, command, &effects(&|_, _| {}, &|_| {}))
        .unwrap_or_else(|e| panic!("er_send failed: {e:?}"))
}

fn pick_second() -> Command {
    Command::SetAiSelection {
        provider_id: "echo".into(),
        model_id: Some("second".into()),
        effort: None,
    }
}

fn fetch_session(state: &AppState) -> (ResourceVersion, Session) {
    let fetched = resources::fetch(&state.desktop, ResourceKey::Session)
        .unwrap_or_else(|e| panic!("session fetch: {e:?}"));
    match fetched.value {
        Resource::Session(session) => (fetched.version, *session),
        other => panic!("not a session resource: {other:?}"),
    }
}

fn fetch_settings(state: &AppState) -> Settings {
    match resources::fetch(&state.desktop, ResourceKey::Settings).map(|f| f.value) {
        Ok(Resource::Settings(settings)) => *settings,
        other => panic!("not a settings resource: {other:?}"),
    }
}

fn json(value: &impl serde::Serialize) -> serde_json::Value {
    serde_json::to_value(value).unwrap_or_default()
}

fn wait_until(what: &str, done: impl Fn() -> bool) {
    let deadline = Instant::now() + WAIT;
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// With the flag on, a pick through `er_send` saves, and the next ask, still
/// on the App until the ask runs read the settings view, runs the new model:
/// the mapping copies the saved pick into the App's session pick.
#[test]
fn a_pick_through_er_send_runs_the_next_ask_with_the_new_model() {
    with_storage(|dir| {
        let state = echo_state(dir);
        let pick = SessionPick::new(Arc::clone(&state.app), Arc::new(AtomicU64::new(0)));
        let follow = |selection: &er_engine::config::AiSelection| pick.follow(selection);
        let applied = apply(&state.desktop, pick_second(), &effects(&|_, _| {}, &follow));
        assert!(applied.is_ok(), "{applied:?}");

        let question = {
            let mut app = state.app.lock().unwrap_or_else(|e| e.into_inner());
            let target = er_engine::app::CommentTarget {
                file: "src/lib.rs".into(),
                hunk_idx: 0,
                line_num: Some(1),
                line_num_end: None,
            };
            let asked = app.submit_comment_text(
                target,
                "Why this change?".into(),
                er_engine::ai::CommentType::Question,
                None,
                None,
            );
            assert!(asked.is_ok(), "{asked:?}");
            app.tab()
                .ai
                .questions
                .as_ref()
                .and_then(|qs| qs.questions.first().map(|q| q.id.clone()))
                .unwrap_or_else(|| panic!("no question was written"))
        };
        let asked = crate::commands::ask_ai_impl(question.clone(), "Explain".into(), None, &state);
        assert!(asked.is_ok(), "{:?}", asked.map(drop));

        let reply = || {
            let app = state.app.lock().unwrap_or_else(|e| e.into_inner());
            app.tab().ai.questions.as_ref().and_then(|qs| {
                qs.questions
                    .iter()
                    .find(|q| q.in_reply_to.as_deref() == Some(question.as_str()))
                    .map(|q| q.text.clone())
            })
        };
        wait_until("the AI reply", || reply().is_some());
        let reply = reply().unwrap_or_default();
        assert!(reply.contains("mark-second"), "the ask ran: {reply}");
        assert!(!reply.contains("mark-first"), "the ask ran: {reply}");
    });
}

/// With the App locked, the picker's providers and the session labels answer
/// the pick just saved: with the flag on both read the settings actor's
/// default selection, never the App's session pick, which here still names
/// the first model.
#[test]
fn the_picker_answers_the_saved_pick_while_the_app_is_locked() {
    with_storage(|dir| {
        let state = echo_state(dir);
        let applied = send(&state, pick_second());
        let (providers, (_, session)) =
            while_the_app_is_locked(&state, "the picker's reads", |state| {
                (crate::commands::ai_providers(state), fetch_session(state))
            });
        let providers = providers.unwrap_or_else(|e| panic!("{e}"));
        let echo = providers.iter().find(|p| p.id == "echo");
        let selected: Vec<_> = echo
            .map(|p| {
                p.models
                    .iter()
                    .filter(|m| m.is_selected)
                    .map(|m| m.id.as_str())
                    .collect()
            })
            .unwrap_or_default();
        assert!(echo.is_some_and(|p| p.is_selected), "{providers:?}");
        assert_eq!(selected, ["second"]);
        assert_eq!(
            session.active_ai_model_label.as_deref(),
            Some("second label")
        );
        assert!(
            session.active_ai_label.starts_with("Echo / second label"),
            "{}",
            session.active_ai_label
        );

        let (version, _) = fetch_session(&state);
        assert!(
            applied.ack.changed.contains(&ResourceChanged {
                key: ResourceKey::Session,
                version,
            }),
            "the pick's Ack names session: {:?}",
            applied.ack.changed
        );
    });
}

/// The wire's pick carries no persist flag, and every pick is saved by the
/// settings actor: it is in `agents.toml` once the `Ack` arrives.
#[test]
fn a_pick_through_er_send_is_saved() {
    with_storage(|dir| {
        let state = echo_state(dir);
        let _ = send(&state, pick_second());
        let _ = send(
            &state,
            Command::SetAiEffort {
                effort: Some("low".into()),
            },
        );
        let path = er_engine::config::managed_config_path().with_file_name("agents.toml");
        let agents = std::fs::read_to_string(path).unwrap_or_default();
        assert!(agents.contains("default_model = \"second\""), "{agents}");
        assert!(agents.contains("default_effort = \"low\""), "{agents}");
    });
}

/// Each chrome field the page reads from `settings`, `session` and
/// `panels` with the flag on equals what the flag-off snapshot carries for
/// the same config and desktop state.
#[test]
fn the_chrome_resources_match_the_flag_off_snapshot() {
    with_storage(|dir| {
        let state = echo_state(dir);
        state.desktop.session.set_watch_status(
            &state.watch_status,
            WatchStatusSnapshot {
                active: true,
                branch: Some("feature".into()),
                root_path: Some("/work/feature".into()),
            },
        );
        let _ = send(&state, Command::TogglePanel { panel: Panel::Tree });
        let snapshot = {
            let app = state.app.lock().unwrap_or_else(|e| e.into_inner());
            build_snapshot_with_delta(
                &app,
                &SnapshotSources {
                    watch_status: Some(&state.watch_status),
                    view: Some(&state.view),
                    ..SnapshotSources::default()
                },
            )
        };
        let settings = fetch_settings(&state);
        let (_, session) = fetch_session(&state);
        let panels = match resources::fetch(&state.desktop, ResourceKey::Panels).map(|f| f.value) {
            Ok(Resource::Panels(panels)) => panels,
            other => panic!("not a panels resource: {other:?}"),
        };
        let snap = json(&snapshot);

        assert_eq!(snapshot.theme, "paper");
        assert_eq!(settings.theme, snapshot.theme);
        assert_eq!(json(&settings.features), snap["features"]);
        assert_eq!(json(&settings.display), snap["display"]);
        assert_eq!(settings.arena_enabled, snapshot.arena_enabled);
        assert_eq!(session.config_load_error, snapshot.config_load_error);
        assert_eq!(session.active_ai_label, snapshot.active_ai_label);
        assert_eq!(
            session.active_ai_provider_label,
            snapshot.active_ai_provider_label
        );
        assert_eq!(
            session.active_ai_model_label,
            snapshot.active_ai_model_label
        );
        assert_eq!(session.active_ai_effort, snapshot.active_ai_effort);
        assert_eq!(session.app_version, snapshot.app_version);
        assert_eq!(session.watch_status.active, snapshot.watch_active);
        assert_eq!(json(&session.watch_status), snap["watch_status"]);
        assert_eq!(json(&panels), snap["panels"]);
        assert!(!panels.tree, "the toggle reached the mirror");
    });
}

/// A watcher write that changes the status moves `session`, so the page's
/// watch badge follows the push; one that changes nothing does not.
#[test]
fn a_watch_status_change_moves_session() {
    with_storage(|dir| {
        let state = echo_state(dir);
        let (before, _) = fetch_session(&state);
        let watching = WatchStatusSnapshot {
            active: true,
            branch: Some("feature".into()),
            root_path: Some("/work/feature".into()),
        };
        state
            .desktop
            .session
            .set_watch_status(&state.watch_status, watching.clone());
        let (moved, session) = fetch_session(&state);
        assert!(moved > before, "{moved:?} after {before:?}");
        assert_eq!(session.watch_status.branch.as_deref(), Some("feature"));
        state
            .desktop
            .session
            .set_watch_status(&state.watch_status, watching);
        assert_eq!(fetch_session(&state).0, moved, "an unchanged status moved");
    });
}

/// The mirror of a toggle moves `panels`, and its `Ack` names the version
/// that holds it.
#[test]
fn a_panel_toggle_moves_panels() {
    with_storage(|dir| {
        let state = echo_state(dir);
        let before = resources::version(&state.desktop, ResourceKey::Panels)
            .unwrap_or_else(|e| panic!("{e:?}"));
        let ack = send(&state, Command::TogglePanel { panel: Panel::Left }).ack;
        let fetched = resources::fetch(&state.desktop, ResourceKey::Panels)
            .unwrap_or_else(|e| panic!("{e:?}"));
        assert!(
            fetched.version > before,
            "{:?} after {before:?}",
            fetched.version
        );
        assert_eq!(
            ack.changed,
            vec![ResourceChanged {
                key: ResourceKey::Panels,
                version: fetched.version,
            }]
        );
        assert!(matches!(fetched.value, Resource::Panels(p) if !p.left));
    });
}

/// The update check answers at once and its result lands in `session` at a
/// higher version.
#[test]
fn an_update_check_lands_in_session() {
    with_storage(|dir| {
        let state = echo_state(dir);
        let (before, _) = fetch_session(&state);
        let applied = apply(
            &state.desktop,
            Command::CheckAppUpdate,
            &Effects {
                notify: &|_, _| {},
                follow_selection: &|_| {},
                check_update: || AppUpdate {
                    current: "1.0.0".into(),
                    latest: Some("9.9.9".into()),
                    update_available: true,
                    release_url: Some("https://example.invalid/r".into()),
                },
                webviews: &crate::browser_webview::NoWebviews,
                pick_folder: &|| None,
            },
        );
        assert!(applied.is_ok(), "{applied:?}");
        wait_until("the update check", || {
            fetch_session(&state).1.update.is_some()
        });
        let (after, session) = fetch_session(&state);
        assert!(after > before, "{after:?} after {before:?}");
        assert_eq!(
            session.update.and_then(|u| u.latest).as_deref(),
            Some("9.9.9")
        );
    });
}

/// A model listing that worked says how many models the provider has, as
/// the flag-off page did; the `Ack` carries no count.
#[test]
fn a_model_refresh_names_the_count() {
    with_storage(|dir| {
        let state = echo_state(dir);
        let notices = Mutex::new(Vec::new());
        let notify = |level, message| {
            if let Ok(mut notices) = notices.lock() {
                notices.push((level, message));
            }
        };
        let applied = apply(
            &state.desktop,
            Command::RefreshAiModels {
                provider_id: Some("echo".into()),
                force: true,
            },
            &effects(&notify, &|_| {}),
        );
        assert!(applied.is_ok(), "{applied:?}");
        let notices = notices.into_inner().unwrap_or_default();
        let count = fetch_settings(&state)
            .providers
            .iter()
            .find(|p| p.id == "echo")
            .map_or(0, |p| p.models.len());
        assert!(count > 2, "the listing added models: {count}");
        assert_eq!(
            notices,
            vec![(NoticeLevel::Success, format!("Echo: {count} models"))]
        );
    });
}

/// The App keeps its last notification set, and the tick sees it on every
/// pass; it goes out as a notice once, and a new one goes out again.
#[test]
fn each_app_notification_is_one_notice() {
    let mut app = er_engine::app::App::new_for_test(Vec::new());
    let sent = AtomicU64::new(0);
    app.notify("Review started");
    let first = tick::app_notice(app.notification.as_ref(), &sent);
    assert_eq!(
        first.map(|n| (n.level, n.message)),
        Some((NoticeLevel::Success, "Review started".to_string()))
    );
    assert_eq!(tick::app_notice(app.notification.as_ref(), &sent), None);
    app.notify("Review failed");
    let next = tick::app_notice(app.notification.as_ref(), &sent);
    assert_eq!(next.map(|n| n.level), Some(NoticeLevel::Error));
}

/// The tick itself sends the App's notification: one tick consumes it, so
/// the dedupe has nothing left to send, and the next tick sends nothing.
#[test]
fn the_tick_sends_the_app_notification_once() {
    with_storage(|dir| {
        let state = review_flow_test_state(dir);
        let set = {
            let mut app = state.app.lock().unwrap_or_else(|e| e.into_inner());
            app.notify("Review started");
            app.notification.as_ref().map_or(0, |n| n.seq)
        };
        assert!(set > 0, "the App numbered its notification");
        let sent = || state.desktop.session.app_notice_seq.load(Ordering::Acquire);

        tick::run(&state.desktop, &state.app);

        let current = {
            let app = state.app.lock().unwrap_or_else(|e| e.into_inner());
            let unsent = tick::app_notice(app.notification.as_ref(), &AtomicU64::new(sent()));
            assert_eq!(unsent, None, "the tick left the notification unsent");
            app.notification.as_ref().map_or(0, |n| n.seq)
        };
        assert!(current >= set, "{current} before {set}");
        assert_eq!(sent(), current);

        tick::run(&state.desktop, &state.app);
        assert_eq!(sent(), current, "a second tick sent again");
    });
}
