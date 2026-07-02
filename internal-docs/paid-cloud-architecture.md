# Plan: Paid Easy Review — one cloud ecosystem (TUI + desktop + mobile + web)

**Status:** proposed
**Branch target:** `claude/easy-review-paid-architecture-sfz50i`
**Author:** drafted 2026-06-30 from a current-state read of both repos
**Repos:** `VilfredSikker/easy-review` (public, MIT — TUI/desktop/engine) ·
`VilfredSikker/easy-review-cloud` (proprietary — `er-api`, `er-runner`, mobile)

---

## In plain terms

We hit `gh` CLI rate limits because every client polls GitHub directly every few
seconds. The fix is a **paid, logged-in mode** where clients talk to a hosted
backend that owns one cached GitHub identity per user and is **push-driven**
(webhooks → SSE) instead of polling. When logged in, a client stops shelling out
to `gh`/local git for PR review and instead reads/writes through the backend — so
the same PR review state is shared across desktop, TUI, mobile, and (later) web.

**The important finding:** this backend is **not greenfield**. It already exists,
is built, and is deployed (currently returning 502 because its Supabase DB is
paused). The remaining work is mostly **connecting the existing desktop and TUI
clients to it**, not building the cloud.

---

## Current state (verified by reading the code)

### Already built in `easy-review-cloud`

| Component | What it does | Key files |
|---|---|---|
| **`er-api`** (axum, Fly.io EU) | Hosted service: auth, PR sessions, snapshots+SSE, webhooks, BYOK keys, job queue | `crates/er-api/src/` |
| **GitHub App auth** | "Connect GitHub" → OAuth code exchange → GitHub App **user-access tokens** (8h + 6-month refresh), envelope-encrypted (KEK), refreshed proactively. Identity = Supabase JWT (HS256 or JWKS/ES256). Client secret never leaves the server. | `er-api/src/auth.rs` |
| **`HttpApiTransport`** | Every `gh` call re-implemented as REST/GraphQL, **byte-compatible** with `er_engine::github` parsing: PR diff (`.diff` media type + `/files` reconstruction fallback), comments, reviews, checks, PR search buckets, GraphQL review threads, submit-review, resolve-thread | `er-api/src/github/mod.rs` |
| **Snapshots + SSE** | `snapshot.rs` is the extracted subset of `er-desktop/src/snapshot.rs` (same `FileSnapshot`/`HunkSnapshot`/`AiSnapshot` fields) + server-side syntect spans. Conditional (`snapshot: null` when revision current). Push-driven: webhook → Postgres `LISTEN/NOTIFY` → SSE revision bump → one conditional GET. | `er-api/src/snapshot.rs`, `session.rs`, `pubsub.rs`, `webhook.rs` |
| **`er-runner`** (BYOK AI) | Cloud equivalent of the `/er-review` skills: Anthropic + OpenAI structured output, anchor referee, triage, summary, tour, validate | `crates/er-runner/src/` |
| **Entitlement gating** | `ER_REQUIRE_ENTITLEMENT` + RevenueCat webhook (`/webhooks/revenuecat`). The paid mechanism exists. | `er-api/src/routes/`, `production-ready-todo.md` |
| **Mobile app** | Full Expo/React Native client: API layer, auth store, PR session store, diff rows, finding/thread/triage cards, BYOK keys, theme sync, push. TestFlight build path documented. | `mobile/src/` |
| **Infra & data** | 9 Supabase migrations + RLS, Dockerfile, `fly.toml`, account-deletion endpoint (Apple 5.1.1(v)) | `supabase/migrations/`, `infra/` |

**Engine reuse:** `er-api` and `er-runner` depend on `er-engine` as a **headless
git dependency** (`default-features = false, features = ["highlight"]`, pinned by
rev). Diff parsing, anchor resolution, the AI data model, and comment-merge logic
stay single-source-of-truth across local and cloud — no logic fork.

### Existing API surface (`er-api` v1)

