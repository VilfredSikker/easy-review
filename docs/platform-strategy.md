# Platform strategy — EasyReview & TechProfessor

> **Shared document.** An identical copy lives in `TechProfessor/internal-docs/platform-strategy.md`.
> Update both in the same change — the June 2026 revisions landed in one copy only and went unnoticed here for three months.

**Status:** Adopted (May 2026); TechProfessor-side decisions revised June–August 2026  
**Last updated:** 2026-09-13

---

## Summary

We ship **two separate products** with different jobs and UX, on **one shared cloud platform** (Supabase). **TechProfessor** went cloud-first and is now a web-first fullstack app; **EasyReview** stays local-first until there is a concrete user need for cloud persistence. The products connect through a **small, explicit API** (ingest packages / trigger tours and quizzes), not through a merged application.

---

## Products

| | **EasyReview** (`er`) | **TechProfessor** |
|---|------------------------|-------------------|
| **Job** | Fast git diff review for builders; AI-assisted findings and comments | Learning and teaching from code: quizzes, guided tours, flashcards, progress |
| **Primary surface** | TUI + desktop; git-native, often offline | Web-first (SvelteKit fullstack, installable PWA); the Tauri shell is retired |
| **Cloud (now)** | None — local-first; GitHub PRs through the `gh` CLI | **Live** — Supabase auth/storage, hosted `/api/*`, sharing |
| **Repo** | `Projects/Easy-review/easy-review` | `Projects/TechProfessor` |

---

## Principles

1. **Separate products, separate UX** — No single merged app. Users who only want review or only want learning are not forced through the other product’s UI.
2. **One account, one cloud** — Shared Supabase project: auth, profiles, and orgs/billing. Table namespaces: `tp_*` (TechProfessor), `er_*` (EasyReview, when needed).
3. **Bridge by contract, not by codebase** — Integration uses the hosted API and a package payload, not shared UI modules or a god monolith.
4. **Local-first where it wins** — EasyReview keeps git and diffs on the machine. TechProfessor is cloud-primary and online-required once its Supabase env is configured.
5. **TechProfessor leads cloud** — Prove signup, sync, and ingest there before investing in EasyReview cloud tables or services.
6. **Avoid feature bloat** — New cross-product features must justify complexity; default is deep link + API, not new nav in both apps.

---

## Architecture (target)

```
                    ┌──────────────────────────────────────┐
                    │  Supabase (one project)              │
                    │  • Auth (shared user pool)           │
                    │  • Postgres + RLS                    │
                    │  • Storage (optional, large diffs)   │
                    └───────────────┬──────────────────────┘
                                    │
              ┌─────────────────────┴─────────────────────┐
              ▼                                           ▼
     ┌─────────────────┐                       ┌─────────────────┐
     │  TechProfessor  │◄── ingest API ────────│  EasyReview     │
     │  (web-first)    │    TechProfPackage    │  (local-first)  │
     │  tp_* tables    │    + source_ref       │  er_* later     │
     └─────────────────┘                       └─────────────────┘
              │                                           │
              ▼                                           ▼
     hosted SvelteKit /api/*                    git + local sidecars
```

### Integration contract

- **Payload:** a `TechProfPackage` — one package is one artifact (quiz, tour, flashcard deck, or playground) plus an optional `source_ref` (e.g. `branch:…`, `pr:42`). The schema is owned by TechProfessor.
- **Entry point:** `POST {PUBLIC_APP_ORIGIN}/api/ingest` → returns session links. Callers holding only a source can ask TechProfessor to generate instead.
- **Idempotency:** upsert on `source_ref`; re-ingest updates, never duplicates.
- **Callers:** CI, agent skills, EasyReview (Phase 3), manual curl. The contract is `TechProfessor/internal-docs/integrations.md`.
- **Auth:** personal API token (`tp_pat_…`); package ownership follows the token.

### What we do not put in the cloud (initially)

