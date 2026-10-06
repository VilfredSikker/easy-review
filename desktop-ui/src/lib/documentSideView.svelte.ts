import { untrack } from "svelte";
import type { CrossFileModel } from "./diffRenderModel";
import { documentBlocks, type DocumentBlock } from "./documentPreview";
import { sideBlockSpans, type SideBlockSpan } from "./documentSideLayout";
import { diffPreview } from "./stores/diffPreview.svelte";
import type { AppSnapshot, FileSnapshot } from "./types";

interface SideViewSource {
  snapshot: AppSnapshot | null;
  files: FileSnapshot[];
  model: CrossFileModel;
  sidePaths: ReadonlySet<string>;
}

const BLOCK_CACHE_LIMIT = 16;

/**
 * State behind the side-by-side document view: loads each side file's full
 * text, splits it into rendered blocks, matches them to the raw rows, and
 * holds the blocks' measured heights. Create it during component init.
 */
export class DocumentSideView {
  // eslint-disable-next-line svelte/prefer-svelte-reactivity -- replaced whole on change (setHeight), like FlatDiffView's overlayHeights
  private heights = $state(new Map<string, number>());
  // Parsing re-renders every block, so keep the result per content key.
  // eslint-disable-next-line svelte/prefer-svelte-reactivity -- memo only; reactivity comes from diffPreview.state
  private blockCache = new Map<string, DocumentBlock[]>();
  // Last blocks shown per path. A live update gives the file a new content key,
  // and showing these until the new text loads keeps the column from blanking.
  // eslint-disable-next-line svelte/prefer-svelte-reactivity -- fallback memo, read inside the spans derived
  private lastBlocks = new Map<string, DocumentBlock[]>();

  readonly spans: SideBlockSpan[] = $derived.by(() => {
    const { files, model, sidePaths } = this.source();
    const spans: SideBlockSpan[] = [];
    for (const path of sidePaths) {
      const file = files.find((f) => f.path === path);
      const startRow = model.fileStartRow.get(path);
      const blocks = file && this.blocksFor(file);
      if (!blocks || startRow === undefined) continue;
      spans.push(...sideBlockSpans(blocks, model.rows, startRow, file));
    }
    return spans;
  });

  constructor(private source: () => SideViewSource) {
    $effect(() => {
      const { snapshot, files, sidePaths } = this.source();
      if (!snapshot) return;
      for (const file of files) {
        if (sidePaths.has(file.path)) untrack(() => void diffPreview.load(snapshot, file));
      }
    });
  }

  private blocksFor(file: FileSnapshot): DocumentBlock[] | null {
    const key = file.preview_key;
    if (!key) return null;
    const state = diffPreview.state(key);
    if (state.status !== "ready") return this.lastBlocks.get(file.path) ?? null;
    let blocks = this.blockCache.get(key);
    if (!blocks) {
      blocks = documentBlocks(state.text);
      if (this.blockCache.size >= BLOCK_CACHE_LIMIT) {
        const oldest = this.blockCache.keys().next().value;
        if (oldest !== undefined) this.blockCache.delete(oldest);
      }
      this.blockCache.set(key, blocks);
    }
    this.lastBlocks.set(file.path, blocks);
    return blocks;
  }

  height(key: string): number | undefined {
    return this.heights.get(key);
  }

  setHeight(key: string, px: number): void {
    if (this.heights.get(key) === px) return;
    // eslint-disable-next-line svelte/prefer-svelte-reactivity -- copy assigned whole to the `heights` $state
    const next = new Map(this.heights);
    next.set(key, px);
    this.heights = next;
  }
}
