# Clean-slate architecture: what to keep, what to change

Status: proposal · written against `v0.6.4` (2026-11-03)

This is a proposal for one deliberate breaking release before `er` reaches many
users. It asks: if the system were designed today, knowing where it ended up,
what would it look like? Then it marks which parts of the current code already
have that shape and which do not.

The figures below are a snapshot taken to show scale. They are not meant to be
maintained, and they will drift. The code wins wherever it disagrees with this
document.

## Decisions taken

| Question | Decision | Where it lands |
|---|---|---|
| Which front end leads? | **The desktop is the main product. The TUI stays supported** but follows the desktop. | §3.9, §6: the service is shaped around the desktop's needs and ported desktop-first. The TUI consumes the same service. |
| Daemon or in-process? | **Whatever is best for performance and stability.** That is in-process. | §3.10 |
| Multi-window desktop? | **Undecided.** | §3.8: the protocol keeps per-subscriber state, so allowing it later costs nothing. Nothing is built for it now. |
| Arena | **Keep the feature; a redesign is welcome.** | §3.11: the arena becomes a stage of the shared workflow engine instead of a subsystem of its own. |
| Old data | **No support needed.** Breaking changes are allowed. | §5: no `er migrate`. The new store starts empty, and every artifact can be regenerated. |

---

## 1. Diagnosis

The product's ideas are sound. The vocabulary in `CONTEXT.md` is precise, and
most ADRs record real trade-offs that should survive any rewrite. The problems
come from structure, and they share one root cause: **there is no layer between
"state" and "everything that touches state."**

### 1.1 Two god structs act as the whole application

| Struct | Size | What it mixes |
|---|---|---|
| `TabState` | ~110 fields, ~4,300-line `impl` | Diff/domain data, cursor and selection, scroll and column widths, a TUI text widget (`comment_textarea: TextArea`), desktop browser-pane layout, caches, staleness tokens, both ends of an mpsc channel, and job receivers |
| `App` | ~34 fields, ~11k lines of `impl` across 10 files | UI root (input mode, overlay), notification, config, job scheduler, review-flow orchestrator, arena registry, host wake callback |

Spreading these over `comments.rs`, `review_flow/*.rs` and `arena.rs` only
changes which file you open. Every file is `impl App` over the same struct with
`use super::*`, so `comments.rs` (6k lines) turns out to hold the background-task
scheduler.

Some symptoms:

- **Function arguments are passed through state.** About a dozen
  `comment_*_override` fields are "transient, consumed by submit", so the
  desktop can call `submit_question()` without parameters.
- **There are five full field-literal constructors**, and each creates its own
  channel.
- **Mutations do IO inline.** `toggle_reviewed` writes to disk, `set_mode`
  reloads sidecars, `refresh_diff` shells out to git, and `open_in_editor`
  spawns `$EDITOR` from the navigation module. On the desktop all of this runs
  under the single `Arc<Mutex<App>>`.
- **ADR 0006 made this policy.** "The engine state is the UI contract" means
  both front ends read and write raw fields, and engine fields that only one
  front end uses are protected from cleanup on purpose.

### 1.2 The desktop protocol is a stack of compensations

The protocol has one aggregate reply type, `AppSnapshot`, with ~60 top-level
fields covering both per-view content and app-wide chrome. Each later layer was
added to recover from the cost of the layer before it:

1. Send the whole world on every command. Of 202 commands, 137 return
   `AppSnapshot`.
2. That is slow, so `poll` gained revision events (ADR 0011).
3. Hunks dominate the payload, so `SentFilesState` tracks per-view `delta_key`s
   and the frontend splices omitted hunks back in (ADR 0012).
4. A checkmark rebuilds the diff, so content, chrome and reviewed revisions were
   split, and chrome-only replies are merged onto the existing view (ADR 0016).
5. Revisions are hand-written hash fingerprints. A new field that should cause a
   repaint has to be added to the hash by hand.
6. Command replies and poll replies race each other. The result is two
   snapshot helpers whose difference is invisible in the reply (ADR 0033),
   plus `shouldDropCommandSnapshot`, `canChromeMerge` and
   `isStaleSnapshotGeneration`.
7. Round-trips are still slow, so 1,336 lines of `optimisticLocal.ts` apply and
   re-apply 36 operation kinds. They re-implement backend rules in TypeScript:
   the saved-PR cap, inbox unread counts, `reviewed_count` arithmetic, and
   thread ordering, which must match exactly or the fingerprint diverges.
8. Building a snapshot still runs git (`rev_list_count`, `merged_into_base`,
   branch lists) and reads `ui-annotations.json` while holding the App lock.
   `poll` also mutates state.

