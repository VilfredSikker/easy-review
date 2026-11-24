//! `annotations(tab)`: one tab's findings, threads, tour, checklist, triage and
//! diagrams, built from the AI section its workspace published. Thread order and every
//! count are decided here, so no front end re-sorts or recounts. The page joins the
//! in-flight ask-AI replies from `jobs`.

use std::collections::HashSet;

use er_engine::ai::{self, AiState, CommentRef, ErTour, Finding, RiskLevel};
use er_engine::file_rows::{finding_is_escalated, severity_str};
use er_engine::tab_identity::DiffMode;
use er_protocol as wire;

use crate::ai_extras::OtherBucketFacts;
use crate::content::TabContent;

/// The version `annotations(tab)` answers at: the newest revision of the sections it
/// reads. The pillar progress reads the reviewed marks and the tour's scope reads the
/// mode; a lazy parse moves neither.
pub fn annotations_version(content: &TabContent) -> u64 {
    content
        .ai
        .revision
        .max(content.reviewed.revision)
        .max(content.mode_revision)
}

/// `content` as `annotations(tab)` carries it.
pub fn annotations_resource(content: &TabContent) -> wire::Annotations {
    let ai = &content.ai.state;
    let shown_tour = ai.tour.as_ref();
    wire::Annotations {
        ai: ai_annotations(content),
        tour: tour_resource(content),
        other_bucket: content.ai.extras.other_bucket.as_ref().map(|facts| {
            let hashes = [
                content.context.branch_diff_hash.as_str(),
                content.context.diff_hash.as_str(),
            ];
            other_bucket(facts, shown_tour, &hashes)
        }),
        view: crate::reviewed_resource::content_view(content),
    }
}

fn count(n: usize) -> u64 {
    u64::try_from(n).unwrap_or(u64::MAX)
}

