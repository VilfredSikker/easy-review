//! A parity frame as it will be frozen: `golden.rs`'s text, with the run's
//! own text masked at the same width, so the same test draws the same file
//! on every run. The masks cover what a run picks for itself: temp dir
//! names, arena run ids, clock times and relative ages. Text keeps its
//! width, so the style digest is untouched.
//!
//! `ER_PARITY_DUMP=<dir>` writes each frame the parity tests compare to
//! `<dir>/<module>/<test>.<order>.<step>.txt`, so two runs can be diffed.

use std::cell::{Cell, RefCell};
use std::fmt::Write as _;
use std::path::PathBuf;

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

/// The test modules whose checks are frozen. Each later P76.1 PR adds its
/// areas; the others still only compare the two legs live.
const FROZEN: [&str; 6] = [
    "tests",
    "settings_tests",
    "config_hub_tests",
    "github_tests",
    "worktree_tests",
    "watched_tests",
];

/// The running test's golden: what the file holds, and in update mode what
/// this run has recorded so far.
struct Golden {
    path: PathBuf,
    module: String,
    held: Vec<Entry>,
    recorded: Vec<Entry>,
}

#[derive(Clone)]
struct Entry {
    head: String,
    lines: Vec<String>,
}

thread_local! {
    static GOLDEN: RefCell<Option<Golden>> = const { RefCell::new(None) };
}

/// `golden.rs`'s text for `frame`, masked.
pub(super) fn stable_text(frame: &Buffer) -> String {
    crate::golden::golden_text(frame)
        .lines()
        .map(|row| mask(row) + "\n")
        .collect()
}

/// Both frames, masked, equal the golden's frame for this check.
pub(super) fn assert_frame(step: &str, flag_on: &Buffer, app: &Buffer) {
    let order = next_order();
    let app = stable_text(app);
    dump(order, step, &app);
    check(order, "frame", step, &app, &stable_text(flag_on));
}

/// Both results, as masked pretty JSON, equal the golden's JSON for this
/// check. The caller passes them as its live comparison takes them.
pub(super) fn assert_json<T: serde::Serialize>(step: &str, flag_on: &T, app: &T) {
    let json = |value: &T| {
        let text = serde_json::to_string_pretty(value).unwrap_or_else(|e| panic!("{e}"));
        text.lines()
            .map(|line| mask(line) + "\n")
            .collect::<String>()
    };
    check(next_order(), "json", step, &json(app), &json(flag_on));
}

fn next_order() -> usize {
    ORDER.with(|order| {
        order.set(order.get() + 1);
        order.get()
    })
}

/// The test's module and name, from the thread libtest names after it.
fn test_name() -> (String, String) {
    let thread = std::thread::current();
    let test = thread.name().unwrap_or("unnamed");
    let mut parts = test.rsplit("::");
    let name = parts.next().unwrap_or(test).to_string();
    let module = parts.next().unwrap_or("tests").to_string();
    (module, name)
}

fn check(order: usize, kind: &str, step: &str, app: &str, flag_on: &str) {
    let (module, name) = test_name();
    if !FROZEN.contains(&module.as_str()) {
        return;
    }
    let step: String = step.split_whitespace().collect::<Vec<_>>().join(" ");
    let head = format!("{order:02} {kind} {step}");
    let update = std::env::var_os("ER_UPDATE_GOLDENS").is_some();
    GOLDEN.with(|cell| {
        let mut cell = cell.borrow_mut();
        let golden = cell.get_or_insert_with(|| Golden::open(&module, &name));
        assert_eq!(golden.module, module, "one thread ran two tests");
        if update {
            assert_eq!(
                app, flag_on,
                "{head}: the legs differ, so no golden is recorded"
            );
            golden.record(head, app);
            return;
        }
        let path = golden.path.display().to_string();
        let held = golden.held.get(order - 1).unwrap_or_else(|| {
            panic!("{path} has no check {head}; record it with ER_UPDATE_GOLDENS=1")
        });
        assert_eq!(held.head, head, "{path}: check {order} is another step");
        let want = held
            .lines
            .iter()
            .map(|line| format!("{line}\n"))
            .collect::<String>();
        let differ: Vec<&str> = [("the App's", app), ("the flag path's", flag_on)]
            .into_iter()
            .filter(|(_, got)| *got != want)
            .map(|(leg, _)| leg)
            .collect();
        assert!(
            differ.is_empty(),
            "{path}: {head}: {} not the golden\n--- golden\n{want}--- App\n{app}--- flag path\n{flag_on}",
            match differ.as_slice() {
                [leg] => format!("{leg} result is"),
                _ => "both legs' results are".to_string(),
            }
        );
    });
}

