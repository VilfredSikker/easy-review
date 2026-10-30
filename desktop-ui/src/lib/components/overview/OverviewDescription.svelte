<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { app } from "$lib/stores/app.svelte";
  import { highlightLines } from "$lib/highlightClient";
  import { syntaxThemeById } from "$lib/syntaxThemes";
  import { onExternalLinkClick, openExternalUrl } from "$lib/openExternalUrl";
  import { fenceLanguage, refClickTarget, renderPrDescription } from "$lib/prDescription";
  import { openGithubRef, type GithubRef } from "$lib/prLinks";
  import ModalShell from "$lib/components/ui/ModalShell.svelte";

  interface Props {
    /** The PR body as written on GitHub (markdown with raw HTML). */
    body: string;
    /** The PR's repo, so a bare `#12` links within it. */
    owner?: string;
    repo?: string;
    /**
     * Turns a GitHub attachment URL into something an `<img>` can show. The
     * default goes through the backend's authenticated fetch; the webview never
     * requests a private attachment itself.
     */
    loadImage?: (url: string) => Promise<string>;
    /**
     * A `#12` or PR link was clicked. The default opens the PR in a new tab,
     * and the page on GitHub when it is not a PR the app can open.
     */
    onOpenRef?: (ref: GithubRef, url: string, e: MouseEvent) => void;
  }

  const {
    body,
    owner,
    repo,
    loadImage = (url) => invoke<string>("fetch_github_image", { url }),
    onOpenRef = (ref, url) =>
      void openGithubRef(
        ref,
        {
          openPr: (r) =>
            app.cmd("open_remote_pr", { owner: r.owner, repo: r.repo, number: r.number, replace: false }, { throwOnError: true }),
          openUrl: openExternalUrl,
        },
        url,
      ),
  }: Props = $props();

  const html = $derived(renderPrDescription(body ?? "", { owner, repo }));
  const theme = $derived(syntaxThemeById(app.currentSyntaxTheme));

  let root = $state<HTMLDivElement | null>(null);
  let zoom = $state<{ src: string; alt: string } | null>(null);

  // Attachments load as they scroll near, so a long body with ten screenshots
  // does not fetch all ten through the backend on open.
  $effect(() => {
    void html;
    const el = root;
    if (!el) return;
    const pending = [...el.querySelectorAll<HTMLImageElement>("img[data-er-src]")];
    if (pending.length === 0) return;
    let live = true;
    const load = (img: HTMLImageElement) => {
      const url = img.dataset.erSrc;
      if (!url) return;
      delete img.dataset.erSrc;
      loadImage(url).then(
        (src) => {
          if (!live) return;
          img.src = src;
          img.classList.remove("md-img-pending");
        },
        () => {
          if (!live) return;
          const link = document.createElement("a");
          link.href = url;
          link.className = "md-img-failed";
          link.textContent = `${img.alt || "Image"} · open on GitHub`;
          img.replaceWith(link);
        },
      );
    };
    if (typeof IntersectionObserver === "undefined") {
      pending.forEach(load);
      return () => (live = false);
    }
    const io = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) {
          if (!entry.isIntersecting) continue;
          io.unobserve(entry.target);
          load(entry.target as HTMLImageElement);
        }
      },
      { rootMargin: "400px 0px" },
    );
    pending.forEach((img) => io.observe(img));
    return () => {
      live = false;
      io.disconnect();
    };
  });

  // Fences highlight off the main thread with the diff's theme; until the
  // worker answers they read as plain code.
  $effect(() => {
    void html;
    const el = root;
    const t = theme;
    if (!el) return;
    let live = true;
    for (const code of el.querySelectorAll<HTMLElement>("pre > code[class*='language-']")) {
      const info = [...code.classList].find((c) => c.startsWith("language-"))?.slice("language-".length);
      const lang = fenceLanguage(info);
      if (!lang) continue;
      const lines = (code.textContent ?? "").replace(/\n$/, "").split("\n");
      highlightLines(lines, lang, t).then(
        (spans) => {
          if (!live || !code.isConnected) return;
          const out = document.createDocumentFragment();
          spans.forEach((line, i) => {
            for (const span of line) {
              const s = document.createElement("span");
              s.textContent = span.text;
              if (span.color) s.style.color = span.color;
              out.append(s);
            }
            if (i < spans.length - 1) out.append("\n");
          });
          code.replaceChildren(out);
          code.classList.add("md-hl");
        },
        () => {},
      );
    }
    return () => (live = false);
  });

  function onClick(e: MouseEvent) {
    const target = e.target as HTMLElement | null;
    const img = target?.closest?.("img");
    if (img instanceof HTMLImageElement && img.src && !img.closest("a")) {
      zoom = { src: img.src, alt: img.alt };
      return;
    }
    if (openRefClick(e)) return;
    onExternalLinkClick(e);
  }

  // Ref links carry no href (see renderPrDescription), so the app's external
  // link guard lets them through and the middle button needs its own handler.
  function openRefClick(e: MouseEvent): boolean {
    const hit = refClickTarget(e);
    if (!hit) return false;
    e.preventDefault();
    e.stopPropagation();
    if (hit.plain) onOpenRef(hit.ref, hit.url, e);
    else void openExternalUrl(hit.url);
    return true;
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key !== "Enter") return;
    const anchor = (e.target as HTMLElement | null)?.closest?.("a[data-er-ref]");
    if (!(anchor instanceof HTMLElement)) return;
    e.preventDefault();
    anchor.click();
  }
