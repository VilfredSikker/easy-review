//! A parity frame as it will be frozen: `golden.rs`'s text and style
//! digest, with the run's own text taken out, so the same test draws the
//! same file on every run and in every year. Temp dir names, arena run ids
//! and clock times are masked at the same width. A relative age such as
//! `7 years ago` grows a digit with time, so it becomes the fixed `<age>`
//! and the rest of its row moves up to meet it, styles included.
//!
//! Each frozen test's checks are held in `goldens/parity/<module>/<test>.txt`,
//! and both the App's result and the flag path's must equal them;
//! `ER_UPDATE_GOLDENS=1` records them from the App. `ER_PARITY_DUMP=<dir>`
//! writes each frame to `<dir>/<module>/<test>.<order>.<step>.txt`, so two
//! runs can be diffed.

use std::cell::{Cell, RefCell};
use std::fmt::Write as _;
use std::path::PathBuf;

use ratatui::buffer::{Buffer, Cell as BufferCell};

thread_local! {
    static ORDER: Cell<usize> = const { Cell::new(0) };
}

/// One mask: how many chars from `at` it covers, or 0.
type Rule = fn(&[char], usize) -> usize;

const RULES: [Rule; 4] = [temp_dir, arena_id, clock, elapsed];

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

/// An agent-log offset, `[   +0s]` or `[+1m05s]`: how long after the run
/// began a line came, which a slow spawn moves.
fn elapsed(chars: &[char], at: usize) -> usize {
    if chars.get(at) != Some(&'[') {
        return 0;
    }
    let digits = |from: usize| {
        chars[from.min(chars.len())..]
            .iter()
            .take_while(|c| c.is_ascii_digit())
            .count()
    };
    let mut end = at + 1;
    end += chars[end..].iter().take_while(|c| **c == ' ').count();
    if chars.get(end) != Some(&'+') {
        return 0;
    }
    end += 1;
    let count = digits(end);
    if count == 0 {
        return 0;
    }
    end += count;
    if chars.get(end) == Some(&'m') {
        let seconds = digits(end + 1);
        if seconds == 0 {
            return 0;
        }
        end += 1 + seconds;
    }
    if chars.get(end..end + 2) == Some(&['s', ']'][..]) {
        end + 2 - at
    } else {
        0
    }
}

/// `01:27` or `01:27:07`, standing alone: a digit or `:` on either side
/// makes it part of something else, such as `f000.rs:12:34`.
fn clock(chars: &[char], at: usize) -> usize {
    let pair = |from: usize| {
        chars
            .get(from..from + 2)
            .is_some_and(|two| two.iter().all(char::is_ascii_digit))
    };
    let colon = |from: usize| chars.get(from) == Some(&':');
    let joined = |c: &char| c.is_ascii_digit() || *c == ':';
    if at > 0 && chars[at - 1].is_ascii_digit() {
        return 0;
    }
    if at > 0 && chars[at - 1] == ':' {
        return 0;
    }
    if !(pair(at) && colon(at + 2) && pair(at + 3)) {
        return 0;
    }
    let len = if colon(at + 5) && pair(at + 6) { 8 } else { 5 };
    if chars.get(at + len).is_some_and(joined) {
        return 0;
    }
    len
}

/// How many chars from `at` a git relative age covers: `7 years ago`,
/// `1 year, 2 months ago`, `3 days ago`.
fn age(chars: &[char], at: usize) -> usize {
    if at > 0 && chars[at - 1].is_ascii_alphanumeric() {
        return 0;
    }
    let units = ["second", "minute", "hour", "day", "week", "month", "year"];
    let count_and_unit = |from: usize| -> usize {
        let digits = chars[from.min(chars.len())..]
            .iter()
            .take_while(|c| c.is_ascii_digit())
            .count();
        if digits == 0 || chars.get(from + digits) != Some(&' ') {
            return 0;
        }
        let word = from + digits + 1;
        units
            .iter()
            .find(|unit| starts(chars, word, unit))
            .map_or(0, |unit| {
                let end = word + unit.len();
                digits + 1 + unit.len() + usize::from(chars.get(end) == Some(&'s'))
            })
    };
    let mut len = count_and_unit(at);
    if len == 0 {
        return 0;
    }
    if starts(chars, at + len, ", ") {
        let more = count_and_unit(at + len + 2);
        if more > 0 {
            len += 2 + more;
        }
    }
    if starts(chars, at + len, " ago") {
        len + 4
    } else {
        0
    }
}

