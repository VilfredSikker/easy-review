# Browser rework status

Release branch: `release/v0.6.0`. Implementation PRs target that branch.
Plan: [PLAN.md](./PLAN.md). Resume notes: [BUILD_LOG.md](./BUILD_LOG.md).
Updated by the orchestrator only. Last update: 2026-10-30.

Legend: planned, implementing, reviewing, fixing, merged, blocked.

| PR | Title | Lane | Model | Status | PR link | Notes |
| --- | --- | --- | --- | --- | --- | --- |
| ER21 | Browser fixtures | Fixtures | Terra | fixing | [#330](https://github.com/VilfredSikker/easy-review/pull/330) | Review requires synthetic clicks on a selectable element |
| ER32a | Engine annotation storage | Storage | Sol | implementing | — | Engine-only; snapshot wiring follows separately |
| ER34a | Shared prompt renderer | Storage | Sol | blocked | — | Needs ER32a; preserves existing exports |
| ER35 | Browser annotations MCP reader | Storage | Sol | blocked | — | Needs ER32a and ER34a |
| ER32b | Desktop snapshot integration | Desktop | Terra | blocked | — | Needs ER32a; coordinate with other snapshot work |
| ER22 | Parking proof | Native | Sol | blocked | — | Needs ER21 and real macOS window |
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

## Pending design decisions

Before the corresponding live feature is wired, reconcile screenshot numbering,
query sanitization, document activation and native failure/popup attribution with
the original reference spec. Independent storage and rendering work does not
decide these product contracts.
