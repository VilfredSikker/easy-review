//! The text the copy actions put on the clipboard, and the notice each
//! shows. Both front ends build it here from the file and cursor they hold,
//! so the rules live once; the clipboard handoff stays with the caller.

use std::ops::RangeInclusive;

use crate::ai::{AiState, InlineLayers};
use crate::git::{DiffFile, LineType};

/// Text to copy, and the notice once it is copied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Copied {
    pub text: String,
    pub notice: String,
}

/// What a copy builds: the text, or the notice that says why there is none.
pub type Built = Result<Copied, String>;

const NO_FILE: &str = "No file selected";
const NO_HUNK: &str = "No hunk selected";

/// Where the cursor stands in the selected file.
#[derive(Debug, Clone, Default)]
pub struct Cursor {
    pub hunk: usize,
    pub line: Option<usize>,
    /// The shift-arrow selection, as line indices in the hunk.
    pub range: Option<RangeInclusive<usize>>,
}

fn copied(text: String, notice: impl Into<String>) -> Built {
    Ok(Copied {
        text,
        notice: notice.into(),
    })
}

pub fn hunk(file: Option<&DiffFile>, hunk: usize) -> Built {
    let file = file.ok_or(NO_FILE)?;
    let hunk = file.hunks.get(hunk).ok_or(NO_HUNK)?;
    copied(hunk.to_text(), "Hunk copied to clipboard")
}

/// Every hunk of the file, in unified diff format.
pub fn full_file(file: Option<&DiffFile>) -> Built {
    let file = file.ok_or(NO_FILE)?;
    let mut text = format!("--- a/{}\n+++ b/{}\n", file.path, file.path);
    for hunk in &file.hunks {
        text.push_str(&hunk.to_text());
        text.push('\n');
    }
    let notice = format!("Copied full file diff ({} hunks)", file.hunks.len());
    copied(text, notice)
}

pub fn path(file: Option<&DiffFile>) -> Built {
    let file = file.ok_or(NO_FILE)?;
    copied(file.path.clone(), format!("Copied: {}", file.path))
}

pub fn line(file: Option<&DiffFile>, at: &Cursor) -> Built {
    let file = file.ok_or(NO_FILE)?;
    let line = at
        .line
        .and_then(|line| file.hunks.get(at.hunk)?.lines.get(line))
        .ok_or("No line selected — use arrow keys to enter line navigation")?;
    copied(line.content.clone(), "Line copied to clipboard")
}

/// Context for an agent terminal: the selection, else the cursor's line,
/// else the whole hunk, under a file and branch header, with the hunk's
/// first finding.
pub fn context(
    file: Option<&DiffFile>,
    at: &Cursor,
    branches: (&str, &str),
    ai: &AiState,
    layers: &InlineLayers,
) -> Built {
    let file = file.ok_or(NO_FILE)?;
    let hunk = file.hunks.get(at.hunk).ok_or(NO_HUNK)?;
    let mut text = format!(
        "File: {}\nBranch: {} (vs {})\n",
        file.path, branches.0, branches.1
    );
    let whole = || {
        (
            hunk.lines.iter().collect(),
            format!("Hunk #{}", at.hunk + 1),
        )
    };
    let (lines, label): (Vec<_>, String) = if let Some(range) = &at.range {
        let selected: Vec<_> = hunk
            .lines
            .iter()
            .enumerate()
            .filter(|(i, _)| range.contains(i))
            .map(|(_, l)| l)
            .collect();
        let start = selected.first().and_then(|l| l.new_num).unwrap_or(0);
        let end = selected.last().and_then(|l| l.new_num).unwrap_or(0);
        let label = if start == end {
            format!("Line {start}")
        } else {
            format!("Lines {start}-{end}")
        };
        (selected, label)
    } else if let Some(line_idx) = at.line {
        match hunk.lines.get(line_idx) {
            Some(line) => (vec![line], format!("Line {}", line.new_num.unwrap_or(0))),
            None => whole(),
        }
    } else {
        whole()
    };
    text.push_str(&format!("{label}:\n\n {}\n", hunk.header));
    for line in &lines {
        let prefix = match line.line_type {
            LineType::Add => "+",
            LineType::Delete => "-",
            LineType::Context => " ",
            LineType::Fold(_) => continue,
        };
        text.push_str(&format!("{prefix}{}\n", line.content));
    }
    let findings = ai.findings_for_hunk(&file.path, at.hunk, file.hunks.len(), layers);
    if let Some(finding) = findings.first() {
        text.push_str(&format!(
            "\nFinding: [{:?}] {}\n",
            finding.severity, finding.title
        ));
        if !finding.suggestion.is_empty() {
            text.push_str(&format!("Suggestion: {}\n", finding.suggestion));
        }
    }
    let scope = if at.range.is_some() {
        "selection"
    } else if at.line.is_some() {
        "line"
    } else {
        "hunk"
    };
    let notice = format!("Copied {scope} ({} lines)", lines.len());
    copied(text, notice)
}
