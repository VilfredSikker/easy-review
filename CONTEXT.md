# Context

Glossary for `er`. Terms only — no design decisions, no implementation. When a term
here conflicts with how code or a spec uses a word, this file wins and the code is
wrong.

## Review artifacts

**Finding** — A single claim about the code under review, anchored to a file and
usually a line. Produced by a reviewer, not by a human. A human's input is a
Question, Note, or Comment instead.

**Lens** — *Who* produced a finding: an expert (`security`, `performance`,
`reliability`, `testing`, `api`, `patterns`, `simplifying`, `mentorship`), or one of
the built-in producers (`general`, `professor`, `arbiter`). A finding merged from
several producers carries all of them.

**Raiser** — One of the lenses that produced a finding, in the plural. A finding
filed under one lens (`Finding.lens`) can have several raisers (`Finding.raised_by`)
when more than one producer found the same thing; the field is the set, the lens is
the one it is filed under.

**Category** — *What kind of defect* a finding describes (`correctness`, and
siblings). Independent of Lens: the security lens can raise a correctness finding.
Historically these two were collapsed into one field, which lost the category.

**Severity** — How bad the defect is if real. Scale: high / medium / low / info.

**Confidence** — How sure we are the finding is real. Scale: confirmed / tentative /
informational / dropped. Distinct from Severity: a confirmed typo and a tentative
data-loss bug are both meaningful, in opposite ways. Self-reported by whichever lens
raised the finding until an Arbiter regrades it.

**Verdict** — An Arbiter's ruling on a finding: kept, merged into another finding,
dropped, or escalated. A verdict is an opinion *about* a finding, not part of it.

## Reviewers

**Expert** — A reviewer with a single lens, run on demand. Experts are chosen before
a run; there is no concept of hiding an expert's output after the fact. If you do not
want a lens, do not run it.

**Arbiter** — A reviewer that judges other reviewers' findings rather than the code
alone: it merges duplicates, regrades confidence, and drops claims it cannot
substantiate. It reads the code the findings point at.

**Arena** — Several reviewers reviewing the same diff in successive rounds, each
round seeing the previous round's findings. A debate. Distinct from an Arbiter pass,
which is a single judgement over findings that already exist.

**Triage** — A fast first scan that decides what is worth reviewing and by which
lenses. It routes; it does not produce findings.

## Ranking

**Risk** — A reviewer's judgement of how dangerous a file's change is. An opinion,
available only after a review has run.

**Importance** — How much of the codebase depends on a file, independent of any
review that has run. Independent of Risk and frequently in disagreement with it; the
disagreement is informative. The article that prompted this vocabulary calls the same
idea *blast radius*.

## Human input

**Question** — Something the reviewer wants answered. Private, never pushed
anywhere.

**Note** — An instruction intended for a coding agent. Private, never pushed.

**Comment** — Feedback on a pull request, shared with whoever can see that pull
request.

**Checklist** — Outcomes a human confirms rather than code a human reads: the schema
change is reviewed, the tests cover the behaviour, the public surface is unchanged.

## Storage and freshness

**Sidecar** — A file holding review artifacts, kept outside the tracked tree. Each
producer owns its own sidecar and never writes another's.

**View bucket** — A diff a reviewer is looking at, as a unit of storage. One branch
can be reviewed as several diffs (the local branch, the pull request), and each keeps
its own artifacts.

**Stale** — Generated against a diff that has since changed. Granularity matters and
differs by artifact: a *file* is stale when its content moved since the review ran; a
*finding* or *comment* is stale when the specific lines it points at moved.
