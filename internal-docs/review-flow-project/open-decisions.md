# Open decisions

Questions the project owner has to answer before or during the tasks in
[README.md](README.md). Each task file states its default and stops at the
point where the answer is needed; this list collects them so they can be
answered in one sitting. Record an answer by dating it here and in the task's
`## Comments`.

Tasks 02, 03, 04, 09, 11 and 13 have no question that blocks starting them.

## Cross-task (raised when the tasks were checked against each other)

- **Important / Nit labels.** Task 06 records the mapping (high and medium
  severity → Important; low and info → Nit; `pre_existing` → Pre-existing), but
  no task draws the labels in the UI. Should one (06 or 07), or do the existing
  severity colours stay?
- **Sensitive → People needed.** Should a flagged `sensitive` triage block (task
  10) set a floor of one person on the People needed label (task 12)? Today
  neither task does it.
- **"Fixed it" dismissals.** When a person dismisses a finding as "Fixed it"
  (task 03), does task 08's ledger mark it `Dismissed` (default: a return stays
  hidden) or `Fixed` (a return shows as recurring)?
- **Fix-run record.** Keep a durable record of each fix run (task 09) so task 19
  can count nits sent to a fixing agent? Without it that metric reads "not
  recorded".

## By task

- **01** — Approve Step 6: show an approval as stale when the PR head moves past
  the approved commit. In the TUI, look up the signed-in login, or show every
  reviewer's stale approval?
- **05** — Approve the ADR, and the precedence rule: REVIEW.md may narrow what a
  review reports, but cannot change caps, anchors or output format.
- **06** — Approve the ADR and the Pre-existing caps (3 for the general review, 1
  per expert lens, 0 for triage and professor).
- **07** — Approve the design note, the ADR, and the terms "Review flow" and
  "Depth". Also:
  - Deep and ADR 0024's explicit accept step. Recommended: keep the accept.
  - Quick verified by the seeded arbiter or by Validate. Recommended: arbiter
    (choosing Validate blocks task 10 Step 4).
  - Feed general-review findings into the seeded arbiter, or have Standard run
    no general review. Recommended: feed them in.
  - Does a "professor" triage verdict map to Quick?
  - Keep er-mcp out of the flow (it spawns no agents)? Recommended: yes.
- **08** — Approve the ADR: `rounds.json`, the no-repeat key without a line
  number, and the changes-only merge. Also:
  - After three failed rounds, does a full re-run by hand start at once
    (default) or ask first?
  - Round count per view bucket (default) or shared across a PR's two views?
  - Does a full re-run by hand clear the dismissed ledger? Default: no.
  - Does the changes-only appendix lower the Pre-existing cap on later rounds?
    Default: no.
- **10** — Group security findings by lens (default) or by category.
- **12** — No gate. The strict trusted-author default (no list means nobody is
  trusted) and the Step 4 table are fixed; say so if a row looks wrong.
- **14** — Approve the ADR and the exception to "nothing runs on its own". Also:
  - The settle rule (per-file diff hash steady for N seconds) and its defaults
    (N = 90 s, a 20 s quiet window, at most 6 runs per hour per tab).
  - `live.json` storage, and reading across view buckets as an exception to ADR
    0004.
  - Does a live run get a verify pass? Recommended: no.
- **15** — Approve the design note: where the cards live, their relation to the
  Checklist, states and colour tokens, what each card shows, TUI keys, the
  evidence-first approve flow, the term "Decision point", and which
  directed-by sources ship first.
- **16** — Approve the ADR and the terms "Lesson" and "Subject": storage under
  `repos/<slug>/lessons/` outside view buckets, staleness by per-path git object
  id, a read-only agent with the host writing the file, and question-only
  Lessons that are never marked stale.
- **17** — Approve the design note's ten answers, including the repeat
  thresholds (at least 3 records across at least 2 diff hashes, two-thirds
  majority), storage in `source-fixes.json`, and the term "Source fix".
- **18** — Approve the ADR and the terms "Sweep", the amended "Finding", and
  `sweep` as a lens. Scheduling: Option A (er-mcp prepare and upload,
  recommended) or Option B (an `er sweep` command).
- **19** — Approve the ADR, the terms (Eval case, Eval run, Recall, False alarm)
  and the compliance-doc line. Human-time metrics: keep them cut (recommended
  for now), time-to-merge from `gh`, or local time-in-review.