Each step was a reasonable local fix. Together, at least six ADRs exist only to
explain the protocol's own invariants. Types are kept in sync by hand
(`types.ts` is 1,172 lines with no codegen).

### 1.3 AI jobs: five spawn paths, three output protocols

| Path | Tracking | How the result returns |
|---|---|---|
| Background queue (review, expert, professor, triage, tour, diagram, importance) | `BackgroundTask { kind: String }` plus a FIFO | Sidecar files |
| Tab-local agent prompt | `tab.command_status` | Sidecar files |
| Shell command | tab command status | Sidecar files |
| Card AI | Caller blocks | stdout text |
| Arena | `ArenaRunHandle`, its own retry loop | stdout JSON, then `run.json` |

Across these paths there are:

- three argv resolvers
- two process-kill implementations; the arena's does not kill the process
  group
- three stream-json decoders
- three progress mechanisms
- three output protocols: sidecar, stdout JSON, and stdout sentinel markers
  (importance output is written into **global config**)

The abstraction that would unify them already exists and is not connected:
`AgentTaskKind`, `ArtifactContract`, and `ArtifactBaseline::validate`, which
has no production caller. The slot cap (ADR 0021) holds only because each call
site remembers to acquire a slot.

### 1.4 Storage and adapters have no seams

- **There are about 20 sidecar formats.** Schemas are defined in up to three
  places: the serde structs, the hand-written JSON Schema in
  `sidecar_specs.rs` (which covers only 3 kinds, for MCP), and each prompt's
  description of its output.
  - `version: u32` is written everywhere and checked almost nowhere.
  - `review.json` has four writers and three merge strategies: overwrite,
    snapshot-merge, and load-time overlay.
- **There is no store abstraction.** About 350 `fs::` calls are spread across
  roughly 25 files. `storage.rs` only computes paths.
- **There are no git or gh seams.** About 150 direct `Command::new` calls exist.
  Two independent GitHub layers (`github.rs` and `gh_pr_actions.rs`) each have
  their own caches. gh failures are classified by matching stderr text
  (`contains("404")`). Nothing touching gh can be tested end to end.
- **Errors are anyhow strings.** There are about 400 `bail!`/`anyhow!` calls,
  the desktop returns `Result<_, String>`, and config load never fails: a parse
  error silently renames the file and returns defaults.
- **Domain logic lives in the desktop crate.** This includes the inbox, the
  project registry, PR caches, auto-triage and tab persistence, so the TUI and
  MCP cannot use them. `projects.json` has two writers, and four remote-URL slug
  parsers have already diverged on `git@github.com:`.

---

## 2. Keep

These are right, and the redesign should build on them rather than around them.

**Vocabulary.** Keep `CONTEXT.md` unchanged. The redesign gives each term a
type and a home, and leaves the meanings alone.

**Decisions that survive (ADRs).**

| ADR | Why it stays |
|---|---|
| 0001 shell out to git | Still right. It moves behind a `Git` port (§3.3). |
| 0002 sync engine, no tokio in core | Still right. The service layer uses threads and channels (§3.5). |
| 0003 / 0004 managed storage, view buckets | The bucket is the right unit. It becomes the key of the artifact store. |
| 0005 global-only config | Keep. Config is split by concern (§3.7), but it stays global. |
| 0007 three comment stores | Keep, and make privacy a type as well as a file (§3.4). |
| 0008 flat threads, 0009 findings are AI-owned, 0010 two staleness conditions | Correct domain rules. They become core functions. |
| 0014 client-side highlighting | Keep. |
| 0023 self-contained prompts, 0031 agent CLI over sidecar files | Keep the model, but change who commits the file (§3.6). |
| 0026 gh CLI rather than HTTP | Keep. It moves behind a `GitHub` port with typed errors. |
| 0030 shared theme tokens, 0038 ratcheted lint gate, 0028 local CRAP gate | Keep. |
| 0035 arbiter verdict overlay | Keep, and generalise it into the rule for every derived view (§3.4). |
| 0036 / 0048 declared risk rules, 0039 reach, 0045 model groups, 0041 document anchors, 0047, 0049 pending review on GitHub | Product decisions. Unaffected. |

**Code that already has the target shape.**

- `git/diff.rs`: a pure parser with no process calls, and 58 tests.
- `ai/prompts.rs`: string in, string out, no App dependency.
- `sync.rs`: the snapshot → fetch → apply split with no App dependency. **This
  is the pattern every IO path should follow.**
- `agent_run::AgentRunHandle`: process-group kill, cancel flag, timeout watchdog.
- `agent_slots`: the semaphore itself is fine. Only where it is acquired
  changes.
- `agent_runtime::{AgentTaskKind, ArtifactContract, ArtifactBaseline}`: the
  right abstraction, waiting to be wired in.
