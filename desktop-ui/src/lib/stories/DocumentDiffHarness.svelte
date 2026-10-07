<script lang="ts">
  import { onMount } from "svelte";
  import { app, type DiffViewMode } from "$lib/stores/app.svelte";
  import { annotationDrafts } from "$lib/stores/annotationDrafts.svelte";
  import { diffPreview } from "$lib/stores/diffPreview.svelte";
  import { diffNav } from "$lib/stores/diffNav.svelte";
  import { diffSel } from "$lib/stores/diffSelection.svelte";
  import { refHighlight } from "$lib/stores/referenceHighlight.svelte";
  import DiffView from "$lib/components/DiffView.svelte";
  import type { AppSnapshot } from "$lib/types";
  import type { DocumentViewMode } from "$lib/documentPreviewCache";

  interface Props {
    snapshot: AppSnapshot;
    documents: Record<string, string>;
    viewModeOverride?: DiffViewMode;
    documentMode?: DocumentViewMode;
  }
  const { snapshot, documents, viewModeOverride = "unified", documentMode = "preview" }: Props = $props();

  onMount(() => {
    const previous = Reflect.get(window, "__TAURI_INTERNALS__");
    Reflect.set(window, "__TAURI_INTERNALS__", {
      invoke: async (command: string, args: { path?: string } = {}) => {
        if (command === "request_file_preview") {
          const file = snapshot.files.find((f) => f.path === args.path);
          if (!file || !(file.path in documents)) throw new Error("Document unavailable");
          return {
            path: file.path,
            text: documents[file.path],
            preview_key: file.preview_key,
            preview_context_key: snapshot.preview_context_key,
          };
        }
        if (command === "request_file_content") return [];
        return snapshot;
      },
    });
    app.snapshot = snapshot;
    diffPreview.sync(snapshot);
    for (const path of Object.keys(documents)) diffPreview.setMode(snapshot, path, documentMode);
    return () => {
      Reflect.set(window, "__TAURI_INTERNALS__", previous);
      app.snapshot = null;
      diffPreview.sync(null);
      annotationDrafts.sync(null);
    };
  });
</script>

<div class="h-screen flex flex-col bg-bg text-fg">
  <div class="flex gap-3 p-2 text-xs shrink-0" data-story-controls>
    <button type="button" onclick={() => diffNav.scrollToHunk("docs/README.MD", 0)}>Jump to document hunk</button>
    <button type="button" onclick={() => { refHighlight.openSearch(); refHighlight.setQuery("Document preview"); }}>Search document diff</button>
    <button type="button" onclick={() => { diffSel.begin(1, false, undefined, { file: "docs/README.MD", side: "new" }); diffSel.finish(); diffSel.text = "Keep this draft"; }}>Open document draft</button>
    <button type="button" onclick={() => diffSel.clear()}>Clear draft</button>
    <button type="button" onclick={() => diffNav.scrollToFile("notes.text")}>Jump to notes</button>
    <button type="button" onclick={() => diffNav.scrollToFile("empty.md")}>Jump to empty document</button>
    <button type="button" onclick={() => diffNav.scrollToFile("unavailable.md")}>Jump to unavailable document</button>
    <span data-story-draft>{diffSel.text}</span>
  </div>
  <DiffView {viewModeOverride} />
</div>