fn ai_annotations(content: &TabContent) -> wire::AiAnnotations {
    let ai = &content.ai.state;
    let branch_diff_hash = content.context.branch_diff_hash.as_str();
    let (active, resolved, dropped) = projected_findings(ai);
    let (mut high, mut med, mut low) = (0, 0, 0);
    for (_, finding) in &active {
        match finding.severity {
            RiskLevel::High => high += 1,
            RiskLevel::Medium => med += 1,
            RiskLevel::Low | RiskLevel::Info => low += 1,
        }
    }
    let comments = ai.github_comments.as_ref().map(|c| c.comments.as_slice());
    let roots_from = |source: &str| {
        comments.map_or(0, |all| {
            all.iter()
                .filter(|c| c.in_reply_to.is_none() && c.source == source)
                .count()
        })
    };
    let flat = |rows: &[(&str, &Finding)]| -> Vec<wire::Finding> {
        rows.iter()
            .map(|(path, f)| finding(content, path, f))
            .collect()
    };
    let findings = flat(&active);
    let resolved_findings = flat(&resolved);
    let dropped_findings = flat(&dropped);
    let file_risks = ai.review.as_ref().map(file_risks).unwrap_or_default();
    let triage = ai.triage.as_ref().map(|t| {
        let review = ai.review.as_ref();
        let mut triage = triage(t, branch_diff_hash);
        triage.review_update = triage_review_update(
            review.is_some_and(|r| r.diff_hash == branch_diff_hash),
            review.map_or("", |r| r.created_at.as_str()),
            &triage.priority_files,
            [&findings, &resolved_findings, &dropped_findings],
            &file_risks,
        );
        triage
    });
    wire::AiAnnotations {
        fresh: !ai.is_stale,
        stale_reason: stale_reason(ai, branch_diff_hash),
        summary_markdown: ai.summary.clone(),
        agent_summaries: ai.agent_summaries.clone().into_iter().collect(),
        high,
        med,
        low,
        local_comment_count: count(roots_from("local")),
        github_comment_count: count(roots_from("github")),
        comments: count(comments.map_or(0, <[_]>::len)),
        questions: count(ai.questions.as_ref().map_or(0, |q| q.questions.len())),
        notes: count(ai.notes.as_ref().map_or(0, |n| n.notes.len())),
        threads: threads(ai),
        findings,
        arbiter_dropped: count(ai.arbiter_effect.dropped),
        arbiter_merged: count(ai.arbiter_effect.merged),
        arbiter_unmatched: count(ai.arbiter_effect.unmatched),
        arbiter_regraded: count(ai.arbiter_effect.regraded),
        file_risks,
        resolved_findings,
        dropped_findings,
        min_trust_default: confidence(ai::min_trust_for(&ai.arbiter_effect)).to_string(),
        review_created_at: ai.review.as_ref().map(|r| r.created_at.clone()),
        review_diff_hash: ai.review.as_ref().map(|r| r.diff_hash.clone()),
        has_review_json: content.ai.extras.has_review_json,
        eligible_comment_count: count(
            ai.github_comments
                .as_ref()
                .map_or(0, ai::count_eligible_github_comments),
        ),
        triage,
        checklist: ai.checklist.as_ref().map(|c| wire::Checklist {
            fresh: c.diff_hash == branch_diff_hash,
            items: c
                .items
                .iter()
                .map(|i| wire::ChecklistItem {
                    id: i.id.clone(),
                    text: i.text.clone(),
                    category: i.category.clone(),
                    checked: i.checked,
                    related_findings: i.related_findings.clone(),
                    related_files: i.related_files.clone(),
                })
                .collect(),
        }),
        // A branch bucket's diagram hashes the branch diff, a PR bucket's the
        // PR diff: either makes it fresh.
        diagrams: ai
            .diagrams
            .iter()
            .map(|d| wire::Diagram {
                id: d.id.clone(),
                kind: d.kind.clone(),
                title: d.title.clone(),
                prompt: d.prompt.clone(),
                mermaid: d.mermaid.clone(),
                fresh: d.diff_hash == branch_diff_hash
                    || (!content.context.diff_hash.is_empty()
                        && d.diff_hash == content.context.diff_hash),
                created_at: d.created_at.clone(),
            })
            .collect(),
        diagram_presets: ai::diagram_presets()
            .into_iter()
            .map(|p| wire::DiagramPreset {
                kind: p.kind,
                label: p.label,
                description: p.description,
            })
            .collect(),
    }
}

fn stale_reason(ai: &AiState, branch_diff_hash: &str) -> Option<String> {
    if !ai.is_stale {
        return None;
    }
    if branch_diff_hash.is_empty() {
        return Some("Current diff hash is unavailable; refresh the diff.".to_string());
    }
    let differs = |hash: Option<&String>| hash.is_some_and(|h| h != branch_diff_hash);
    let reason = if differs(ai.review.as_ref().map(|r| &r.diff_hash)) {
        "Review was generated for an older diff. Re-run or validate the review."
    } else if differs(ai.order.as_ref().map(|o| &o.diff_hash))
        || differs(ai.checklist.as_ref().map(|c| &c.diff_hash))
    {
        "Review metadata is out of date for the current diff."
    } else {
        "Review artifacts do not match the current diff."
    };
    Some(reason.to_string())
}

type Rows<'a> = Vec<(&'a str, &'a Finding)>;

/// The findings to draw, the resolved and the arbiter-ruled-out, with duplicates collapsed.
fn projected_findings(ai: &AiState) -> (Rows<'_>, Rows<'_>, Rows<'_>) {
    let (mut active, mut resolved, mut dropped) = (Vec::new(), Vec::new(), Vec::new());
    if let Some(review) = &ai.review {
        for (path, file) in &review.files {
            for finding in &file.findings {
                let row = (path.as_str(), finding);
                if matches!(finding.confidence, ai::Confidence::Dropped)
                    && !finding_is_escalated(ai, path, finding)
                {
                    dropped.push(row);
                } else if finding.resolved {
                    resolved.push(row);
                } else {
                    active.push(row);
                }
            }
        }
    }
    let keys = &ai.arbiter_graded_keys;
    (
        ai::project_visible_findings(active, keys),
        ai::project_visible_findings(resolved, keys),
        ai::project_visible_findings(dropped, keys),
    )
}

