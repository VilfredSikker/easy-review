//! The shared navigation fixture: every case in
//! `fixtures/navigation_cases.json` holds for the engine's functions, as it
//! does for desktop-ui's port (`desktop-ui/src/lib/navigation.test.ts`).

use er_engine::navigation::{
    next_file, next_hunk, next_unreviewed_file, prev_file, prev_hunk, FileMove,
};
use serde_json::Value;

fn cases() -> Value {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/navigation_cases.json"
    );
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn list(value: &Value, case: &Value) -> Vec<usize> {
    let items = value
        .as_array()
        .unwrap_or_else(|| panic!("not a list in {case}"));
    items.iter().map(|item| index(item, case)).collect()
}

fn index(value: &Value, case: &Value) -> usize {
    let n = value
        .as_u64()
        .unwrap_or_else(|| panic!("not an index in {case}"));
    usize::try_from(n).unwrap_or_else(|e| panic!("{e} in {case}"))
}

fn optional(value: &Value, case: &Value) -> Option<usize> {
    (!value.is_null()).then(|| index(value, case))
}

/// A move as the fixture writes it.
fn encoded(movement: FileMove) -> Value {
    match movement {
        FileMove::Stay => serde_json::json!(["stay"]),
        FileMove::LeaveWatched => serde_json::json!(["leave_watched"]),
        FileMove::File(idx) => serde_json::json!(["file", idx]),
        FileMove::Watched(idx) => serde_json::json!(["watched", idx]),
        FileMove::Snap(idx) => serde_json::json!(["snap", idx]),
    }
}

#[test]
fn every_file_move_in_the_fixture_holds() {
    let cases = cases();
    for case in cases["file_moves"].as_array().into_iter().flatten() {
        let name = &case["name"];
        let file = index(&case["file"], case);
        let watched_sel = optional(&case["watched_sel"], case);
        let visible = list(&case["visible"], case);
        let watched = list(&case["watched"], case);
        let next = next_file(file, watched_sel, &visible, &watched);
        let prev = prev_file(file, watched_sel, &visible, &watched);
        assert_eq!(encoded(next), case["next"], "next_file: {name}");
        assert_eq!(encoded(prev), case["prev"], "prev_file: {name}");
    }
}

#[test]
fn every_hunk_move_in_the_fixture_holds() {
    let cases = cases();
    for case in cases["hunk_moves"].as_array().into_iter().flatten() {
        let current = index(&case["current"], case);
        let count = index(&case["count"], case);
        assert_eq!(
            next_hunk(current, count),
            optional(&case["next"], case),
            "next_hunk: {case}"
        );
        assert_eq!(
            prev_hunk(current),
            optional(&case["prev"], case),
            "prev_hunk: {case}"
        );
    }
}

#[test]
fn every_unreviewed_jump_in_the_fixture_holds() {
    let cases = cases();
    for case in cases["unreviewed"].as_array().into_iter().flatten() {
        let file = index(&case["file"], case);
        let pairs = case["visible"].as_array().into_iter().flatten();
        let visible: Vec<(usize, bool)> = pairs
            .map(|pair| (index(&pair[0], case), pair[1].as_bool() == Some(true)))
            .collect();
        let expected = optional(&case["next"], case);
        assert_eq!(
            next_unreviewed_file(file, &visible),
            expected,
            "{}",
            case["name"]
        );
    }
}
