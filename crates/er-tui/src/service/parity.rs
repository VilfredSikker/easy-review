//! A parity frame as it will be frozen: `golden.rs`'s text and style
//! digest, with the run's own text taken out, so the same test draws the
//! same file on every run and in every year. Temp dir names, arena run ids
//! and clock times are masked at the same width. A relative age such as
//! `7 years ago` grows a digit with time, so it becomes the fixed `<age>`
//! and the rest of its row moves up to meet it, styles included.
//!
//! `ER_PARITY_DUMP=<dir>` writes each frame the parity tests compare to
//! `<dir>/<module>/<test>.<order>.<step>.txt`, so two runs can be diffed.

use std::cell::Cell;
use std::fmt::Write as _;

use ratatui::buffer::{Buffer, Cell as BufferCell};

thread_local! {
    static ORDER: Cell<usize> = const { Cell::new(0) };
}

/// One mask: how many chars from `at` it covers, or 0.
type Rule = fn(&[char], usize) -> usize;

const RULES: [Rule; 3] = [temp_dir, arena_id, clock];

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

/// The row with each relative age replaced by `<age>` and the cells after
/// it moved up, padded at the end to the row's width.
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
        let len = age(&chars, at);
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
    let mut out = String::new();
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for row in frame.content.chunks(width) {
        let row = collapse_ages(row);
        let text: String = row.iter().map(BufferCell::symbol).collect();
        out.push_str(&mask(text.trim_end()));
        out.push('\n');
        for cell in &row {
            for byte in format!("{:?}", cell.style()).bytes() {
                hash ^= u64::from(byte);
                hash = hash.wrapping_mul(0x0100_0000_01b3);
            }
        }
    }
    let _ = writeln!(out, "styles {hash:016x}");
    out
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
    std::fs::write(&file, stable_text(frame)).unwrap_or_else(|e| panic!("{e}"));
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
        Some(" ") => "Space".to_string(),
        Some(c) => c.to_string(),
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
    use ratatui::style::{Color, Style};
    // The diff stat after the age is red, so the digest sees it move.
    let draw = |text: &str| {
        let mut buffer = Buffer::empty(ratatui::layout::Rect::new(0, 0, 40, 1));
        let (end, _) = buffer.set_stringn(0, 0, text, 40, Style::default());
        buffer.set_string(end, 0, "  +2 -40", Style::default().fg(Color::Red));
        stable_text(&buffer)
    };
    let now = draw("· abc · 9 years ago");
    assert!(now.starts_with("· abc · <age>  +2 -40"), "{now}");
    for later in [
        "· abc · 10 years ago",
        "· abc · 1 year, 2 months ago",
        "· abc · 3 days ago",
    ] {
        assert_eq!(draw(later), now, "{later}");
    }
    for kept in ["1h ago", "12 files", "line 3 ago"] {
        assert!(draw(kept).starts_with(kept), "{kept}");
    }
}
