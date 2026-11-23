//! The loads that no route asks for: the tab the registry makes
//! active loads on that change, and the warmer loads the active repo's other
//! tabs once each, never the active one.

use std::sync::Arc;
use std::time::{Duration, Instant};

use er_engine::tabs_record::TabDescriptor;
use er_engine::TabKey;
use er_service::AppCommand;

use super::diff_resource_tests::{fake_ports, listed, state_over, with_storage};
use crate::commands::{content_routes, DesktopState};
use crate::tabs;

const WAIT: Duration = Duration::from_secs(10);

fn loaded(state: &DesktopState, key: TabKey, tab: &TabDescriptor) -> bool {
    state.service.content(key, tab).is_some()
}

fn wait_for_load(state: &DesktopState, key: TabKey, tab: &TabDescriptor) -> bool {
    let deadline = Instant::now() + WAIT;
    while !loaded(state, key, tab) {
        if Instant::now() > deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    true
}

/// Nothing fetches `diff(tab)` here, so only the follower can load: the
/// restored active tab loads at once, and a stub loads when it is selected.
#[test]
fn a_followed_stub_gets_its_first_diff_when_it_becomes_active() {
    with_storage(|dir| {
        let state = state_over(dir, fake_ports());
        let (stub, stub_tab) = listed(&state, dir, "stub");
        let (shown, shown_tab) = listed(&state, dir, "shown");
        tabs::follow_service(
            &state.service,
            super::app_stub(),
            Arc::clone(&state.view),
            fake_ports(),
        );
        assert!(
            wait_for_load(&state, shown, &shown_tab),
            "the active tab never loaded"
        );
        assert!(!loaded(&state, stub, &stub_tab), "a stub loaded unasked");

        let selected = state.service.send_app(AppCommand::SelectTab { key: stub });
        assert!(selected.is_ok(), "{selected:?}");
        assert!(
            wait_for_load(&state, stub, &stub_tab),
            "the selected stub never got its first diff"
        );
    });
}

/// The warmer loads the other tabs on the active tab's repo, once each, and
/// leaves another repo's tab alone. It never loads the active tab, which its
/// activation loads: here nothing follows the registry, so the active tab
/// stays unloaded.
#[test]
fn the_warmer_loads_each_unloaded_tab_of_the_active_repo_once() {
    with_storage(|dir| {
        let state = state_over(dir, fake_ports());
        let (elsewhere, elsewhere_tab) = listed(&state, dir, "elsewhere");
        let (stub, stub_tab) = listed(&state, dir, "repo");
        let (active, active_tab) = listed(&state, dir, "repo");

        assert_eq!(tabs::warm(&state.service, Duration::ZERO), 1);
        assert!(loaded(&state, stub, &stub_tab), "the warmer skipped a stub");
        assert!(
            !loaded(&state, elsewhere, &elsewhere_tab),
            "the warmer loaded another repo's tab"
        );
        assert!(
            !loaded(&state, active, &active_tab),
            "the warmer loaded the active tab"
        );
        assert_eq!(tabs::warm(&state.service, Duration::ZERO), 0);
    });
}

/// An open's own load races the follower's activation load of the same new
/// tab. The actor refuses whichever first load lands second, and the open
/// still succeeds, since the tab loaded.
#[test]
fn an_open_succeeds_when_the_follower_loads_its_tab_first() {
    with_storage(|dir| {
        let state = state_over(dir, fake_ports());
        tabs::follow_service(
            &state.service,
            super::app_stub(),
            Arc::clone(&state.view),
            fake_ports(),
        );
        let mut failed = Vec::new();
        for n in 0..40 {
            let root = dir.path().join(format!("open{n}"));
            std::fs::create_dir_all(&root).unwrap_or_else(|e| panic!("repo dir: {e}"));
            let tab = content_routes::working_tab(root.to_string_lossy().into_owned());
            if let Err(error) = content_routes::open(&state, tab.clone(), false) {
                failed.push(error);
                continue;
            }
            let key = state.service.app_registry().tabs_view().active;
            let key = key.unwrap_or_else(|| panic!("no active tab"));
            assert!(
                loaded(&state, key, &tab),
                "open {n} answered before its tab loaded"
            );
        }
        assert!(
            failed.is_empty(),
            "{} of 40 opens failed, first: {:?}",
            failed.len(),
            failed.first()
        );
    });
}

/// A route's first load can overlap another first load of the same key, as
/// the follower's: whichever lands second is refused, and the route still
/// answers, since the key holds content. Nothing follows the registry here,
/// so each overlap is posted by hand.
#[test]
fn a_route_load_refused_behind_another_first_load_answers_from_its_content() {
    with_storage(|dir| {
        let state = state_over(dir, fake_ports());
        let mut failed = Vec::new();
        for n in 0..40 {
            let (key, tab) = listed(&state, dir, &format!("race{n}"));
            assert!(state.service.request_load(key, &tab), "load {n} not posted");
            if let Err(error) = content_routes::refresh_tab(&state, key, false) {
                failed.push(error);
                continue;
            }
            assert!(
                loaded(&state, key, &tab),
                "load {n} answered before its tab loaded"
            );
        }
        assert!(
            failed.is_empty(),
            "{} of 40 loads failed, first: {:?}",
            failed.len(),
            failed.first()
        );
    });
}

/// A route's load claims its key, so a follower that reaches the key while
/// the route's load is on its way posts no second one.
#[test]
fn a_route_load_claims_its_key() {
    with_storage(|dir| {
        let state = state_over(dir, fake_ports());
        let (key, tab) = listed(&state, dir, "claimed");
        let refreshed = content_routes::refresh_tab(&state, key, false);
        assert!(refreshed.is_ok(), "{refreshed:?}");
        assert!(
            !state.service.request_load(key, &tab),
            "a second load was posted for a key the route claimed"
        );
    });
}
