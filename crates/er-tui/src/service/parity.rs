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
        Some(c) => key_name(c.trim_start_matches('\\')),
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
    let key = if key.is_empty() { "\\" } else { key };
    NAMES
        .iter()
        .find(|(punct, _)| *punct == key)
        .map_or_else(|| key.to_string(), |(_, name)| (*name).to_string())
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