fn confidence(c: ai::Confidence) -> &'static str {
    match c {
        ai::Confidence::Confirmed => "confirmed",
        ai::Confidence::Tentative => "tentative",
        ai::Confidence::Informational => "informational",
        ai::Confidence::Dropped => "dropped",
    }
}

fn finding(content: &TabContent, path: &str, f: &Finding) -> wire::Finding {
    let ai = &content.ai.state;
    let root_for = |finding_ref: Option<&str>, in_reply_to: Option<&String>, id: &str| {
        (finding_ref == Some(f.id.as_str()) && in_reply_to.is_none()).then(|| id.to_string())
    };
    let thread_id = ai
        .github_comments
        .iter()
        .flat_map(|gc| &gc.comments)
        .find_map(|c| root_for(c.finding_ref.as_deref(), c.in_reply_to.as_ref(), &c.id))
        .or_else(|| {
            ai.questions
                .iter()
                .flat_map(|qs| &qs.questions)
                .find_map(|q| root_for(q.finding_ref.as_deref(), q.in_reply_to.as_ref(), &q.id))
        });
    let arbiter = ai
        .arbiter_verdicts
        .get(&ai::finding_key(path, f.line_start, &f.title));
    wire::Finding {
        id: f.id.clone(),
        file: path.to_string(),
        line: f.line_start.map(count),
        hunk_index: f.hunk_index.map(count),
        severity: severity_str(f.severity).to_string(),
        confidence: confidence(f.confidence).to_string(),
        arbiter_verdict: arbiter.map(|(verdict, _)| {
            match verdict {
                ai::ArbiterRuling::Kept => "kept",
                ai::ArbiterRuling::Escalated => "escalated",
                ai::ArbiterRuling::Merged => "merged",
                ai::ArbiterRuling::Dropped => "dropped",
            }
            .to_string()
        }),
        arbiter_rationale: arbiter
            .and_then(|(_, rationale)| (!rationale.trim().is_empty()).then(|| rationale.clone())),
        lens_category: f.lens_category_tag(),
        resolved: f.resolved,
        expert_label: ai::expert_label_for_id(&f.lens).map(str::to_string),
        agent_label: ai::agent_label_for_id(&f.lens).to_string(),
        raised_by: f.named_raisers().iter().map(|s| s.to_string()).collect(),
        providers: f.providers.clone(),
        security_stage: f.is_security_stage(),
        title: f.title.clone(),
        message_markdown: f.description.clone(),
        promoted_to: content
            .ai
            .extras
            .promotions
            .get(&f.id)
            .cloned()
            .or_else(|| f.promoted_to.clone()),
        thread_id,
        responses: f
            .responses
            .iter()
            .map(|r| wire::FindingResponse {
                id: r.id.clone(),
                author: "AI".to_string(),
                kind: "ai".to_string(),
                timestamp: r.timestamp.clone(),
                body_markdown: r.text.clone(),
                origin: if ai::is_arbiter_ruling(r) {
                    "arbiter"
                } else {
                    "finding_response"
                }
                .to_string(),
                editable: false,
                deletable: true,
            })
            .collect(),
    }
}

/// Riskiest file first, then by path.
fn file_risks(review: &ai::ErReview) -> Vec<wire::FileRisk> {
    let rank = |risk: RiskLevel| match risk {
        RiskLevel::High => 0,
        RiskLevel::Medium => 1,
        RiskLevel::Low => 2,
        RiskLevel::Info => 3,
    };
    let mut entries: Vec<_> = review.files.iter().collect();
    entries
        .sort_by(|(pa, fa), (pb, fb)| rank(fa.risk).cmp(&rank(fb.risk)).then_with(|| pa.cmp(pb)));
    entries
        .into_iter()
        .map(|(path, file)| wire::FileRisk {
            path: path.clone(),
            risk: severity_str(file.risk).to_string(),
            risk_reason: file.risk_reason.clone(),
            summary: file.summary.clone(),
        })
        .collect()
}