- **The Svelte component tree and the TUI renderers.** Their inputs change; the
  code that draws them mostly does not.

---

## 3. Target architecture

### 3.1 Layers

```
            ┌───────────┐  ┌────────────┐  ┌──────────┐  ┌─────────┐
front ends  │  er-tui   │  │ er-desktop │  │  er-mcp  │  │ er (cli)│
            └─────┬─────┘  └─────┬──────┘  └────┬─────┘  └────┬────┘
                  │  Command ▼   ▲ Event / Resource           │
            ┌─────┴──────────────┴──────────────┴─────────────┴────┐
service     │ er-service: Workspace actor · use cases · workflows  │
            └──┬────────────┬───────────────┬─────────────┬────────┘
adapters    ┌──┴───┐   ┌────┴────┐    ┌─────┴────┐   ┌────┴─────┐
(ports)     │ Git  │   │ GitHub  │    │  Store   │   │ AgentRun │
            └──┬───┘   └────┬────┘    └─────┬────┘   └────┬─────┘
core        ┌──┴────────────┴───────────────┴─────────────┴────────┐
            │ er-core: diff model · artifacts · staleness · gate · │
            │ risk rules · reach · tour/triage types · no IO       │
            └──────────────────────────────────────────────────────┘
```

Dependencies point down only. The core does no IO and has no channels or
threads. Each adapter does exactly one kind of IO. The service is the only layer
that orchestrates. Front ends translate between people and the service, and hold
their own view state.

### 3.2 Crates

| Crate | Responsibility | Comes from |
|---|---|---|
| `er-core` | Domain types and pure rules: `Diff`, `FileDiff`, `Hunk`, `ViewBucket`, `Finding`, `Question`, `Note`, `GithubComment`, `Tour`, `Triage`, `Confidence`, `Gate`, `RiskRule`, `Tier`, `Reach`, staleness, thread rules | `git/diff.rs`, `ai/review.rs` types, `config/risk_rules.rs` evaluation, staleness from `ai/loader.rs` |
| `er-git` | The `Git` trait and a CLI implementation | `git/status.rs`, plus every `Command::new("git")` in app, arena and desktop code |
| `er-github` | The `GitHub` trait, a `gh` implementation, one TTL cache, and typed errors | `github.rs` and `gh_pr_actions.rs` merged; parts of `pr_resolve`, `gh_stack`, `review_queue` |
| `er-store` | `ArtifactStore`: typed keys, versioned envelopes, atomic writes, migrations, change notifications | `storage.rs`, `paths.rs`, `ai/loader.rs`, and all scattered `fs::` calls |
| `er-agent` | `JobSpec` → `JobRunner`: provider adapters, slot acquisition, process handle, stream decoder, output contracts | `agent_run`, `agent_runtime`, `agent_slots`, `arena/adapter.rs`, `card_ai_spawn.rs`, spawn code in `comments.rs`, agent-argv code in `config/mod.rs` |
| `er-service` | The `Workspace` actor, use cases (open target, mark reviewed, comment, promote, sync, run review), workflows (review flow, arena, tour), inbox, projects | `app/state/*` without the UI fields; `er-desktop`'s inbox, projects, caches and tabs |
| `er-protocol` | `Command`, `Event`, `Resource` and `ErError` wire types, with generated TypeScript | `snapshot.rs` DTOs, `types.ts` |
| `er-tui`, `er-desktop`, `er-mcp` | Adapters only | |

`er-crap` and `er-lint-gate` are unchanged.

The `ui` feature flag on the engine goes away. The core has nothing UI-related
to gate.

### 3.3 Ports: every external effect goes through a trait

```rust
pub trait Git: Send + Sync {
    fn diff(&self, repo: &RepoRoot, spec: &DiffSpec) -> Result<RawDiff, GitError>;
    fn status(&self, repo: &RepoRoot) -> Result<Status, GitError>;
    fn rev_parse(&self, repo: &RepoRoot, rev: &str) -> Result<Oid, GitError>;
    fn show_blob(&self, repo: &RepoRoot, rev: &str, path: &Path) -> Result<Vec<u8>, GitError>;
    // … one method per git operation the product actually uses
}

pub trait GitHub: Send + Sync {
    fn pr(&self, pr: &PrRef) -> Result<PullRequest, GhError>;
    fn pr_diff(&self, pr: &PrRef) -> Result<RawDiff, GhError>;
    fn review_threads(&self, pr: &PrRef) -> Result<Vec<RemoteThread>, GhError>;
    fn post_comment(&self, pr: &PrRef, c: &NewComment) -> Result<RemoteId, GhError>;
    // …
}

pub enum GhError { NotFound, Unauthenticated, RateLimited { reset: Option<Instant> },
                   TooLarge, Conflict, Network, Tool { status: i32, stderr: String } }
```

