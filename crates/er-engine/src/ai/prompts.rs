/// Read-budget line for card-level AI (Ask / Validate on a single thread).
pub const CARD_AI_READ_BUDGET_LINE: &str = r#"### Investigation budget

Up to **~10** file/range reads for **this** finding or thread only; **no global session cap**. Use `Read` / `grep` / `rg` under `repo_root`. Cite paths and line ranges in your reply.

"#;

/// Embedded prompt templates for guided AI actions.
///
/// These replicate the logic from the external Claude Code skills
/// (`/er-review`, `/er-questions`) so the TUI can invoke them directly
/// via the configured agent command without requiring skill files.
///
/// Shared rules live in `skills/REVIEW_RULES.md` and `review_rules_preamble()`.
use super::experts::{expert_by_id, expert_summary_focus, FindingCaps};
use super::professor::PROFESSOR_SUMMARY_FOCUS;
use super::triage::TRIAGE_SKILL;
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

/// Canonical review rules block (aligned with `skills/REVIEW_RULES.md`).
///
/// When `prepared_diff` is false, pass `git_diff_capture` as the full step-1 shell command
/// (e.g. `git diff main --unified=20 ... > .er/diff-tmp && shasum ...`).
pub fn review_rules_preamble(
    output_dir: &str,
    prepared_diff: bool,
    caps: FindingCaps,
    git_diff_capture: Option<&str>,
) -> String {
    let safe_output_dir = sanitize_for_shell(output_dir)
        .replace('{', "{{")
        .replace('}', "}}");
    let diff_tmp = format!("{output_dir}/diff-tmp");
    let diff_annotated = format!("{output_dir}/diff-annotated");
    let annotate = annotate_diff_command(&diff_tmp, &diff_annotated);
    let hash_step = if prepared_diff {
        format!(
            "1. Run: `(sha256sum {safe_output_dir}/diff-tmp 2>/dev/null || shasum -a 256 {safe_output_dir}/diff-tmp)`\n   - Save the SHA-256 hash as `diff_hash` (do **not** run `git diff` — the diff is already prepared)"
        )
    } else {
        let capture = git_diff_capture.unwrap_or(
            "git diff <base> --unified=20 --no-color --no-ext-diff > .er/diff-tmp && (sha256sum .er/diff-tmp 2>/dev/null || shasum -a 256 .er/diff-tmp)",
        );
        format!(
            "1. Run: `{capture}`\n   - Use a **two-dot** diff (`git diff <base>`), never three-dot (`main...HEAD`)\n   - Always `--unified=20 --no-color --no-ext-diff`\n   - Save the SHA-256 hash as `diff_hash`"
        )
    };
    let categories = if caps.is_expert {
        "Set `category` to the expert id for every finding — only report issues in that lens."
    } else {
        "Categories: `security`, `logic`, `performance`, `correctness`, `error-handling`, `testing`, `api` — **no `style`**."
    };
    let speed = if caps.is_expert {
        "Target: complete in under 2 minutes."
    } else {
        "Target: complete in under 3 minutes."
    };
    format!(
        r#"## Review rules (required)

### Diff and hash
{hash_step}
2. Annotate: `{annotate}`
3. Read `{safe_output_dir}/diff-annotated` — each line has `[h<hunk> L<file_line>]` tags (20 lines of context per hunk).

### Annotate and anchor
- Findings **only** on `+` or `-` lines in the annotated diff
- Copy `hunk_index` from `[h<N>]` and `line_start` from `[h<N> L<M>]` — **never** count or infer line numbers
- Set `outside_diff: false`; drop findings you cannot anchor to a tagged line

### Severity (P0 / P1 / P2)
- P0 → `high` — must fix (security, data loss, broken contract)
- P1 → `medium` — should fix (logic gap, missing edge case, shallow test)
- P2 → `low` — nice to fix
- `info` — observation only

**Do not flag:** naming, formatting, style, import order, file moves without logic change, comment nits.
**Gate:** "Does this affect correctness, security, or reliability?" — if no, skip.

### Confidence and verification
- `confidence`: `confirmed` | `informational` | `tentative` (with `verification_plan`)
- `evidence`: cite files/ranges read; budget ~10 reads per finding (no global session cap)

### Finding caps
- Max {per_file} findings per file, max {total} total
- {categories}

### Speed
{speed}
Short-circuit obvious issues. Read source files to verify, not to expand scope."#,
        per_file = caps.per_file,
        total = caps.total,
    )
}

fn general_review_instructions_read_analyze() -> &'static str {
    r#"4. Analyse every changed file. **Findings target only `+` or `-` lines** — context is for comprehension. Per file:
   - `risk`: "high" | "medium" | "low" | "info"
   - `risk_reason`: why this risk level
   - `summary`: one-line description of changes
   - `findings`: array of issues (within caps)
5. **Verify findings agentically** when significance depends on code outside the diff — read/grep sibling files, callers, tests; append `evidence` entries; mark `tentative` if budget runs out.
6. Set `confidence` on every finding: `confirmed`, `informational`, or `tentative` (with `verification_plan`)."#
}

fn general_review_json_example() -> &'static str {
    r#"        {{
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
        }}"#
}

fn general_review_outputs_section(
    output_dir: &str,
    scope: &str,
    base_branch: &str,
    head_branch_hint: &str,
) -> String {
    let safe_output_dir = sanitize_for_shell(output_dir)
        .replace('{', "{{")
        .replace('}', "}}");
    let example = general_review_json_example();
    format!(
        r#"7. Write these four files:

### `{safe_output_dir}/review.json`
```json
{{
  "version": 1,
  "diff_hash": "<sha256 from step 1>",
  "diff_scope": "{scope}",
  "created_at": "<ISO 8601>",
  "base_branch": "{base_branch}",
  "head_branch": "{head_branch_hint}",
  "file_hashes": {{}},
  "files": {{
    "path/to/file.rs": {{
      "risk": "medium",
      "risk_reason": "Modifies error handling logic",
      "summary": "Adds retry mechanism for network calls",
      "findings": [
{example}
      ]
    }}
  }}
}}
```

`resolved` / `resolved_note` / `resolved_at` are populated by the validate pass — leave defaults during review.

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

- Be specific and actionable; titles under 60 characters.
- Risk: high = likely bug/security, medium = missing edge case, low = minor quality, info = observation only.
- Checklist items are for the human reviewer to verify manually."#
    )
}

