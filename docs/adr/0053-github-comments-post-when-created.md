# GitHub comments post when created

Creating a GitHub comment, thread or reply posts it to the pull request immediately, and `github-comments.json` records it only after GitHub returns it. There is no "unpushed local GitHub comment" state: a create that does not reach GitHub leaves nothing behind but the draft. This reverses the assumption behind the bulk push paths (removed in P06): a GitHub comment is not a local artifact waiting to be sent.

## Deciding a retry

A create has three outcomes: GitHub accepted it, GitHub refused it before doing anything (`4xx`, or "one pending review"), or the POST ran and its fate is unknown (timeout, `5xx`, an unparseable success body). The third case is the reason for the rest of this record: a blind retry would post a second copy, and a duplicate is visible to other people and cannot be undone by `er`.

- **Reconcile before retry.** An unknown outcome is remembered under the create's client id, with every body attempted, the ids already in the store, the first attempt's start time, and a not-before time of five seconds. A retry before that time neither lists nor posts; only an attempt that actually listed or posted moves the deadline. After it, the PR's review comments are listed, and a match is adopted instead of posting again.
- **The list fails closed.** A parse error in the list is an unknown outcome, not an empty list. The old `--paginate` splitter counted brackets inside strings and skipped bad chunks, so a partial list looked complete; it is replaced by a streaming parse that propagates every error.
- **The match rule.** A listed comment counts only when its id predates neither the attempt nor `known_ids`, its author is the viewer, its body is one of the attempted bodies, its timestamp is within a minute of the first attempt, and its target fits: same path and range (current or original) for a line comment, same `in_reply_to_id` for a reply. The greatest `(created_at, id)` match wins. `known_ids` stops an identical earlier comment on the same line from being mistaken for the retry.
- **The in-flight guard is per process.** A client id is claimed for the duration of an attempt by an RAII guard, so a second submit of the same draft is refused, and an early return cannot wedge the id. It is not cross-process: two `er` processes on one draft are outside this rule.
- **General comments are posted and not recorded.** They go to the issues API, which no comment pull reads; recording one would drop it on the next pull. They are visible in the PR conversation. `known_ids` therefore cannot exclude an identical earlier issue comment; the author, body and time checks bound that risk.

## The pending-review experiment

GitHub allows one pending review per reviewer per PR. A standalone line comment is refused while it exists. A **reply** while it exists was tested on a real pull request: GitHub refuses it with HTTP 422 ("user_id can only have one pending review per pull request") and does **not** attach it to the pending review. The reply path therefore needs no pre-check; the 4xx classification reports it as not posted, with the same message as the line-comment path.

## Consequences

- **Creation now needs the network.** A GitHub-kind composer keeps its text and shows the error when the post fails; it closes only on success. The desktop's diff composer already worked this way through the pending review (ADR 0049).
- **A pull fetched before a post can rebuild the sidecar without the new entry.** The pull writes `github-comments.json` after fetching outside the lock; a post that lands in between is on GitHub but absent from the sidecar until the next pull rebuilds it. Do not close this with a lock around the write. A generation counter on the PR-comment bundle cache stops a fetch that raced an invalidation from re-caching stale data for 60 seconds.
- **Drafts are restored whole.** `SessionState` carries the reply target, finding reference, edit id, side, end line and client id, so a restored finding reply is still a private finding reply and never posts as a public line comment.
- **Promote removes its source only on success.** A refusal or unknown outcome keeps the question, note or finding, so the next send is the same create (the client id is derived from the source id) and reconciles instead of posting twice.
- **A tab known to have no PR refuses the GitHub composer.** The TUI's `c` and "Comment on PR" check `pr_absent`, which the startup probe or a failed sync records; no `gh` call runs on a key press. A tab whose PR is still unknown opens the composer and reports the failure on submit. The composer never silently becomes a question: a reviewer who pressed `c` meant a public comment.
- **An adopted edit stays in the composer.** When a retry adopts an earlier attempt whose body differs from the current text, the comment that landed is recorded, the composer keeps the edited text, and it says the edit was not posted. The client id is cleared, so sending again is a deliberate new comment rather than another reconcile.