/// The root threads: open questions, open notes, then every comment, in file order, replies oldest first.
fn threads(ai: &AiState) -> Vec<wire::Thread> {
    let questions = ai.questions.iter().flat_map(|q| &q.questions);
    let notes = ai.notes.iter().flat_map(|n| &n.notes);
    let open = |q: &&ai::PrivateAnnotation| q.in_reply_to.is_none() && !q.resolved;
    let private = questions
        .filter(open)
        .map(CommentRef::Question)
        .chain(notes.filter(open).map(CommentRef::Note));
    let comments = ai
        .github_comments
        .iter()
        .flat_map(|gc| &gc.comments)
        .filter(|c| c.in_reply_to.is_none())
        .map(CommentRef::GithubComment);
    private.chain(comments).map(|c| thread(ai, &c)).collect()
}

fn thread(ai: &AiState, c: &CommentRef<'_>) -> wire::Thread {
    let (kind, row) = match c {
        CommentRef::Question(q) => ("question", Root::private(q)),
        CommentRef::Note(n) => ("note", Root::private(n)),
        CommentRef::GithubComment(g) => {
            let you = g.author.is_empty() || g.author == "You";
            let row = Root {
                id: &g.id,
                file: &g.file,
                line_start: g.line_start,
                line_end: g.line_end,
                side: &g.side,
                source: g.source.clone(),
                stale: g.stale || g.outdated,
                resolved: g.resolved,
                author: &g.author,
                author_kind: if you { "you" } else { "human" },
                timestamp: &g.timestamp,
                text: &g.comment,
                promoted_to: None,
                github_id: g.github_id,
            };
            ("comment", row)
        }
    };
    wire::Thread {
        id: row.id.to_string(),
        kind: kind.to_string(),
        file: row.file.to_string(),
        line: count(row.line_start.unwrap_or(0)),
        line_end: row.line_end.map(count),
        side: row.side.to_string(),
        source: row.source,
        stale: row.stale,
        resolved: row.resolved,
        root: wire::ThreadMessage {
            id: row.id.to_string(),
            author: display_author(row.author),
            kind: row.author_kind.to_string(),
            timestamp: row.timestamp.to_string(),
            body_markdown: row.text.to_string(),
            origin: None,
            source: None,
            editable: None,
            deletable: None,
            ai_action: None,
        },
        replies: replies(ai, row.id),
        promoted_to: row.promoted_to,
        github_id: row.github_id,
        hunk_index: c.hunk_index().map(count),
        old_line: c.old_line_start().map(count),
    }
}

/// What a root thread shows, whichever sidecar holds it.
struct Root<'a> {
    id: &'a str,
    file: &'a str,
    line_start: Option<usize>,
    line_end: Option<usize>,
    side: &'a str,
    source: String,
    stale: bool,
    resolved: bool,
    author: &'a str,
    author_kind: &'static str,
    timestamp: &'a str,
    text: &'a str,
    promoted_to: Option<String>,
    github_id: Option<u64>,
}

impl<'a> Root<'a> {
    fn private(q: &'a ai::PrivateAnnotation) -> Self {
        Self {
            id: &q.id,
            file: &q.file,
            line_start: q.line_start,
            line_end: q.line_end,
            side: &q.side,
            source: "local".to_string(),
            stale: q.stale,
            resolved: q.resolved,
            author: &q.author,
            author_kind: "you",
            timestamp: &q.timestamp,
            text: &q.text,
            promoted_to: q.promoted_to.clone(),
            github_id: None,
        }
    }
}

