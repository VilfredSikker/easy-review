# Platform strategy — EasyReview & TechProfessor

> **Shared document.** An identical copy lives in `TechProfessor/docs/platform-strategy.md`.  
> Update both when this strategy changes.

**Status:** Adopted (May 2026)  
**Last updated:** 2026-05-23

---

## Summary

We ship **two separate products** with different jobs and UX, on **one shared cloud platform** (Supabase). **TechProfessor** moves to the cloud first; **EasyReview** stays local-first until there is a concrete user need for cloud persistence. The products connect through a **small, explicit API** (ingest packages / trigger tours and quizzes), not through a merged application.

---

## Products

| | **EasyReview** (`er`) | **TechProfessor** |
|---|------------------------|-------------------|
| **Job** | Fast git diff review for builders; AI-assisted findings and comments | Learning and teaching from code: quizzes, guided tours, progress |
| **Primary surface** | TUI + desktop (Tauri); git-native, often offline | Desktop (Tauri) + Svelte; repo-linked content |
| **Cloud (now)** | None — local/git/`.er/` sidecars, optional GitHub via `gh` | **First adopter** — auth, persistence, sharing, ingest API |
| **Repo** | `Projects/Easy-review/easy-review` | `Projects/TechProfessor` |

---

## Principles

1. **Separate products, separate UX** — No single merged app. Users who only want review or only want learning are not forced through the other product’s UI.
2. **One account, one cloud** — Shared Supabase project: auth, profiles, and (later) orgs/billing. Table namespaces: `tp_*` (TechProfessor), `er_*` (EasyReview, when needed).
3. **Bridge by contract, not by codebase** — Integration uses `TechProfPackage` JSON and HTTP (Edge Functions), not shared UI modules or a god monolith.
4. **Local-first where it wins** — EasyReview keeps git and diffs on the machine. TechProfessor may stay hybrid (SQLite cache + cloud sync) rather than cloud-only.
5. **TechProfessor leads cloud** — Prove signup, sync, and ingest API there before investing in EasyReview cloud tables or services.
6. **Avoid feature bloat** — New cross-product features must justify complexity; default is deep link + API, not new nav in both apps.

---

## Architecture (target)

```
                    ┌──────────────────────────────────────┐
                    │  Supabase (one project)              │
                    │  • Auth (shared user pool)           │
                    │  • Postgres + RLS                  │
                    │  • Edge Functions (ingest, etc.)     │
                    │  • Storage (optional, large diffs)   │
                    └───────────────┬──────────────────────┘
                                    │
              ┌─────────────────────┴─────────────────────┐
              ▼                                           ▼
     ┌─────────────────┐                       ┌─────────────────┐
     │  TechProfessor   │◄── ingest API ───────│  EasyReview      │
     │  (cloud-first)   │    TechProfPackage   │  (local-first)   │
     │  tp_* tables     │    + source_ref      │  er_* later      │
     └─────────────────┘                       └─────────────────┘
              │                                           │
              ▼                                           ▼
     SQLite (hybrid cache)                      git, .er/, gh CLI
```

### Integration contract

- **Payload:** `TechProfPackage` (`type`: `quiz` | `tour` | `both`; optional `source_ref` e.g. `branch:…`, `pr:42`).
- **Entry point:** `POST` Edge Function (e.g. `ingest-package`) → returns `package_id` + deep link.
- **Callers (later):** EasyReview desktop/TUI, Claude skills, CI, manual curl.
- **Auth:** User JWT (OAuth in EasyReview when wired) or org-scoped integration secret — never ship Supabase service role in client binaries.

### What we do not put in the cloud (initially)

- Full git mirrors or large repo snapshots
- EasyReview diff/comment primary store (until a defined need: multi-device, team dashboard, etc.)
- Merged “one app” shell or shared main navigation

---

## Phased path

### Phase 0 — Platform skeleton

- [ ] Supabase project: Auth, `profiles` (trigger on signup)
- [ ] Optional: `organizations` / memberships if team signup is near-term
- [ ] `profiles.products` or entitlements: `['learn']` today; `review` when EasyReview joins

### Phase 1 — TechProfessor cloud MVP

- [ ] Cloud tables for packages (quiz/tour content) and learning state (attempts, pins, bookmarks — scope TBD vs v1 JSON blob)
- [ ] Repo **metadata** only (`remote_url`, display name) — not full git hosting
- [ ] Sign-in in TechProfessor (Tauri + Supabase PKCE)
- [ ] Hybrid sync: SQLite remains for offline/power users; signed-in users sync to cloud (policy TBD: write-through vs cloud-primary for new accounts)

### Phase 2 — Ingest API (bridge)

- [ ] Edge Function: validate `TechProfPackage`, idempotent upsert on `source_ref`
- [ ] Rate limits and payload size caps; large `diff_content` → Storage if needed
- [ ] Deep link to open package in TechProfessor

### Phase 3 — EasyReview bridge (minimal)

- [ ] Shared types package optional (`TechProfPackage` in monorepo or copied schema)
- [ ] “Send to TechProfessor” (or skill-driven export) → ingest API
- [ ] No EasyReview cloud DB until a documented user story requires it

### Phase 4 — Expand only on evidence

- Org billing, `review` + `learn` entitlements
- EasyReview `er_*` tables (shared comments, team review rooms, etc.)
- Realtime, LMS features, unified marketing site — only if metrics support it

---

## Success criteria (before expanding scope)

**TechProfessor cloud MVP is done when:**

1. A new user can sign up, create or import a quiz/tour, complete an attempt, and see the same data on a second device.
2. An external caller can ingest a `TechProfPackage` and open it via link in TechProfessor.
3. We have chosen and documented offline behavior (hybrid vs online-required for signup).

**EasyReview cloud work starts when** at least one of: multi-device review state, team-hosted review, or non-GitHub sync has a written spec and user demand — not for parity with TechProfessor.

---

## Monorepo / code layout (optional, later)

Repos may stay separate. When convenient:

```
packages/
  package-schema/   # TechProfPackage TypeScript types
  supabase-types/   # generated DB types
```

EasyReview depends only on schema + HTTP client for the bridge — not on TechProfessor UI.

---

## Anti-goals (explicit)

- Merging EasyReview and TechProfessor into one application or one undifferentiated UI
- Two Supabase projects / two user pools unless compliance forces it
- Big-bang removal of TechProfessor SQLite before hybrid sync works
- EasyReview cloud persistence “because TechProfessor has it”
- Service role keys in desktop/TUI binaries

---

## Open decisions

| Topic | Options | Decide when |
|-------|---------|-------------|
| Offline vs online-required signup | Hybrid SQLite sync vs cloud-only v1 | Before public signup |
| Postgres shape v1 | Normalized tables vs `package` JSON column | Phase 1 design spike |
| Org model | Solo-only vs teams/classes | Before B2B or classroom pitch |
| Shared monorepo | Separate repos vs `packages/*` | When bridge coding starts |

---

## References

- TechProfessor package shape: `TechProfessor/src/lib/api/package-schema.ts`
- Tour/quiz skill output: `TechProfessor/.agents/skills/tour/SKILL.md`, `quiz/SKILL.md`
- EasyReview architecture: `easy-review/CLAUDE.md`