fn expert_lens_instructions(expert_id: &str) -> String {
    let def = expert_by_id(expert_id).unwrap_or_else(|| {
        panic!("unknown expert_id: {expert_id}");
    });
    let mut lens = format!(
        r#"## Expert lens: {label}

Focus only on **{id}** issues in this diff. Ignore problems outside this lens (other experts or general review cover them).

{description}

Write findings and a lens-specific `summary` in the expert JSON — do not write order.json, checklist.json, or summary.md."#,
        label = def.label,
        id = def.id,
        description = def.description,
    );
    lens.push_str(&format!(
        "\n\n**Summary (`summary` field):** 2–3 short markdown paragraphs on {}. No general change log — only what matters for the {label} lens.",
        expert_summary_focus(expert_id),
        label = def.label,
    ));
    if expert_id == "patterns" {
        lens.push_str(
            r#"

**Patterns workflow:**
1. Identify symbols/patterns introduced or changed in the diff
2. `grep` / read 2–5 similar usages elsewhere (same directory or module first)
3. Flag only deviations that affect correctness or maintainability (no naming/style nits)
4. Cite established pattern locations in `evidence` (file + line range + note)"#,
        );
    }
    if expert_id == "simplifying" {
        lens.push_str(
            r#"

**Simplifying workflow:**
1. Flag code that is hard to understand from the diff alone: deep nesting, clever one-liners, implicit conventions, heavy indirection, dense generics/macros, or patterns that require tribal knowledge
2. Prefer a concrete simplification in `suggestion` (flatten control flow, name intermediates, extract helper, replace abstraction with direct code)
3. If simplification would change behavior or is too risky in this diff, ask for a brief comment instead — `suggestion` should quote the comment text to add
4. Do not flag naming, formatting, or style-only nits
5. `severity`: `medium` when complexity blocks review confidence; `low` when a comment would suffice; `info` for optional polish"#,
        );
    }
    if expert_id == "mentorship" {
        lens.push_str(
            r#"

**Mentorship workflow (positive-only):**
1. Highlight **good** patterns in the diff: clear APIs, strong tests, thoughtful error handling, readable structure, safe defaults, useful types, or designs that match team quality bar
2. Explain **why** it is exemplary in `description` so reviewers learn what to foster
3. Use `suggestion` to name where to replicate the pattern elsewhere, or how to extend it — not fixes
4. Do **not** report bugs, risks, or nitpicks (other reviewers cover those)
5. `severity`: always `info`; `confidence`: `informational`"#,
        );
    }
    lens
}

fn expert_review_output_section(output_dir: &str, expert_id: &str) -> String {
    let def = expert_by_id(expert_id).expect("unknown expert");
    let safe_output_dir = sanitize_for_shell(output_dir)
        .replace('{', "{{")
        .replace('}', "}}");
    let example = general_review_json_example();
    format!(
        r#"7. Write **only** `{safe_output_dir}/experts/{expert_id}.json`:

```json
{{
  "version": 1,
  "expert_id": "{expert_id}",
  "diff_hash": "<sha256 from step 1>",
  "diff_scope": "<scope>",
  "created_at": "<ISO 8601>",
  "summary": "2–3 short markdown paragraphs ({summary_focus})",
  "files": {{
    "path/to/file.rs": {{
      "findings": [
{example}
      ]
    }}
  }}
}}
```

- Finding `id` prefix: `{prefix}-` (e.g. `{prefix}-1`)
- Finding `category`: `{expert_id}`
- `summary`: lens-specific only — {summary_focus}
- `mkdir -p {safe_output_dir}/experts` before writing"#,
        prefix = def.id_prefix,
        summary_focus = expert_summary_focus(expert_id),
    )
}

/// Build the review prompt for Desktop local-managed mode.
///
/// Same review logic as `build_review_prompt` but uses absolute `output_dir` paths
/// instead of repo-relative `.er/` paths. The agent runs with cwd = repo root so
/// `git diff` resolves correctly; all outputs land in `output_dir`.
pub fn build_review_prompt_local_managed(
    base_branch: &str,
    scope: &str,
    output_dir: &str,
) -> String {
    let safe_base_branch = sanitize_for_shell(base_branch)
        .replace('{', "{{")
        .replace('}', "}}");
    let base_branch_escaped = base_branch.replace('{', "{{").replace('}', "}}");
    let safe_output_dir = sanitize_for_shell(output_dir)
        .replace('{', "{{")
        .replace('}', "}}");
    let diff_args = match scope {
        "unstaged" => "--unified=20 --no-color --no-ext-diff".to_string(),
        "staged" => "--staged --unified=20 --no-color --no-ext-diff".to_string(),
        _ => format!("{safe_base_branch} --unified=20 --no-color --no-ext-diff"),
    };
    let capture = format!(
        "mkdir -p {safe_output_dir} && git diff {diff_args} > {safe_output_dir}/diff-tmp && (sha256sum {safe_output_dir}/diff-tmp 2>/dev/null || shasum -a 256 {safe_output_dir}/diff-tmp)"
    );
    let preamble = review_rules_preamble(output_dir, false, FindingCaps::general(), Some(&capture));
    let outputs =
        general_review_outputs_section(output_dir, scope, &base_branch_escaped, "<current branch>");
    format!(
        r#"You are a code reviewer. Perform a thorough review of the current git diff and write results to `{safe_output_dir}/`.

{preamble}

{analyze}

{outputs}"#,
        analyze = general_review_instructions_read_analyze(),
    )
}

/// Build the review prompt when `{output_dir}/diff-tmp` is already written by `er`.
///
/// Desktop uses this after materializing the tab's UI diff. The agent must not
/// run `git diff` or `gh pr diff` — it hashes and annotates the prepared file.
pub fn build_review_prompt_prepared_diff(scope: &str, output_dir: &str) -> String {
    let safe_output_dir = sanitize_for_shell(output_dir)
        .replace('{', "{{")
        .replace('}', "}}");
    let preamble = review_rules_preamble(output_dir, true, FindingCaps::general(), None);
    let outputs = general_review_outputs_section(
        output_dir,
        scope,
        "<base branch if known>",
        "<head branch if known>",
    );
    format!(
        r#"You are a code reviewer. Perform a thorough review of the prepared diff and write results to `{safe_output_dir}/`.

{preamble}

{analyze}

{outputs}"#,
        analyze = general_review_instructions_read_analyze(),
    )
}

/// Specialized expert review for local-managed app/TUI runs.
pub fn build_expert_review_prompt_local_managed(
    base_branch: &str,
    scope: &str,
    output_dir: &str,
    expert_id: &str,
) -> String {
    let _ = expert_by_id(expert_id).expect("unknown expert_id");
    let safe_base_branch = sanitize_for_shell(base_branch)
        .replace('{', "{{")
        .replace('}', "}}");
    let safe_output_dir = sanitize_for_shell(output_dir)
        .replace('{', "{{")
        .replace('}', "}}");
    let diff_args = match scope {
        "unstaged" => "--unified=20 --no-color --no-ext-diff".to_string(),
        "staged" => "--staged --unified=20 --no-color --no-ext-diff".to_string(),
        _ => format!("{safe_base_branch} --unified=20 --no-color --no-ext-diff"),
    };
    let capture = format!(
        "mkdir -p {safe_output_dir}/experts && git diff {diff_args} > {safe_output_dir}/diff-tmp && (sha256sum {safe_output_dir}/diff-tmp 2>/dev/null || shasum -a 256 {safe_output_dir}/diff-tmp)"
    );
    let preamble = review_rules_preamble(output_dir, false, FindingCaps::expert(), Some(&capture));
    let lens = expert_lens_instructions(expert_id);
    let output = expert_review_output_section(output_dir, expert_id);
    format!(
        r#"You are a specialized code reviewer. Write expert findings to `{safe_output_dir}/experts/`.

{preamble}

{lens}

{analyze}

{output}"#,
        analyze = general_review_instructions_read_analyze(),
    )
}

