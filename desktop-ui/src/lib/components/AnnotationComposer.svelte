<script lang="ts">
  import { registerBrowserAnnotationComposerDismiss } from "$lib/stores/keyboard";
  import type { UiDomContext } from "$lib/types";

  export type AnnotationComposerState = {
    x: number;
    y: number;
    w: number;
    h: number;
    selector: string | null;
    element_context: string | null;
    dom_context: UiDomContext | null;
    text: string;
    screenshotDataUrl: string | null;
  };

  interface Props {
    composer?: AnnotationComposerState | null;
    /** `overlay` positions near click coords; `toolbar` is a full-width bar. */
    variant?: "overlay" | "toolbar";
    width?: number;
    height?: number;
    getIframeRect?: () => DOMRect | null;
    onSave: (
      bbox: [number, number, number, number],
      selector: string | null,
      text: string,
      screenshotDataUrl: string | null,
      elementContext: string | null,
      domContext: UiDomContext | null,
    ) => void;
    onCancel?: () => void;
  }

  let {
    composer = $bindable<AnnotationComposerState | null>(null),
    variant = "overlay",
    width = 1280,
    height = 800,
    getIframeRect,
    onSave,
    onCancel,
  }: Props = $props();

  const canCapture =
    typeof navigator !== "undefined" &&
    typeof navigator.mediaDevices !== "undefined" &&
    typeof navigator.mediaDevices.getDisplayMedia === "function";

  let capturing = $state(false);
  let captureError = $state<string | null>(null);

  function clampedLeft(x: number, boxWidth: number) {
    return Math.max(0, Math.min(x, Math.max(0, width - boxWidth)));
  }

  function clampedTop(y: number, boxHeight: number) {
    return Math.max(0, Math.min(y, Math.max(0, height - boxHeight)));
  }

  function cancelComposer() {
    composer = null;
    captureError = null;
    onCancel?.();
  }

  function onComposerKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      cancelComposer();
    } else if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
      e.preventDefault();
      e.stopPropagation();
      saveComposer();
    }
  }

  $effect(() => {
    if (!composer) {
      registerBrowserAnnotationComposerDismiss(null);
      return;
    }
    registerBrowserAnnotationComposerDismiss(cancelComposer);
    return () => registerBrowserAnnotationComposerDismiss(null);
  });

  function saveComposer() {
    if (!composer || !composer.text.trim()) {
      composer = null;
      return;
    }
    onSave(
      [composer.x, composer.y, composer.w, composer.h],
      composer.selector,
      composer.text.trim(),
      composer.screenshotDataUrl,
      composer.element_context,
      composer.dom_context,
    );
    composer = null;
    captureError = null;
  }

  async function captureScreenshot() {
    if (!canCapture || !composer || capturing) return;
    capturing = true;
    captureError = null;
    let stream: MediaStream | null = null;
    try {
      stream = await navigator.mediaDevices.getDisplayMedia({
        video: true,
        audio: false,
      });
      const track = stream.getVideoTracks()[0];
      if (!track) throw new Error("No video track available");

      const video = document.createElement("video");
      video.srcObject = stream;
      video.muted = true;
      await video.play();
      await new Promise<void>((r) => requestAnimationFrame(() => r()));

      const captureW = video.videoWidth || 1280;
      const captureH = video.videoHeight || 800;

      const iframeRect = getIframeRect?.();
      let sx = 0, sy = 0, sw = captureW, sh = captureH;
      if (iframeRect && window.innerWidth > 0 && window.innerHeight > 0) {
        const scaleX = captureW / window.innerWidth;
        const scaleY = captureH / window.innerHeight;
        sx = Math.round(iframeRect.left * scaleX);
        sy = Math.round(iframeRect.top * scaleY);
        sw = Math.round(iframeRect.width * scaleX);
        sh = Math.round(iframeRect.height * scaleY);
        sx = Math.max(0, Math.min(sx, captureW - 1));
        sy = Math.max(0, Math.min(sy, captureH - 1));
        sw = Math.max(1, Math.min(sw, captureW - sx));
        sh = Math.max(1, Math.min(sh, captureH - sy));
      }

      const canvas = document.createElement("canvas");
      canvas.width = sw;
      canvas.height = sh;
      const ctx = canvas.getContext("2d");
      if (!ctx) throw new Error("Canvas 2D context unavailable");
      ctx.drawImage(video, sx, sy, sw, sh, 0, 0, sw, sh);

      const dataUrl = canvas.toDataURL("image/png");
      if (composer) composer.screenshotDataUrl = dataUrl;
    } catch (err) {
      captureError = err instanceof Error ? err.message : "Screen capture failed";
    } finally {
      if (stream) {
        for (const t of stream.getTracks()) t.stop();
      }
      capturing = false;
    }
  }

  function clearScreenshot() {
    if (composer) composer.screenshotDataUrl = null;
  }