```
POST /v1/auth/github/exchange        GET  /v1/me
GET  /v1/inbox                       GET  /v1/prs?bucket=mine|review
POST /v1/sessions                    GET  /v1/sessions/{id}/snapshot?revision=N
POST /v1/sessions/{id}/file-content  POST /v1/sessions/{id}/refresh
GET  /v1/sessions/{id}/events        # SSE revision push
POST /v1/sessions/{id}/comments      POST .../comments/{cid}/reply
PATCH|DELETE .../comments/{cid}       POST .../comments/{cid}/resolve
POST /v1/sessions/{id}/review        GET|POST /v1/sessions/{id}/questions
POST /v1/sessions/{id}/ai/review     POST /v1/sessions/{id}/ai/triage
POST /v1/sessions/{id}/ai/revalidate GET  /v1/jobs/{id}
GET|POST /v1/provider-keys           DELETE /v1/provider-keys/{provider}
GET|PUT  /v1/notification-prefs      DELETE /v1/account
POST /webhooks/github                POST /webhooks/revenuecat
```

### Not built yet

- **Desktop (Tauri) has no `er-api` client.** It still uses `gh` CLI + local git.
- **TUI has no `er-api` client and no HTTP layer.** Same.
- **No web app.** (Mobile is the only finished cloud client.)

---

## The rate-limit mechanism (be precise)

The read/write path uses **per-user** GitHub App tokens, which still count against
the user's 5,000/hr limit. The relief comes from two structural changes, not from
a bigger raw quota:

1. **Stop multiplying.** One cached identity per user on the server instead of
   each of desktop + TUI + mobile burning its own `gh` quota.
