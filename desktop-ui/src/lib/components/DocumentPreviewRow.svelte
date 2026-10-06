<script lang="ts">
  import { untrack } from "svelte";
  import { diffPreview } from "$lib/stores/diffPreview.svelte";
  import type { PreviewState } from "$lib/documentPreviewCache";
  import type { AppSnapshot, FileSnapshot } from "$lib/types";
  import DocumentPreview from "./DocumentPreview.svelte";

  const { snapshot, file }: { snapshot: AppSnapshot; file: FileSnapshot } = $props();
  // A mounted row owns its result. Evicting the shared cache must not erase it.
  let previewState = $state<PreviewState>({ status: "loading" });
  let retry = $state(0);
  let lastRetry = 0;
  const requestKey = $derived(diffPreview.requestKey(snapshot, file));
  $effect(() => {
    requestKey;
    const [current, document] = untrack(() => [snapshot, file] as const);
    const force = retry !== lastRetry;
    lastRetry = retry;
    let active = true;
    previewState = { status: "loading" };
    void untrack(() => diffPreview.load(current, document, force)).then((result) => {
      if (active && result) previewState = result;
    });
    return () => { active = false; };
  });
</script>

<DocumentPreview path={file.path} state={previewState} {file} onretry={() => { retry++; }} />
