/// Embedded prompt templates for guided AI actions.
///
/// These replicate the logic from the external Claude Code skills
/// (`/er-review`, `/er-questions`) so the TUI can invoke them directly
/// via the configured agent command without requiring skill files.
/// Returns true if the value contains only characters safe for shell interpolation.
/// Allows alphanumeric, dots, underscores, hyphens, colons, slashes, and @.
#[cfg(test)]
fn is_safe_shell_value(s: &str) -> bool {
    s.chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | ':' | '/' | '@'))
}

/// Wraps a value in single quotes and escapes any embedded single quotes.
/// This makes the value safe to embed inside a shell command string.
pub fn sanitize_for_shell(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Build the shell command that annotates a raw diff file with file line numbers.
///
/// Reads `input` (raw `git diff` / `gh pr diff` output) and writes `output` where each
/// content line is prefixed with `[h<hunk> L<file_line>]`. `hunk` is the 0-based hunk
/// index within the file; `file_line` is the new-side line number for `+`/context, and
/// `L-<old>` for deleted lines.
///
/// The model reads the annotated file and copies these tags directly into `Finding.line_start`
/// and `Finding.hunk_index`, eliminating the line-counting errors the prompt-side computation
/// caused. The hash is still computed on the raw `input` so it matches what `er` itself sees.
///
/// POSIX-compatible awk; works on macOS BSD awk and gawk alike. Paths are shell-quoted
/// inside the helper, so callers pass raw paths.
fn annotate_diff_command(input: &str, output: &str) -> String {
    let safe_input = sanitize_for_shell(input);
    let safe_output = sanitize_for_shell(output);
    // Single-quoted awk script. Inside single quotes, no shell escaping is needed.
    // Braces are escaped {{ }} for format!.
    format!(
        "awk 'BEGIN{{h=-1}} \
/^diff --git/{{h=-1;print;next}} \
/^\\+\\+\\+/{{print;next}} \
/^---/{{print;next}} \
/^@@ /{{h++;s=$0;sub(/^@@ -[0-9]+(,[0-9]+)? \\+/,\"\",s);sub(/[, ].*/,\"\",s);n=s-1;t=$0;sub(/^@@ -/,\"\",t);sub(/[, ].*/,\"\",t);o=t-1;print \"[h\" h \"] \" $0;next}} \
/^\\+/{{n++;printf \"[h%d L%d] %s\\n\",h,n,$0;next}} \
/^-/{{o++;printf \"[h%d L-%d] %s\\n\",h,o,$0;next}} \
/^ /{{o++;n++;printf \"[h%d L%d] %s\\n\",h,n,$0;next}} \
{{print}}' {safe_input} > {safe_output}"
    )
}

/// Build the review prompt with repo context substituted.
///
/// The prompt instructs the agent to:
/// 1. Read the diff via `git diff`
/// 2. Analyse all files
/// 3. Write `.er/review.json`, `.er/order.json`, `.er/checklist.json`, `.er/summary.md`
pub fn build_review_prompt(base_branch: &str, scope: &str) -> String {
    let safe_base_branch = sanitize_for_shell(base_branch);
    let safe_base_branch = safe_base_branch.replace('{', "{{").replace('}', "}}");
    let base_branch = base_branch.replace('{', "{{").replace('}', "}}");
    let diff_args = match scope {
        "unstaged" => "--unified=20 --no-color --no-ext-diff".to_string(),
        "staged" => "--staged --unified=20 --no-color --no-ext-diff".to_string(),
        _ => format!("{safe_base_branch} --unified=20 --no-color --no-ext-diff"),
    };
    let annotate = annotate_diff_command(".er/diff-tmp", ".er/diff-annotated");

    format!(
        r#"You are a code reviewer. Perform a thorough review of the current git diff and write results to the `.er/` directory.

## Instructions

1. Run: `git diff {diff_args} > .er/diff-tmp && (sha256sum .er/diff-tmp 2>/dev/null || shasum -a 256 .er/diff-tmp)`
   - Save the SHA-256 hash as `diff_hash`
2. Annotate the diff with file line numbers: `{annotate}`
3. Read `.er/diff-annotated` to get the diff with `[h<hunk> L<file_line>]` tags on every line. The diff includes 20 lines of context around each change so you can see surrounding functions, imports, and helpers.
4. Analyse every changed file. **Findings target only `+` or `-` lines** — the wider context is for comprehension. For each file determine:
   - `risk`: "high" | "medium" | "low" | "info"
   - `risk_reason`: why this risk level
   - `summary`: one-line description of changes
   - `findings`: array of issues found (max 3-4 per file)
5. **Verify findings agentically.** When a finding's significance depends on something not visible in the diff, read or grep:
   - "Does this new function follow the established pattern?" → read 1–2 sibling files in the same directory
   - "Will this break callers?" → grep for the symbol; read the top 2–3 hits
   - "Is this covered by tests?" → look for `tests/test_<name>.py` (or analogue)
   - Append each read/grep finding as an `EvidenceItem` on the relevant `Finding`
   - Budget: ~5 file reads per finding, ~30 reads total. If you run out, mark the finding `tentative` with a `verification_plan` describing what remains.
6. Set `confidence` on every finding:
   - `confirmed` — evidence supports the finding; reviewer should act
   - `informational` — real but pre-existing or low-impact relative to this PR
   - `tentative` — couldn't verify within budget; `verification_plan` is required
7. Write these four files:

### `.er/review.json`
```json
{{
  "version": 1,
  "diff_hash": "<sha256 from step 1>",
  "diff_scope": "{scope}",
  "created_at": "<ISO 8601>",
  "base_branch": "{base_branch}",
  "head_branch": "<current branch>",
  "file_hashes": {{}},
  "files": {{
    "path/to/file.rs": {{
      "risk": "medium",
      "risk_reason": "Modifies error handling logic",
      "summary": "Adds retry mechanism for network calls",
      "findings": [
        {{
          "id": "f-1",
          "severity": "medium",
          "category": "correctness",
          "title": "Short title (max 60 chars)",
          "description": "What the issue is and why it matters",
          "hunk_index": 0,
          "line_start": 42,
          "suggestion": "What to do about it",
          "related_files": [],
          "outside_diff": false,
          "confidence": "confirmed",
          "verification_plan": "",
          "evidence": [
            {{
              "file": "src/api.py",
              "line_start": 110,
              "line_end": 118,
              "note": "All three callers pass user-controlled input directly."
            }}
          ],
          "responses": [],
          "resolved": false,
          "resolved_note": "",
          "resolved_at": ""
        }}
      ]
    }}
  }}
}}
```

`resolved` / `resolved_note` / `resolved_at` are populated by the `validate` pass — leave them at defaults during review.

### `.er/order.json`
```json
{{
  "version": 1,
  "diff_hash": "<sha256>",
  "order": [
    {{"path": "src/file.rs", "reason": "Core change", "group": "main"}}
  ],
  "groups": {{
    "main": {{"label": "Main Changes", "color": "red"}}
  }}
}}
```

### `.er/checklist.json`
```json
{{
  "version": 1,
  "diff_hash": "<sha256>",
  "items": [
    {{
      "id": "c-1",
      "text": "Verify error handling covers all edge cases",
      "category": "correctness",
      "checked": false,
      "related_findings": ["f-1"],
      "related_files": ["src/file.rs"]
    }}
  ]
}}
```

### `.er/summary.md`
A 3-5 paragraph markdown summary of the overall changes.

## Guidelines

- Be specific. "Check error handling" is bad. "Handle the None case in parse_token() at line 42" is good.
- `hunk_index`: copy the number from the `[h<N>]` tag in `.er/diff-annotated`. It is 0-based per file (first `@@` in a file = 0, second = 1). Never compute it yourself.
- `line_start`: copy the number from the `[h<N> L<M>]` tag on the relevant `+` or context line in `.er/diff-annotated`. **Never infer, count, or compute line numbers** — copy the tag verbatim from the line you are pointing at. If the line you want to reference has no `L<M>` tag (e.g. it is a deleted line tagged `L-<M>`), pick the nearest `+` or context line instead.
- **Findings MUST anchor to a `+` or `-` line.** The 20 lines of surrounding context exist so you can understand the change — not so you can find issues in unchanged code. Set `outside_diff: false` for review-pass findings; if an issue lives entirely in unchanged code, drop it (the `validate` pass can re-add it as informational later).
- Findings MUST point to a line tagged `[h<N> L<M>]` in `.er/diff-annotated`. If you cannot anchor a finding to any tagged line, drop it rather than guessing.
- `confidence` and `evidence`: see step 6. Use `confirmed` when you read enough to verify it, `informational` when the issue is real but minor or pre-existing, `tentative` when you ran out of budget. `evidence` should cite the actual files/ranges you read.
- `verification_plan`: required for `tentative`; one line saying what would resolve it (e.g. "grep `parse_token` and check whether any callers pass user input directly").
- Risk levels: high = likely bug or security issue, medium = code smell or missing edge case, low = style, info = observation.
- Keep finding titles under 60 characters.
- The `suggestion` field should be actionable.
- Max 3-4 findings per file, max 15 total.
- The checklist should be things the reviewer should manually verify.
- Categories: security, logic, performance, correctness, error-handling, style, testing.
- Ensure `.er/` directory exists before writing: `mkdir -p .er`

## Speed

Target: complete in under 3 minutes. Agentic verification adds time vs. the old diff-only flow; that's the trade for higher-confidence findings. Short-circuit when findings are obvious — you don't need to grep for a typo or an unambiguous null deref.
Reading source files IS allowed and expected (within the read budget above). Use it to verify, not to expand scope."#
    )
}

/// Build the questions-answering prompt.
///
/// The prompt instructs the agent to:
/// 1. Read `.er/questions.json`
/// 2. Read the diff
/// 3. Answer each unresolved question
/// 4. Write updated `.er/questions.json`
pub fn build_questions_prompt(base_branch: &str, scope: &str) -> String {
    let safe_base_branch = sanitize_for_shell(base_branch);
    let safe_base_branch = safe_base_branch.replace('{', "{{").replace('}', "}}");
    let diff_args = match scope {
        "unstaged" => "--unified=3 --no-color --no-ext-diff".to_string(),
        "staged" => "--staged --unified=3 --no-color --no-ext-diff".to_string(),
        _ => format!("{safe_base_branch} --unified=3 --no-color --no-ext-diff"),
    };
    let annotate = annotate_diff_command(".er/diff-tmp", ".er/diff-annotated");

    format!(
        r#"You are answering code review questions. Read the questions file and the diff, then provide answers.

## Instructions

1. Read `.er/questions.json`
   - If it doesn't exist or has no unresolved questions: print "No questions to answer" and stop.
2. Run: `git diff {diff_args} > .er/diff-tmp`
3. Annotate with file line numbers: `{annotate}`
4. Read `.er/diff-annotated` — each content line carries `[h<hunk> L<file_line>]` tags matching `Question.hunk_index` and `Question.line_start`
5. For each question where `resolved == false` and no existing reply (no entry with `in_reply_to` == that question's `id`):
   a. Locate the relevant code in the diff (using `file`, `hunk_index`, `line_start`)
   b. Write a thoughtful answer as a NEW entry appended to the `questions` array:
      ```json
      {{
        "id": "a-<timestamp>-<seq>",
        "timestamp": "<ISO 8601>",
        "file": "<same as question>",
        "hunk_index": <same as question>,
        "line_start": <same as question>,
        "line_content": "<same as question>",
        "text": "<your answer referencing actual code from the diff>",
        "resolved": false,
        "in_reply_to": "<question.id>",
        "author": "Claude"
      }}
      ```
   c. Set the original question's `resolved` field to `true`
6. Write the updated `.er/questions.json`
7. Back up: `cp .er/questions.json .er/questions.prev.json`

## Answer Quality

- Actually read the code the human is asking about. Don't give generic answers.
- If they ask "why?", explain with specifics from the diff.
- Keep responses concise — they render in a TUI with limited width.
- Never be defensive. If the code is fine, say so.

## Speed

Target: complete in under 60 seconds. Read the diff once, answer all questions in-context."#
    )
}

/// Build the validate prompt — re-runs verification on existing findings.
///
/// Reads `.er/review.json` + `.er/diff-annotated`, follows each finding's `verification_plan`,
/// and rewrites `confidence` + `evidence` in place. Used to:
/// - Resolve `tentative` findings the review pass couldn't verify in budget.
/// - Re-validate after the diff or codebase has changed.
/// - Optionally surface `informational` issues in unchanged code that review skipped.
pub fn build_validate_prompt(base_branch: &str, scope: &str) -> String {
    let safe_base_branch = sanitize_for_shell(base_branch);
    let safe_base_branch = safe_base_branch.replace('{', "{{").replace('}', "}}");
    let diff_args = match scope {
        "unstaged" => "--unified=20 --no-color --no-ext-diff".to_string(),
        "staged" => "--staged --unified=20 --no-color --no-ext-diff".to_string(),
        _ => format!("{safe_base_branch} --unified=20 --no-color --no-ext-diff"),
    };
    let annotate = annotate_diff_command(".er/diff-tmp", ".er/diff-annotated");

    format!(
        r#"You are validating an existing code review. For every finding, do two things:
(1) check whether the user has FIXED it since review ran, and (2) for any still-tentative
findings, execute their `verification_plan`. Update the finding fields in place.

## Instructions

1. Read `.er/review.json`. If it does not exist, print "No review to validate" and stop.
2. Refresh the annotated diff so line numbers stay current:
   - `git diff {diff_args} > .er/diff-tmp`
   - `{annotate}`
3. **Resolution check (run for every finding, any confidence):**
   For each finding, read the current state of `<file>:<line_start>` (and a few lines around it).
   Compare to the original concern in `description` / `suggestion`.
   - If the issue is no longer present (line deleted, refactored, null-check added, etc.):
     - Set `resolved: true`
     - Write a one-line `resolved_note` saying what changed (e.g. "added null check on line 42",
       "function deleted", "replaced with helper that handles the case").
     - Stamp `resolved_at` with the current ISO 8601 UTC timestamp.
     - Do NOT change `confidence` — keep the audit trail of what review thought.
   - If the issue is still present, leave `resolved: false`.
   - **Never delete findings.** Resolved ones stay in the JSON for history.
   - Budget for this step: 1 file read per finding.
4. **Tentative promotion (run only when `confidence == "tentative"` or empty AND `resolved == false`):**
   Execute the finding's `verification_plan`:
      - Read the named files / grep the named symbols
      - Inspect callers, sibling implementations, or tests as instructed
   Decide the new `confidence`:
      - `confirmed` — evidence supports the finding; reviewer should act
      - `informational` — real but pre-existing or low-impact relative to this PR
      - `dropped` — the verification disproved the concern (keep the finding for the audit trail)
   Append `EvidenceItem`s to `evidence` with a one-line `note` per item.
   If `confidence` was already `confirmed` / `informational` / `dropped`, leave it alone unless new
   evidence flips the verdict.
5. (Optional) If while reading callers/siblings you spot a real issue in unchanged code that
   the review pass deliberately skipped, you MAY add a new finding with `outside_diff: true`
   and `confidence: "informational"`. Anchor it to a context line tagged in `.er/diff-annotated`
   (no inventing line numbers). Cap: max 3 such additions.
6. Write the updated `.er/review.json` back. Preserve `diff_hash`, `version`, `created_at`,
   `base_branch`, `head_branch`, and every untouched field on every finding.

## Budget

- 1 file read per finding for the resolution check (step 3).
- ~5 file reads per finding being verified in step 4, ~50 reads total across the review.
- If you exceed budget, leave remaining tentative findings as `tentative` and note that in the
  `verification_plan` (e.g. "needs deeper trace through middleware/auth.py — re-run validate").

## Speed

Target: complete in under 5 minutes. Validate is opt-in and earns its keep with thorough
verification, so don't rush — but don't read the entire codebase either. Each evidence read
should map to a specific finding."#
    )
}

/// Build the summary-only prompt with repo context substituted.
pub fn build_summary_prompt(base_branch: &str, scope: &str) -> String {
    let safe_base_branch = sanitize_for_shell(base_branch);
    let safe_base_branch = safe_base_branch.replace('{', "{{").replace('}', "}}");
    let diff_args = match scope {
        "unstaged" => "--unified=3 --no-color --no-ext-diff".to_string(),
        "staged" => "--staged --unified=3 --no-color --no-ext-diff".to_string(),
        _ => format!("{safe_base_branch} --unified=3 --no-color --no-ext-diff"),
    };

    format!(
        r#"Summarize the current git diff and write the result to `.er/summary.md`.

## Instructions

1. Ensure `.er/` exists: `mkdir -p .er`
2. Run: `git diff {diff_args} > .er/diff-tmp`
3. Read `.er/diff-tmp`
4. Write `.er/summary.md` as 3-5 short markdown paragraphs covering:
   - what changed
   - the most important files or subsystems touched
   - the main implementation risks or things a reviewer should pay attention to

## Guidelines

- Be concrete and reference actual changes from the diff.
- Focus on behavior and review relevance, not commit-style fluff.
- Do not write any other files.
- Do NOT read individual source files — the diff contains everything needed."#
    )
}

/// Build review prompt for remote mode (uses gh pr diff instead of git diff).
///
/// The prompt instructs the agent to:
/// 1. Fetch the PR diff via `gh pr diff`
/// 2. Analyse all files
/// 3. Write review artifacts to `output_dir/`
pub fn build_review_prompt_remote(
    owner: &str,
    repo: &str,
    pr_number: u64,
    output_dir: &str,
) -> String {
    let safe_owner = sanitize_for_shell(owner)
        .replace('{', "{{")
        .replace('}', "}}");
    let safe_repo = sanitize_for_shell(repo)
        .replace('{', "{{")
        .replace('}', "}}");
    let safe_output_dir = sanitize_for_shell(output_dir)
        .replace('{', "{{")
        .replace('}', "}}");
    // pr_number is u64 — inherently numeric, no sanitization needed
    let annotate = annotate_diff_command(
        &format!("{output_dir}/diff-tmp"),
        &format!("{output_dir}/diff-annotated"),
    );
    format!(
        r#"You are a code reviewer. Perform a thorough review of the GitHub PR diff and write results to `{safe_output_dir}/`.

## Instructions

1. Run: `gh pr diff {pr_number} --repo {safe_owner}/{safe_repo} > {safe_output_dir}/diff-tmp && (sha256sum {safe_output_dir}/diff-tmp 2>/dev/null || shasum -a 256 {safe_output_dir}/diff-tmp)`
   - Save the SHA-256 hash as `diff_hash`
2. Annotate the diff with file line numbers: `{annotate}`
3. Read `{safe_output_dir}/diff-annotated` — each content line carries `[h<hunk> L<file_line>]` tags. (Note: `gh pr diff` returns ~3 lines of context — agentic verification matters more here than in local review.)
4. Analyse every changed file. **Findings target only `+` or `-` lines** — surrounding context is for comprehension. For each file determine:
   - `risk`: "high" | "medium" | "low" | "info"
   - `risk_reason`: why this risk level
   - `summary`: one-line description of changes
   - `findings`: array of issues found (max 3-4 per file)
5. **Verify findings agentically.** When a finding's significance depends on something not in the diff:
   - If the PR branch is checked out locally, read sibling files / grep callers / inspect tests directly.
   - If not, attempt `gh pr checkout {pr_number}` once, then proceed. If that fails, mark the finding `tentative` with a `verification_plan` describing what would resolve it.
   - Cap: ~5 file reads per finding, ~30 reads total.
6. Set `confidence` on every finding:
   - `confirmed` — evidence supports the finding
   - `informational` — real but pre-existing or low-impact
   - `tentative` — couldn't verify within budget; `verification_plan` is required
7. Write these four files:

### `{safe_output_dir}/review.json`
```json
{{
  "version": 1,
  "diff_hash": "<sha256 from step 1>",
  "diff_scope": "branch",
  "created_at": "<ISO 8601>",
  "base_branch": "",
  "head_branch": "",
  "file_hashes": {{}},
  "files": {{
    "path/to/file.rs": {{
      "risk": "medium",
      "risk_reason": "Modifies error handling logic",
      "summary": "Adds retry mechanism for network calls",
      "findings": [
        {{
          "id": "f-1",
          "severity": "medium",
          "category": "correctness",
          "title": "Short title (max 60 chars)",
          "description": "What the issue is and why it matters",
          "hunk_index": 0,
          "line_start": 42,
          "suggestion": "What to do about it",
          "related_files": [],
          "outside_diff": false,
          "confidence": "confirmed",
          "verification_plan": "",
          "evidence": [
            {{
              "file": "src/api.py",
              "line_start": 110,
              "line_end": 118,
              "note": "All three callers pass user-controlled input directly."
            }}
          ],
          "responses": [],
          "resolved": false,
          "resolved_note": "",
          "resolved_at": ""
        }}
      ]
    }}
  }}
}}
```

### `{safe_output_dir}/order.json`
```json
{{
  "version": 1,
  "diff_hash": "<sha256>",
  "order": [
    {{"path": "src/file.rs", "reason": "Core change", "group": "main"}}
  ],
  "groups": {{
    "main": {{"label": "Main Changes", "color": "red"}}
  }}
}}
```

### `{safe_output_dir}/checklist.json`
```json
{{
  "version": 1,
  "diff_hash": "<sha256>",
  "items": [
    {{
      "id": "c-1",
      "text": "Verify error handling covers all edge cases",
      "category": "correctness",
      "checked": false,
      "related_findings": ["f-1"],
      "related_files": ["src/file.rs"]
    }}
  ]
}}
```

### `{safe_output_dir}/summary.md`
A 3-5 paragraph markdown summary of the overall changes.

## Guidelines

- Be specific. "Check error handling" is bad. "Handle the None case in parse_token() at line 42" is good.
- `hunk_index`: copy the number from the `[h<N>]` tag in the annotated diff. Per file, 0-based. Never compute it.
- `line_start`: copy the number from the `[h<N> L<M>]` tag on the relevant `+` or `-` line in the annotated diff. **Never infer or count lines yourself** — always copy a tag verbatim.
- **Findings MUST anchor to a `+` or `-` line.** Set `outside_diff: false` for review-pass findings; if an issue lives entirely in unchanged code, drop it (validate can re-add it as informational).
- If no `[h<N> L<M>]` tag exists for a finding, drop the finding rather than guessing.
- `confidence` and `evidence`: see step 6. Cite the actual files/ranges you read in `evidence`.
- `verification_plan`: required for `tentative`; one line describing what would resolve it.
- Risk levels: high = likely bug or security issue, medium = code smell or missing edge case, low = style, info = observation.
- Keep finding titles under 60 characters.
- The `suggestion` field should be actionable.
- Max 3-4 findings per file, max 15 total.
- The checklist should be things the reviewer should manually verify.
- Categories: security, logic, performance, correctness, error-handling, style, testing.
- Ensure `{safe_output_dir}/` directory exists before writing: `mkdir -p {safe_output_dir}`

## Speed

Target: complete in under 3 minutes. Agentic verification adds time but produces higher-confidence findings. Short-circuit when findings are obvious.
Reading source files IS allowed and expected (within the read budget above) once the PR is checked out."#
    )
}

/// Build the summary-only prompt for remote mode (uses gh pr diff instead of git diff).
pub fn build_summary_prompt_remote(
    owner: &str,
    repo: &str,
    pr_number: u64,
    output_dir: &str,
) -> String {
    let safe_owner = sanitize_for_shell(owner)
        .replace('{', "{{")
        .replace('}', "}}");
    let safe_repo = sanitize_for_shell(repo)
        .replace('{', "{{")
        .replace('}', "}}");
    let safe_output_dir = sanitize_for_shell(output_dir)
        .replace('{', "{{")
        .replace('}', "}}");

    format!(
        r#"Summarize the current GitHub PR diff and write the result to `{safe_output_dir}/summary.md`.

## Instructions

1. Ensure the output dir exists: `mkdir -p {safe_output_dir}`
2. Run: `gh pr diff {pr_number} --repo {safe_owner}/{safe_repo} > {safe_output_dir}/diff-tmp`
3. Read `{safe_output_dir}/diff-tmp`
4. Write `{safe_output_dir}/summary.md` as 3-5 short markdown paragraphs covering:
   - what changed
   - the most important files or subsystems touched
   - the main implementation risks or review hotspots

## Guidelines

- Be concrete and grounded in the diff.
- Do not write any other files.
- Do NOT read individual source files — the diff contains everything needed."#
    )
}

/// Build questions-answering prompt for remote mode (uses gh pr diff instead of git diff).
///
/// The prompt instructs the agent to:
/// 1. Read `output_dir/questions.json`
/// 2. Fetch the PR diff via `gh pr diff`
/// 3. Answer each unresolved question
/// 4. Write updated `output_dir/questions.json`
pub fn build_questions_prompt_remote(
    owner: &str,
    repo: &str,
    pr_number: u64,
    output_dir: &str,
) -> String {
    let safe_owner = sanitize_for_shell(owner)
        .replace('{', "{{")
        .replace('}', "}}");
    let safe_repo = sanitize_for_shell(repo)
        .replace('{', "{{")
        .replace('}', "}}");
    let safe_output_dir = sanitize_for_shell(output_dir)
        .replace('{', "{{")
        .replace('}', "}}");
    // pr_number is u64 — inherently numeric, no sanitization needed
    let annotate = annotate_diff_command(
        &format!("{output_dir}/diff-tmp"),
        &format!("{output_dir}/diff-annotated"),
    );
    format!(
        r#"You are answering code review questions. Read the questions file and the PR diff, then provide answers.

## Instructions

1. Read `{safe_output_dir}/questions.json`
   - If it doesn't exist or has no unresolved questions: print "No questions to answer" and stop.
2. Run: `gh pr diff {pr_number} --repo {safe_owner}/{safe_repo} > {safe_output_dir}/diff-tmp`
3. Annotate with file line numbers: `{annotate}`
4. Read `{safe_output_dir}/diff-annotated` — each content line carries `[h<hunk> L<file_line>]` tags matching `Question.hunk_index` and `Question.line_start`
5. For each question where `resolved == false` and no existing reply (no entry with `in_reply_to` == that question's `id`):
   a. Locate the relevant code in the diff (using `file`, `hunk_index`, `line_start`)
   b. Write a thoughtful answer as a NEW entry appended to the `questions` array:
      ```json
      {{
        "id": "a-<timestamp>-<seq>",
        "timestamp": "<ISO 8601>",
        "file": "<same as question>",
        "hunk_index": <same as question>,
        "line_start": <same as question>,
        "line_content": "<same as question>",
        "text": "<your answer referencing actual code from the diff>",
        "resolved": false,
        "in_reply_to": "<question.id>",
        "author": "Claude"
      }}
      ```
   c. Set the original question's `resolved` field to `true`
6. Write the updated `{safe_output_dir}/questions.json`
7. Back up: `cp {safe_output_dir}/questions.json {safe_output_dir}/questions.prev.json`

## Answer Quality

- Actually read the code the human is asking about. Don't give generic answers.
- If they ask "why?", explain with specifics from the diff.
- Keep responses concise — they render in a TUI with limited width.
- Never be defensive. If the code is fine, say so.

## Speed

Target: complete in under 60 seconds. Read the diff once, answer all questions in-context."#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── sanitize_for_shell ──

    #[test]
    fn sanitize_empty_string() {
        assert_eq!(sanitize_for_shell(""), "''");
    }

    #[test]
    fn sanitize_plain_value_wraps_in_single_quotes() {
        assert_eq!(sanitize_for_shell("main"), "'main'");
    }

    #[test]
    fn sanitize_embedded_single_quote_escaped() {
        assert_eq!(sanitize_for_shell("it's"), "'it'\\''s'");
    }

    #[test]
    fn sanitize_multiple_single_quotes() {
        assert_eq!(sanitize_for_shell("a'b'c"), "'a'\\''b'\\''c'");
    }

    #[test]
    fn sanitize_special_shell_chars_are_inert_inside_single_quotes() {
        let result = sanitize_for_shell("main; rm -rf /");
        assert_eq!(result, "'main; rm -rf /'");
    }

    #[test]
    fn sanitize_braces_preserved() {
        assert_eq!(sanitize_for_shell("feature/{foo}"), "'feature/{foo}'");
    }

    #[test]
    fn sanitize_backticks_and_dollar() {
        let result = sanitize_for_shell("`whoami` $HOME");
        assert_eq!(result, "'`whoami` $HOME'");
    }

    // ── is_safe_shell_value ──

    #[test]
    fn safe_shell_value_alphanumeric() {
        assert!(is_safe_shell_value("main"));
        assert!(is_safe_shell_value("feature123"));
    }

    #[test]
    fn safe_shell_value_allowed_special_chars() {
        assert!(is_safe_shell_value("feature/branch-name"));
        assert!(is_safe_shell_value("user@host:path"));
        assert!(is_safe_shell_value("v1.2.3"));
        assert!(is_safe_shell_value("a_b"));
    }

    #[test]
    fn safe_shell_value_rejects_dangerous_chars() {
        assert!(!is_safe_shell_value("main; rm -rf /"));
        assert!(!is_safe_shell_value("$(whoami)"));
        assert!(!is_safe_shell_value("`id`"));
        assert!(!is_safe_shell_value("a b"));
        assert!(!is_safe_shell_value("a'b"));
    }

    #[test]
    fn safe_shell_value_empty_is_safe() {
        assert!(is_safe_shell_value(""));
    }

    // ── build_review_prompt ──

    #[test]
    fn review_prompt_branch_scope_includes_base_branch() {
        let prompt = build_review_prompt("main", "branch");
        assert!(
            prompt.contains("'main'"),
            "should include sanitized base branch"
        );
        assert!(prompt.contains("--unified=20 --no-color --no-ext-diff"));
        assert!(!prompt.contains("--staged"));
    }

    #[test]
    fn review_prompt_staged_scope_uses_staged_flag() {
        let prompt = build_review_prompt("main", "staged");
        assert!(prompt.contains("--staged --unified=20 --no-color --no-ext-diff"));
    }

    #[test]
    fn review_prompt_unstaged_scope_no_base_branch() {
        let prompt = build_review_prompt("main", "unstaged");
        assert!(prompt.contains("--unified=20 --no-color --no-ext-diff"));
        assert!(!prompt.contains("'main' --unified"));
        assert!(!prompt.contains("--staged"));
    }

    #[test]
    fn review_prompt_dangerous_branch_name_sanitized() {
        let prompt = build_review_prompt("main; rm -rf /", "branch");
        assert!(prompt.contains("'main; rm -rf /'"));
        assert!(!prompt.contains("main; rm -rf / --unified"));
    }

    // ── build_questions_prompt ──

    #[test]
    fn questions_prompt_branch_scope_includes_base() {
        let prompt = build_questions_prompt("develop", "branch");
        assert!(prompt.contains("'develop'"));
    }

    #[test]
    fn questions_prompt_staged_scope() {
        let prompt = build_questions_prompt("main", "staged");
        assert!(prompt.contains("--staged"));
    }

    // ── build_review_prompt_remote ──

    #[test]
    fn remote_review_prompt_uses_gh_pr_diff() {
        let prompt = build_review_prompt_remote("owner", "repo", 42, "/tmp/er-cache");
        assert!(prompt.contains("gh pr diff 42"));
        assert!(prompt.contains("'owner'/'repo'"));
        assert!(prompt.contains("'/tmp/er-cache'"));
    }

    #[test]
    fn remote_review_prompt_sanitizes_owner_repo() {
        let prompt = build_review_prompt_remote("own'er", "re;po", 1, "/tmp/out");
        assert!(prompt.contains("'own'\\''er'"));
        assert!(prompt.contains("'re;po'"));
    }

    // ── build_questions_prompt_remote ──

    #[test]
    fn remote_questions_prompt_uses_gh_pr_diff() {
        let prompt = build_questions_prompt_remote("owner", "repo", 10, "/tmp/cache");
        assert!(prompt.contains("gh pr diff 10"));
        assert!(prompt.contains("'owner'/'repo'"));
    }

    // ── annotate_diff_command ──

    #[test]
    fn annotate_diff_command_quotes_paths_and_redirects() {
        let cmd = annotate_diff_command(".er/diff-tmp", ".er/diff-annotated");
        assert!(cmd.starts_with("awk '"));
        assert!(cmd.contains("'.er/diff-tmp'"));
        assert!(cmd.ends_with("> '.er/diff-annotated'"));
    }

    #[test]
    fn review_prompt_includes_annotation_step() {
        let prompt = build_review_prompt("main", "branch");
        assert!(
            prompt.contains("'.er/diff-tmp'"),
            "annotation reads raw diff"
        );
        assert!(
            prompt.contains("'.er/diff-annotated'"),
            "annotation writes annotated diff"
        );
        assert!(prompt.contains("[h<hunk> L<file_line>]"));
        assert!(prompt.contains("Read `.er/diff-annotated`"));
    }

    #[test]
    fn questions_prompt_includes_annotation_step() {
        let prompt = build_questions_prompt("main", "branch");
        assert!(prompt.contains("'.er/diff-annotated'"));
        assert!(prompt.contains("[h<hunk> L<file_line>]"));
    }

    #[test]
    fn remote_review_prompt_includes_annotation_step() {
        let prompt = build_review_prompt_remote("owner", "repo", 42, "/tmp/cache");
        assert!(prompt.contains("'/tmp/cache/diff-tmp'"));
        assert!(prompt.contains("'/tmp/cache/diff-annotated'"));
        assert!(prompt.contains("[h<hunk> L<file_line>]"));
    }

    #[test]
    fn remote_questions_prompt_includes_annotation_step() {
        let prompt = build_questions_prompt_remote("owner", "repo", 10, "/tmp/cache");
        assert!(prompt.contains("'/tmp/cache/diff-annotated'"));
    }

    // ── agentic review + confidence ──

    #[test]
    fn review_prompt_uses_unified_20() {
        let prompt = build_review_prompt("main", "branch");
        assert!(prompt.contains("--unified=20"));
        assert!(!prompt.contains("--unified=3"));
    }

    #[test]
    fn review_prompt_allows_reading_source_files() {
        let prompt = build_review_prompt("main", "branch");
        // No "Do NOT read individual source files" — that rule is removed for review.
        assert!(!prompt.contains("Do NOT read individual source files"));
        // Must explicitly invite agentic verification.
        assert!(prompt.contains("Verify findings agentically"));
    }

    #[test]
    fn review_prompt_requires_findings_to_anchor_to_plus_or_minus() {
        let prompt = build_review_prompt("main", "branch");
        assert!(prompt.contains("Findings MUST anchor to a `+` or `-` line"));
    }

    #[test]
    fn review_prompt_includes_confidence_and_evidence() {
        let prompt = build_review_prompt("main", "branch");
        assert!(prompt.contains("\"confidence\""));
        assert!(prompt.contains("\"evidence\""));
        assert!(prompt.contains("\"verification_plan\""));
        assert!(prompt.contains("confirmed"));
        assert!(prompt.contains("informational"));
        assert!(prompt.contains("tentative"));
    }

    #[test]
    fn remote_review_prompt_includes_confidence_and_evidence() {
        let prompt = build_review_prompt_remote("owner", "repo", 42, "/tmp/cache");
        assert!(prompt.contains("\"confidence\""));
        assert!(prompt.contains("\"evidence\""));
        assert!(prompt.contains("\"verification_plan\""));
    }

    // ── build_validate_prompt ──

    #[test]
    fn validate_prompt_reads_review_and_diff_annotated() {
        let prompt = build_validate_prompt("main", "branch");
        assert!(prompt.contains(".er/review.json"));
        assert!(prompt.contains(".er/diff-annotated"));
    }

    #[test]
    fn validate_prompt_uses_unified_20_for_refresh() {
        let prompt = build_validate_prompt("main", "branch");
        assert!(prompt.contains("--unified=20"));
    }

    #[test]
    fn validate_prompt_explains_confidence_transitions() {
        let prompt = build_validate_prompt("main", "branch");
        assert!(prompt.contains("confirmed"));
        assert!(prompt.contains("informational"));
        assert!(prompt.contains("dropped"));
    }

    #[test]
    fn validate_prompt_branch_scope_includes_base() {
        let prompt = build_validate_prompt("develop", "branch");
        assert!(prompt.contains("'develop'"));
    }

    #[test]
    fn validate_prompt_staged_scope_uses_staged_flag() {
        let prompt = build_validate_prompt("main", "staged");
        assert!(prompt.contains("--staged --unified=20"));
    }
}