/// The row with each commit age replaced by `<age>`, in the age's own
/// style, and the cells after it moved up, padded at the end to the row's
/// width.
fn collapse_ages(row: &[BufferCell]) -> Vec<BufferCell> {
    const TOKEN: [&str; 5] = ["<", "a", "g", "e", ">"];
    let chars: Vec<char> = row
        .iter()
        .map(|cell| {
            let mut symbol = cell.symbol().chars();
            match (symbol.next(), symbol.next()) {
                (Some(c), None) => c,
                _ => '\u{0}',
            }
        })
        .collect();
    let mut out = Vec::with_capacity(row.len());
    let mut at = 0;
    while at < row.len() {
        let len = if after_short_hash(&chars, at) {
            age(&chars, at)
        } else {
            0
        };
        if len == 0 {
            out.push(row[at].clone());
            at += 1;
            continue;
        }
        for symbol in TOKEN {
            let mut cell = row[at].clone();
            cell.set_symbol(symbol);
            out.push(cell);
        }
        at += len;
    }
    let pad = row.last().cloned().map(|mut cell| {
        cell.set_symbol(" ");
        cell
    });
    while out.len() < row.len() {
        out.extend(pad.clone());
    }
    out.truncate(row.len());
    out
}

/// Whether `at` follows ` · <short hash> · `, where the status bar's
/// History row puts a commit's age. An age anywhere else is the row's own
/// text and is kept.
fn after_short_hash(chars: &[char], at: usize) -> bool {
    let sep: Vec<char> = " · ".chars().collect();
    let Some(hash_end) = at.checked_sub(sep.len()) else {
        return false;
    };
    if chars[hash_end..at] != sep[..] {
        return false;
    }
    let hash = chars[..hash_end]
        .iter()
        .rev()
        .take_while(|c| c.is_ascii_hexdigit())
        .count();
    hash >= 7
        && hash_end
            .checked_sub(hash + sep.len())
            .is_some_and(|from| chars[from..hash_end - hash] == sep[..])
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

/// `golden.rs`'s text and style digest for `frame`, ages collapsed and
/// the rest masked. A frame with neither keeps `golden.rs`'s digest.
pub(super) fn stable_text(frame: &Buffer) -> String {
    let width = usize::from(frame.area.width).max(1);
    let rows: Vec<Vec<BufferCell>> = frame.content.chunks(width).map(collapse_ages).collect();
    crate::golden::rows_text(rows.iter().map(Vec::as_slice), mask)
}

/// The test modules whose checks are frozen, or single tests as
/// `module::test`. Each later P76.1 PR adds its
/// areas; the others still only compare the two legs live.
const FROZEN: [&str; 23] = [
    "copy_tests",
    "open_tests",
    "duties_tests",
    "config_hub_tests",
    "pr_stack_tests",
    "flows_tests",
    "ai_runs_tests",
    "tour_tests",
    "findings_tests",
    "clears_tests",
    "history_tests",
    "modes_tests",
    "context_tests",
    "tests",
    "filter_tests",
    "reviewed_tests",
    "settings_tests",
    "github_tests",
    "threads_tests",
    "thread_writes_tests",
    "worktree_tests",
    "watched_tests",
    "chrome_tests",
];

/// Whether a test's checks are enforced: its module is in [`FROZEN`], or
/// the test is, as `module::test`, while its module is split across parts.
fn frozen(module: &str, name: &str) -> bool {
    FROZEN
        .iter()
        .any(|entry| *entry == module || entry.split_once("::") == Some((module, name)))
}

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
    static GUARDED: Cell<bool> = const { Cell::new(false) };
}

/// Held for the whole of a frozen test. When it drops it fails the test if
/// the golden holds a check the test never reached, so a test whose last
/// steps stop running cannot pass on the checks before them.
pub(super) struct Guard(());

/// Starts a frozen test's checks; a frozen check without one fails.
#[must_use = "the guard checks the golden when it drops"]
pub(super) fn guard() -> Guard {
    let (module, name) = test_name();
    assert!(
        frozen(&module, &name),
        "{module}::{name} holds a guard, but neither it nor {module} is in FROZEN"
    );
    GUARDED.with(|guarded| guarded.set(true));
    Guard(())
}