/// An empty author is the local user.
fn display_author(raw: &str) -> String {
    if raw.is_empty() { "You" } else { raw }.to_string()
}

/// Every reply to `root`, from questions, notes and comments, oldest first.
fn replies(ai: &AiState, root: &str) -> Vec<wire::ThreadMessage> {
    let questions = ai.questions.iter().flat_map(|q| &q.questions);
    let notes = ai.notes.iter().flat_map(|n| &n.notes);
    let private = questions
        .map(|q| (q, "question"))
        .chain(notes.map(|n| (n, "note")))
        .filter(|(q, _)| q.in_reply_to.as_deref() == Some(root))
        .map(|(q, source)| {
            message(
                &q.id,
                &q.author,
                &q.timestamp,
                &q.text,
                source,
                q.ai_action.clone(),
            )
        });
    let comments = ai
        .github_comments
        .iter()
        .flat_map(|gc| &gc.comments)
        .filter(|c| c.in_reply_to.as_deref() == Some(root))
        .map(|c| message(&c.id, &c.author, &c.timestamp, &c.comment, &c.source, None));
    let mut replies: Vec<_> = private.chain(comments).collect();
    replies.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
    replies
}

fn message(
    id: &str,
    author: &str,
    timestamp: &str,
    body: &str,
    source: &str,
    ai_action: Option<String>,
) -> wire::ThreadMessage {
    let author = display_author(author);
    let kind = match author.as_str() {
        "You" => "you",
        "ai" => "ai",
        _ => "human",
    };
    wire::ThreadMessage {
        id: id.to_string(),
        kind: kind.to_string(),
        timestamp: timestamp.to_string(),
        body_markdown: body.to_string(),
        origin: Some("thread_reply".to_string()),
        source: Some(source.to_string()),
        editable: Some(kind == "you"),
        deletable: Some(true),
        ai_action,
        author,
    }
}

fn triage(t: &ai::TriageReview, branch_diff_hash: &str) -> wire::Triage {
    wire::Triage {
        fresh: ai::triage_is_fresh(t, branch_diff_hash),
        first_impression: t.first_impression.clone(),
        verdict_primary: ai::verdict_primary_str(&t.verdict.primary).to_string(),
        experts: t.verdict.experts.clone(),
        rationale: t.verdict.rationale.clone(),
        confidence: t.verdict.confidence.clone(),
        priority_files: t
            .priority_files
            .iter()
            .map(|pf| wire::TriagePriorityFile {
                path: pf.path.clone(),
                reason: pf.reason.clone(),
                risk: pf.risk.as_str().to_string(),
            })
            .collect(),
        files_changed: t.diff_stats.files_changed,
        approx_risk: t.diff_stats.approx_risk.as_str().to_string(),
        domains: t.diff_stats.domains.clone(),
        reach: t.reach.level.as_str().to_string(),
        reach_reason: t.reach.reason.clone(),
        touch_points: t.reach.touch_points.clone(),
        guard: t
            .reach
            .guard
            .as_ref()
            .filter(|g| g.is_evidenced())
            .map(|g| wire::TriageGuard {
                kind: g.kind.clone(),
                name: g.name.clone(),
                evidence: g.evidence.clone(),
            }),
        sensitive: t
            .sensitive
            .reasons
            .iter()
            .map(|r| wire::TriageSensitiveReason {
                kind: r.kind.clone(),
                evidence: r.evidence.clone(),
                note: r.note.clone(),
            })
            .collect(),
        created_at: t.created_at.clone(),
        diff_hash: t.diff_hash.clone(),
        review_update: None,
    }
}

fn rank(risk: &str) -> u8 {
    match risk {
        "high" => 0,
        "med" => 1,
        _ => 2,
    }
}

