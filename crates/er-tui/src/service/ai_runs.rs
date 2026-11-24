//! The AI hub's run rows, the Verify and package hubs, and the prompts that
//! ask whether to clear a last run first. A run is planned on the service
//! host's worker and queued on its task host (ADR 0074): a key only asks for
//! it, and the reply says whether the planner took it.

use er_engine::config::ErConfig;
use er_service::{Command, ReviewArtifact, RunRequest, TabContent};

use super::ai_picker::HubFacts;

/// A run that asks first when its last run left output behind: `y` clears
/// it and runs once the clear lands, `k` keeps it and runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rerun {
    Review,
    Questions,
    Notes,
}

impl Rerun {
    pub fn request(self) -> RunRequest {
        match self {
            Self::Review => RunRequest::Review {
                scope: "current".into(),
            },
            Self::Questions => RunRequest::Questions,
            Self::Notes => RunRequest::Notes,
        }
    }

    /// The clear `y` sends before the run.
    pub fn clear(self, content: &TabContent) -> Command {
        let context = content.context.clone();
        match self {
            Self::Review => Command::DeleteReviewArtifact {
                context,
                artifact: ReviewArtifact::Review,
            },
            Self::Questions => Command::ClearQuestionAnswers { context },
            Self::Notes => Command::ClearNoteReplies { context },
        }
    }

    /// What a refused clear says; the run does not start.
    pub fn refused(self, error: &str) -> String {
        let what = match self {
            Self::Review => "Review",
            Self::Questions => "Questions",
            Self::Notes => "Notes",
        };
        format!("{what} not started: {error}")
    }

    /// Whether the last run left something the prompt offers to clear, by
    /// the App's rules.
    fn left_over(self, content: &TabContent) -> bool {
        let ai = &content.ai.state;
        match self {
            Self::Review => ai.review.is_some(),
            Self::Questions => ai.questions.as_ref().is_some_and(|q| {
                q.questions
                    .iter()
                    .any(|q| q.in_reply_to.is_some() && q.author == "Claude")
            }),
            Self::Notes => ai.notes.as_ref().is_some_and(|ns| {
                ns.notes
                    .iter()
                    .any(|n| n.in_reply_to.is_some() && n.author == "Claude")
            }),
        }
    }
}

/// What a run row starts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Start {
    Rerun(Rerun),
    Run(RunRequest),
}

/// What choosing a run row does next.
pub enum Next {
    Ask(Rerun),
    Run(RunRequest),
}

pub fn next(start: Start, content: &TabContent) -> Next {
    match start {
        Start::Rerun(rerun) if rerun.left_over(content) => Next::Ask(rerun),
        Start::Rerun(rerun) => Next::Run(rerun.request()),
        Start::Run(request) => Next::Run(request),
    }
}

/// The App's start notice for a run the host tells nothing about: the tab
/// commands and named commands, which take no queue place. A queued run's
/// start or queue place comes from the host, and validate tells its own.
pub fn started(request: &RunRequest) -> Option<String> {
    let name = match request {
        RunRequest::Questions => "questions",
        RunRequest::Notes => "notes",
        RunRequest::Summary => "summary",
        RunRequest::Command { name, .. } => name,
        _ => return None,
    };
    Some(format!("{name} started..."))
}

/// One hub row as the App lists it.
pub struct Row {
    pub label: String,
    pub hint: String,
    pub description: String,
    pub enabled: bool,
    pub header: bool,
    pub choice: Option<Choice>,
}

/// What choosing a Verify or expert row does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Choice {
    Start(Start),
    Package(String),
}

fn row(label: &str, description: String, enabled: bool, choice: Option<Choice>) -> Row {
    Row {
        label: label.to_string(),
        hint: String::new(),
        description,
        enabled,
        header: false,
        choice,
    }
}

fn header(label: &str) -> Row {
    Row {
        header: true,
        ..row(label, String::new(), false, None)
    }
}

