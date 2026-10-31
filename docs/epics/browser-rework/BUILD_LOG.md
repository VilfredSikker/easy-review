# Browser rework status

Release branch: `release/v0.6.1`. Implementation PRs target that branch.
Plan: [PLAN.md](./PLAN.md). Resume notes: [BUILD_LOG.md](./BUILD_LOG.md).
Updated by the orchestrator only. Last update: 2026-10-31.

Legend: planned, implementing, reviewing, fixing, merged, blocked.

| PR | Title | Lane | Model | Status | PR link | Notes |
| --- | --- | --- | --- | --- | --- | --- |
| ER21 | Browser fixtures | Fixtures | Terra | merged | [#330](https://github.com/VilfredSikker/easy-review/pull/330) | HTTP checks passed; Safari/native observations pending |
| ER32a | Engine annotation storage | Storage | Sol | merged | [#332](https://github.com/VilfredSikker/easy-review/pull/332) | Twelve storage tests, independent review and CI passed |
| ER34a | Shared prompt renderer | Storage | Sol | merged | [#335](https://github.com/VilfredSikker/easy-review/pull/335) | Review and CI passed; existing exports preserved |
| ER35 | Browser annotations MCP reader | Storage | Sol | merged | [#352](https://github.com/VilfredSikker/easy-review/pull/352) | Review, 25 MCP tests and CI passed |
| ER32b | Desktop snapshot integration | Desktop | Sol | merged | [#353](https://github.com/VilfredSikker/easy-review/pull/353) | Repair re-review and full desktop CI passed |
| ER22 | Parking proof | Native | Sol | draft | [#354](https://github.com/VilfredSikker/easy-review/pull/354) | Source reviewed; macOS proof remains pending |
| ER23 | Content-world proof | Native | Sol | blocked | — | Needs ER21 and real macOS window |
| ER24 | Host editor surface proof | Native | Sol | blocked | — | Needs ER21 and real macOS window |
| ER25 | Navigation capture data-store proof | Native | Sol | blocked | — | Needs ER22, ER23, ER24 and real macOS |
| ER26 | Stable browser identity | Desktop | Sol | blocked | — | Native proof gate |
| ER27 | Native navigation state | Desktop | Sol | blocked | — | Needs ER26 and reliable native observations |
| ER28 | Sender validation | Desktop | Sol | blocked | — | Needs ER26 and ER27 |
| ER29 | Presentation leases | Desktop | Sol | blocked | — | Needs ER22 and ER26 |
| ER30-31 | Runtime and host drafts | Desktop | Sol | blocked | — | One combined PR; native and storage gates |
| ER33 | Screenshot capture | Desktop | Sol | blocked | — | Native, runtime, draft and storage gates |
| ER34b | Export integration | Handoff | Terra | blocked | — | Wait for active v2 host flow |
| ER36 | Cutover and acceptance | Release | Sol | blocked | — | All predecessors and recorded macOS acceptance |

## Owner actions

The native proof tasks require mouse and keyboard checks in a macOS Tauri app.
The current Linux environment cannot supply that evidence. Their proof results
remain pending until recorded in the proof log; compilation is not a substitute.

## Release target

The owner directed creating `release/v0.6.1` from `release/v0.6.0` and targeting
every new PR at it. See the [addendum](./ADDENDUM-release-0.6.1.md). Remaining
snapshot, MCP and parking-probe branches will merge this base before publication.
The parking-probe PR stays a draft until macOS validation.

## Pending design decisions

Before the corresponding live feature is wired, reconcile screenshot numbering,
query sanitization, document activation and native failure/popup attribution with
the original reference spec. Independent storage and rendering work does not
decide these product contracts.