2. **Stop polling.** Webhook + SSE driven instead of a ~5s `gh` poll loop per
   client. (This matches the public repo's `docs/github-sync-architecture.md`
   "Lesson from idle profiling" — REST alone doesn't help; webhooks + cache do.)

**Optional later:** GitHub App **installation tokens** (15,000/hr, per-install)
for the read path — escapes per-user limits entirely. The module is already
stubbed (`er-api/src/github/app_auth.rs`, `#[allow(dead_code)]`, "wired up when
webhook-driven prefetch lands"; needs `GITHUB_APP_ID` + `GITHUB_APP_PRIVATE_KEY`).

---

## Scope boundary (clear-eyed)

Cloud sessions are **PR-scoped**. The "synced everywhere, nothing on local disk"
vision applies to **PR review**. Local-branch / unstaged / staged / history /
watch-mode review is inherently local-git and **stays on-device** — it can't move
to the cloud without a different sync model, and arguably shouldn't.

**Product shape:** local tool for working-tree review · cloud for PR review shared
across desktop/TUI/mobile/web. Not "all review state lives in the cloud."

---

## Plan — phased

### Phase 0 — Revive the deployment (hours)

The backend is deployed but 502, most likely a paused Supabase DB. Source of
truth for this phase is `easy-review-cloud/production-ready-todo.md`.

- [ ] Restore the Supabase project; confirm migrations `0001`–`0009` are applied.
- [ ] `fly secrets list` / `fly status` / `fly logs` — confirm env (`DATABASE_URL`
      via **Session pooler**, `SUPABASE_URL`, `SUPABASE_JWT_SECRET`, GitHub App
      creds, `GITHUB_WEBHOOK_SECRET`, `ER_KEK` — **never rotate**).
- [ ] `ER_REQUIRE_ENTITLEMENT=false` while dogfooding.
- [ ] Verify `GET /healthz` → `ok`; create a session for a PR on this repo and
      compare `GET /v1/sessions/{id}/snapshot` with desktop remote mode.

### Phase 1 — Desktop as a cloud client (highest value)

Goal: logged-in desktop PR review, synced with mobile, no `gh` for that path.
This is the step that actually retires the rate-limit problem for daily use.

- [ ] **Auth:** Supabase login (email/PKCE) + "Connect GitHub" → `POST
      /v1/auth/github/exchange`. Store the Supabase session token in the OS
      keyring. New surface; no equivalent today.
- [ ] **Transport toggle:** a `CloudTransport` the desktop selects when logged in.
      For PR tabs, Tauri commands (`open_remote_pr`, `poll`, `select_file`,
      `request_file_content`, comment/review/question writes, AI run/triage) proxy
      to the matching `er-api` endpoint instead of mutating the local `App`.
- [ ] **Rendering:** reuse the Svelte frontend nearly as-is — `er-api`'s
      `PrReviewSnapshot` mirrors `AppSnapshot` field-for-field. Map SSE
      `revision` events onto the existing `er://revision` → `poll` loop.
- [ ] **Local stays local:** Branch/Unstaged/Staged/History/watch tabs keep using
      the local `App`. Only PR tabs go through the cloud when logged in.

### Phase 2 — TUI as a cloud client

Same model, scoped to remote-PR review. Bigger relative lift: the TUI has no HTTP
client today and reads `App` state directly to render.

- [ ] HTTP client + device-flow (or paste-token) login; token in `~/.config/er/`.
- [ ] Route the remote-PR tab through `er-api`; keep local-git modes on the
      local `App`. Consider an `er login` / `er --cloud` entry point.

### Phase 3 — Web app

Net-new frontend, same API. `mobile/src/api/*` + types + `diffModel.ts` port
directly (React Native → React web). Already envisioned in the cloud README.

### Phase 4 — Hardening / paid switch-on

- [ ] Installation tokens for the read path (escape per-user limits) — wire
      `app_auth.rs`, set `GITHUB_APP_ID` + private key.
- [ ] Flip `ER_REQUIRE_ENTITLEMENT=true`; finish RevenueCat wiring + APNs push.
- [ ] App icon/splash, monitoring/alerts on `/healthz`, secrets hygiene.

---

## The `er-github` extraction (cross-repo cleanup)

The cloud README notes `HttpApiTransport` lives in `er-api/src/github/` "until the
public repo grows the `er-github` crate." Once desktop/TUI need an HTTP transport
(Phase 1–2), extracting a shared `er-github` crate in the public repo — with
`GhCliTransport` + `HttpApiTransport` behind one trait (per
`docs/github-sync-architecture.md`) — lets desktop, TUI, and `er-api` share call
sites and avoids a third copy of the GitHub I/O layer. Sequence this with Phase 1.

---

## Risks / open decisions

1. **Privacy/compliance:** logged-in PR review passes diffs through the hosted API
   (EU Fly region today). Employer-repo policies may forbid this — keep local
   `gh` mode as the default/offline path; cloud is opt-in.
2. **`ER_KEK` is load-bearing:** it decrypts every stored GitHub + provider token.
   Never rotate casually.
3. **Token model:** user-access tokens (current) vs installation tokens (Phase 4)
   — decide before marketing "no rate limits."
4. **Engine pin:** `er-api` pins `er-engine` by rev. Desktop/TUI cloud work may
   want engine changes (shared snapshot types, `er-github`); plan the bump.
5. **Auth UX parity:** three login surfaces (desktop keyring, TUI config, mobile
   deep-link) — keep the Supabase + Connect-GitHub flow consistent.

---

## Bottom line

Not "how complex to build this" — "how complex to **finish and connect** it." The
backend, GitHub App auth, AI runner, and a working mobile client already exist.
Order of value: **revive deployment → desktop cloud client → TUI → web**, with the
`er-github` extraction folded into the desktop step.

## References

- Cloud backend: `easy-review-cloud/crates/er-api/src/{auth,github/mod,snapshot,session,pubsub,webhook}.rs`
- Cloud AI runner: `easy-review-cloud/crates/er-runner/src/`
- Mobile client: `easy-review-cloud/mobile/src/api/`, `mobile/src/stores/`
- Deployment runbook: `easy-review-cloud/production-ready-todo.md`
- Public-repo design refs: `docs/github-sync-architecture.md`, `docs/mobile-port.md`, `docs/platform-strategy.md`
- Desktop snapshot contract: `crates/er-desktop/src/snapshot.rs`, `crates/er-desktop/src/commands.rs` (`poll`)
