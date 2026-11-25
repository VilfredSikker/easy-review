//! A parity frame as it will be frozen: `golden.rs`'s text, with the run's
//! own text masked at the same width, so the same test draws the same file
//! on every run. The masks cover what a run picks for itself: temp dir
//! names, arena run ids, clock times and relative ages. Text keeps its
//! width, so the style digest is untouched.
//!
//! `ER_PARITY_DUMP=<dir>` writes each frame the parity tests compare to
//! `<dir>/<module>/<test>.<order>.<step>.txt`, so two runs can be diffed.

use std::cell::Cell;

use ratatui::buffer::Buffer;

thread_local! {
    static ORDER: Cell<usize> = const { Cell::new(0) };
}

/// One mask: how many chars from `at` it covers, or 0.
type Rule = fn(&[char], usize) -> usize;

const RULES: [Rule; 4] = [temp_dir, arena_id, clock, age];

/// `.tmpAb12Cd`, as `tempfile` names a dir; a clipped name masks what shows.
/// Storage keys a repo as its dir name and a hash of its path,
/// `.tmpAb12Cd-a1da9c47`, so the hash goes with the name.
fn temp_dir(chars: &[char], at: usize) -> usize {
    if !starts(chars, at, ".tmp") {
        return 0;
    }
    let name = chars[at + 4..]
        .iter()
        .take(6)
        .take_while(|c| c.is_ascii_alphanumeric())
        .count();
    let key = at + 4 + name;
    let hash = chars
        .get(key + 1..key + 9)
        .is_some_and(|hash| chars[key] == '-' && hash.iter().all(char::is_ascii_hexdigit));
    match (name, hash) {
        (0, _) => 0,
        (6, true) => 4 + 6 + 9,
        _ => 4 + name,
    }
}

/// `arena-1795569923677-1`: the run's start in ms and a process counter.
fn arena_id(chars: &[char], at: usize) -> usize {
    if !starts(chars, at, "arena-") {
        return 0;
    }
    let id = chars[at + 6..]
        .iter()
        .take_while(|c| c.is_ascii_digit() || **c == '-')
        .count();
    if id == 0 {
        0
    } else {
        6 + id
    }
}

/// `01:27` or `01:27:07`.
fn clock(chars: &[char], at: usize) -> usize {
    let pair = |from: usize| {
        chars
            .get(from..from + 2)
            .is_some_and(|two| two.iter().all(char::is_ascii_digit))
    };
    let colon = |from: usize| chars.get(from) == Some(&':');
    if at > 0 && chars[at - 1].is_ascii_digit() {
        return 0;
    }
    if !(pair(at) && colon(at + 2) && pair(at + 3)) {
        return 0;
    }
    if colon(at + 5) && pair(at + 6) {
        8
    } else {
        5
    }
}

/// The number in `7 years ago`, which moves as the fixtures' dates age.
fn age(chars: &[char], at: usize) -> usize {
    if at > 0 && chars[at - 1].is_ascii_digit() {
        return 0;
    }
    let digits = chars[at..]
        .iter()
        .take_while(|c| c.is_ascii_digit())
        .count();
    if digits == 0 {
        return 0;
    }
    let rest: String = chars[at + digits..].iter().take(16).collect();
    let units = ["second", "minute", "hour", "day", "week", "month", "year"];
    let unit = units.iter().any(|unit| {
        rest.strip_prefix(' ')
            .and_then(|r| r.strip_prefix(unit))
            .is_some_and(|r| r.starts_with(" ago") || r.starts_with("s ago"))
    });
    if unit {
        digits
    } else {
        0
    }
}

fn starts(chars: &[char], at: usize, text: &str) -> bool {
    let want: Vec<char> = text.chars().collect();
    chars.get(at..at + want.len()) == Some(&want[..])
}

/// One row with every rule's matches masked.
pub(super) fn mask(row: &str) -> String {
    let chars: Vec<char> = row.chars().collect();
    let mut out = String::with_capacity(row.len());
    let mut at = 0;
    while at < chars.len() {
        let covered = RULES.iter().map(|rule| rule(&chars, at)).max().unwrap_or(0);
        if covered == 0 {
            out.push(chars[at]);
            at += 1;
        } else {
            out.extend(std::iter::repeat_n('#', covered));
            at += covered;
        }
    }
    out
}

/// `golden.rs`'s text for `frame`, masked.
pub(super) fn stable_text(frame: &Buffer) -> String {
    crate::golden::golden_text(frame)
        .lines()
        .map(|row| mask(row) + "\n")
        .collect()
}

/// Writes the frame under `ER_PARITY_DUMP`, named by the test thread, the
/// check's order within the test and its step.
pub(super) fn dump(step: &str, frame: &Buffer) {
    let order = ORDER.with(|order| {
        order.set(order.get() + 1);
        order.get()
    });
    let Some(dir) = std::env::var_os("ER_PARITY_DUMP") else {
        return;
    };
    let thread = std::thread::current();
    let test = thread.name().unwrap_or("unnamed");
    let mut parts = test.rsplit("::");
    let name = parts.next().unwrap_or(test);
    let module = parts.next().unwrap_or("tests");
    let step: String = step
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    let dir = std::path::Path::new(&dir).join(module);
    std::fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("{e}"));
    let file = dir.join(format!("{name}.{order:02}.{}.txt", step.trim_matches('_')));
    std::fs::write(&file, stable_text(frame)).unwrap_or_else(|e| panic!("{e}"));
}

#[test]
fn masks_keep_the_width_and_cover_only_the_runs_own_text() {
    let cases = [
        (
            "┌ /tmp/.tmpMLHFmN (Enter=open",
            "┌ /tmp/########## (Enter=open",
        ),
        (
            "repos/.tmpAb12Cd-a1da9c47/branches",
            "repos/###################/branches",
        ),
        (
            "· arena-1795569923677-1 · 1 of",
            "· ##################### · 1 of",
        ),
        ("│  You  01:27:07", "│  You  ########"),
        ("· 2776a5f · 7 years ago  +2", "· 2776a5f · # years ago  +2"),
        ("+2 -40 · 12 files · line 3", "+2 -40 · 12 files · line 3"),
    ];
    for (row, masked) in cases {
        assert_eq!(mask(row), masked);
        assert_eq!(mask(row).chars().count(), row.chars().count());
    }
}
