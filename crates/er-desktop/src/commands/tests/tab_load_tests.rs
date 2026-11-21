//! The flag path's loads that no route asks for: the tab the registry makes
//! active loads on that change, and the warmer loads the active repo's other
//! tabs once each, never the active one.

use std::sync::Arc;
use std::time::{Duration, Instant};

use er_engine::tabs_record::TabDescriptor;
use er_engine::TabKey;
use er_service::AppCommand;

use super::diff_resource_tests::{fake_ports, listed, opened, state_over, with_storage};
use crate::commands::AppState;
use crate::tabs;

const WAIT: Duration = Duration::from_secs(10);

fn loaded(state: &AppState, key: TabKey, tab: &TabDescriptor) -> bool {
    state.service.content(key, tab).is_some()
}

fn wait_for_load(state: &AppState, key: TabKey, tab: &TabDescriptor) -> bool {
    let deadline = Instant::now() + WAIT;
    while !loaded(state, key, tab) {
        if Instant::now() > deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    true
}

fn version(state: &AppState, key: TabKey, tab: &TabDescriptor) -> Option<u64> {
    let content = state.service.content(key, tab)?;
    Some(er_service::diff_version(&content))
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
            Arc::clone(&state.app),
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
/// leaves the active tab and another repo's tab alone.
#[test]
fn the_warmer_loads_each_unloaded_tab_of_the_active_repo_once() {
    with_storage(|dir| {
        let state = state_over(dir, fake_ports());
        let (elsewhere, elsewhere_tab) = listed(&state, dir, "elsewhere");
        let (stub, stub_tab) = listed(&state, dir, "repo");
        let (active, active_tab) = opened(&state, dir, "repo");
        let before = version(&state, active, &active_tab);
        assert!(before.is_some(), "the active tab did not load");

        assert_eq!(tabs::warm(&state.service, Duration::ZERO), 1);
        assert!(loaded(&state, stub, &stub_tab), "the warmer skipped a stub");
        assert!(
            !loaded(&state, elsewhere, &elsewhere_tab),
            "the warmer loaded another repo's tab"
        );
        assert_eq!(
            version(&state, active, &active_tab),
            before,
            "the warmer loaded the active tab again"
        );
        assert_eq!(tabs::warm(&state.service, Duration::ZERO), 0);
    });
}