</script>

<p class="m-0 mb-2.5 text-[11px] font-medium uppercase tracking-[.06em] text-muted">Description</p>
{#if (body ?? "").trim()}
  <!-- Clicks are delegated: links open outside, refs open a tab, images zoom.
       Every one of those targets is itself a link or an image. -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="pr-description" bind:this={root} onclick={onClick} onauxclick={openRefClick} onkeydown={onKeydown}>
    <!-- Keyed: the effects below swap nodes inside the rendered HTML (a failed
         image, highlighted code), so a new body replaces the block whole. -->
    {#key html}
      <div>
        <!-- eslint-disable-next-line svelte/no-at-html-tags -- renderPrDescription runs DOMPurify with a formatting allowlist and re-checks every link and image. -->
        {@html html}
      </div>
    {/key}
  </div>
{:else}
  <p class="m-0 text-[13px] italic text-muted">No description provided.</p>
{/if}

<ModalShell
  open={zoom !== null}
  ariaLabel="Image"
  onClose={() => (zoom = null)}
  backdropClass="fixed inset-0 z-[250] bg-bg/80"
  panelClass="left-1/2 top-1/2 -translate-x-1/2 -translate-y-1/2"
  dataModal="description-image"
>
  {#if zoom}
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <img
      src={zoom.src}
      alt={zoom.alt}
      class="block max-h-[calc(100vh-96px)] max-w-[calc(100vw-48px)] cursor-zoom-out rounded-lg border border-border bg-card"
      onclick={() => (zoom = null)}
    />
    <div class="mt-2 flex items-center gap-3 text-[12px] text-fg-3">
      <span class="min-w-0 truncate">{zoom.alt}</span>
      {#if !zoom.src.startsWith("data:")}
        <button type="button" class="ml-auto shrink-0 rounded px-1.5 py-0.5 hover:bg-hover hover:text-fg" onclick={() => zoom && void openExternalUrl(zoom.src)}>Open in browser</button>
      {/if}
    </div>
  {/if}
</ModalShell>

<style>
  .pr-description {
    max-width: 68ch;
    font-size: 14px;
    line-height: 1.6;
    color: var(--color-fg);
    overflow-wrap: anywhere;
  }
  .pr-description :global(p) { margin: 0 0 12px; }
  .pr-description :global(h1), .pr-description :global(h2), .pr-description :global(h3),
  .pr-description :global(h4), .pr-description :global(h5), .pr-description :global(h6) {
    font-weight: 600;
    line-height: 1.3;
    margin: 16px 0 8px;
  }
  .pr-description :global(h1) { font-size: 18px; }
  .pr-description :global(h2) { font-size: 16px; }
  .pr-description :global(h3), .pr-description :global(h4), .pr-description :global(h5), .pr-description :global(h6) { font-size: 14px; }
  .pr-description :global(> div > :first-child) { margin-top: 0; }
  .pr-description :global(ul) { list-style: disc; padding-left: 22px; margin: 0 0 12px; }
  .pr-description :global(ol) { list-style: decimal; padding-left: 22px; margin: 0 0 12px; }
  .pr-description :global(li > p) { margin: 0 0 4px; }
  .pr-description :global(a) { color: var(--color-action); text-decoration: none; }
  .pr-description :global(a:hover) { text-decoration: underline; }
  .pr-description :global(a[data-er-ref]) { cursor: pointer; }
  .pr-description :global(a.md-mention), .pr-description :global(a.md-ref) { font-weight: 500; }
  .pr-description :global(code) {
    font-family: "JetBrains Mono", monospace;
    font-size: 12px;
    background: var(--color-hover);
    border: 1px solid var(--color-hairline);
    border-radius: 4px;
    padding: 0 4px;
  }
  .pr-description :global(pre) {
    margin: 0 0 12px;
    padding: 10px 12px;
    background: var(--color-card);
    border: 1px solid var(--color-hairline);
    border-radius: 8px;
    overflow-x: auto;
  }
  .pr-description :global(pre code) {
    display: block;
    background: none;
    border: 0;
    padding: 0;
    white-space: pre;
    line-height: 1.6;
  }
  .pr-description :global(blockquote) {
    margin: 0 0 12px;
    padding-left: 12px;
    border-left: 3px solid var(--color-border);
    color: var(--color-fg-3);
  }
  .pr-description :global(hr) { border: 0; border-top: 1px solid var(--color-hairline); margin: 16px 0; }

  /* GitHub callouts: one semantic token per kind (ADR 0030). */
  .pr-description :global(.md-alert) {
    --alert: var(--color-action);
    margin: 0 0 12px;
    padding: 8px 12px;
    border-left: 3px solid var(--alert);
    border-radius: 0 8px 8px 0;
    background: color-mix(in srgb, var(--alert) 8%, transparent);
    font-size: 13.5px;
  }
  .pr-description :global(.md-alert-tip) { --alert: var(--color-success); }
  .pr-description :global(.md-alert-important) { --alert: var(--color-question); }
  .pr-description :global(.md-alert-warning) { --alert: var(--color-warning); }
  .pr-description :global(.md-alert-caution) { --alert: var(--color-error); }
  .pr-description :global(.md-alert p) { margin: 2px 0 0; }
  .pr-description :global(.md-alert .md-alert-title) { margin: 0; color: var(--alert); font-size: 12.5px; font-weight: 600; }

  .pr-description :global(ul.md-tasks) { list-style: none; padding: 0; display: grid; gap: 4px; }
  .pr-description :global(.md-task) { display: flex; align-items: baseline; gap: 8px; font-size: 13.5px; }
  .pr-description :global(.md-task > span:last-child > p) { margin: 0; }
  .pr-description :global(.md-tb) {
    position: relative;
    top: 2px;
    flex: none;
    width: 15px;
    height: 15px;
    display: inline-grid;
    place-items: center;
    border-radius: 4px;
    border: 1.5px solid var(--color-border);
    font-size: 10px;
    color: var(--color-bg);
  }
  .pr-description :global(.md-done) { color: var(--color-fg-2); }
  .pr-description :global(.md-done .md-tb) { background: var(--color-add-fg); border-color: var(--color-add-fg); }

  .pr-description :global(.md-table-wrap) { margin: 0 0 12px; max-width: 100%; overflow-x: auto; }
  .pr-description :global(table) { border-collapse: collapse; font-size: 13px; }
  .pr-description :global(th), .pr-description :global(td) {
    border: 1px solid var(--color-border);
    padding: 5px 10px;
    text-align: left;
    white-space: nowrap;
  }
  .pr-description :global(th) { background: var(--color-hover); font-weight: 600; font-size: 12px; }

  .pr-description :global(details) {
    margin: 0 0 12px;
    padding: 6px 12px;
    border: 1px solid var(--color-hairline);
    border-radius: 8px;
    background: var(--color-card);
  }
  .pr-description :global(summary) { cursor: pointer; font-weight: 500; font-size: 13.5px; }
  .pr-description :global(details[open] > summary) { margin-bottom: 6px; }

  .pr-description :global(img) {
    max-width: min(100%, 640px);
    height: auto;
    border-radius: 8px;
    border: 1px solid var(--color-hairline);
    background: var(--color-card);
    cursor: zoom-in;
    vertical-align: middle;
  }
  /* Holds the attachment's place while the backend fetches it. */
  .pr-description :global(img.md-img-pending) { width: min(100%, 640px); min-height: 120px; cursor: progress; }
  .pr-description :global(.md-img-failed) { font-size: 12px; color: var(--color-fg-3); }
</style>
