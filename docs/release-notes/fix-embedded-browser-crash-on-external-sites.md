# Fix: embedded browser could crash the whole app on external sites

## What changed

Opening the in-app review browser and navigating to a real external site (e.g.
`tech-professor.com`) could crash Easy Review. The `erp://` / `erps://` proxy
that backs the embedded browser builds an HTTP redirect response from the
upstream site's `Location` header. That value is fully controlled by the remote
server, and real sites occasionally send redirect targets containing bytes that
are not valid in an HTTP header value (raw UTF-8, control characters, spaces).
Building the header with such a value fails, and the code then called
`.unwrap()` on the result.

Because the proxy runs **synchronously inside the native webview's URI-scheme
callback**, that panic unwound into non-Rust frames and aborted the entire app
instead of just failing the page load. A local dev server (`localhost:5173`) —
the common case — never sends such a `Location`, which is why the crash only
showed up on real external sites.

## The fix

Two layers:

1. **Targeted** — `browser_redirect_response` now validates the rewritten
   `Location` and, when it is not a valid header value, degrades to the existing
   HTML navigation handoff (which carries the URL in a JS-escaped body, not a
   header). The redirect still happens; nothing crashes.

2. **Defense in depth** — the `erp(s)://` handlers are wrapped so any panic in
   the proxy is caught and turned into a `500` page. A single bad upstream
   response can no longer take the app down with it.

## Why it's safe

The happy path (a header-safe `Location`) is unchanged and still covered by the
existing test. A new regression test exercises a redirect whose `Location`
contains a space and a non-ASCII byte and asserts it falls back to the handoff
instead of panicking.