/// An AI hub run row by its label: its description, whether it is live,
/// and what it starts, by the App's rules. `None` for a row that is no run.
pub fn hub_row(label: &str, facts: &HubFacts, via: &str) -> Option<(String, bool, Start)> {
    let run = |request| Start::Run(request);
    Some(match label {
        "Triage branch" => (
            format!("Fast scan — first impression and review routing via {via}"),
            true,
            run(RunRequest::Triage),
        ),
        "Review work" => (
            if facts.has_review {
                format!("Full AI review via {via} (will ask to clear previous)")
            } else {
                format!("Full review: risk, order, checklist, summary via {via}")
            },
            true,
            Start::Rerun(Rerun::Review),
        ),
        "Generate summary" => (
            format!("Generate summary via {via}"),
            true,
            run(RunRequest::Summary),
        ),
        "Generate guided tour" => (
            if facts.has_tour {
                "Regenerate the guided tour for this branch / PR diff".into()
            } else {
                "AI-guided walkthrough: pillars + per-file reasons (adds the Guide tab)".into()
            },
            facts.has_diff,
            run(RunRequest::Tour),
        ),
        "Professor" => (
            "Learn the implementation — teaching insights, not a review".into(),
            true,
            run(RunRequest::Professor),
        ),
        "Validate review" => {
            let comments = facts.comment_count;
            let description = match (facts.has_review, comments) {
                (true, n) if n > 0 => {
                    format!("Re-verify review findings and {n} GitHub comment(s) via {via}")
                }
                (true, _) => format!("Re-verify findings against the codebase via {via}"),
                (false, n) if n > 0 => {
                    format!("Re-anchor {n} unresolved GitHub comment(s) via {via}")
                }
                _ => "Run review or add GitHub comments first".into(),
            };
            let request = RunRequest::Validate {
                scope: "current".into(),
            };
            (description, facts.has_review || comments > 0, run(request))
        }
        "Answer questions" => (
            if facts.has_unanswered {
                format!("Answer unresolved questions via {via}")
            } else {
                "No unresolved questions".into()
            },
            facts.has_unanswered,
            Start::Rerun(Rerun::Questions),
        ),
        "Address notes" => (
            if facts.has_unaddressed {
                format!("Implement unresolved notes via {via}")
            } else {
                "No unresolved notes".into()
            },
            facts.has_unaddressed,
            Start::Rerun(Rerun::Notes),
        ),
        "Propose risk rules" => (
            "Agent reads the repo and writes its [risk] table".into(),
            true,
            run(RunRequest::Importance),
        ),
        _ => return None,
    })
}

/// The Specialized review picker: one row per expert lens.
pub fn expert_rows() -> Vec<Row> {
    er_engine::ai::EXPERTS
        .iter()
        .map(|expert| {
            let request = RunRequest::Expert {
                scope: "current".into(),
                expert_id: expert.id.to_string(),
            };
            let choice = Choice::Start(Start::Run(request));
            row(expert.label, expert.description.into(), true, Some(choice))
        })
        .collect()
}

const COMMANDS: [(&str, &str); 4] = [
    ("Run tests", "test"),
    ("Run linter", "lint"),
    ("Type check", "typecheck"),
    ("Security scan", "security"),
];

/// A named command's row; live once the config sets its line.
fn command_row(label: &str, name: String, line: Option<String>, unset: &str) -> Row {
    let enabled = line.is_some();
    let description = line.clone().unwrap_or_else(|| unset.to_string());
    let choice =
        line.map(|shell_cmd| Choice::Start(Start::Run(RunRequest::Command { name, shell_cmd })));
    row(label, description, enabled, choice)
}

/// The Verify hub as the App lists it: the packages first when any are
/// configured, then the global `[commands]`.
pub fn verify_rows(config: &ErConfig) -> Vec<Row> {
    let mut rows = Vec::new();
    if config.has_packages() {
        rows.push(header("── Packages ──"));
        for (id, package) in &config.packages.items {
            let count = package.command_count();
            let label = package.label.clone().unwrap_or_else(|| id.clone());
            rows.push(Row {
                hint: id.clone(),
                ..row(
                    &label,
                    format!("{count} command(s) configured"),
                    count > 0,
                    Some(Choice::Package(id.clone())),
                )
            });
        }
        rows.push(header("── Global ──"));
    }
    let unset = "set in [commands] in the config hub (,)";
    for (label, name) in COMMANDS {
        let line = config.resolve_command(name);
        rows.push(command_row(label, name.to_string(), line, unset));
    }
    rows
}

/// One package's commands and the hub's title; `None` for a package the
/// config no longer lists.
pub fn package_rows(config: &ErConfig, id: &str) -> Option<(String, Vec<Row>)> {
    let package = config.packages.items.get(id)?;
    let unset = "set in [packages] in the global config.toml";
    let rows = COMMANDS
        .iter()
        .map(|(label, name)| {
            let line = config.resolve_package_command(id, name);
            command_row(label, format!("{id}/{name}"), line, unset)
        })
        .collect();
    let label = package.label.as_deref().unwrap_or(id);
    Some((format!("VERIFY / {label}"), rows))
}

/// The row the App's Verify hubs select first: the first live row, else the
/// first that is no header.
pub fn first_selected(rows: &[Row]) -> usize {
    rows.iter()
        .position(|row| !row.header && row.enabled)
        .or_else(|| rows.iter().position(|row| !row.header))
        .unwrap_or(0)
}