/// What a review of the same diff says about triage's call: its open findings and file
/// risks regrade triage's files and add missed ones. None unless it graded the same diff.
fn triage_review_update(
    review_fresh: bool,
    created_at: &str,
    triage_files: &[wire::TriagePriorityFile],
    [open, resolved, dropped]: [&[wire::Finding]; 3],
    file_risks: &[wire::FileRisk],
) -> Option<wire::TriageReviewUpdate> {
    let silent =
        file_risks.is_empty() && open.is_empty() && resolved.is_empty() && dropped.is_empty();
    if !review_fresh || silent {
        return None;
    }
    let at = |sev: &str| count(open.iter().filter(|f| f.severity == sev).count());
    let (high, med, low) = (at("high"), at("med"), at("low"));
    let review_risk = |path: &str| -> Option<&str> {
        let file = file_risks
            .iter()
            .find(|r| r.path == path)
            .map(|r| r.risk.as_str());
        let finding = open
            .iter()
            .filter(|f| f.file == path)
            .map(|f| f.severity.as_str())
            .min_by_key(|s| rank(s));
        match (file, finding) {
            (Some(a), Some(b)) => Some(if rank(b) < rank(a) { b } else { a }),
            (a, b) => a.or(b),
        }
    };
    let mut priority_files: Vec<wire::TriagePriorityFile> = triage_files
        .iter()
        .map(|pf| wire::TriagePriorityFile {
            risk: review_risk(&pf.path).unwrap_or(&pf.risk).to_string(),
            ..pf.clone()
        })
        .collect();
    for f in open.iter().filter(|f| f.severity != "low") {
        if !priority_files.iter().any(|pf| pf.path == f.file) {
            priority_files.push(wire::TriagePriorityFile {
                path: f.file.clone(),
                reason: f.title.clone(),
                risk: review_risk(&f.file).unwrap_or(&f.severity).to_string(),
            });
        }
    }
    priority_files.sort_by_key(|pf| rank(&pf.risk));
    let lowest_file_risk = || {
        file_risks
            .iter()
            .map(|r| r.risk.as_str())
            .min_by_key(|s| rank(s))
    };
    let (approx_risk, next) = match (high, med) {
        (1.., _) => ("high", "fix"),
        (_, 1..) => ("med", "check"),
        _ => (lowest_file_risk().unwrap_or("low"), "done"),
    };
    let mut reviewed_by: Vec<String> = Vec::new();
    for label in open
        .iter()
        .chain(resolved)
        .chain(dropped)
        .map(|f| &f.agent_label)
    {
        if !label.is_empty() && !reviewed_by.contains(label) {
            reviewed_by.push(label.clone());
        }
    }
    Some(wire::TriageReviewUpdate {
        approx_risk: approx_risk.to_string(),
        next: next.to_string(),
        high,
        med,
        low,
        reviewed_by,
        priority_files,
        created_at: created_at.to_string(),
    })
}

