//! Report rendering: human table and machine-readable JSON.

use std::fmt;

use anyhow::Context;
use serde::Serialize;

/// Output format for [`crate::run`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub enum OutputFormat {
    /// Aligned terminal table plus a summary line.
    #[default]
    Human,
    /// Versioned-ish JSON envelope (field names mirror cargo-crap's schema).
    Json,
}

impl fmt::Display for OutputFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Human => write!(f, "human"),
            Self::Json => write!(f, "json"),
        }
    }
}

/// One scored function.
#[derive(Debug, Clone, Serialize)]
pub struct Entry {
    /// Path relative to the analysis root.
    pub file: String,
    /// Readable function name (impl methods prefixed with their type).
    pub function: String,
    /// 1-based start line in the file.
    pub line: usize,
    /// Cyclomatic complexity.
    pub cyclomatic: usize,
    /// Test coverage in percent (0.0 when absent from the coverage report).
    pub coverage: f64,
    /// CRAP score.
    pub crap: f64,
}

/// Render the human report. With `summary_only`, only the aggregate line is
/// printed (the table is skipped).
pub fn render_human(entries: &[Entry], threshold: f64, summary_only: bool) -> String {
    let mut s = String::new();
    if !summary_only && !entries.is_empty() {
        let w_fn = entries
            .iter()
            .map(|e| e.function.len())
            .max()
            .unwrap_or(0)
            .min(64);
        let w_loc = entries
            .iter()
            .map(|e| e.file.len() + 6)
            .max()
            .unwrap_or(0)
            .min(64);
        s.push_str(&format!(
            "{:>7} {:>5} {:>8}  {:<w_fn$}  {:<w_loc$}\n",
            "CRAP", "CC", "COVERAGE", "FUNCTION", "LOCATION"
        ));
        for e in entries {
            let flag = if crate::is_crappy(e.crap, threshold) {
                "✗"
            } else {
                "✓"
            };
            s.push_str(&format!(
                "{flag} {:>6.1} {:>5} {:>7.1}%  {:<w_fn$}  {:<w_loc$}\n",
                e.crap,
                e.cyclomatic,
                e.coverage,
                e.function,
                format!("{}:{}", e.file, e.line)
            ));
        }
    }
    let crappy = entries
        .iter()
        .filter(|e| crate::is_crappy(e.crap, threshold))
        .count();
    s.push_str(&format!(
        "\n{0}/{1} function(s) exceed the CRAP threshold of {threshold:.0}.\n",
        crappy,
        entries.len()
    ));
    s
}

/// Render the JSON report. With `summary`, per-function entries are omitted
///
/// and only the envelope (`threshold`/`total`/`crappy`) is emitted, matching
/// `--summary` behavior in the human format. Serialization failures bubble up
/// as errors instead of being swallowed.
pub fn render_json(entries: &[Entry], threshold: f64, summary: bool) -> anyhow::Result<String> {
    #[derive(Serialize)]
    struct Envelope<'a> {
        threshold: f64,
        total: usize,
        crappy: usize,
        #[serde(skip_serializing_if = "Option::is_none")]
        entries: Option<&'a [Entry]>,
    }
    let crappy = entries
        .iter()
        .filter(|e| crate::is_crappy(e.crap, threshold))
        .count();
    let envelope = Envelope {
        threshold,
        total: entries.len(),
        crappy,
        entries: if summary { None } else { Some(entries) },
    };
    serde_json::to_string_pretty(&envelope).context("failed to serialize CRAP report")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(crap: f64) -> Entry {
        Entry {
            file: "src/lib.rs".to_string(),
            function: "f".to_string(),
            line: 1,
            cyclomatic: 2,
            coverage: 50.0,
            crap,
        }
    }

    #[test]
    fn output_format_displays_clap_names() {
        assert_eq!(OutputFormat::Human.to_string(), "human");
        assert_eq!(OutputFormat::Json.to_string(), "json");
    }

    #[test]
    fn human_report_marks_crappy_rows() {
        let entries = vec![entry(40.0), entry(5.0)];
        let report = render_human(&entries, 30.0, false);
        assert!(
            report.contains("✗"),
            "crappy row flagged:
{report}"
        );
        assert!(
            report.contains("✓"),
            "clean row marked:
{report}"
        );
        assert!(report.contains("1/2 function(s) exceed the CRAP threshold of 30."));
    }

    #[test]
    fn json_report_counts_crappy_entries() {
        let entries = vec![entry(40.0), entry(5.0)];
        let report = render_json(&entries, 30.0, false).unwrap();
        let v: serde_json::Value = serde_json::from_str(&report).unwrap();
        assert_eq!(v["total"], 2);
        assert_eq!(v["crappy"], 1);
        assert_eq!(v["threshold"], 30.0);
        assert_eq!(v["entries"][0]["function"], "f");
    }

    #[test]
    fn json_summary_omits_entries() {
        let entries = vec![entry(40.0), entry(5.0)];
        let report = render_json(&entries, 30.0, true).unwrap();
        let v: serde_json::Value = serde_json::from_str(&report).unwrap();
        assert_eq!(v["total"], 2);
        assert_eq!(v["crappy"], 1);
        assert!(
            v.get("entries").is_none(),
            "--summary json has no entries array: {report}"
        );
    }
}