What this buys:

- **Fakes for tests.** gh paths such as comment sync, pending review and merge
  get end-to-end tests for the first time.
- **One place to classify errors.** Classification moves to the adapter and
  uses `gh api --include` status lines and exit codes, replacing stderr
  `contains` checks scattered across call sites.
- **One cache.**
- **A swappable `GitHub` implementation.** A future HTTP client (ADR 0026
  already expects one for a web front end) becomes a second implementation.

### 3.4 Storage: a typed artifact store with one writer per artifact

```rust
pub struct ArtifactKey<T: Artifact> { bucket: ViewBucket, slot: T::Slot }

pub trait Artifact: Serialize + DeserializeOwned + JsonSchema {
    const KIND: &'static str;          // "review", "triage", "questions", …
    const VERSION: u32;
    fn migrate(from: u32, raw: Value) -> Result<Value, StoreError>;
}

// On disk, every sidecar is an envelope:
// { "kind": "triage", "version": 2, "diff_hash": "…", "producer": "job:…",
//   "written_at": "…", "payload": { … } }
```

Rules:

1. **Every read and write goes through `ArtifactStore`.** It owns paths,
   tmp+rename atomicity, version checks, migrations, and a change channel the
   service subscribes to, which replaces mtime polling.
2. **One producer owns each artifact file.** The general review writes
   `review.json`. A validation pass writes `validation.json`. Experts write
   their own files, and so does the arbiter (already the case). The "review the
   user sees" is a **projection** computed on read from those parts. This
   generalises ADR 0035's overlay rule and removes `scoped_merge.rs`, the
   expert merge-into-review, and the arena import rewrite.
3. **Privacy is a type.** `PrivateAnnotation` (questions, notes) and
   `GithubComment` are different types in different files (ADR 0007, kept). The
   `GitHub` port's write methods accept only `GithubComment`, and `promote` is
   the single function that converts one into the other. The `n-`/`q-` id
   prefixes stop carrying meaning, because the type carries it.
4. **One storage root.** Delete `ER_REPO_LOCAL` and `ErRoot::RepoLocal`.
5. **App-level records** (projects, inbox state, saved PRs, update check) use
   the same envelope under an app-scoped key, each with one writer.

### 3.5 Service: a Workspace actor with Commands in and Events out

The engine stays synchronous (ADR 0002). The model changes from "front ends lock
a shared struct and poke fields" to "front ends send commands to an owner":

```rust
// One actor thread per workspace (a repo/worktree). It owns all domain state.
pub struct WorkspaceHandle { tx: Sender<Envelope> }

impl WorkspaceHandle {
    pub fn send(&self, cmd: Command) -> Receiver<Result<Reply, ErError>>;
    pub fn subscribe(&self) -> Receiver<Event>;
}

pub enum Command {
    OpenTarget { target: ReviewTarget },               // branch | pr | commit range | history
    SetDiffMode { tab: TabId, mode: DiffMode },
    MarkReviewed { tab: TabId, path: RepoPath, reviewed: bool },
    AddQuestion { tab: TabId, anchor: Anchor, body: String, client_id: ClientId },
    Promote { tab: TabId, id: AnnotationId },
    SyncGithub { tab: TabId },
    StartJob { tab: TabId, spec: JobRequest },          // review flow, tour, card ask, …
    CancelJob { job: JobId },
    // …
}

pub enum Event {
    ResourceChanged { key: ResourceKey, version: u64 },
    JobProgress { job: JobId, progress: Progress },
    Notice { level: Level, text: String },
}
```

Rules:

- **The actor never blocks on IO.** IO runs as effects on worker threads, in the
  `sync.rs` shape: read inputs → fetch off-actor → send a typed `Msg` back →
  apply. One `Msg` enum and one apply function per use case replace stored
  channel endpoints, sequence tokens on struct fields, and the two job systems.
- **Commands take explicit parameters.** The `comment_*_override` fields go
  away, and so does any reliance on "whatever is currently selected." A front
  end that wants "the current file" passes it.
- **No view state.** Cursor, scroll, column widths, input mode, overlays, text
  editors, browser-pane layout and panel visibility belong to the front end.
  `SessionState` restore (scroll position and so on) becomes a front-end file.
  The engine keeps only what has domain meaning: the reviewed set, the gate
  override, and the active tab list.
- **Domain logic now in the desktop crate moves here.** That covers inbox
  categorisation, the project registry, PR-open caching, auto-triage and tab
  persistence, so the TUI and MCP get them for free.

### 3.6 Jobs: one model for every AI run