impl Golden {
    fn open(module: &str, name: &str) -> Self {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("goldens/parity")
            .join(module)
            .join(format!("{name}.txt"));
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        Self {
            path,
            module: module.to_string(),
            held: decode(&text),
            recorded: Vec::new(),
        }
    }

    /// Adds the check and rewrites the file with every check so far, so the
    /// file ends at the test's last check.
    fn record(&mut self, head: String, text: &str) {
        let lines = text.lines().map(str::to_string).collect();
        self.recorded.push(Entry { head, lines });
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir).unwrap_or_else(|e| panic!("{e}"));
        }
        std::fs::write(&self.path, encode(&self.recorded)).unwrap_or_else(|e| panic!("{e}"));
    }
}

/// A check is `@@@ <head>`, then its lines as `|line`. A frame after the
/// first can instead name the earlier frame it differs least from,
/// `@@@ <head> < <order>`, and list only the lines that differ, as
/// `<index>|line`.
fn encode(entries: &[Entry]) -> String {
    let mut out = String::new();
    for (at, entry) in entries.iter().enumerate() {
        let changed = |base: &Entry| -> Option<Vec<usize>> {
            let same_shape = base.lines.len() == entry.lines.len()
                && base.head.split(' ').nth(1) == Some("frame")
                && entry.head.split(' ').nth(1) == Some("frame");
            same_shape.then(|| {
                (0..entry.lines.len())
                    .filter(|&line| base.lines[line] != entry.lines[line])
                    .collect()
            })
        };
        let base = entries[..at]
            .iter()
            .enumerate()
            .filter_map(|(order, base)| changed(base).map(|lines| (order + 1, lines)))
            .min_by_key(|(_, lines)| lines.len());
        match base {
            Some((order, lines)) if lines.len() < entry.lines.len() => {
                let _ = writeln!(out, "@@@ {} < {order:02}", entry.head);
                for line in lines {
                    let _ = writeln!(out, "{line:02}|{}", entry.lines[line]);
                }
            }
            _ => {
                let _ = writeln!(out, "@@@ {}", entry.head);
                for line in &entry.lines {
                    let _ = writeln!(out, "|{line}");
                }
            }
        }
    }
    out
}

fn decode(text: &str) -> Vec<Entry> {
    let mut entries: Vec<Entry> = Vec::new();
    for line in text.lines() {
        if let Some(head) = line.strip_prefix("@@@ ") {
            let (head, base) = match head.rsplit_once(" < ") {
                Some((head, base)) => (head, base.parse::<usize>().ok()),
                None => (head, None),
            };
            let lines = base
                .and_then(|base| entries.get(base - 1))
                .map(|base| base.lines.clone())
                .unwrap_or_default();
            entries.push(Entry {
                head: head.to_string(),
                lines,
            });
            continue;
        }
        let Some(entry) = entries.last_mut() else {
            continue;
        };
        let (index, rest) = line.split_once('|').unwrap_or(("", line));
        match index.parse::<usize>() {
            Ok(index) if index < entry.lines.len() => entry.lines[index] = rest.to_string(),
            _ => entry.lines.push(rest.to_string()),
        }
    }
    entries
}

/// Writes the frame under `ER_PARITY_DUMP`, named by the test, the check's
/// order within the test and its step, so two runs can be diffed.
fn dump(order: usize, step: &str, text: &str) {
    let Some(dir) = std::env::var_os("ER_PARITY_DUMP") else {
        return;
    };
    let (module, name) = test_name();
    let step: String = step
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    let dir = std::path::Path::new(&dir).join(module);
    std::fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("{e}"));
    let file = dir.join(format!("{name}.{order:02}.{}.txt", step.trim_matches('_')));
    std::fs::write(&file, text).unwrap_or_else(|e| panic!("{e}"));
}

#[test]
fn a_delta_golden_decodes_to_the_frames_it_encodes() {
    let entry = |head: &str, lines: &[&str]| Entry {
        head: head.to_string(),
        lines: lines.iter().map(|line| (*line).to_string()).collect(),
    };
    let entries = [
        entry("01 frame start", &["a", "b", "", "styles 1"]),
        entry("02 frame k", &["a", "B", "", "styles 2"]),
        entry("03 json saved", &["{", "  \"x\": 1", "}"]),
        entry("04 frame Esc", &["a", "b", "", "styles 1"]),
        entry("05 frame j", &["x", "y", "z", "w"]),
    ];
    let text = encode(&entries);
    assert!(text.contains("@@@ 04 frame Esc < 01\n"), "{text}");
    let decoded = decode(&text);
    assert_eq!(decoded.len(), entries.len());
    for (decoded, entry) in decoded.iter().zip(&entries) {
        assert_eq!(decoded.head, entry.head);
        assert_eq!(decoded.lines, entry.lines, "{text}");
    }
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
