# `gh` errors are classified by HTTP status first, then stderr

`gh` is spawned (ADR 0026), and callers used to match stderr text to tell a 404 from a 403 from a rate limit. `er-github` now classifies once, in `classify.rs`: with `gh api --include` the HTTP status line and headers are read directly; without it, or when no response block carries a status, a stderr table runs. `GhError` names the outcomes (not found, forbidden, rate limited with a reset time, too large, conflict, unprocessable, network, not installed, timeout, unauthenticated).

## The rule order

1. A spawn failure is `NotInstalled`, `Timeout`, or `Tool`.
2. Exit 0 succeeds; a 2xx rate-limit header on a success is not classified.
3. A non-zero `gh api` exit with `--include`: exit 4 is `Unauthenticated`; the first block with status >= 400 picks the variant (401, 404, 403 unless it is a rate limit, 429, 409, 422, 406/413 as `TooLarge`); with no such block, a block whose `x-ratelimit-remaining` is 0 is `RateLimited`; otherwise the stderr table runs.
4. The stderr table matches `HTTP NNN` first, then GitHub's own phrases (`Could not resolve to a`, rate limit, connection/TLS markers, auth markers), else `Tool`.

A GraphQL query can answer HTTP 200 with an `errors` array; the stderr line then carries the reason, which is why it falls through to rule 4.

## Messages

An HTTP-backed message keeps GitHub's top-level `message` and the first `errors[].message`, gh's stderr line, and `(HTTP NNN)` when not already present. A review 422 carries its reasons as plain `errors` strings, so `Unprocessable` keeps the top-level message plus every entry joined with `; `; otherwise the matcher that looks for "one pending review" or "approve your own" would lose the text. When the body carries no message and no errors, gh's stderr line is kept instead.

## Pagination

Without `--paginate`, exactly one leading header block is stripped and every byte after it is the body, so a body that contains its own `HTTP/` text passes through unchanged.

`gh api --include --paginate` prints a status line and headers before every page. A page boundary starts at offset 0, after `\n`, or immediately after `]` or `}`, at bracket depth 0 and outside a JSON string: the scanner tracks quotes and `\` escapes, because a review comment can contain `[RFC]HTTP/1.1 200 OK`. String and bracket state is tracked only in page bodies; header blocks are skipped, so a quote in a header value cannot hide a later page. Header blocks are stripped and the bodies are concatenated byte for byte.

`--paginate` together with `--jq` is not supported: gh applies the filter to each page, so the joined output no longer has a reliable boundary to split.

## Fixtures

Every classification rule is pinned by a capture under `crates/er-github/tests/fixtures/gh/<case>/` (`args`, `exit`, `stdout`, `stderr`, `expect`, optional `message`). The directory name has no suffix for a real capture and ends in `.synthetic` for a constructed one, so a real capture replaces a fabricated case by dropping the suffix.
