# easy-review

`er` is a review tool for diffs, built for the case where an AI writes changes
faster than a person can read them. Review artifacts accumulate against a diff
and are addressed by the view that produced them.

This file is the vocabulary. It defines what things *are*. For why the system
is shaped this way, see [`docs/adr/`](./docs/adr/).

## Language

**View bucket**:
The partition of review artifacts belonging to one view of one branch. A single
tab can produce several, and artifacts from one bucket are not visible in
another.
_Avoid_: scope, namespace, section

**Local branch view**:
The view of a branch's own work — its diff against the base branch, its staged
and unstaged changes, its history.
_Avoid_: branch mode, working view

**PR diff view**:
The view of a pull request's head against its base. On a local PR tab this is a
different diff from the local branch view, even though both describe the same
branch.
_Avoid_: remote mode, PR mode

**Sidecar**:
A file holding review artifacts for one bucket. Sidecars are written next to
each other in managed storage and are read back by both the TUI and the desktop
app.
_Avoid_: state file, cache, metadata

**Diff mode**:
Which diff a tab is currently showing. Modes are not interchangeable views of
one diff — several describe genuinely different changes.
_Avoid_: view mode, screen

**Tab**:
One independent review target, with its own diff, selection, and artifacts.
_Avoid_: session, workspace, pane

**Finding**:
A remark an AI review produced about a specific place in the diff. Findings
belong to the AI that wrote them; a person reads them and acts, and does not
edit them.
_Avoid_: issue, comment, annotation

**Question**:
Something the reviewer wants answered. Private to the reviewer, and it never
reaches a pull request.
_Avoid_: query, TODO

**Note**:
An instruction the reviewer intends to hand to an agent. Private, like a
question, and distinct from it by intent rather than by storage.
_Avoid_: todo, task, action item

**GitHub comment**:
A review comment belonging to a pull request, which syncs with GitHub in both
directions. Distinct from a question or a note, which are private by
construction.
_Avoid_: PR comment, review comment, thread

**Promote**:
To turn a private question or note into a GitHub comment. The move is
deliberate, one-way, and always an explicit act by the reviewer.
_Avoid_: publish, share, export

**Reviewed**:
A file the reviewer has finished with. Reviewed files are hidden or dimmed, and
the mark is cleared automatically when that file's diff changes, so the mark
never outlives the content it was given to.
_Avoid_: approved, done, checked

**Stale**:
Said of an artifact whose diff has moved since it was produced. Two distinct
conditions share the word and are tracked separately: a *finding* is stale when
the diff it was generated against no longer matches, and a *comment* is stale
when the line it was anchored to can no longer be found.
_Avoid_: outdated, invalid, expired

**Tour**:
A guided walkthrough that reorders a diff into a narrative. A tour is bound to
the diff it was generated from, and is regenerated rather than edited when that
diff moves.
_Avoid_: guide, walkthrough, explainer

**Pillar**:
One section of a tour — a group of related changes presented together.
_Avoid_: section, chapter, group

**Arena**:
A review run in which several reviewers work the same diff independently and
their findings are then judged against each other. Chosen when a single pass is
not trusted enough to act on.
_Avoid_: panel, tribunal, multi-review

**Triage**:
A cheap first pass that classifies a diff and recommends where to look, without
reviewing it. Its output routes review effort; it is not itself a review.
_Avoid_: scan, summary, overview

**Hub**:
A modal list of actions or settings opened from a key. Distinct from an overlay
that displays information rather than offering choices.
_Avoid_: menu, dialog, palette

**Base hint**:
A notice that the branch's detected base differs from the base its pull request
targets. It informs; it does not switch anything.
_Avoid_: warning, mismatch, conflict

**Watched file**:
A file that git ignores and that the reviewer has asked to see anyway, tracked
against a saved baseline.
_Avoid_: untracked file, ignored file, extra file

**Agent slot**:
A permit to run one AI subprocess. Slots exist so a burst of review actions
cannot start more agent processes than the machine or the provider will
tolerate.
_Avoid_: worker, thread, lock

**AI Hub**:
The set of AI actions `er` offers over a diff. The hub builds the prompt and
runs the agent; it does not itself reason about the code.
_Avoid_: AI panel, copilot, assistant
