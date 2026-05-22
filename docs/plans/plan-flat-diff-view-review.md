# Plan Review: Desktop UI Flat Cross-File Virtualizer (2nd pass)

**Reviewed:** 2026-05-22  
**Subject:** Updated flat virtualizer plan (G hoisted, G-prereq, C0 spike, B0 flash, drag-select, DiffComposer).

---

## Verdict

**Ready to execute.** The update addresses every blocker from the first review: Step G is first, `annotationVersion` / `model.identity` are specified, D11 has C0, B0 covers flash + `select_file` + no smooth scroll, stub height uses 24px, `hunk.threads` is in A0, split drag + file clamp are specified, DiffComposer `topPx` is scoped, compacted expand no longer depends on DOM mount.

Treat the items below as **doc/consistency fixes** before or during implementation — not architectural rethinks.

---

## What improved since pass 1

| Change | Impact |
|--------|--------|
| **Order: G → A0 → … → C0 → C** | Correct dependency chain; flat view no longer ships on broken `windowFromScrollVariable`. |
| **Step G documents the off-by-one bug** | Matches `diffRenderModel.ts` (`length = flatRows + 1`) vs `virtualWindow.ts:53`. |
| **G-prereq / `annotationVersion`** | Replaces nonexistent `ai.cacheKey`; `CrossFileModel.identity` defined. |
| **A0 takes `files` + `threadsByHunk`** | Matches runtime: threads on `hunk.threads`, not only `ai.threads`. |
| **24px lazy-stub estimate** | Aligns with D2; reduces stub→loaded jump. |
| **B0 flash + `select_file` + `behavior: "auto"`** | Preserves Comments-card UX; avoids keyboard smooth-scroll storm. |
| **C0 spike** | De-risks D11 before 10 row components. |
| **DiffComposer addendum (a)** | Honest about virtualization vs sticky-bottom. |
| **Drag-select: `rowFile`, split X-half, `startRowIdx`** | Closes largest interaction gaps. |
| **Compacted stub: `select_file` + `toggle_compacted`** | Matches `expandCompacted()` today (`DiffView.svelte:559-564`). |
| **`row.identity` keys only** | Correct for Svelte 5 keyed each. |
| **Clear overlay on view-mode change** | Prevents D10 leak across cached models. |

---

## Remaining inconsistencies (fix in the plan text)

### 1. `ai.cacheKey` still mentioned in Step B and Risks

Step B cache key and Risks table still say `ai.cacheKey`. **Replace everywhere with `annotationIndex.version`** (and per-file `file.cache_key` inside WeakMap keys only). Step B test **"ai.cacheKey bust"** → **"annotationVersion bust"**.

### 2. `G-prereq` placement vs execution order

The numbered order is **G → A0 → …**, but **G-prereq** is documented after Step F. Move **G-prereq into Step A0** (or a one-liner: "implemented in A0, consumed in B") so executors don't run B with a stale cache key spec.

### 3. `annotationVersion` must include `files` / `hunk.threads`

G-prereq hashes only `ai.threads` + `ai.findings`. Row enumeration also depends on **`files[].hunks[].threads`** (and file set). If poll updates hunk threads without changing `ai.threads` identity set, version can lie.

**Add to hash:** per-file `cache_key` or `(path, hunk count, thread ids per hunk)` from `buildAnnotationIndex`'s walk. Cheap: fold `files.length` + sum of `hunk.threads.length` + thread ids from hunks into `annotationVersion` inside `buildAnnotationIndex(files, …)`.

### 4. D1 vs Step C drag-select

**D1** still says selection relies on **`onmouseenter` on real DOM rows**. **Step C** removes that and uses **container `onmousemove`**. Update D1 rationale to: "logical line selection via row index + geometry; container hit-testing replaces per-row mouseenter under virtualization."

### 5. `diffSel.startRowIdx` is new — not in plan's file list

Drag clamp (D6) needs **`startRowIdx` (and likely `startFileIndex`)** on `diffSelection.svelte.ts`, set in `begin()` from `rowIndexAtOffset`. Add to Step C: extend `diffSel`, test file-boundary clamp.

### 6. Split drag: line numbers from `splitRows`, not `hunk.lines[row.lineIdx]`

Step C says `hunk.lines[row.lineIdx]` for unified (good). For split, use **`model.splitRowsByFile.get(path)[hunkIdx][splitRowIdx]`** left/right line nums — same as today's `findingsForSplitRow` inputs.

### 7. Step E "hide overlay when file-header in viewport"

Needs explicit check: `scrollTopPx <= cumulativeOffsets[fileHeaderRow] + FILE_HEADER_HEIGHT` (40px) **and** `scrollTopPx + headerHeight > cumulativeOffsets[fileHeaderRow]`. `rowIndexAtOffset` alone is insufficient (header row vs band overlap).

### 8. `CrossFileModel` data shape missing `identity`

Body lists fields but not **`identity: string`**. Add to interface block (G-prereq already defines it).

### 9. `windowFromScrollVariable` empty input

After G fix, **0 rows** → `cumulativeOffsets = [0]`, `length === 1`, `totalItems === 0`. Add one test; JSDoc edge case.

### 10. "Reused utilities" footer is stale

Still says "Just wire it up" for `windowFromScrollVariable` — add **"after Step G contract fix"**.

---

## Minor notes (non-blocking)

- **`snapshotKey` in `model.identity`:** Define explicitly (e.g. existing `viewKey`: `active_tab:mode:base:branch` or snapshot poll generation). Document in B.
- **Cross-file concat on every file patch:** Still O(total rows); fine for bun#30412 stubs; optional future incremental splice.
- **R3 ResizeObserver:** Re-query `[data-row-idx]` when `windowedRows` changes — effect should depend on `vw.start/end`, not only mount.
- **`scrollToRow` + `align: "center"`:** B0 registers align but snippet only sets `scrollTop = offset[i]` — implement center for thread/finding nav if needed.
- **IPC budget:** Outcome target remains valid — flat model row count ≈ files × ~2 for stub PR, not 1M rows.

---

## Step order — confirmed

```
G → A0 (+ annotationVersion/identity) → A → B → B0 → C0 → C → D → E → F → H
```

Do **not** run C before G + C0. D/E/F can land after C behind the flag.

---

## Verification — still good; one addition

Existing checklist is strong. Add:

- **`annotationVersion` changes** when a thread is added only on `hunk.threads` (fixture with no `ai.threads` delta if that's possible — or simulate file patch).
- **`diffSel.startRowIdx` clamp:** drag from file A into file B → `end` clamped, `file` unchanged until explicit file change.

---

## Summary

The updated plan is **implementation-grade**. Hoisting G, defining cache identity, and spelling out B0/C0/C drag/composer closes the prior gaps. Before coding, **grep-replace `ai.cacheKey` → `annotationIndex.version`**, **fold `hunk.threads` into `annotationVersion`**, **align D1 with mousemove selection**, and **add `diffSel.startRowIdx` + split row line lookup** to Step C's scope.

---

## Related

- First review: same file (superseded by sections above).
- Implementation plan: user doc (flat cross-file virtualizer).
- Code: `desktop-ui/src/lib/virtualWindow.ts`, `diffRenderModel.ts`, `DiffView.svelte`.