```rust
pub struct JobSpec {
    pub kind: JobKind,              // Review, Expert(id), Triage, Tour, Arbiter, CardAsk, Importance, …
    pub target: ViewBucket,
    pub input: DiffGeneration,      // the diff hash it is bound to
    pub model: ModelChoice,         // resolved from model groups (ADR 0045)
    pub output: OutputContract,     // Artifact type(s) it must produce, or Stdout<T>
    pub workload: Workload,         // slot pool
}
```

- **The runner owns the whole lifecycle.** `JobRunner::run(spec)` acquires the
  slot (so the cap holds by construction), resolves argv through a
  `Provider` adapter (claude, codex, opencode; agent argv code moves out of
  config), spawns through `AgentRunHandle`, decodes the stream with **one**
  decoder, emits progress events, and validates output against the contract.
- **The host commits outputs.** The agent writes to a per-job staging
  directory. The runner parses and validates the result against the
  artifact's type and `diff_hash`, then commits it through `ArtifactStore`. This
  is what `ArtifactBaseline` and MCP's `sidecar_upload` already do, applied to
  every job. A malformed agent output is then a failed job with a reason,
  instead of a sidecar the loader silently skips.
- **Each output shape has one definition.** The `Artifact` Rust type derives
  its JSON Schema (`schemars`). The schema produces the prompt's "output"
  section, the post-run validator, and the MCP spec, which retires the
  hand-written `sidecar_specs.rs` and covers all ~20 kinds instead of 3.
- **Stdout outputs are typed too.** Card replies, arena rounds and importance
  rules use `OutputContract::Stdout<T>`. The sentinel-marker protocol goes
  away, and importance output becomes a `RiskRules` artifact, not a write into
  global config.
- **The engine builds prompt variants.** `JobRequest → JobSpec` lives in the
  service, so the TUI's 32 and the desktop's 13 hand-assembled
  `build_agent_*_prompt(app, …)` call sites disappear. The ADR 0051 class of
  bug (one front end skipping the review-ignore filter) cannot recur, because
  only one code path exists.
- **Workflows are persisted state machines over jobs.** Review flow, arena and
  tour each keep their state in a store record (`flows.json` already exists).
  That gives restart reconciliation for all of them, not only the arena. The
  bucket lease moves from App memory into the store, and with it ADR 0051's tour
  exemption becomes a general rule: a job that writes only its own artifact
  needs no lease.

### 3.7 Config: split by concern, fail loudly

`ErConfig` currently has 15 sections and about 130 leaf fields. Split it into:

| File | Contents | Writer |
|---|---|---|
| `config.toml` | User preferences: display, features, hints, watched, file kinds, inbox | Settings UI |
| `agents.toml` | Providers, model groups, custom commands | Settings UI; the catalog is merged on read (ADR 0042) |
| `risk-rules.toml` | User rules and per-repo rules | Settings UI, plus the importance job via the store |

Both front ends render one `SettingsSchema` generated from the types.
`DesktopSettingsSnapshot` and `ConfigHubFieldDto` leave the engine root.

Loading returns `Result<Config, ConfigDiagnostics>`. A parse error is shown to
the user, not silently renamed. Delete the legacy path migration, the
`[importance.<repo>]` section, and the `agent.effort` fallback (see §5).

### 3.8 Desktop protocol: resources with versions, generated types

Replace `AppSnapshot`, the poll, the three revisions, `SentFilesState`, chrome
merging, both snapshot helpers and most of the optimistic layer with:

**1. Resources.** Each resource is small, independently versioned and
independently fetched:

| Resource key | Contents |
|---|---|
| `tabs` | Open tabs, active tab, per-tab mode and target |
| `diff(view)` | File list and stats (hunks are not included) |
| `file(view, path)` | The file's hunks, **addressed by content hash** |
| `annotations(view)` | Findings projection, questions, notes, GitHub comments, reviewed set |
| `jobs` | Running and recent jobs and flows |
| `pr(repo#n)`, `branches(repo)`, `inbox`, `projects`, `settings` | Sidebar and chrome |

**2. Push invalidation.** The service emits
`ResourceChanged { key, version }`. The frontend refetches only the keys it is
currently showing. This keeps ADR 0011's push model but adds per-resource
precision, so no fingerprint hashing is needed: the owner bumps a version when
it writes.

Subscriptions belong to a subscriber, a webview, not to the process. There is
no global `last_sent_*`. One window costs nothing extra, and if multiple
windows are ever wanted, each simply subscribes on its own.

**3. Content addressing replaces delta bookkeeping.** A file's hunks are
immutable for a given content hash, so the frontend cache is keyed by hash. A
hit is a hit, and the server keeps no record of what it sent. This removes
`SentFilesState`, `delta_key`, `hunks_omitted`, the lazy-stub downgrade, and
the two duplicated "cap 8" constants.