/// The tour for the view the tab shows. In the branch, tour and PR views
/// each file shows once, under the first pillar that names it, with only
/// the related files the diff holds, and the files no pillar names close it
/// as "Other changes"; the working-tree views show the pillars as written.
/// A front end that needs the tour alone reads it here, once per
/// [`annotations_version`].
pub fn tour_resource(content: &TabContent) -> wire::Tour {
    let ai = &content.ai.state;
    let scope = if content.tab.identity().tour_context_is_pr() {
        "pr"
    } else {
        "branch"
    };
    let Some(tour) = ai.tour.as_ref().filter(|t| !t.pillars.is_empty()) else {
        return wire::Tour {
            scope: scope.to_string(),
            ..wire::Tour::default()
        };
    };
    let marks = &content.reviewed.marks;
    let pillar =
        |id: &str, title: &str, description: &str, summary: &str, files: Vec<wire::TourFile>| {
            let paths = files
                .iter()
                .flat_map(|f| std::iter::once(&f.path).chain(f.related.iter().map(|r| &r.path)));
            let (reviewed, total) = paths.fold((0, 0), |(r, t), p| {
                (r + usize::from(marks.contains_key(p)), t + 1)
            });
            wire::TourPillar {
                id: id.to_string(),
                title: title.to_string(),
                description_markdown: description.to_string(),
                summary_markdown: summary.to_string(),
                importance: 0,
                foundation: false,
                files,
                reviewed_count: count(reviewed),
                total_count: count(total),
            }
        };
    let file = |f: &ai::TourFile, related: Vec<wire::TourRelatedFile>| wire::TourFile {
        path: f.path.clone(),
        reason: f.reason.clone(),
        finding_ids: f.finding_ids.clone(),
        related,
    };
    let mut pillars = Vec::new();
    let branch_scope = matches!(
        content.tab.mode,
        DiffMode::Branch | DiffMode::Tour | DiffMode::PrDiff
    );
    let in_diff: HashSet<&str> = content.diff.files.iter().map(|f| f.path.as_str()).collect();
    let mut seen: HashSet<String> = HashSet::new();
    for p in tour.ordered_pillars() {
        let mut files = Vec::new();
        for f in &p.files {
            if !branch_scope {
                files.push(file(f, Vec::new()));
                continue;
            }
            if !in_diff.contains(f.path.as_str()) || !seen.insert(f.path.clone()) {
                continue;
            }
            let mut related = Vec::new();
            for r in &f.related {
                if in_diff.contains(r.path.as_str()) && seen.insert(r.path.clone()) {
                    related.push(wire::TourRelatedFile {
                        path: r.path.clone(),
                        kind: r.kind.clone(),
                        reason: r.reason.clone(),
                    });
                }
            }
            files.push(file(f, related));
        }
        if branch_scope && files.is_empty() {
            continue;
        }
        pillars.push(wire::TourPillar {
            importance: p.importance,
            foundation: p.foundation,
            ..pillar(&p.id, &p.title, &p.description, &p.summary, files)
        });
    }
    if branch_scope {
        let other: Vec<wire::TourFile> = content
            .diff
            .files
            .iter()
            .filter(|f| !seen.contains(&f.path))
            .map(|f| wire::TourFile {
                path: f.path.clone(),
                reason: String::new(),
                finding_ids: Vec::new(),
                related: Vec::new(),
            })
            .collect();
        if !other.is_empty() {
            let text = "Files not assigned to a tour pillar.";
            pillars.push(pillar("__other__", "Other changes", text, text, other));
        }
    }
    wire::Tour {
        available: true,
        fresh: !ai.tour_stale,
        scope: scope.to_string(),
        title: tour.title.clone(),
        overview_markdown: tour.overview.clone(),
        pillars,
        created_at: tour.created_at.clone(),
        diff_hash: tour.diff_hash.clone(),
    }
}

/// What the other view's bucket holds, against the diff on screen.
fn other_bucket(
    other: &OtherBucketFacts,
    shown_tour: Option<&ErTour>,
    active_hashes: &[&str],
) -> wire::OtherBucket {
    let matches = |hash: &str| !hash.is_empty() && active_hashes.contains(&hash);
    // The tour on screen is that bucket's when it is the same file: the hash
    // alone could match a tour generated separately in each bucket.
    let tour_shared = match (&other.tour, shown_tour) {
        (Some((hash, created, _)), Some(shown)) => {
            *hash == shown.diff_hash && *created == shown.created_at
        }
        _ => false,
    };
    let same_diff = [&other.triage_hash, &other.review_hash]
        .into_iter()
        .flatten()
        .chain(other.tour.as_ref().map(|(hash, _, _)| hash))
        .any(|hash| matches(hash));
    wire::OtherBucket {
        view: other.view.to_string(),
        has_triage: other.triage_hash.is_some(),
        has_review: other.review_hash.is_some(),
        finding_count: count(other.finding_count),
        pillar_count: count(other.tour.as_ref().map_or(0, |(_, _, n)| *n)),
        tour_shared,
        same_diff,
    }
}

#[cfg(test)]
mod tests;