impl Drop for Guard {
    fn drop(&mut self) {
        GUARDED.with(|guarded| guarded.set(false));
        if std::thread::panicking() || std::env::var_os("ER_UPDATE_GOLDENS").is_some() {
            return;
        }
        let (module, name) = test_name();
        let ran = ORDER.with(Cell::get);
        let golden = GOLDEN
            .with(|cell| cell.borrow_mut().take())
            .unwrap_or_else(|| Golden::open(&module, &name));
        assert!(
            golden.held.len() <= ran,
            "{}: the golden holds {} checks, and the test ran {ran}; the next is {}",
            golden.path.display(),
            golden.held.len(),
            golden.held[ran].head
        );
    }
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
        // Through a `Value`, whose maps sort their keys, so a `HashMap`
        // reads the same on every run.
        let value = serde_json::to_value(value).unwrap_or_else(|e| panic!("{e}"));
        let text = serde_json::to_string_pretty(&value).unwrap_or_else(|e| panic!("{e}"));
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
    if !frozen(&module, &name) {
        return;
    }
    assert!(
        GUARDED.with(Cell::get),
        "{module}::{name} is frozen: hold `parity::guard()` for the whole test"
    );
    let step: String = short_step(step)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    assert!(
        !step.contains(" < "),
        "{module}::{name}: step `{step}` holds ` < `, which a golden reads as a base"
    );
    let head = format!("{order:02} {kind} {step}");
    let update = std::env::var_os("ER_UPDATE_GOLDENS").is_some();
    assert!(
        !(update && std::env::var_os("CI").is_some()),
        "ER_UPDATE_GOLDENS is refused under CI: record goldens locally"
    );
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
        let want = held.lines.iter().fold(String::new(), |mut want, line| {
            let _ = writeln!(want, "{line}");
            want
        });
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
        let held = decode(&path.display().to_string(), &text);
        Self {
            path,
            module: module.to_string(),
            held,
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
                && base.head.split(' ').nth(1) == entry.head.split(' ').nth(1);
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

/// Reads a golden back. A base that is not an earlier check, or a row past
/// its base's length, fails with the file and line, so a hand edit that
/// breaks the format cannot read as some other frame.
fn decode(path: &str, text: &str) -> Vec<Entry> {
    let mut entries: Vec<Entry> = Vec::new();
    for (at, line) in text.lines().enumerate() {
        let at = at + 1;
        if let Some(head) = line.strip_prefix("@@@ ") {
            let (head, lines) = match head.rsplit_once(" < ") {
                Some((head, base)) => {
                    let earlier = base
                        .parse::<usize>()
                        .ok()
                        .filter(|base| (1..=entries.len()).contains(base));
                    let Some(base) = earlier else {
                        panic!(
                            "{path}:{at}: base `< {base}` is not an earlier check (1 to {})",
                            entries.len()
                        );
                    };
                    (head, entries[base - 1].lines.clone())
                }
                None => (head, Vec::new()),
            };
            entries.push(Entry {
                head: head.to_string(),
                lines,
            });
            continue;
        }
        let Some(entry) = entries.last_mut() else {
            panic!("{path}:{at}: a line before the first `@@@` check");
        };
        match line.split_once('|') {
            Some(("", rest)) => entry.lines.push(rest.to_string()),
            Some((index, rest)) => match index.parse::<usize>() {
                Ok(index) if index < entry.lines.len() => entry.lines[index] = rest.to_string(),
                _ => panic!(
                    "{path}:{at}: row `{index}` is past its base's {} rows",
                    entry.lines.len()
                ),
            },
            None => panic!("{path}:{at}: a line with no `|`"),
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
    let step: String = short_step(step)
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let dir = std::path::Path::new(&dir).join(module);
    std::fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("{e}"));
    let file = dir.join(format!("{name}.{order:02}.{step}.txt"));
    std::fs::write(&file, text).unwrap_or_else(|e| panic!("{e}"));
}

/// A step named by a key's `Debug` text, shortened: `KeyEvent { code:
/// Up, modifiers: SHIFT, .. }` is `S-Up`, `Char('k')` is `k`, and a space
/// is `Space`. Any other step is kept.
pub(super) fn short_step(step: &str) -> String {
    let (code, modifiers) = match step.strip_prefix("KeyEvent { code: ") {
        Some(rest) => {
            let (code, rest) = rest.split_once(", modifiers: ").unwrap_or((rest, ""));
            let modifiers = rest
                .strip_prefix("KeyModifiers(")
                .and_then(|rest| rest.split_once(')'))
                .map_or("", |(modifiers, _)| modifiers);
            (code, modifiers)
        }
        None => (step, ""),
    };
    let code = match code
        .strip_prefix("Char('")
        .and_then(|c| c.strip_suffix("')"))
    {
        Some(c) => key_name(c),
        None => match code.strip_prefix("F(").and_then(|n| n.strip_suffix(')')) {
            Some(n) => format!("F{n}"),
            None => code.to_string(),
        },
    };
    let bits = u8::from_str_radix(modifiers.trim_start_matches("0x"), 16).unwrap_or(0);
    let held = |name: &str, bit: u8| modifiers.contains(name) || bits & bit != 0;
    let mut short = String::new();
    for (prefix, name, bit) in [
        ("C-", "CONTROL", 0b10),
        ("A-", "ALT", 0b100),
        ("S-", "SHIFT", 0b1),
    ] {
        if held(name, bit) {
            short.push_str(prefix);
        }
    }
    short + &code
}

/// A printable key's name, spelled out where it is punctuation, so it
/// reads in a golden's header and survives a file name.
fn key_name(key: &str) -> String {
    // `Debug` escapes some chars; each is named before the escape goes, so
    // `\n` reads `nl`, not `n`.
    const ESCAPED: [(&str, &str); 6] = [
        ("\\n", "nl"),
        ("\\t", "tab"),
        ("\\r", "cr"),
        ("\\0", "nul"),
        ("\\\\", "backslash"),
        ("\\'", "quote"),
    ];
    const NAMES: [(&str, &str); 33] = [
        (" ", "Space"),
        ("<", "lt"),
        (">", "gt"),
        ("{", "lbrace"),
        ("}", "rbrace"),
        ("[", "lbracket"),
        ("]", "rbracket"),
        ("(", "lparen"),
        (")", "rparen"),
        ("/", "slash"),
        ("\\", "backslash"),
        ("?", "qmark"),
        ("!", "bang"),
        (",", "comma"),
        (".", "dot"),
        (";", "semicolon"),
        (":", "colon"),
        ("'", "quote"),
        ("\"", "dquote"),
        ("`", "backtick"),
        ("~", "tilde"),
        ("|", "pipe"),
        ("-", "minus"),
        ("+", "plus"),
        ("=", "eq"),
        ("_", "underscore"),
        ("@", "at"),
        ("#", "hash"),
        ("$", "dollar"),
        ("%", "percent"),
        ("^", "caret"),
        ("&", "amp"),
        ("*", "star"),
    ];
    if let Some((_, name)) = ESCAPED.iter().find(|(escaped, _)| *escaped == key) {
        return (*name).to_string();
    }
    let key = key.strip_prefix('\\').unwrap_or(key);
    NAMES
        .iter()
        .find(|(punct, _)| *punct == key)
        .map_or_else(|| key.to_string(), |(_, name)| (*name).to_string())
}

/// Every module with goldens is frozen, and every golden file is a frozen
/// test's, so a golden cannot sit unread.
#[test]
fn every_golden_directory_is_a_frozen_module() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("goldens/parity");
    let dirs = std::fs::read_dir(&root).unwrap_or_else(|e| panic!("{e}"));
    for dir in dirs {
        let dir = dir.unwrap_or_else(|e| panic!("{e}")).path();
        let module = dir
            .file_name()
            .map(|name| name.to_string_lossy().into_owned());
        let module = module.unwrap_or_default();
        assert!(
            FROZEN
                .iter()
                .any(|entry| entry.split("::").next() == Some(module.as_str())),
            "goldens/parity/{module} is not in FROZEN"
        );
        // Each file is read only by its own test, so a file whose test is
        // not frozen, or does not exist, would sit unread.
        let source = std::fs::read_to_string(
            dir.with_extension("rs")
                .to_string_lossy()
                .replace("goldens/parity", "src/service"),
        )
        .unwrap_or_else(|e| panic!("{module}.rs: {e}"));
        let files = std::fs::read_dir(&dir).unwrap_or_else(|e| panic!("{e}"));
        for file in files {
            let path = file.unwrap_or_else(|e| panic!("{e}")).path();
            let test = path
                .file_stem()
                .map(|stem| stem.to_string_lossy().into_owned());
            let test = test.unwrap_or_default();
            assert!(
                frozen(&module, &test),
                "goldens/parity/{module}/{test}.txt is for a test FROZEN does not list"
            );
            assert!(
                source.contains(&format!("fn {test}()")),
                "goldens/parity/{module}/{test}.txt names no test in {module}.rs"
            );
        }
    }
}

/// A base that is not an earlier check, or a row past the base, names
/// itself and its line.
#[test]
fn a_broken_reference_names_the_file_and_line() {
    let frame = "@@@ 01 frame a\n|x\n|y\n";
    for (bad, said) in [
        ("@@@ 02 frame b < 0x\n00|z\n", "f:4: base `< 0x`"),
        ("@@@ 02 frame b < 00\n00|z\n", "f:4: base `< 00`"),
        ("@@@ 02 frame b < 02\n00|z\n", "f:4: base `< 02`"),
        ("@@@ 02 frame b < 01\n02|z\n", "f:5: row `02`"),
    ] {
        let text = format!("{frame}{bad}");
        let panic = std::panic::catch_unwind(|| decode("f", &text));
        let message = panic
            .err()
            .and_then(|e| e.downcast_ref::<String>().cloned())
            .unwrap_or_default();
        assert!(message.starts_with(said), "{bad}: {message}");
    }
}

/// A check takes its base only from an earlier check of its own kind, so a
/// JSON result never reads as a frame that happens to match its lines.
#[test]
fn a_base_is_taken_only_from_the_same_kind() {
    let entry = |head: &str| Entry {
        head: head.to_string(),
        lines: vec!["[".to_string(), "  1".to_string(), "]".to_string()],
    };
    let entries = [
        entry("01 frame a"),
        entry("02 json b"),
        entry("03 frame c"),
        entry("04 json d"),
    ];
    let text = encode(&entries);
    assert!(text.contains("@@@ 02 json b\n|[\n"), "{text}");
    assert!(text.contains("@@@ 03 frame c < 01\n"), "{text}");
    assert!(text.contains("@@@ 04 json d < 02\n"), "{text}");
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
    let decoded = decode("unit", &text);
    assert_eq!(decoded.len(), entries.len());
    for (decoded, entry) in decoded.iter().zip(&entries) {
        assert_eq!(decoded.head, entry.head);
        assert_eq!(decoded.lines, entry.lines, "{text}");
    }
}

#[test]
fn key_steps_read_short() {
    let event = |code, modifiers| format!("{:?}", crossterm::event::KeyEvent::new(code, modifiers));
    use crossterm::event::{KeyCode, KeyModifiers};
    let cases = [
        (event(KeyCode::Up, KeyModifiers::NONE), "Up"),
        (event(KeyCode::Up, KeyModifiers::SHIFT), "S-Up"),
        (event(KeyCode::Char('k'), KeyModifiers::CONTROL), "C-k"),
        (event(KeyCode::Char('q'), KeyModifiers::NONE), "q"),
        (event(KeyCode::Char(' '), KeyModifiers::NONE), "Space"),
        (event(KeyCode::Char('<'), KeyModifiers::NONE), "lt"),
        (event(KeyCode::Char('>'), KeyModifiers::NONE), "gt"),
        (event(KeyCode::Char('{'), KeyModifiers::NONE), "lbrace"),
        (event(KeyCode::Char('}'), KeyModifiers::NONE), "rbrace"),
        (event(KeyCode::Char('/'), KeyModifiers::NONE), "slash"),
        (event(KeyCode::Char('?'), KeyModifiers::SHIFT), "S-qmark"),
        (event(KeyCode::Char('\''), KeyModifiers::NONE), "quote"),
        (event(KeyCode::Char('\\'), KeyModifiers::NONE), "backslash"),
        (event(KeyCode::Char('\n'), KeyModifiers::NONE), "nl"),
        (event(KeyCode::Char('\t'), KeyModifiers::NONE), "tab"),
        (event(KeyCode::Char('n'), KeyModifiers::NONE), "n"),
        (event(KeyCode::Char('t'), KeyModifiers::NONE), "t"),
        (format!("{:?}", KeyCode::Char('*')), "star"),
        (event(KeyCode::F(5), KeyModifiers::ALT), "A-F5"),
        (format!("{:?}", KeyCode::Char('U')), "U"),
        (format!("{:?}", KeyCode::PageDown), "PageDown"),
        ("start".to_string(), "start"),
    ];
    for (step, short) in cases {
        assert_eq!(short_step(&step), short, "{step}");
    }
}

#[test]
fn masks_keep_the_width_and_cover_only_the_runs_own_text() {
    let cases = [
        ("┌ /tmp/.tmpMLHFmN (Enter", "┌ /tmp/########## (Enter"),
        ("repos/.tmpAb12Cd-a1da9c47/b", "repos/###################/b"),
        ("· arena-1795569923677-1 · 1", "· ##################### · 1"),
        ("│  You  01:27:07", "│  You  ########"),
        ("at 09:30 ok", "at ##### ok"),
        ("123:45", "123:45"),
        ("12:345", "12:345"),
        ("f000.rs:12:34", "f000.rs:12:34"),
        ("12:34:567", "12:34:567"),
        ("[   +0s] [review] line-a", "######## [review] line-a"),
        ("[  +12s] x", "######## x"),
        ("[+1m05s] x", "######## x"),
        ("[x] [+s] [ 3s]", "[x] [+s] [ 3s]"),
        ("+2 -40 · 12 files · line 3", "+2 -40 · 12 files · line 3"),
    ];
    for (row, masked) in cases {
        assert_eq!(mask(row), masked);
        assert_eq!(mask(row).chars().count(), row.chars().count());
    }
}

/// Each age reads as `<age>` whatever its width, so a frame drawn in 2030
/// matches one drawn now, and a count that is no age stays.
#[test]
fn an_age_collapses_to_one_token_and_the_row_moves_up() {
    use ratatui::style::{Color, Modifier, Style};
    let dim = Style::default().add_modifier(Modifier::DIM);
    // The History row: the age is dim, as drawn, and the diff stat after
    // it is red, so the digest sees it move.
    let history = |age: &str| {
        let mut buffer = Buffer::empty(ratatui::layout::Rect::new(0, 0, 50, 1));
        let (end, _) = buffer.set_stringn(0, 0, " feature · 3b3fb05 · ", 50, Style::default());
        let (end, _) = buffer.set_stringn(end, 0, age, 50, dim);
        buffer.set_string(end, 0, "  +2 -40", Style::default().fg(Color::Red));
        buffer
    };
    let now = stable_text(&history("9 years ago"));
    assert!(
        now.starts_with(" feature · 3b3fb05 · <age>  +2 -40"),
        "{now}"
    );
    for later in ["10 years ago", "1 year, 2 months ago", "3 days ago"] {
        assert_eq!(stable_text(&history(later)), now, "{later}");
    }
    let token = collapse_ages(&history("9 years ago").content);
    let symbols: String = token[21..26].iter().map(BufferCell::symbol).collect();
    assert_eq!(symbols, "<age>");
    for cell in &token[21..26] {
        assert!(
            cell.style().add_modifier.contains(Modifier::DIM),
            "{cell:?}"
        );
    }
    // A diff row's own text, with a border after it, is kept as drawn.
    let mut row = Buffer::empty(ratatui::layout::Rect::new(0, 0, 40, 1));
    row.set_string(0, 0, "+ // fixed 3 days ago", Style::default());
    row.set_string(30, 0, "│", Style::default().fg(Color::Blue));
    let kept = collapse_ages(&row.content);
    assert_eq!(kept, row.content);
    let text = stable_text(&row);
    assert!(
        text.starts_with("+ // fixed 3 days ago         │"),
        "{text}"
    );
    for kept in ["1h ago", "12 files", "line 3 ago"] {
        let mut row = Buffer::empty(ratatui::layout::Rect::new(0, 0, 40, 1));
        row.set_string(0, 0, kept, Style::default());
        assert!(stable_text(&row).starts_with(kept), "{kept}");
    }
}
