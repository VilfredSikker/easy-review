/// Embedded prompt templates for guided AI actions.
///
/// These replicate the logic from the external Claude Code skills
/// (`/er-review`, `/er-questions`) so the TUI can invoke them directly
/// via the configured agent command without requiring skill files.
/// Build the review prompt with repo context substituted.
///
/// The prompt instructs the agent to:
/// 1. Read the diff via `git diff`
/// 2. Analyse all files
/// 3. Write `.er/review.json`, `.er/order.json`, `.er/checklist.json`, `.er/summary.md`
pub fn build_review_prompt(base_branch: &str, scope: &str) -> String {
    let diff_args = match scope {
        "unstaged" => "--unified=3 --no-color --no-ext-diff".to_string(),
        "staged" => "--staged --unified=3 --no-color --no-ext-diff".to_string(),
        _ => format!("{base_branch} --unified=3 --no-color --no-ext-diff"),
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
    let diff_args = match scope {
        "unstaged" => "--unified=3 --no-color --no-ext-diff".to_string(),
        "staged" => "--staged --unified=3 --no-color --no-ext-diff".to_string(),
        _ => format!("{base_branch} --unified=3 --no-color --no-ext-diff"),
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
    format!(
        r#"You are a code reviewer. Perform a thorough review of the GitHub PR diff and write results to `{output_dir}/`.

## Instructions

1. Run: `gh pr diff {pr_number} --repo {owner}/{repo} > {output_dir}/diff-tmp && (sha256sum {output_dir}/diff-tmp 2>/dev/null || shasum -a 256 {output_dir}/diff-tmp)`
   - Save the SHA-256 hash as `diff_hash`
2. Read `{output_dir}/diff-tmp` to get the full diff
3. Analyse every changed file and hunk. For each file determine:
   - `risk`: "high" | "medium" | "low" | "info"
   - `risk_reason`: why this risk level
   - `summary`: one-line description of changes
   - `findings`: array of issues found (max 3-4 per file)
4. Write these four files:

### `{output_dir}/review.json`
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

### `{output_dir}/order.json`
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

### `{output_dir}/checklist.json`
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

### `{output_dir}/summary.md`
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
- Ensure `{output_dir}/` directory exists before writing: `mkdir -p {output_dir}`

## Speed

Target: complete in under 90 seconds. Read the diff once, analyse in-context, write all files.
Do NOT read individual source files — the diff contains everything needed."#
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
    format!(
        r#"You are answering code review questions. Read the questions file and the PR diff, then provide answers.

## Instructions

1. Read `{output_dir}/questions.json`
   - If it doesn't exist or has no unresolved questions: print "No questions to answer" and stop.
2. Run: `gh pr diff {pr_number} --repo {owner}/{repo} > {output_dir}/diff-tmp`
3. Read `{output_dir}/diff-tmp` to get the full diff context
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
5. Write the updated `{output_dir}/questions.json`
6. Back up: `cp {output_dir}/questions.json {output_dir}/questions.prev.json`

## Answer Quality

- Actually read the code the human is asking about. Don't give generic answers.
- If they ask "why?", explain with specifics from the diff.
- Keep responses concise — they render in a TUI with limited width.
- Never be defensive. If the code is fine, say so.

## Speed

Target: complete in under 60 seconds. Read the diff once, answer all questions in-context."#
    )
}
