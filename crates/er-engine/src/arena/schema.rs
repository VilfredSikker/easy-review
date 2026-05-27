use crate::ai::RiskLevel;
use anyhow::{bail, Context, Result};
use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Clone, Deserialize)]
pub struct Round1Finding {
    pub file: String,
    #[serde(default)]
    pub line: Option<usize>,
    pub title: String,
    pub body: String,
    pub severity: String,
    #[serde(default)]
    pub confidence: Option<f32>,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Round1Output {
    pub findings: Vec<Round1Finding>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Round2Ballot {
    pub finding_id: String,
    pub vote: String,
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub merge_target: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Round2Output {
    pub ballots: Vec<Round2Ballot>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Round3Verdict {
    pub finding_id: String,
    pub verdict: String,
    pub confidence: f32,
    pub rationale: String,
    #[serde(default)]
    pub merged_into: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Round3Output {
    pub verdicts: Vec<Round3Verdict>,
}

pub fn parse_round1(value: &Value) -> Result<Round1Output> {
    serde_json::from_value(value.clone()).context("round-1 schema")
}

pub fn parse_round2(value: &Value) -> Result<Round2Output> {
    serde_json::from_value(value.clone()).context("round-2 schema")
}

pub fn parse_round3(value: &Value) -> Result<Round3Output> {
    serde_json::from_value(value.clone()).context("round-3 schema")
}

pub fn validate_round1_output(value: &Value) -> Result<Round1Output> {
    let out = parse_round1(value)?;
    for (i, f) in out.findings.iter().enumerate() {
        if f.file.trim().is_empty() {
            bail!("finding[{i}]: file is required");
        }
        if f.title.trim().is_empty() || f.body.trim().is_empty() {
            bail!("finding[{i}]: title and body are required");
        }
        parse_severity(&f.severity)
            .with_context(|| format!("finding[{i}]: invalid severity"))?;
    }
    Ok(out)
}

pub fn validate_round2_output(value: &Value) -> Result<Round2Output> {
    let out = parse_round2(value)?;
    for (i, b) in out.ballots.iter().enumerate() {
        if b.finding_id.trim().is_empty() {
            bail!("ballot[{i}]: finding_id is required");
        }
        parse_vote(&b.vote).with_context(|| format!("ballot[{i}]: invalid vote"))?;
        if b.note.trim().is_empty() && !matches!(b.vote.as_str(), "abstain") {
            bail!("ballot[{i}]: note is required (grounding)");
        }
    }
    Ok(out)
}

pub fn validate_round3_output(value: &Value) -> Result<Round3Output> {
    let out = parse_round3(value)?;
    for (i, v) in out.verdicts.iter().enumerate() {
        if v.finding_id.trim().is_empty() {
            bail!("verdict[{i}]: finding_id is required");
        }
        parse_verdict(&v.verdict).with_context(|| format!("verdict[{i}]: invalid verdict"))?;
        if !(0.0..=1.0).contains(&v.confidence) {
            bail!("verdict[{i}]: confidence must be 0..1");
        }
        if v.rationale.trim().is_empty() {
            bail!("verdict[{i}]: rationale is required");
        }
    }
    Ok(out)
}

pub fn parse_severity(s: &str) -> Result<RiskLevel> {
    match s.to_ascii_lowercase().as_str() {
        "high" => Ok(RiskLevel::High),
        "med" | "medium" => Ok(RiskLevel::Medium),
        "low" => Ok(RiskLevel::Low),
        "info" => Ok(RiskLevel::Info),
        other => bail!("unknown severity: {other}"),
    }
}

pub fn parse_vote(s: &str) -> Result<()> {
    match s.to_ascii_lowercase().as_str() {
        "propose" | "keep" | "drop" | "merge" | "escalate" | "lower" | "abstain" | "flag" => Ok(()),
        other => bail!("unknown vote: {other}"),
    }
}

pub fn parse_verdict(s: &str) -> Result<()> {
    match s.to_ascii_lowercase().as_str() {
        "kept" | "escalated" | "merged" | "dropped" | "pending" => Ok(()),
        other => bail!("unknown verdict: {other}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn rejects_round1_without_body() {
        let v = json!({ "findings": [{ "file": "a.rs", "title": "t", "body": "", "severity": "high" }] });
        assert!(validate_round1_output(&v).is_err());
    }

    #[test]
    fn accepts_med_severity_alias() {
        let v = json!({ "findings": [{ "file": "a.rs", "title": "t", "body": "b", "severity": "med" }] });
        assert!(validate_round1_output(&v).is_ok());
    }
}
