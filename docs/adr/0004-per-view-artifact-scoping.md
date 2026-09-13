# Artifacts are scoped per view, with two deliberate exceptions

A local PR tab can review two different diffs — the branch's own work, and the pull request's head against its base. They are the *same* diff whenever local HEAD equals the PR head, which is the common case and the reason ADR 0025 reuses a tour across buckets on a matching hash; they diverge as soon as the branch has unpushed commits or the PR head is not what is checked out. Both views run the same code against the same base and differ only in `pr_head_ref` (`None` for the branch, `refs/er/pr/<N>/head` for the PR), so nothing in the system can tell you which case you are in — only the hash does.

The buckets exist for the divergent case, where showing one view's artifact as describing the other would be wrong. Triage, `review.json`, professors and experts, questions and notes, `reviewed` and `checklist.json` are therefore split per view bucket, not per branch. Two artifacts are treated differently: `github-comments.json` is always PR-scoped, and the guided tour is per-view but will reuse a tour from the other bucket when its diff hash matches the active diff — which is how the common case avoids paying for two generations.

## Consequences

GitHub comments belong to the pull request rather than to a way of looking at it — they sync with GitHub in both directions, where a bucket has no meaning. So `github_comments_dir()` resolves to the PR bucket regardless of the active view, and the comments are cleared in Unstaged, Staged and History, where PR line anchors do not apply to the lines on screen.

Tour reuse means identical branch and PR diffs pay for one generation, while drift splits them into two. The read crosses buckets, so the Guide can show a tour whose file lives in the other bucket's directory; generation still writes to the bucket of the view it was launched from, which keeps "re-run" landing where the next read will look.
