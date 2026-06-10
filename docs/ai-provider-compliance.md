# AI Provider Compliance Note

> Internal decision record — **not legal advice.** If `er` is ever commercialized, hosted,
> or starts collecting data, the assumptions here no longer hold: get the corresponding
> legal documents reviewed by an actual lawyer (EU / GDPR applies, since we are EU-based).

_Last reviewed: 2026-06-10_

## Context

`er` is used alongside AI coding tools — Claude Code, OpenAI Codex, Cursor. A common
workflow is to take reviews from more than one provider and reconcile their findings
against each other (see the `/review-arena` and `/review-panel` skills). This note records
**why that is allowed** and **where the lines are**, so future changes don't drift across
them without anyone noticing.

## Position: `er` is an application on top of LLMs, not a competing model

Every major provider's terms (Anthropic, OpenAI, Cursor) restrict using their output to
**develop or train a model or service that competes with them.** The axis that matters is
*model vs. application* — **not** *review vs. generate*:

- A code-*generation* tool (Cursor, Codex) is fine — it is an API **customer**, not a rival model.
- A code-*review* tool would still violate the clause if it trained its own model on harvested outputs.

So "we review instead of generate" is not what keeps us compliant. What keeps us compliant
is that `er` is unambiguously an application:

- It does **not** run or host any model.
- Today it does **not** even call provider APIs — it reads `.er/` files generated in the
  user's own AI session and renders them. The **user** is the provider's customer; `er` is a
  file viewer.
- It **drives** usage of those tools rather than replacing them.

**Conclusion:** `er` does not qualify as a competing product under any of the three
providers' terms.

## Lines we will not cross

1. **Do not train a model on aggregated outputs.** Collecting Claude / Codex / Cursor
   reviews into a dataset to fine-tune an in-house "ER reviewer model" is exactly what the
   competing-model clause targets. This is the bright line.
2. **Do not resell raw API access as "our AI."** Selling a review UX that happens to use
   provider APIs is fine; thinly wrapping an API and selling it as a model service is not.
3. **Do not publish head-to-head benchmark comparisons** ("Claude vs Codex vs Cursor review
   quality") for marketing without first checking each provider's current benchmarking
   terms. Private / internal comparison is fine.

## Triggers that change this analysis

If any of these becomes true, this note is no longer sufficient — get the corresponding
legal docs and a lawyer:

| Trigger | New obligations |
|---|---|
| `er` becomes hosted SaaS, or calls provider APIs on a user's behalf | ToS, Privacy Policy, provider usage-policy **flow-down** to end users, commercial API agreements |
| `er` charges money / adds accounts | ToS, billing & refund terms |
| `er` adds telemetry / analytics | Privacy Policy + consent (GDPR — we are EU-based) |
| `er` stores user code or reviews on a backend | Privacy Policy, DPA, GDPR data-processing terms |

## Current legal footing (2026-06-10)

- **License:** MIT (`LICENSE`, set across the workspace via `license = "MIT"`).
- **Telemetry:** none. No analytics / telemetry SDKs in the codebase
  (verified: no PostHog, Sentry, Mixpanel, or Amplitude references).
- **Data transmission:** none by `er` itself. It shells out to the user's local `git` / `gh`;
  the desktop app's only HTTP client (`ureq` in `crates/er-desktop/src/browser_proxy.rs`)
  proxies the webview's **own** requests — it does not collect data centrally.
- **Legal docs required today:** none beyond the license. `er` is a local, no-server,
  no-data-collection tool, so there is no service relationship (no ToS) and no data
  collection (no Privacy Policy).