**4. Commands return small results.** A command returns `Ack`, the created id,
or the updated resource versions, never the world. With no IO under a lock,
`MarkReviewed` or `AddQuestion` returns in well under a frame. Optimistic UI
shrinks to "show pending until ack" for comment creation, with no
re-implemented backend rules in TypeScript.

**5. Generated types.** Use `tauri-specta` (or `ts-rs` plus a small command
wrapper) to generate `types.ts` and typed command bindings from
`er-protocol`. Errors become `ErError { kind, message }`, so the frontend can
branch on the kind.

**6. A handful of commands instead of 202.** The desktop exposes
`send(Command)`, `fetch(ResourceKey)` and `subscribe()`, plus a few
window-mechanics commands. The TUI and MCP use the same `Command` enum
in-process.

ADRs 0011, 0012, 0016, 0017, 0018, 0033 and 0034 would be superseded by one new
ADR describing this protocol.

### 3.9 Front ends

The desktop is the main product. The service, the resource set and the
`Command` enum are designed around what the desktop needs, and the desktop is
ported first. The TUI stays supported as a second consumer of the same service:
it gets every domain feature for free, since nothing domain-related lives in a
front end anymore, but it does not drive the design.

- **er-tui** owns a `TuiView` (cursor, scroll, input mode, overlay, text area)
  and renders from `(service resources, TuiView)`. Key handling maps to
  `Command`s or to local view changes. It never mutates domain state directly.
- **er-desktop** becomes a thin Tauri adapter. It keeps the webview, terminal
  and browser-proxy plumbing, which are genuinely desktop-only, and loses
  `commands.rs` (14k lines) and `snapshot.rs` (7.8k lines).
- **desktop-ui** gets one store per resource. The rest of the store layer
  (`snapshotDelta`, `snapshotChrome`, `tabSnapshotCache`, most of
  `optimisticLocal`) goes away. Components stay.
- **er-mcp** calls the same service use cases. `pr_upload` becomes "commit an
  artifact through the store", the same path every agent job uses.
- **`er` CLI subcommands** (`er review --json`, `er tour`, `er sync`) become
  almost free once use cases are commands. This is a natural headless and CI
  entry point.

### 3.10 In-process service, with cross-process safety in the store

**Decision: the service runs in-process in each front end. There is no daemon.**

A per-user daemon would let the desktop, TUI and MCP share one job runner. The
cost is a second process to start, supervise, version and recover. That adds
failure modes a desktop user would see: a stale daemon after an upgrade, a
socket left behind by a crash, or a client and daemon on different protocol
versions. Every hunk would also cross a socket with serialisation, which is
the hot path §3.8 exists to make cheap. Neither trade suits a "performance and
stability" priority.

What the daemon would have bought, and how to get it in-process instead:

