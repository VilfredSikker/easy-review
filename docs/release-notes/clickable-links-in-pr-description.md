# Clickable links in the PR description

## What changed

Bare `http(s)://…` URLs in a PR's Description block (desktop) are now rendered
as clickable links. Previously only markdown-style `[text](url)` links were
linkified; a plain URL — the common case when someone pastes a preview/deploy
link into a PR body — showed as inert text.

Clicking an autolinked URL behaves like every other link in the markdown
renderer: it opens in the system browser via `open_url_in_browser` (the
`onExternalLinkClick` handler on `MarkdownText`), never in-app.

## Why it's safe

The autolinker runs as the **last** step of `renderInline`, over HTML we've
already generated and HTML-escaped, and it skips any URL that is already part
of a tag — the character preceding the URL must not be `"` (an `href` value),
`>` (anchor text or a code span), `=` (an attribute), or a word char. So a URL
inside a markdown link, a code span, or an existing anchor is never
double-wrapped, and the existing href-escaping guard (quotes → `&quot;`) is
preserved. Query strings keep their escaped `&amp;` inside a single href, and
trailing sentence punctuation / unbalanced closing parens are peeled out of the
link so they don't get swallowed — except a trailing `;` that terminates an
escaped HTML entity (`&amp;`, `&gt;`, …), which is kept so the URL isn't
corrupted.

## Scope note: PR description images

A related ask was inlining images embedded in PR descriptions (e.g. an
`<img src="https://github.com/user-attachments/assets/…">`). That is **not**
included here: `user-attachments` asset URLs are gated behind the viewer's
GitHub session and aren't anonymously hotlinkable, and the desktop webview has
no GitHub credentials. Displaying them would require a backend pipeline that
downloads the asset with authenticated `gh`, caches it under the app data dir,
and serves the webview a local file URL — a separate change. As a lighter
interim, an embedded image could be surfaced as a clickable link to the asset
(opens in the browser, where the user is authenticated).

## Implementation

- `desktop-ui/src/lib/markdown.ts` — new `linkifyUrls()` helper, applied as the
  final transform in `renderInline()`.
- `desktop-ui/src/lib/markdown.test.ts` — covers autolinking, escaped query
  strings, trailing-punctuation/paren peeling, and the no-double-link cases
  (markdown links, code spans).