</script>

{#if composer}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="{variant === 'toolbar'
      ? 'w-full bg-card border border-border rounded-md shadow-lg p-2'
      : 'absolute bg-card border border-border rounded-md shadow-lg p-2 w-64 z-10'}"
    style={variant === "overlay"
      ? `left: ${clampedLeft(composer.x, 260)}px; top: ${clampedTop(composer.y + 14, 130)}px; pointer-events: auto;`
      : "pointer-events: auto;"}
    onclick={(e) => e.stopPropagation()}
  >
    <!-- svelte-ignore a11y_autofocus -->
    <textarea
      class="w-full text-sm bg-bg border border-hairline rounded p-1 outline-none resize-none"
      rows="3"
      placeholder="What's wrong here?"
      bind:value={composer.text}
      onkeydown={onComposerKeydown}
      autofocus
    ></textarea>
    {#if composer.selector}
      <div class="mt-1 text-[10px] text-muted mono truncate" title={composer.selector}>
        {composer.element_context ?? composer.selector}
      </div>
    {:else if composer.element_context}
      <div class="mt-1 text-[10px] text-muted truncate" title={composer.element_context}>
        {composer.element_context}
      </div>
    {:else}
      <div class="mt-1 text-[10px] text-muted italic">Approximate location</div>
    {/if}
    {#if composer.screenshotDataUrl}
      <div class="mt-2 flex items-center gap-2">
        <img
          src={composer.screenshotDataUrl}
          alt="Captured screenshot"
          class="h-12 w-auto rounded border border-hairline object-cover"
        />
        <button
          type="button"
          class="text-[10px] text-muted hover:text-fg underline"
          onclick={clearScreenshot}
        >
          Remove
        </button>
      </div>
    {/if}
    {#if captureError}
      <div class="mt-1 text-[10px] text-red-400" title={captureError}>{captureError}</div>
    {/if}
    <div class="flex justify-between items-center gap-2 mt-2">
      {#if canCapture}
        <button
          type="button"
          class="text-xs px-2 py-1 rounded bg-hover hover:opacity-80 text-fg disabled:opacity-50"
          onclick={captureScreenshot}
          disabled={capturing}
          title="Pick a screen/window to share; one frame is saved as PNG."
        >
          {capturing
            ? "Capturing…"
            : composer.screenshotDataUrl
              ? "Recapture"
              : "Capture screenshot"}
        </button>
      {:else}
        <span class="text-[10px] text-muted italic" title="Screen capture unavailable in this webview">
          No screen capture
        </span>
      {/if}
      <div class="flex items-center gap-2">
        <span class="text-[10px] text-muted hidden sm:inline">
          <span class="kbd">⌘↩</span> save · <span class="kbd">esc</span> cancel
        </span>
        <button
          type="button"
          class="text-xs px-2 py-1 rounded hover:bg-hover text-muted"
          onclick={cancelComposer}
        >
          Cancel
        </button>
        <button
          type="button"
          class="text-xs px-2 py-1 rounded bg-accent text-white hover:opacity-90"
          onclick={saveComposer}
        >
          Save
        </button>
      </div>
    </div>
  </div>
{/if}