/// Specialized expert review when `{output_dir}/diff-tmp` is already prepared (desktop).
pub fn build_expert_review_prompt_prepared_diff(
    scope: &str,
    output_dir: &str,
    expert_id: &str,
) -> String {
    let _ = expert_by_id(expert_id).expect("unknown expert_id");
    let safe_output_dir = sanitize_for_shell(output_dir)
        .replace('{', "{{")
        .replace('}', "}}");
    let preamble = review_rules_preamble(output_dir, true, FindingCaps::expert(), None);
    let lens = expert_lens_instructions(expert_id);
    let output = expert_review_output_section(output_dir, expert_id);
    format!(
        r#"You are a specialized code reviewer for scope `{scope}`. Write expert findings to `{safe_output_dir}/experts/`.

{preamble}

{lens}

{analyze}

{output}"#,
        analyze = general_review_instructions_read_analyze(),
    )
}

/// Guided tour generation when `{output_dir}/diff-tmp` is already prepared
/// (desktop "Generate tour"). Writes only `{output_dir}/{output_file}`.
///
/// `output_file` is the context-scoped sidecar name: `tour.json` for the local
/// branch diff, `tour.pr.json` for the PR diff. The tour stays attached to
/// whichever diff was being viewed when generated.
pub fn build_tour_prompt_prepared_diff(scope: &str, output_dir: &str, output_file: &str) -> String {
    let safe_output_dir = sanitize_for_shell(output_dir)
        .replace('{', "{{")
        .replace('}', "}}");
    let safe_output_file = output_file.replace('{', "{{").replace('}', "}}");
    format!(
        r#"You are preparing a guided **Tour** of a code diff for a reviewer. A diff for scope `{scope}` is already captured at `{safe_output_dir}/diff-tmp`.

## Steps
1. Compute the diff hash: `sha256sum {safe_output_dir}/diff-tmp 2>/dev/null || shasum -a 256 {safe_output_dir}/diff-tmp`.
2. Read `{safe_output_dir}/diff-tmp` (the full diff) into context.
3. Optionally read `{safe_output_dir}/review.json` if it exists and its `diff_hash` matches — reuse its groupings and reference finding ids.
4. Group the changed files into **pillars** ordered foundation-first, then by importance:
   - `foundation: true` for pillars other pillars build on (data models, core types, shared utilities, schema). Order these first.
   - `importance` (0–100) ranks reviewer attention; higher sorts earlier among non-foundation.
   - Each pillar: a short `title`, a 1–3 sentence markdown `description` (what it is and what to look for), and its `files` (new-side paths) in reading order, each with a one-line `reason`.
   - Every changed file appears in exactly one pillar. 3–7 pillars is ideal.
5. Write `{safe_output_dir}/{safe_output_file}` (and nothing else) with this exact shape:

```json
{{
  "version": 1,
  "diff_hash": "<sha256 from step 1>",
  "created_at": "<ISO 8601>",
  "title": "<short tour title>",
  "overview": "<1-2 sentence markdown intro>",
  "pillars": [
    {{
      "id": "p-1",
      "title": "Foundation: ...",
      "description": "...",
      "order": 0,
      "importance": 90,
      "foundation": true,
      "files": [
        {{"path": "src/foo.rs", "reason": "...", "finding_ids": []}}
      ]
    }}
  ]
}}
```

Do NOT modify `review.json`, `order.json`, or any other file. Write only `{safe_output_file}`."#
    )
}

/// When `review-files.txt` exists, agents must limit analysis to those paths.
pub fn file_scope_appendix(output_dir: &str) -> String {
    let safe_output_dir = sanitize_for_shell(output_dir)
        .replace('{', "{{")
        .replace('}', "}}");
    format!(
        r#"
## Scope: selected files only

Read `{safe_output_dir}/review-files.txt` — analyze **only** those paths. Ignore all other files in the diff."#
    )
}

fn professor_rules_preamble(
    output_dir: &str,
    prepared_diff: bool,
    git_diff_capture: Option<&str>,
) -> String {
    let caps = FindingCaps {
        per_file: 3,
        total: 12,
        is_expert: true,
    };
    let mut preamble = review_rules_preamble(output_dir, prepared_diff, caps, git_diff_capture);
    preamble.push_str(
        r#"

### Professor mode (not a review)
- **Do not** flag bugs, security issues, or style nits — `/er-review` covers those.
- Teach: purpose, architecture, data flow, invariants, non-obvious design.
- Every finding: `severity: "info"`, `confidence: "informational"`, `category: "professor"`.
- Titles are concept labels; descriptions explain *how* and *why*."#,
    );
    preamble
}

fn professor_lens_instructions(user_focus: Option<&str>) -> String {
    let mut lens = r#"## Professor lens: Learn the implementation

Explain what this diff implements so a skilled developer can understand it without reading every line.

**Highlight:** purpose of changes, how components connect, state/IO boundaries, design tradeoffs, what to read next (`related_files`).

**Skip:** must-fix framing, naming/style, duplicate security/perf concerns.

**Caps:** ~3 insights per file, ~12 total. Quality over quantity.

Write teaching insights and a `summary` in professor.json — no order.json, checklist.json, or summary.md."#
        .to_string();
    lens.push_str(&format!(
        "\n\n**Summary (`summary` field):** 2–3 short markdown paragraphs on {PROFESSOR_SUMMARY_FOCUS}."
    ));
    if let Some(focus) = user_focus.filter(|s| !s.trim().is_empty()) {
        lens.push_str("\n\n## Learner focus (user-provided)\n");
        lens.push_str(focus.trim());
        lens.push_str(
            "\n\nPrioritize insights that answer this focus. Still note 1–2 other central mechanisms if needed.",
        );
    }
    lens
}

