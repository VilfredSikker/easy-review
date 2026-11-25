# Comment threads are one level deep, enforced at the input layer

A GitHub comment may have replies; a reply may not have a reply. Parent plus N children keeps the comment model, both inline renderers, and the two-way GitHub sync free of recursive nesting. The rule is not enforced where comments are written, though: the service's `reply_to_thread` (`crates/er-service/src/annotations.rs`) and `reply_to_github_thread` (`crates/er-service/src/github_comments.rs`) both accept any parent id and will happily store a reply to a reply. It is enforced at the entry points instead — the TUI's `r` key checks `CommentRef::can_reply()` (`in_reply_to.is_none()`) before opening a reply draft (`start_reply` in `crates/er-tui/src/service/threads.rs`), and the desktop offers Reply only on a thread root.

## Consequences

A new caller that passes a reply's id as the parent writes a two-deep thread that nothing rejects, and the renderers and the GitHub push path assume they will never see one. Either check the parent at the new entry point, or push the guard down into the service's reply writers and delete the surface-level copies.

Questions and notes obey the same one-level rule through a different guard: `validate_parent_thread` in `crates/er-engine/src/pr_review_feedback.rs` bails when the external feedback API is asked to reply to a reply.