- Full git mirrors or large repo snapshots
- EasyReview diff/comment primary store (until a defined need: multi-device, team dashboard, etc.)
- Merged “one app” shell or shared main navigation

---

## Web-first (decided June 2026)

TechProfessor is a **SvelteKit fullstack app** on a Node/Vercel runtime serving its `/api/*` routes. Supabase is Postgres + Auth + Storage only. The Tauri shell is retired; the local-git review path is EasyReview’s job, not TechProfessor’s.

- **Web is the primary profile.** A GitHub App replaces local git: branch/PR diffs, file reads, and repo search run server-side through installation tokens, read-only.
- **Mobile:** the web app is an installable PWA. Store presence comes later, from the same codebase.
- **Teams:** users create orgs, invite by email, and share repos/sessions into a team space (`org_id`). Reads are RLS-driven, writes owner-only, and per-person state (answers, attempts, bookmarks) stays personal.

Detail: `TechProfessor/internal-docs/web-first-ssr-migration.md`, `multi-view-strategy.md`.

---

## Phased path

### Phase 0 — Platform skeleton

- [x] Supabase project: Auth, `profiles` (trigger on signup)
- [x] Orgs / memberships — decided (see Open decisions)
- [ ] Entitlements: `learn` live; `review` when EasyReview joins

### Phase 1 — TechProfessor cloud MVP

- [ ] Cloud tables for packages and learning state (shape decided — see Open decisions)
- [ ] Repo **metadata** only — not full git hosting
- [x] Sign-in in TechProfessor
- [x] Cloud-only persistence — no SQLite product store
- [ ] Second-device sync proven end-to-end

### Phase 2 — Ingest API (bridge)

- [x] Ingest route: validate the package, idempotent upsert on `source_ref`
- [x] Rate limits and payload size caps
- [x] Deep link into TechProfessor

### Phase 3 — EasyReview bridge (minimal)

- [ ] Shared types package optional (`TechProfPackage` in monorepo or copied schema)
- [ ] “Send to TechProfessor” (or skill-driven export) → ingest API
- [ ] No EasyReview cloud DB until a documented user story requires it

### Phase 4 — Expand only on evidence

- `review` + `learn` entitlements
- EasyReview `er_*` tables (shared comments, team review rooms, etc.)
- Realtime, LMS features, unified marketing site — only if metrics support it

---

## Success criteria (before expanding scope)

**TechProfessor cloud MVP is done when:**

1. A new user can sign up, create or import a quiz/tour, complete a quiz attempt or mark a whole tour read, and see their own state on a second device.
2. An external caller can ingest a `TechProfPackage` and open it via link in TechProfessor.
3. **Decided:** online-required signup when the Supabase env is configured.

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
- Re-introducing SQLite as TechProfessor’s product source of truth after the cloud cutover
- EasyReview cloud persistence “because TechProfessor has it”
- Service role keys in desktop/TUI binaries

---

## Open decisions

| Topic | State |
|-------|-------|
| Offline vs online-required signup | **Decided:** online-required when the Supabase env is set |
| Postgres shape v1 | **Decided:** jsonb `content` on `tp_packages` + normalized learn tables |
| Org model | **Decided (June 2026):** one billing home per account (personal or org) assigning a seat; teams are billing-agnostic shared spaces; orgs are the billing umbrella |
| Shared monorepo | Open — separate repos vs `packages/*`; decide when bridge coding starts |

---

## References

- Package schema (owned by TechProfessor): `TechProfessor/src/lib/api/package-schema.ts`
- Ingest and external API contract: `TechProfessor/internal-docs/integrations.md`
- Surfaces, web-first migration, org model: `TechProfessor/internal-docs/web-first-ssr-migration.md`, `multi-view-strategy.md`, `accounts-teams-orgs-model.md`
- Tour/quiz authoring skills: `TechProfessor/.agents/skills/tour/SKILL.md`, `quiz/SKILL.md`
- EasyReview architecture: `easy-review/CLAUDE.md`; decisions: `docs/adr/`