fn professor_output_section(output_dir: &str) -> String {
    let safe_output_dir = sanitize_for_shell(output_dir)
        .replace('{', "{{")
        .replace('}', "}}");
    format!(
        r#"7. Write **only** `{safe_output_dir}/professor.json`:

```json
{{
  "version": 1,
  "diff_hash": "<sha256 from step 1>",
  "diff_scope": "<scope>",
  "created_at": "<ISO 8601>",
  "focus_prompt": "<user focus or empty string>",
  "summary": "2–3 short markdown paragraphs ({professor_summary_focus})",
  "files": {{
    "path/to/file.rs": {{
      "findings": [
        {{
          "id": "prof-1",
          "severity": "info",
          "category": "professor",
          "title": "Short concept label",
          "description": "Teaching explanation (markdown ok)",
          "hunk_index": 0,
          "line_start": 42,
          "suggestion": "",
          "related_files": [],
          "outside_diff": false,
          "confidence": "informational",
          "verification_plan": "",
          "evidence": [],
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

- Finding `id` prefix: `prof-` (e.g. `prof-1`)
- `summary`: {professor_summary_focus}"#,
        professor_summary_focus = PROFESSOR_SUMMARY_FOCUS,
    )
}

/// Professor learning agent for local-managed app/TUI runs.
pub fn build_professor_review_prompt_local_managed(
    base_branch: &str,
    scope: &str,
    output_dir: &str,
    user_focus: Option<&str>,
) -> String {
    let safe_base_branch = sanitize_for_shell(base_branch)
        .replace('{', "{{")
        .replace('}', "}}");
    let safe_output_dir = sanitize_for_shell(output_dir)
        .replace('{', "{{")
        .replace('}', "}}");
    let diff_args = match scope {
        "unstaged" => "--unified=20 --no-color --no-ext-diff".to_string(),
        "staged" => "--staged --unified=20 --no-color --no-ext-diff".to_string(),
        _ => format!("{safe_base_branch} --unified=20 --no-color --no-ext-diff"),
    };
    let capture = format!(
        "mkdir -p {safe_output_dir} && git diff {diff_args} > {safe_output_dir}/diff-tmp && (sha256sum {safe_output_dir}/diff-tmp 2>/dev/null || shasum -a 256 {safe_output_dir}/diff-tmp)"
    );
    let preamble = professor_rules_preamble(output_dir, false, Some(&capture));
    let lens = professor_lens_instructions(user_focus);
    let output = professor_output_section(output_dir);
    let file_scope = file_scope_if_present(output_dir);
    format!(
        r#"You are a code professor. Teach what this diff implements; write insights to `{safe_output_dir}/professor.json`.

{preamble}

{lens}

{analyze}

{output}{file_scope}"#,
        analyze = general_review_instructions_read_analyze(),
    )
}

/// Professor when `{output_dir}/diff-tmp` is already prepared (desktop).
pub fn build_professor_review_prompt_prepared_diff(
    scope: &str,
    output_dir: &str,
    user_focus: Option<&str>,
    scoped_files: bool,
) -> String {
    let safe_output_dir = sanitize_for_shell(output_dir)
        .replace('{', "{{")
        .replace('}', "}}");
    let preamble = professor_rules_preamble(output_dir, true, None);
    let lens = professor_lens_instructions(user_focus);
    let output = professor_output_section(output_dir);
    let file_scope = if scoped_files {
        file_scope_appendix(output_dir)
    } else {
        file_scope_if_present(output_dir)
    };
    format!(
        r#"You are a code professor for scope `{scope}`. Teach what this diff implements; write insights to `{safe_output_dir}/professor.json`.

{preamble}

{lens}

{analyze}

{output}{file_scope}"#,
        analyze = general_review_instructions_read_analyze(),
    )
}

fn file_scope_if_present(output_dir: &str) -> String {
    let path = std::path::Path::new(output_dir).join("review-files.txt");
    if path.exists() {
        file_scope_appendix(output_dir)
    } else {
        String::new()
    }
}

/// Append file-scope section to any prepared-diff prompt when manifest exists.
pub fn append_file_scope_if_present(mut prompt: String, output_dir: &str) -> String {
    prompt.push_str(&file_scope_if_present(output_dir));
    prompt
}

fn triage_lens_instructions() -> String {
    r#"## Triage lens: breadth over depth

Scan every changed file at **file + hunk-header** level. Do **not** hunt P0 bugs line-by-line.

**Deliver:**
1. `first_impression` — 2–4 short paragraphs: what changed, blast radius, gut feel.
2. `diff_stats` — file count, `approx_risk` (`low`|`medium`|`high`), `domains` touched (e.g. auth, api, tests).
3. `verdict` — route the human to the next review:
   - `skip` — cosmetic/docs/lockfiles only; no logic to review.
   - `general` — mixed concerns; run full `/er-review`.
   - `expert` — dominant lens; set `experts` to one or more ids: security, performance, reliability, testing, api, patterns, simplifying, mentorship.
   - `arena` — large/high-stakes diff or needs multi-model second opinion.
   - `professor` — novel subsystem the reader should learn first.
4. `priority_files` — up to **12** paths worth reading line-by-line before anything else (`path`, `reason`, `risk`).

**Speed budget:** ≤8 tool calls, <60 seconds. Read diff once in context; write only `triage.json`.

**Do not write** `review.json`, `order.json`, `checklist.json`, or `summary.md."#
        .to_string()
}

fn triage_output_section(output_dir: &str) -> String {
    let safe_output_dir = sanitize_for_shell(output_dir)
        .replace('{', "{{")
        .replace('}', "}}");
    format!(
        r#"Write **only** `{safe_output_dir}/triage.json`:

```json
{{
  "version": 1,
  "diff_hash": "<sha256 from step 1>",
  "diff_scope": "<scope>",
  "created_at": "<ISO 8601>",
  "first_impression": "2–4 short markdown paragraphs",
  "diff_stats": {{
    "files_changed": 0,
    "approx_risk": "low|medium|high",
    "domains": ["auth", "api"]
  }},
  "verdict": {{
    "primary": "general|expert|arena|professor|skip",
    "experts": ["security"],
    "rationale": "Why this next step",
    "confidence": "high|medium|low"
  }},
  "priority_files": [
    {{ "path": "src/lib.rs", "reason": "Core logic change", "risk": "high" }}
  ]
}}
```

Skill reference: `/{TRIAGE_SKILL}`."#,
        TRIAGE_SKILL = TRIAGE_SKILL,
    )
}

/// Triage scan for local-managed app/TUI runs.
pub fn build_triage_review_prompt_local_managed(
    base_branch: &str,
    scope: &str,
    output_dir: &str,
) -> String {
    let safe_base_branch = sanitize_for_shell(base_branch)
        .replace('{', "{{")
        .replace('}', "}}");
    let safe_output_dir = sanitize_for_shell(output_dir)
        .replace('{', "{{")
        .replace('}', "}}");
    let diff_args = match scope {
        "unstaged" => "--unified=20 --no-color --no-ext-diff".to_string(),
        "staged" => "--staged --unified=20 --no-color --no-ext-diff".to_string(),
        _ => format!("{safe_base_branch} --unified=20 --no-color --no-ext-diff"),
    };
    let capture = format!(
        "mkdir -p {safe_output_dir} && git diff {diff_args} > {safe_output_dir}/diff-tmp && (sha256sum {safe_output_dir}/diff-tmp 2>/dev/null || shasum -a 256 {safe_output_dir}/diff-tmp)"
    );
    let preamble = review_rules_preamble(
        output_dir,
        false,
        FindingCaps {
            per_file: 0,
            total: 2,
            is_expert: true,
        },
        Some(&capture),
    );
    let lens = triage_lens_instructions();
    let output = triage_output_section(output_dir);
    format!(
        r#"You are a code review triage agent. Scan the branch diff broadly and write routing guidance to `{safe_output_dir}/triage.json`.

{preamble}

{lens}

{analyze}

{output}"#,
        analyze = general_review_instructions_read_analyze(),
    )
}

/// Triage when `{output_dir}/diff-tmp` is already prepared (desktop).
pub fn build_triage_review_prompt_prepared_diff(scope: &str, output_dir: &str) -> String {
    let safe_output_dir = sanitize_for_shell(output_dir)
        .replace('{', "{{")
        .replace('}', "}}");
    let preamble = review_rules_preamble(
        output_dir,
        true,
        FindingCaps {
            per_file: 0,
            total: 2,
            is_expert: true,
        },
        None,
    );
    let lens = triage_lens_instructions();
    let output = triage_output_section(output_dir);
    format!(
        r#"You are a code review triage agent for scope `{scope}`. Scan broadly and write routing guidance to `{safe_output_dir}/triage.json`.

{preamble}

{lens}

{analyze}

{output}"#,
        analyze = general_review_instructions_read_analyze(),
    )
}

/// Build the questions-answering prompt for local-managed app/TUI runs.
pub fn build_questions_prompt_local_managed(
    base_branch: &str,
    scope: &str,
    output_dir: &str,
) -> String {
    let safe_base_branch = sanitize_for_shell(base_branch)
        .replace('{', "{{")
        .replace('}', "}}");
    let safe_output_dir = sanitize_for_shell(output_dir)
        .replace('{', "{{")
        .replace('}', "}}");
    let diff_args = match scope {
        "unstaged" => "--unified=3 --no-color --no-ext-diff".to_string(),
        "staged" => "--staged --unified=3 --no-color --no-ext-diff".to_string(),
        _ => format!("{safe_base_branch} --unified=3 --no-color --no-ext-diff"),
    };
    let annotate = annotate_diff_command(
        &format!("{output_dir}/diff-tmp"),
        &format!("{output_dir}/diff-annotated"),
    );

    format!(
        r#"You are answering code review questions. Read the questions file and the diff, then provide answers.

## Instructions

1. Read `{safe_output_dir}/questions.json`
   - If it doesn't exist or has no unresolved questions: print "No questions to answer" and stop.
2. Run: `mkdir -p {safe_output_dir} && git diff {diff_args} > {safe_output_dir}/diff-tmp`
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

/// Build the validate prompt — validates and re-anchors existing findings.
/// Build the validate prompt with paths anchored at an absolute managed
/// directory (`output_dir`). Use this for non-remote local-managed tabs so
/// the agent updates the same `review.json` the UI loads from, rather than
/// writing to the repo-local `<repo>/.er/`.
pub fn build_validate_prompt_local_managed(
    base_branch: &str,
    scope: &str,
    output_dir: &str,
) -> String {
    let safe_base_branch = sanitize_for_shell(base_branch)
        .replace('{', "{{")
        .replace('}', "}}");
    let safe_output_dir = sanitize_for_shell(output_dir)
        .replace('{', "{{")
        .replace('}', "}}");
    let diff_args = match scope {
        "unstaged" => "--unified=20 --no-color --no-ext-diff".to_string(),
        "staged" => "--staged --unified=20 --no-color --no-ext-diff".to_string(),
        _ => format!("{safe_base_branch} --unified=20 --no-color --no-ext-diff"),
    };
    let annotate = annotate_diff_command(
        &format!("{output_dir}/diff-tmp"),
        &format!("{output_dir}/diff-annotated"),
    );

    format!(
        r#"You are validating and re-anchoring an existing code review.
Do not create unrelated new findings in this action.

## Instructions

1. Read `{safe_output_dir}/review.json`. If it does not exist, print "No review to validate" and stop.
2. Refresh the annotated diff so line numbers stay current:
   - `mkdir -p {safe_output_dir} && git diff {diff_args} > {safe_output_dir}/diff-tmp`
   - `{annotate}`
3. For each active finding, read existing replies (`responses`) before deciding the outcome.
4. For each finding, choose exactly one result:
   - `RESOLVED_OR_INVALID`: concern no longer applies. Remove it from active `files[].findings`.
   - `PERSISTS`: concern still applies. Keep it and update title/description/suggestion if needed.
   - `SHIFTED`: concern still applies but moved. Keep it and update `hunk_index`, `line_start`, `line_end`.
5. If a finding remains uncertain, use `verification_plan` and update confidence/evidence (`confirmed`,
   `informational`, `dropped`) based on current code.
6. Preserve `diff_hash`, `version`, and unchanged file entries unless your existing refresh workflow recomputes them.
7. Write updated `{safe_output_dir}/review.json`.
8. Append a one-line note to `{safe_output_dir}/summary.md` in this exact format:
   `Refresh: N removed, M updated, K re-anchored.`
9. Do not discover unrelated new findings in this action.

## Budget

- ~10 file reads per finding (no global session cap).

## Speed

Target: complete in under 5 minutes. Each evidence read should map to a specific finding."#
    )
}

/// Validate prompt when `{output_dir}/diff-tmp` is already written by `er`.
pub fn build_validate_prompt_prepared_diff(_scope: &str, output_dir: &str) -> String {
    let safe_output_dir = sanitize_for_shell(output_dir)
        .replace('{', "{{")
        .replace('}', "}}");
    let annotate = annotate_diff_command(
        &format!("{output_dir}/diff-tmp"),
        &format!("{output_dir}/diff-annotated"),
    );

    format!(
        r#"You are validating and re-anchoring an existing code review.
Do not create unrelated new findings in this action.

## Instructions

1. Read `{safe_output_dir}/review.json`. If it does not exist, print "No review to validate" and stop.
2. Refresh the annotated diff from the prepared file on disk:
   - `(sha256sum {safe_output_dir}/diff-tmp 2>/dev/null || shasum -a 256 {safe_output_dir}/diff-tmp)`
   - `{annotate}`
3. For each active finding, read existing replies (`responses`) before deciding the outcome.
4. For each finding, choose exactly one result:
   - `RESOLVED_OR_INVALID`: concern no longer applies. Remove it from active `files[].findings`.
   - `PERSISTS`: concern still applies. Keep it and update title/description/suggestion if needed.
   - `SHIFTED`: concern still applies but moved. Keep it and update `hunk_index`, `line_start`, `line_end`.
5. If a finding remains uncertain, use `verification_plan` and update confidence/evidence (`confirmed`,
   `informational`, `dropped`) based on current code.
6. Preserve `diff_hash`, `version`, and unchanged file entries unless your existing refresh workflow recomputes them.
7. Write updated `{safe_output_dir}/review.json`.
8. Append a one-line note to `{safe_output_dir}/summary.md` in this exact format:
   `Refresh: N removed, M updated, K re-anchored.`
9. Do not discover unrelated new findings in this action.

## Budget

- ~10 file reads per finding (no global session cap).

## Speed

Target: complete in under 5 minutes. Each evidence read should map to a specific finding."#
    )
}

/// Validate and re-anchor GitHub PR comments when `{output_dir}/diff-tmp` is already on disk.
pub fn build_validate_github_comments_prompt_prepared_diff(output_dir: &str) -> String {
    let safe_output_dir = sanitize_for_shell(output_dir)
        .replace('{', "{{")
        .replace('}', "}}");
    let annotate = annotate_diff_command(
        &format!("{output_dir}/diff-tmp"),
        &format!("{output_dir}/diff-annotated"),
    );

    format!(
        r#"You are validating and re-anchoring existing GitHub PR review comments.
Do not add new review comments in this action.

## Instructions

1. Read `{safe_output_dir}/github-comments.json`. If it does not exist, print "No comments to validate" and stop.
2. Consider only **top-level** line comments where `resolved` is false and `outdated` is false (skip replies — `in_reply_to` set).
3. Refresh the annotated diff from the prepared file on disk:
   - `(sha256sum {safe_output_dir}/diff-tmp 2>/dev/null || shasum -a 256 {safe_output_dir}/diff-tmp)`
   - `{annotate}`
4. For each eligible comment, read the current code at the anchored location and decide:
   - `PERSISTS`: still applies — keep the comment; update text only if needed.
   - `RESOLVED`: addressed or no longer applies — set `resolved: true` (do not delete unless your workflow removes resolved threads).
   - `SHIFTED`: still applies but the line moved — update `hunk_index`, `line_start`, `line_content`, `context_before`, `context_after`, `old_line_start`, `hunk_header`, set `anchor_status` to `relocated`, update `relocated_at_hash` to the current diff hash.
   - `LOST`: cannot anchor — set `anchor_status` to `lost` (leave `resolved` false unless the concern is clearly obsolete).
5. Do **not** set `outdated` — that flag reflects GitHub thread state from sync.
6. Preserve `version`, `github` sync metadata, `github_id`, `source`, `synced`, and reply threads unless a parent is resolved.
7. Write updated `{safe_output_dir}/github-comments.json`.
8. If `{safe_output_dir}/summary.md` exists, append:
   `Comment refresh: N resolved, M re-anchored, K lost.`

## Budget

- ~8 file reads per comment (no global session cap).

## Speed

Target: complete in under 5 minutes."#
    )
}

/// Build the summary-only prompt for local-managed app/TUI runs.
pub fn build_summary_prompt_local_managed(
    base_branch: &str,
    scope: &str,
    output_dir: &str,
) -> String {
    let safe_base_branch = sanitize_for_shell(base_branch)
        .replace('{', "{{")
        .replace('}', "}}");
    let safe_output_dir = sanitize_for_shell(output_dir)
        .replace('{', "{{")
        .replace('}', "}}");
    let diff_args = match scope {
        "unstaged" => "--unified=3 --no-color --no-ext-diff".to_string(),
        "staged" => "--staged --unified=3 --no-color --no-ext-diff".to_string(),
        _ => format!("{safe_base_branch} --unified=3 --no-color --no-ext-diff"),
    };

    format!(
        r#"Summarize the current git diff and write the result to `{safe_output_dir}/summary.md`.

## Instructions

1. Ensure the output dir exists: `mkdir -p {safe_output_dir}`
2. Run: `git diff {diff_args} > {safe_output_dir}/diff-tmp`
3. Read `{safe_output_dir}/diff-tmp`
4. Write `{safe_output_dir}/summary.md` as 3-5 short markdown paragraphs covering:
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
    let capture = format!(
        "mkdir -p {safe_output_dir} && gh pr diff {pr_number} --repo {safe_owner}/{safe_repo} > {safe_output_dir}/diff-tmp && (sha256sum {safe_output_dir}/diff-tmp 2>/dev/null || shasum -a 256 {safe_output_dir}/diff-tmp)"
    );
    let preamble = review_rules_preamble(output_dir, false, FindingCaps::general(), Some(&capture));
    let outputs = general_review_outputs_section(output_dir, "branch", "", "");
    format!(
        r#"You are a code reviewer. Perform a thorough review of the GitHub PR diff and write results to `{safe_output_dir}/`.

Note: `gh pr diff` returns less context than local `git diff` — agentic verification matters more.

{preamble}

{analyze}

{outputs}

Ensure `{safe_output_dir}/` exists before writing: `mkdir -p {safe_output_dir}`"#,
        analyze = general_review_instructions_read_analyze(),
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

// ── AI Review Arena (JSON-only stdout) ──

pub fn build_arena_round1_prompt(diff_patch_path: &str, reviewer_label: &str) -> String {
    build_arena_round1_prompt_agent(diff_patch_path, reviewer_label, None)
}

/// Round-1 arena prompt; optional `agent_kind` applies specialized lens (general / expert / professor).
pub fn build_arena_round1_prompt_agent(
    diff_patch_path: &str,
    reviewer_label: &str,
    agent_kind: Option<&str>,
) -> String {
    let path = diff_patch_path.replace('\\', "/");
    let lens = agent_kind.map(agent_lens_block).unwrap_or_default();
    format!(
        r#"You are reviewer "{reviewer_label}" in a multi-agent code review arena.
{lens}
Read the pinned diff at `{path}` (do not run git diff).

Respond with ONLY a single JSON object on stdout (no markdown fences), matching:
{{"findings":[{{"file":"path","line":12,"title":"short","body":"detail","severity":"high|med|low","confidence":0.0,"tags":[]}}]}}

Rules:
- Propose real issues grounded in the diff.
- severity: high | med | low
- If no issues: {{"findings":[]}}
"#,
    )
}

fn agent_lens_block(agent_kind: &str) -> String {
    if agent_kind == "general" {
        return "\nLens: broad correctness, logic, and risk — same scope as a general code review.\n"
            .to_string();
    }
    if agent_kind == "professor" {
        return "\nLens: Professor mode — teach what this diff implements; informational insights only.\n"
            .to_string();
    }
    if let Some(id) = agent_kind.strip_prefix("expert:") {
        if expert_by_id(id).is_some() {
            return format!("\n{}\n", expert_lens_instructions(id));
        }
    }
    String::new()
}

pub fn build_arena_round2_prompt(
    diff_patch_path: &str,
    reviewer_id: &str,
    round: u8,
    findings_json: &str,
) -> String {
    format!(
        r#"You are reviewer "{reviewer_id}" in round {round} (cross-check).

Diff: `{diff_patch_path}`

Findings to vote on (JSON array):
{findings_json}

For EACH finding return exactly one vote: keep | drop | merge | escalate | lower | abstain | flag
Plus a one-sentence note that references the finding's content (required except abstain).

Self-review: for findings you proposed in round 1, you may lower/drop/withdraw.

Respond ONLY with JSON:
{{"ballots":[{{"finding_id":"...","vote":"keep","note":"...","merge_target":null}}]}}
"#,
        diff_patch_path = diff_patch_path.replace('\\', "/"),
    )
}

pub fn build_arena_round3_prompt(findings_summary_json: &str) -> String {
    format!(
        r#"You are the arena arbiter. Consolidate final verdicts.

Input (findings + round-2 votes):
{findings_summary_json}

For each finding_id return: verdict (kept|escalated|merged|dropped), confidence 0..1, rationale (1-3 sentences citing reviewers), merged_into when verdict is merged.

Respond ONLY with JSON:
{{"verdicts":[{{"finding_id":"...","verdict":"kept","confidence":0.82,"rationale":"...","merged_into":null}}]}}
"#
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

    // ── build_review_prompt_local_managed ──

    #[test]
    fn review_prompt_branch_scope_includes_base_branch() {
        let prompt = build_review_prompt_local_managed("main", "branch", "/tmp/er-test");
        assert!(
            prompt.contains("'main'"),
            "should include sanitized base branch"
        );
        assert!(prompt.contains("--unified=20 --no-color --no-ext-diff"));
        assert!(!prompt.contains("--staged"));
    }

    #[test]
    fn review_prompt_staged_scope_uses_staged_flag() {
        let prompt = build_review_prompt_local_managed("main", "staged", "/tmp/er-test");
        assert!(prompt.contains("--staged --unified=20 --no-color --no-ext-diff"));
    }

    #[test]
    fn review_prompt_unstaged_scope_no_base_branch() {
        let prompt = build_review_prompt_local_managed("main", "unstaged", "/tmp/er-test");
        assert!(prompt.contains("--unified=20 --no-color --no-ext-diff"));
        assert!(!prompt.contains("'main' --unified"));
        assert!(!prompt.contains("--staged"));
    }

    #[test]
    fn review_prompt_dangerous_branch_name_sanitized() {
        let prompt = build_review_prompt_local_managed("main; rm -rf /", "branch", "/tmp/er-test");
        assert!(prompt.contains("'main; rm -rf /'"));
        assert!(!prompt.contains("main; rm -rf / --unified"));
    }

    // ── build_questions_prompt_local_managed ──

    #[test]
    fn questions_prompt_branch_scope_includes_base() {
        let prompt = build_questions_prompt_local_managed("develop", "branch", "/tmp/er-test");
        assert!(prompt.contains("'develop'"));
    }

    #[test]
    fn questions_prompt_staged_scope() {
        let prompt = build_questions_prompt_local_managed("main", "staged", "/tmp/er-test");
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
        let prompt = build_review_prompt_local_managed("main", "branch", "/tmp/er-test");
        assert!(
            prompt.contains("'/tmp/er-test/diff-tmp'"),
            "annotation reads raw diff"
        );
        assert!(
            prompt.contains("'/tmp/er-test/diff-annotated'"),
            "annotation writes annotated diff"
        );
        assert!(prompt.contains("[h<hunk> L<file_line>]"));
        assert!(prompt.contains("diff-annotated"));
    }

    #[test]
    fn questions_prompt_includes_annotation_step() {
        let prompt = build_questions_prompt_local_managed("main", "branch", "/tmp/er-test");
        assert!(prompt.contains("'/tmp/er-test/diff-annotated'"));
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
        let prompt = build_review_prompt_local_managed("main", "branch", "/tmp/er-test");
        assert!(prompt.contains("--unified=20"));
        assert!(!prompt.contains("--unified=3"));
    }

    #[test]
    fn review_prompt_allows_reading_source_files() {
        let prompt = build_review_prompt_local_managed("main", "branch", "/tmp/er-test");
        // No "Do NOT read individual source files" — that rule is removed for review.
        assert!(!prompt.contains("Do NOT read individual source files"));
        // Must explicitly invite agentic verification.
        assert!(prompt.contains("Verify findings agentically"));
    }

    #[test]
    fn review_prompt_requires_findings_to_anchor_to_plus_or_minus() {
        let prompt = build_review_prompt_local_managed("main", "branch", "/tmp/er-test");
        assert!(prompt.contains("Findings **only** on `+` or `-` lines"));
    }

    #[test]
    fn review_prompt_per_finding_read_budget_no_global_cap() {
        let prompt = build_review_prompt_local_managed("main", "branch", "/tmp/er-test");
        assert!(prompt.contains("~10 reads per finding"));
        assert!(!prompt.contains("~30 total"));
        assert!(!prompt.contains("~50 total"));
    }

    #[test]
    fn review_prompt_includes_confidence_and_evidence() {
        let prompt = build_review_prompt_local_managed("main", "branch", "/tmp/er-test");
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

    // ── build_validate_prompt_local_managed ──

    #[test]
    fn validate_github_comments_prompt_reads_github_comments_json() {
        let prompt = build_validate_github_comments_prompt_prepared_diff("/tmp/out");
        assert!(prompt.contains("github-comments.json"));
        assert!(prompt.contains("outdated"));
        assert!(prompt.contains("diff-tmp"));
    }

    #[test]
    fn validate_prompt_reads_review_and_diff_annotated() {
        let prompt = build_validate_prompt_local_managed("main", "branch", "/tmp/er-test");
        assert!(prompt.contains("'/tmp/er-test'/review.json"));
        assert!(prompt.contains("'/tmp/er-test/diff-annotated'"));
    }

    #[test]
    fn validate_prompt_uses_unified_20_for_refresh() {
        let prompt = build_validate_prompt_local_managed("main", "branch", "/tmp/er-test");
        assert!(prompt.contains("--unified=20"));
    }

    #[test]
    fn validate_prompt_explains_confidence_transitions() {
        let prompt = build_validate_prompt_local_managed("main", "branch", "/tmp/er-test");
        assert!(prompt.contains("confirmed"));
        assert!(prompt.contains("informational"));
        assert!(prompt.contains("dropped"));
    }

    #[test]
    fn validate_prompt_branch_scope_includes_base() {
        let prompt = build_validate_prompt_local_managed("develop", "branch", "/tmp/er-test");
        assert!(prompt.contains("'develop'"));
    }

    #[test]
    fn validate_prompt_staged_scope_uses_staged_flag() {
        let prompt = build_validate_prompt_local_managed("main", "staged", "/tmp/er-test");
        assert!(prompt.contains("--staged --unified=20"));
    }

    // ── prepared diff prompts (desktop) ──

    #[test]
    fn prepared_review_prompt_hashes_existing_diff_tmp() {
        let prompt = build_review_prompt_prepared_diff("branch", "/tmp/er-managed");
        assert!(prompt.contains("'/tmp/er-managed/diff-tmp'"));
        assert!(prompt.contains("sha256sum"));
        assert!(prompt.contains("do **not** run `git diff`"));
        assert!(!prompt.contains("gh pr diff"));
    }

    #[test]
    fn prepared_review_prompt_annotates_from_diff_tmp() {
        let prompt = build_review_prompt_prepared_diff("branch", "/tmp/out");
        assert!(prompt.contains("'/tmp/out/diff-tmp'"));
        assert!(prompt.contains("'/tmp/out/diff-annotated'"));
    }

    #[test]
    fn prepared_validate_prompt_no_git_or_gh() {
        let prompt = build_validate_prompt_prepared_diff("branch", "/tmp/out");
        assert!(prompt.contains("'/tmp/out/diff-tmp'"));
        assert!(!prompt.contains("git diff"));
        assert!(!prompt.contains("gh pr"));
    }

    #[test]
    fn local_managed_review_prompt_targets_output_dir() {
        let prompt = build_review_prompt_local_managed("main", "branch", "/tmp/managed-er");
        assert!(prompt.contains("'/tmp/managed-er/diff-tmp'"));
        assert!(prompt.contains("`'/tmp/managed-er'/review.json`"));
        assert!(!prompt.contains("> .er/diff-tmp"));
        assert!(!prompt.contains("write results to `.er/`"));
    }

    #[test]
    fn local_managed_questions_prompt_targets_output_dir() {
        let prompt = build_questions_prompt_local_managed("main", "branch", "/tmp/managed-er");
        assert!(prompt.contains("'/tmp/managed-er'/questions.json"));
        assert!(prompt.contains("'/tmp/managed-er/diff-tmp'"));
        assert!(prompt.contains("'/tmp/managed-er/diff-annotated'"));
        assert!(prompt.contains("cp '/tmp/managed-er'/questions.json"));
        assert!(!prompt.contains("Read `.er/questions.json`"));
        assert!(!prompt.contains("> .er/diff-tmp"));
    }

    #[test]
    fn local_managed_summary_prompt_targets_output_dir() {
        let prompt = build_summary_prompt_local_managed("main", "staged", "/tmp/managed-er");
        assert!(prompt.contains("'/tmp/managed-er'/summary.md"));
        assert!(prompt.contains(
            "git diff --staged --unified=3 --no-color --no-ext-diff > '/tmp/managed-er'/diff-tmp"
        ));
        assert!(!prompt.contains("write the result to `.er/summary.md`"));
        assert!(!prompt.contains("> .er/diff-tmp"));
    }

    #[test]
    fn local_managed_specialized_prompts_target_output_dir() {
        let triage = build_triage_review_prompt_local_managed("main", "branch", "/tmp/out");
        assert!(triage.contains("'/tmp/out'/triage.json"));
        assert!(triage.contains("'/tmp/out/diff-tmp'"));
        assert!(!triage.contains("`.er/triage.json`"));

        let professor =
            build_professor_review_prompt_local_managed("main", "unstaged", "/tmp/out", None);
        assert!(professor.contains("'/tmp/out'/professor.json"));
        assert!(professor.contains("'/tmp/out/diff-tmp'"));
        assert!(!professor.contains("`.er/professor.json`"));

        let expert =
            build_expert_review_prompt_local_managed("main", "branch", "/tmp/out", "security");
        assert!(expert.contains("'/tmp/out'/experts/security.json"));
        assert!(expert.contains("'/tmp/out/diff-tmp'"));
        assert!(!expert.contains("`.er/experts/`"));
    }

    // ── review_rules_preamble + experts ──

    #[test]
    fn review_rules_preamble_no_style_category() {
        let preamble = review_rules_preamble(".er", false, FindingCaps::general(), None);
        assert!(!preamble.contains(
            "Categories: security, logic, performance, correctness, error-handling, style, testing"
        ));
        assert!(preamble.contains("**no `style`**"));
        assert!(preamble.contains("P0"));
        assert!(preamble.contains("two-dot"));
    }

    #[test]
    fn review_rules_preamble_expert_caps_stricter() {
        let expert = review_rules_preamble(".er", true, FindingCaps::expert(), None);
        let general = review_rules_preamble(".er", true, FindingCaps::general(), None);
        assert!(expert.contains("Max 2 findings per file, max 10 total"));
        assert!(general.contains("Max 4 findings per file, max 15 total"));
    }

    #[test]
    fn general_prompt_still_requests_four_output_files() {
        let prompt = build_review_prompt_prepared_diff("branch", "/tmp/out");
        assert!(prompt.contains("review.json"));
        assert!(prompt.contains("order.json"));
        assert!(prompt.contains("checklist.json"));
        assert!(prompt.contains("summary.md"));
        assert!(!prompt.contains("experts/"));
    }

    #[test]
    fn expert_prepared_prompt_targets_expert_json_only() {
        let prompt = build_expert_review_prompt_prepared_diff("branch", "/tmp/out", "security");
        assert!(prompt.contains("Expert lens: Security"));
        assert!(prompt.contains("experts/security.json"));
        assert!(prompt.contains("Max 2 findings per file, max 10 total"));
        assert!(prompt.contains("Write **only**"));
        assert!(prompt.contains("\"summary\""));
        assert!(prompt.contains("security posture"));
        assert!(!prompt.contains("### '/tmp/out'/review.json"));
    }

    #[test]
    fn testing_expert_prompt_asks_for_test_coverage_summary() {
        let prompt =
            build_expert_review_prompt_local_managed("main", "branch", "/tmp/er-test", "testing");
        assert!(prompt.contains("test coverage"));
        assert!(prompt.contains("\"summary\""));
    }

    #[test]
    fn patterns_expert_prompt_requires_grep() {
        let prompt =
            build_expert_review_prompt_local_managed("main", "branch", "/tmp/er-test", "patterns");
        assert!(prompt.contains("grep"));
        assert!(prompt.contains("Expert lens: Patterns"));
    }

    #[test]
    fn simplifying_expert_prompt_mentions_comments() {
        let prompt = build_expert_review_prompt_local_managed(
            "main",
            "branch",
            "/tmp/er-test",
            "simplifying",
        );
        assert!(prompt.contains("Expert lens: Simplifying"));
        assert!(prompt.contains("brief comment"));
    }

    #[test]
    fn mentorship_expert_prompt_is_positive_only() {
        let prompt = build_expert_review_prompt_prepared_diff("branch", "/tmp/out", "mentorship");
        assert!(prompt.contains("Expert lens: Mentorship"));
        assert!(prompt.contains("positive-only"));
        assert!(prompt.contains("experts/mentorship.json"));
    }

    #[test]
    fn professor_prompt_targets_professor_json_only() {
        let prompt = build_professor_review_prompt_prepared_diff("branch", "/tmp/out", None, false);
        assert!(prompt.contains("Professor lens"));
        assert!(prompt.contains("professor.json"));
        assert!(prompt.contains("category: \"professor\""));
        assert!(prompt.contains("\"summary\""));
        assert!(prompt.contains("teaching tone"));
        assert!(!prompt.contains("review.json"));
    }

    #[test]
    fn professor_prompt_includes_focus_when_set() {
        let prompt = build_professor_review_prompt_prepared_diff(
            "branch",
            "/tmp/out",
            Some("auth flow"),
            false,
        );
        assert!(prompt.contains("Learner focus"));
        assert!(prompt.contains("auth flow"));
    }

    #[test]
    fn file_scope_appendix_mentions_review_files() {
        let appendix = file_scope_appendix("/tmp/out");
        assert!(appendix.contains("review-files.txt"));
        assert!(appendix.contains("selected files only"));
    }

    #[test]
    fn triage_prompt_targets_triage_json_only() {
        let prompt = build_triage_review_prompt_local_managed("main", "branch", "/tmp/er-test");
        assert!(prompt.contains("triage.json"));
        assert!(prompt.contains("verdict"));
        assert!(prompt.contains("≤8 tool calls"));
        assert!(prompt.contains("priority_files"));
        assert!(!prompt.contains("`.er/review.json`"));
    }

    #[test]
    fn triage_prepared_prompt_mentions_routing_verdicts() {
        let prompt = build_triage_review_prompt_prepared_diff("branch", "/tmp/out");
        assert!(prompt.contains("general|expert|arena|professor|skip"));
        assert!(prompt.contains("triage.json"));
    }
}
