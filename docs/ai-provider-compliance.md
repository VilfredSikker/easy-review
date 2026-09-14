# AI Provider Compliance Note

> Internal decision record — **not legal advice.** If `er` is ever commercialized, hosted,
> or starts collecting data, the assumptions here no longer hold: get the corresponding
> legal documents reviewed by an actual lawyer (EU / GDPR applies, since we are EU-based).

_Last reviewed: 2026-09-13_

## Context

`er` is used alongside AI coding tools — Claude Code, OpenAI Codex, Cursor, OpenCode. A common
workflow is to take reviews from more than one provider and reconcile their findings against
each other (in `er`, the arena — ADR 0024 — and the `/review-panel` skill from the CLI). This
note records **why that is allowed** and **where the lines are**, so future changes don't drift
across them without anyone noticing.

## Position: `er` is an application on top of LLMs, not a competing model

Every major provider's terms (Anthropic, OpenAI, Cursor) restrict using their output to
**develop or train a model or service that competes with them.** The axis that matters is
*model vs. application* — **not** *review vs. generate*:

- A code-*generation* tool (Cursor, Codex) is fine — it is an API **customer**, not a rival model.
- A code-*review* tool would still violate the clause if it trained its own model on harvested outputs.

So "we review instead of generate" is not what keeps us compliant. What keeps us compliant is
that `er` is unambiguously an application:

- It runs and hosts no model, and holds no provider credentials — no API key or provider
  account appears anywhere in the codebase.
- It **drives** the provider CLIs the user already installed and authenticated (`claude`,
  `codex`, `agent`, `opencode`) as subprocesses, and renders the artifacts they write — the
  tools are used, not replaced. The **user** is the provider's customer; `er` is the client
  and the viewer.

**Conclusion:** `er` does not qualify as a competing product under the providers' terms above.

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
| `er` becomes hosted SaaS, or calls provider APIs directly (its own credentials or account, rather than the user's installed CLI) | ToS, Privacy Policy, provider usage-policy **flow-down** to end users, commercial API agreements |
| `er` charges money / adds accounts | ToS, billing & refund terms |
| `er` adds telemetry / analytics | Privacy Policy + consent (GDPR — we are EU-based) |
| `er` stores user code or reviews on a backend | Privacy Policy, DPA, GDPR data-processing terms |

## Current legal footing (2026-09-13)

- **License:** MIT (`LICENSE`, set workspace-wide via `license = "MIT"`).
- **Telemetry:** none. No analytics / telemetry SDKs in the codebase
  (verified: no PostHog, Sentry, Mixpanel, or Amplitude references).
- **Data transmission:** no user data. `er` shells out to the user's local `git` / `gh` /
  provider CLIs. The desktop app's HTTP clients are the `ureq` browser proxy, which forwards
  the webview's **own** requests, and one anonymous update check against the public GitHub
  Releases endpoint — no payload beyond a User-Agent, and nothing collected centrally.
- **Legal docs required today:** none beyond the license. `er` is a local, no-server,
  no-data-collection tool, so there is no service relationship (no ToS) and no data
  collection (no Privacy Policy).
