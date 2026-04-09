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
        "unstaged" => "--unified=3 --no-color --no-ext-diff".to_string(),
        "staged" => "--staged --unified=3 --no-color --no-ext-diff".to_string(),
        _ => format!("{safe_base_branch} --unified=3 --no-color --no-ext-diff"),
    };

    format!(
        r#"You are a code reviewer. Perform a thorough review of the current git diff and write results to the `.er/` directory.

## Instructions

1. Run: `git diff {diff_args} > .er/diff-tmp && (sha256sum .er/diff-tmp 2>/dev/null || shasum -a 256 .er/diff-tmp)`
   - Save the SHA-256 hash as `diff_hash`
2. Read `.er/diff-tmp` to get the full diff
3. Analyse every changed file and hunk. For each file determine:
   - `risk`: "high" | "medium" | "low" | "info"
   - `risk_reason`: why this risk level
   - `summary`: one-line description of changes
   - `findings`: array of issues found (max 3-4 per file)
4. Write these four files:

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
          "responses": []
        }}
      ]
    }}
  }}
}}
```

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
- Pin findings to hunks using `hunk_index` (0-based).
- Risk levels: high = likely bug or security issue, medium = code smell or missing edge case, low = style, info = observation.
- Keep finding titles under 60 characters.
- The `suggestion` field should be actionable.
- Max 3-4 findings per file, max 15 total.
- The checklist should be things the reviewer should manually verify.
- Categories: security, logic, performance, correctness, error-handling, style, testing.
- Ensure `.er/` directory exists before writing: `mkdir -p .er`

## Speed

Target: complete in under 90 seconds. Read the diff once, analyse in-context, write all files.
Do NOT read individual source files — the diff contains everything needed."#
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

    format!(
        r#"You are answering code review questions. Read the questions file and the diff, then provide answers.

## Instructions

1. Read `.er/questions.json`
   - If it doesn't exist or has no unresolved questions: print "No questions to answer" and stop.
2. Run: `git diff {diff_args} > .er/diff-tmp`
3. Read `.er/diff-tmp` to get the full diff context
4. For each question where `resolved == false` and no existing reply (no entry with `in_reply_to` == that question's `id`):
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
5. Write the updated `.er/questions.json`
6. Back up: `cp .er/questions.json .er/questions.prev.json`

## Answer Quality

- Actually read the code the human is asking about. Don't give generic answers.
- If they ask "why?", explain with specifics from the diff.
- Keep responses concise — they render in a TUI with limited width.
- Never be defensive. If the code is fine, say so.

## Speed

Target: complete in under 60 seconds. Read the diff once, answer all questions in-context."#
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
    format!(
        r#"You are a code reviewer. Perform a thorough review of the GitHub PR diff and write results to `{safe_output_dir}/`.

## Instructions

1. Run: `gh pr diff {pr_number} --repo {safe_owner}/{safe_repo} > {safe_output_dir}/diff-tmp && (sha256sum {safe_output_dir}/diff-tmp 2>/dev/null || shasum -a 256 {safe_output_dir}/diff-tmp)`
   - Save the SHA-256 hash as `diff_hash`
2. Read `{safe_output_dir}/diff-tmp` to get the full diff
3. Analyse every changed file and hunk. For each file determine:
   - `risk`: "high" | "medium" | "low" | "info"
   - `risk_reason`: why this risk level
   - `summary`: one-line description of changes
   - `findings`: array of issues found (max 3-4 per file)
4. Write these four files:

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
          "responses": []
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
- Pin findings to hunks using `hunk_index` (0-based).
- Risk levels: high = likely bug or security issue, medium = code smell or missing edge case, low = style, info = observation.
- Keep finding titles under 60 characters.
- The `suggestion` field should be actionable.
- Max 3-4 findings per file, max 15 total.
- The checklist should be things the reviewer should manually verify.
- Categories: security, logic, performance, correctness, error-handling, style, testing.
- Ensure `{safe_output_dir}/` directory exists before writing: `mkdir -p {safe_output_dir}`

## Speed

Target: complete in under 90 seconds. Read the diff once, analyse in-context, write all files.
Do NOT read individual source files — the diff contains everything needed."#
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

/// Build wizard tour prompt for remote mode (uses gh pr diff instead of git diff).
///
/// The prompt instructs the agent to:
/// 1. Fetch the PR diff via `gh pr diff`
/// 2. Analyse changes and build a guided tour
/// 3. Write `output_dir/wizard.json`
pub fn build_wizard_prompt_remote(
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
        r#"You are generating a guided tour of the important changes in a GitHub PR. Read the diff and produce a wizard tour file.

## Instructions

1. Run: `mkdir -p {safe_output_dir} && gh pr diff {pr_number} --repo {safe_owner}/{safe_repo} > {safe_output_dir}/diff-tmp && (sha256sum {safe_output_dir}/diff-tmp 2>/dev/null || shasum -a 256 {safe_output_dir}/diff-tmp)`
   - Save the SHA-256 hash as `diff_hash`
2. (Optional) Read `{safe_output_dir}/review.json` if it exists — for additional context, not required
3. Read `{safe_output_dir}/diff-tmp` to get the full diff
4. Analyse the changes and identify fundamental, important, and supporting files
5. Write `{safe_output_dir}/wizard.json`

### `{safe_output_dir}/wizard.json`
```json
{{
  "version": 1,
  "diff_hash": "<sha256 from step 1>",
  "tour": [
    {{
      "path": "src/auth.rs",
      "importance": "fundamental",
      "summary": "What changed and why (1-3 sentences)",
      "key_changes": ["Specific change 1", "Specific change 2"],
      "related_files": ["src/middleware.rs"]
    }}
  ]
}}
```

## Importance levels

| Level | Meaning |
|-------|---------|
| `fundamental` | Core change — the reason this PR exists |
| `important` | Directly supports or enables the fundamental changes |
| `supporting` | Completes the picture but not essential to understand first |

## Tour design rules

- **Include every file that matters** — don't skip files just because they're small
- **Order by understanding flow** — fundamental changes first, then dependencies, then supporting
- **Group related files** — if 3 files work together, put them adjacent in the tour
- **Write clear summaries** — explain what changed AND why it matters
- **Key changes as bullets** — concrete, specific (not vague)
- **Link related files** — if file A calls into file B, note the relationship

## Guidelines

- Every tour entry should be anchored to actual diff content — no speculative entries
- Summaries should teach the reader what to look for, not just describe line changes
- If the diff is trivial, produce a minimal tour

## Speed

Target: complete in under 45 seconds. Read the diff once, analyse in-context, write the file."#
    )
}

/// Build wizard tour prompt with repo context substituted.
pub fn build_wizard_prompt(base_branch: &str, scope: &str) -> String {
    let safe_base_branch = sanitize_for_shell(base_branch);
    let safe_base_branch = safe_base_branch.replace('{', "{{").replace('}', "}}");
    let diff_args = match scope {
        "unstaged" => "--unified=3 --no-color --no-ext-diff".to_string(),
        "staged" => "--staged --unified=3 --no-color --no-ext-diff".to_string(),
        _ => format!("{safe_base_branch} --unified=3 --no-color --no-ext-diff"),
    };

    format!(
        r#"You are generating a guided tour of the important changes in the current git diff.

## Instructions

1. Ensure `.er/` exists: `mkdir -p .er`
2. Run: `git diff {diff_args} > .er/diff-tmp && (sha256sum .er/diff-tmp 2>/dev/null || shasum -a 256 .er/diff-tmp)`
   - Save the SHA-256 hash as `diff_hash`
3. Optionally read `.er/review.json` if it exists for extra context
4. Read `.er/diff-tmp`
5. Write `.er/wizard.json`

### `.er/wizard.json`
```json
{{
  "version": 1,
  "diff_hash": "<sha256 from step 2>",
  "tour": [
    {{
      "path": "src/auth.rs",
      "importance": "fundamental",
      "summary": "What changed and why it matters.",
      "key_changes": ["Specific change 1", "Specific change 2"],
      "related_files": ["src/middleware.rs"]
    }}
  ]
}}
```

## Guidelines

- Importance must be one of `fundamental`, `important`, or `supporting`.
- Include every changed file that matters to understanding the diff.
- Focus on understanding, not bug-finding.
- Keep summaries concrete and grounded in actual changes."#
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
    format!(
        r#"You are answering code review questions. Read the questions file and the PR diff, then provide answers.

## Instructions

1. Read `{safe_output_dir}/questions.json`
   - If it doesn't exist or has no unresolved questions: print "No questions to answer" and stop.
2. Run: `gh pr diff {pr_number} --repo {safe_owner}/{safe_repo} > {safe_output_dir}/diff-tmp`
3. Read `{safe_output_dir}/diff-tmp` to get the full diff context
4. For each question where `resolved == false` and no existing reply (no entry with `in_reply_to` == that question's `id`):
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
5. Write the updated `{safe_output_dir}/questions.json`
6. Back up: `cp {safe_output_dir}/questions.json {safe_output_dir}/questions.prev.json`

## Answer Quality

- Actually read the code the human is asking about. Don't give generic answers.
- If they ask "why?", explain with specifics from the diff.
- Keep responses concise — they render in a TUI with limited width.
- Never be defensive. If the code is fine, say so.

## Speed

Target: complete in under 60 seconds. Read the diff once, answer all questions in-context."#
    )
}

/// Build quiz generation prompt with repo context substituted.
pub fn build_quiz_prompt(base_branch: &str, scope: &str) -> String {
    let safe_base_branch = sanitize_for_shell(base_branch);
    let safe_base_branch = safe_base_branch.replace('{', "{{").replace('}', "}}");
    let diff_args = match scope {
        "unstaged" => "--unified=3 --no-color --no-ext-diff".to_string(),
        "staged" => "--staged --unified=3 --no-color --no-ext-diff".to_string(),
        _ => format!("{safe_base_branch} --unified=3 --no-color --no-ext-diff"),
    };

    format!(
        r#"Generate a comprehension quiz for the current git diff and write `.er/quiz.json`.

## Instructions

1. Ensure `.er/` exists: `mkdir -p .er`
2. Run: `git diff {diff_args} > .er/diff-tmp && (sha256sum .er/diff-tmp 2>/dev/null || shasum -a 256 .er/diff-tmp)`
   - Save the SHA-256 hash as `diff_hash`
3. Optionally read `.er/review.json` if it exists
4. Read `.er/diff-tmp`
5. Write `.er/quiz.json`

### `.er/quiz.json`
```json
{{
  "version": 1,
  "diff_hash": "<sha256>",
  "questions": [
    {{
      "id": "q1",
      "level": 2,
      "category": "correctness",
      "text": "Why was this change made?",
      "options": [
        {{"label": "A", "text": "Option A", "is_correct": false}},
        {{"label": "B", "text": "Option B", "is_correct": true}},
        {{"label": "C", "text": "Option C", "is_correct": false}},
        {{"label": "D", "text": "Option D", "is_correct": false}}
      ],
      "freeform": false,
      "expected_reasoning": "",
      "explanation": "Teach the concept behind the right answer.",
      "related_file": "src/file.rs",
      "related_hunk": 0,
      "related_lines": [10, 20]
    }}
  ]
}}
```

## Guidelines

- Produce 2-6 questions depending on diff size.
- Mix multiple-choice and freeform when the diff is non-trivial.
- Use categories like `security`, `logic`, `performance`, `correctness`, `error-handling`, `testing`.
- Anchor every question to a real diff change."#
    )
}

/// Build quiz review prompt.
pub fn build_quiz_review_prompt() -> String {
    r#"Review quiz answers and write teaching feedback to `.er/quiz-feedback.json`.

## Instructions

1. Read `.er/quiz.json`
   - If missing, print `No quiz found.` and stop.
2. Read `.er/quiz-answers.json`
   - If missing, print `No answers found.` and stop.
3. Compute the SHA-256 of `.er/quiz.json` and save it as `quiz_hash`
4. Evaluate each submitted answer against the corresponding question
5. Write `.er/quiz-feedback.json`

### `.er/quiz-feedback.json`
```json
{
  "version": 1,
  "quiz_hash": "<sha256 of quiz.json>",
  "score": {
    "correct": 2,
    "partial": 1,
    "total": 3,
    "points": 2.5
  },
  "items": [
    {
      "question_id": "q1",
      "answer_type": "choice",
      "answer_given": "B",
      "correct": true,
      "feedback": "Short teaching feedback."
    }
  ]
}
```

## Guidelines

- Feedback should be 1-4 sentences per question.
- For freeform answers, use `partial: true` and `points: 0.5` when the answer is partly right.
- Ground feedback in the actual quiz content and expected reasoning.
- Do not modify the quiz or answers files."#
        .to_string()
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
        assert!(prompt.contains("--unified=3 --no-color --no-ext-diff"));
        assert!(!prompt.contains("--staged"));
    }

    #[test]
    fn review_prompt_staged_scope_uses_staged_flag() {
        let prompt = build_review_prompt("main", "staged");
        assert!(prompt.contains("--staged --unified=3 --no-color --no-ext-diff"));
    }

    #[test]
    fn review_prompt_unstaged_scope_no_base_branch() {
        let prompt = build_review_prompt("main", "unstaged");
        assert!(prompt.contains("--unified=3 --no-color --no-ext-diff"));
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
}