| Need | In-process answer |
|---|---|
| Two processes must not write the same bucket at once (ADR 0044's lease is App-local) | `ArtifactStore` takes an OS advisory lock (`flock` / `LockFileEx`) on `<bucket>/.lock` for every write, and a longer lease lock for a running workflow. This works across the desktop, TUI and MCP with no shared process. |
| One front end should see another's writes | The store's change notification is fed by the file watcher on the store root (the `watch` code already exists), so an external write raises `ResourceChanged` just like an internal one. |
| A global cap on agent processes across front ends | Not needed in practice: the desktop is the main front end, and the cap protects a single user's machine and provider quota. If it becomes needed, a counting lock directory in the store root gives a cross-process semaphore. |

`er-protocol` types still derive `Serialize`, because the desktop's IPC needs
that anyway. A daemon therefore stays possible later without redesigning
anything.

### 3.11 Arena: a workflow stage, not a subsystem

**Keep the product behaviour.** Several independent reviewers work the same
diff, an independent agreement signal is required (ADR 0024's quorum), an
arbiter rules, and nothing reaches the review until the user accepts it. These
are the reasons the arena exists, and they stay.

**Change how it is built.** Today the arena is a second AI platform beside the
review flow:

- its own adapter, argv resolver and retry loop
- its own registry and kill path (no process-group kill)
- its own `progress.jsonl` progress channel
- its own storage, an import path that rewrites `review.json`, and
  ~1.3k lines of `impl App` glue

That is about 6.5k lines in `arena/` plus the glue. The review flow then drives
it as a foreign system, which is why a Deep run spans two job systems.

The redesign makes the review flow and the arena **one workflow engine with
composable stages**:

```rust
pub enum Stage {
    Triage,                                         // picks depth + lenses
    Finders  { lenses: Vec<Lens>, models: ModelGroup },
    Panel    { reviewers: Vec<ModelChoice>, rounds: PanelRounds, quorum: u8 },
    Arbiter  { mode: ArbiterMode },                 // Seeded verification | Panel ruling
    AwaitAcceptance,
}

// Quick    = [Triage, Finders{general}, Arbiter{Seeded}]
// Standard = [Triage, Finders{general + experts}, Arbiter{Seeded}]
// Deep     = [Triage, Panel{…}, Arbiter{Panel}, AwaitAcceptance]
// "Run arena" from the launcher = [Panel{user's pick}, Arbiter{Panel}, AwaitAcceptance]
```

- **Every reviewer and arbiter call is an ordinary `JobSpec`** on the shared
  `JobRunner` (§3.6), so it gets slots, cancellation, process-group kill,
  progress events and output validation for free. The arena adapter, registry,
  retry loop and progress file are deleted.
- **The rules move to `er-core` as pure functions.** Voting, merge, identity
  and provenance are the arena's real intellectual content, and they are
  already nearly pure: `quorum(votes) -> Agreement`,
  `merge(candidates) -> Vec<Candidate>`, `normalize_reviewer(id) -> Lens`. They
  become directly testable without spawning anything.
- **The panel writes candidates, never findings.** A `PanelRun` artifact holds
  candidates, votes and rulings (one writer). Accepting a candidate writes an
  `AcceptedFinding` into its own artifact (one writer), and the findings
  projection reads it alongside the general review. The `review.json` rewrite
  in `arena/import.rs` goes away, and ADR 0024's "a run cannot silently rewrite
  the artifacts it was meant to inform" becomes structural instead of
  procedural.
- **One arbiter.** The seeded verification arbiter and the panel arbiter are
  the same job kind with different inputs (seeded claims vs. voted candidates).
  `arbiter.json` and the panel's ruling share one schema.
- **The workflow record is durable.** It lives in the store, under the store's
  lease lock (§3.10). A crash mid-panel leaves a run marked interrupted, as
  today, and the user can re-run only the stages that did not finish, because
  each stage's output is an artifact bound to the same diff generation.

Two product suggestions to consider while the arena is being rebuilt:

1. **Make rounds a parameter, and measure.** Round 2 (cross-voting) is the most
   expensive part of a panel. Record per run how often a cross-vote changes the
   outcome compared with independent agreement plus the arbiter alone. If it
   rarely does, `PanelRounds::Two` (independent work, then the arbiter) becomes
   the default, and Deep gets cheaper with no change in quality.
2. **Use diverse reviewers by default.** Independent agreement is only a signal
   when the reviewers can fail differently. Default a panel to distinct
   providers or models from the Advanced model group (ADR 0045), and show
   "2 of 3 agreed (same model)" differently from "2 of 3 agreed (different
   providers)".

---

## 4. What this removes

The counts are indicative, as of `v0.6.4`.

| Removed | Replaced by |
|---|---|
| `TabState`'s ~110 fields and `App`'s ~34 | Domain state in the service, view state in each front end |
| `app/state/mod.rs` (9.5k lines of production code), `comments.rs` (4.7k) | Use cases in `er-service`, one per command |
| `er-desktop/src/commands.rs` (14k), `snapshot.rs` (7.8k) | `send`/`fetch`/`subscribe` plus resource builders |
| `optimisticLocal.ts` (1.3k), `snapshotDelta.ts`, `snapshotChrome.ts`, `tabSnapshotCache.ts` | Per-resource stores, pending-until-ack |
| Hand-written `types.ts` (1.2k) | Generated bindings |
| Five spawn paths, three argv resolvers, two kill paths, three decoders | `JobRunner` |
| `sidecar_specs.rs` hand-written JSON Schema | `schemars` derive |
| `scoped_merge.rs`, the expert merge-into-review, the arena import rewrite of `review.json` | One writer per artifact, plus read-time projection |
| `github.rs` + `gh_pr_actions.rs` (two layers, two cache styles) | `er-github` |
| Four remote-slug parsers, three `now_epoch_ms` copies, two `projects.json` writers | One function or one writer each, in core or service |
| `arena/` adapter, registry, retry loop, `progress.jsonl`, import, `app/state/arena.rs` glue | A `Panel` workflow stage on the shared `JobRunner`, with voting and merge rules in `er-core` |

---

## 5. Legacy to delete in the breaking release

These can be deleted regardless of whether the rest of this plan goes ahead.

- [ ] `feedback.json`, `CommentRef::Legacy` (~20 match arms), and
      `ErReview.feedback`
- [ ] Legacy config path migration and fallback (`config/mod.rs`
      `legacy_config_candidates`, `migrate_legacy_config`), plus related
      `uninstall.rs` paths
- [ ] The `[importance.<repo>]` section and its conversion, and the older tier
      spellings (`foundational`, `isolated`, `normal`)
- [ ] The `agent.effort` fallback and legacy model retirement in
      `supplement_ai_hub`
- [ ] `ER_REPO_LOCAL` / `ErRoot::RepoLocal`
- [ ] Back-compat deserialisers for `Finding` and `Confidence`
      (`ai/review.rs`)
- [ ] `ai/browser_annotations*` and `ai/browser_prompt.rs`. Their own header
      says "No runtime caller". Delete them or wire them up; do not keep them.
- [ ] Decide between `submit_github_review` (local unpushed comments) and the
      GitHub pending review (ADR 0049). Keep one review-draft path.

**No migration of existing data.** Old data does not need to be supported:

- The new store uses a new root, for example `<app data>/easy-review/v1/`. On
  first launch it starts empty. Projects, saved PRs and settings are re-entered
  or re-detected, and every AI artifact is regenerable by definition.
- The old root is left untouched. A single notice tells the user where it is,
  and `er uninstall` offers to delete it.
- `Artifact::migrate` exists from day one, but its registry starts empty. From
  this release onward a format change ships with a migration, because this is
  the format users will keep. Until then, a file with an unknown version is
  treated as absent and regenerated, never half-read.

This also simplifies the deletion list above: none of it needs a compatibility
read path, so each item is a plain deletion.

---

## 6. Sequencing

"Clean slate" describes the target, not how to get there. A rewrite from zero
would discard working diff parsing, prompts, sync logic and UI components,
and would freeze features for months. Each phase below ships on its own and
leaves the tree green.

| Phase | Work | Why in this order | Risk |
|---|---|---|---|
| **0. Delete** | §5 | Cheap. Shrinks every later phase. | Low; old data is not carried over |
| **1. Ports** | `er-git`, `er-github` traits and typed errors. Move every `Command::new` behind them. | Mechanical. Gives fakes, so later phases have tests. | Low |
| **2. Store** | `er-store` on a fresh root, envelopes, `schemars`, one writer per artifact, read-time projection, cross-process locks (§3.10) | Jobs and the service both need it | Low: no old data to carry |
| **3. Jobs + workflows** | `er-agent` `JobRunner`. Port the paths one at a time: card AI → background → tab-local. Then the workflow engine (§3.11), with the arena rebuilt as a `Panel` stage and its rules moved to core. | Independent of UI. Removes the most duplicated code. | Medium |
| **4. Core + service** | Extract `er-core`. Build the `Workspace` actor around the desktop's use cases. Move use cases one command at a time. Move view state into the front ends. | The large phase. Ports, store and jobs make it tractable. | High: touches everything |
| **5. Desktop protocol** | `er-protocol`, generated TS, resources plus content-addressed hunks. Port desktop-ui store by store. Delete `snapshot.rs` and `commands.rs`. | The main product, so it moves first and gets the most testing. | High |
| **5b. TUI** | Port the TUI onto the service with its own `TuiView`. | Follows the desktop. By then the service is settled, so this is mostly key mapping and rendering. | Medium |
| **6. Consolidate** | Move inbox and projects into the service. Move MCP onto the service. Add CLI subcommands. Write the superseding ADRs. Update `CONTEXT.md` only if a term changed. | Cleanup | Low |

During phases 4 and 5, run the old and new paths side by side behind a build
flag, one front end at a time, rather than on a long-lived branch.
`main` and `release/v*` already differ (`CLAUDE.md`); a third divergent line
would be worse.

---

## 7. Guardrails that keep it this way

ADR 0038's ratchet approach can enforce the new boundaries:

- **Banned dependencies.** `er-core` may not depend on `std::process`,
  `std::fs`, `std::thread` or `std::sync::mpsc`. Enforce this with a clippy
  `disallowed_methods` / `disallowed_types` config per crate.
  `Command::new` is allowed only in `er-git`, `er-github` and `er-agent`.
- **Front ends depend on `er-protocol` and `er-service`.** They never depend on
  adapter crates, which `cargo deny` / workspace dependency rules can check.
- **No `String` errors across the protocol.** `ErError` only.
- **No command returns a type named `*Snapshot`.**
- **Every artifact type implements `Artifact`.** A test lists the kinds found in
  the store directory against the registry.

---

## 8. Still open

The earlier questions are answered in "Decisions taken" at the top. What
remains:

1. **Multi-window desktop.** Nothing is blocked. The protocol supports it
   (§3.8), but no window-management work is planned until there is a product
   reason.
2. **Panel round count (§3.11).** Decide after the measurement, not before.
3. **One review-draft path (§5).** Choose between local unpushed comments and
   the GitHub pending review.
