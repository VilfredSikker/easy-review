# REVIEW.md is read from the base branch and handed to the reviewers

A team had no way to tell er's AI reviewers what matters in its repo: which paths to skip, what "important" means here, how many nits are too many. No prompt named a `REVIEW.md`, other agent CLIs never saw one, and a remote review with no clone got nothing.

er reads `REVIEW.md` at the repo root and in each ancestor folder of a changed file, at the base ref, with `git ls-tree` and `git cat-file` (ADR 0001). With no local clone it reads only the root file with `gh api …/contents/REVIEW.md?ref=<base>` (ADR 0026). It never reads the working tree or the PR head, so a change cannot edit the rules it is reviewed under. The text goes to `review-md.md` in the view bucket and, for arena runs, inline into the prompt. Only the exact name `REVIEW.md` counts.

## Not configuration

ADR 0005 and ADR 0036 keep repo-local settings out. This changes no er setting and nothing in er reads it as one: it is never written back, has no reload path and cannot shadow a global setting. It is input to the agent, like the diff. The failure ADR 0005 records, a repo layer permanently shadowing global saves, has no counterpart here.

## Not a runtime prompt

ADR 0023 keeps prompts compiled in. The instructions that say how to use the file stay compiled in. The file is repository content the agent reads, the same way it reads source files.

## Precedence

`REVIEW.md` may narrow a review: skip paths, a lower nit count, extra evidence, what counts as important. It cannot change the output format or the anchor rules, or raise the finding caps. Where it conflicts with those, the compiled rules win.

## Consequences

- A remote review with no clone sees only the root file.
- A `REVIEW.md` change takes effect only after it merges to the base branch.
- Each review spawn costs one `git ls-tree` plus one `git cat-file` per file found.
- Any read failure means no file, never a failed review.
- Agents running in the repo still load `CLAUDE.md` themselves; that is unchanged.
