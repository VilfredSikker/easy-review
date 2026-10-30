<script lang="ts">
  import type { GhCommentSummary } from "$lib/types";
  import { app } from "$lib/stores/app.svelte";
  import { browser } from "$lib/stores/browser.svelte";
  import { copyToClipboard } from "$lib/clipboard";
  import { openExternalUrl } from "$lib/openExternalUrl";
  import { ACTION_LABEL, extractPrLinks, type PrLink, type PrLinkAction, type PrLinkKind } from "$lib/prLinks";

  interface Props {
    body: string;
    /** Conversation comments; only bots' are read, for previews and issue links. */
    comments?: readonly GhCommentSummary[];
    owner?: string;
    repo?: string;
    /** The PR on screen, so a link back to it is not listed. */
    number?: number;
    /** Override what a row's button does; the defaults reach the app's own flows. */
    onAction?: (action: PrLinkAction, link: PrLink, e: MouseEvent) => void;
  }

  const { body, comments = [], owner, repo, number, onAction }: Props = $props();

  const links = $derived(extractPrLinks({ body: body ?? "", comments, owner, repo, number }));

  async function openInSplit(url: string) {
    await browser.setUrl(url, { navigate: true });
    if (browser.layout === "hidden") await browser.setLayout("split");
  }

  function run(action: PrLinkAction, link: PrLink, e: MouseEvent) {
    if (onAction) {
      onAction(action, link, e);
      return;
    }
    switch (action) {
      case "browser-split":
      case "view-log":
        void openInSplit(link.url);
        return;
      case "open-tab":
        if (link.ref) {
          void app.cmd("open_remote_pr", { owner: link.ref.owner, repo: link.ref.repo, number: link.ref.number, replace: false });
        } else {
          void openExternalUrl(link.url);
        }
        return;
      case "open-issue":
      case "open":
        void openExternalUrl(link.url);
        return;
      case "copy":
        copyToClipboard(link.url).then(
          () => app.showToast("success", "Link copied"),
          () => app.showToast("error", "Could not copy the link"),
        );
    }
  }

  // Glyph and colour say what kind of place the link goes; the colour is a
  // theme token (ADR 0030), tinted for the tile behind it.
  const KIND_ICON: Record<PrLinkKind, { glyph: string; tone: string }> = {
    preview: { glyph: "▲", tone: "var(--color-fg)" },
    linear: { glyph: "◐", tone: "var(--color-periwinkle)" },
    pull: { glyph: "⑂", tone: "var(--color-add-fg)" },
    issue: { glyph: "◎", tone: "var(--color-success)" },
    ref: { glyph: "#", tone: "var(--color-fg-3)" },
    actions: { glyph: "▶", tone: "var(--color-info)" },
    sentry: { glyph: "◆", tone: "var(--color-question)" },
    generic: { glyph: "↗", tone: "var(--color-fg-3)" },
  };

  function iconStyle(link: PrLink): string {
    const tone = link.status === "failed" ? "var(--color-error)" : KIND_ICON[link.kind].tone;
    return `color: ${tone}; background: color-mix(in srgb, ${tone} 13%, transparent)`;
  }

  function chipClass(link: PrLink): string {
    switch (link.status) {
      case "ready":
        return "text-add-fg bg-add-fg/13";
      case "building":
        return "text-warning bg-warning/14";
      case "failed":
        return "text-error bg-error/14";
      default:
        return "text-fg-2 bg-hover";
    }
  }
</script>

{#if links.length > 0}
  <div class="mb-2.5 flex items-baseline gap-2">
    <p class="m-0 text-[11px] font-medium uppercase tracking-[.06em] text-muted">Links</p>
    <span class="text-[11.5px] text-muted">from the description and bots</span>
  </div>
  <div class="grid gap-1.5">
    {#each links as link (link.key)}
      {@const primary = link.actions.find((a) => a !== "copy")}
      {@const secondary = link.actions.filter((a) => a !== primary && a !== "copy")}
      <div
        class="lk grid items-center gap-3 rounded-[10px] border border-hairline bg-card px-3 py-2.5 hover:border-border"
        data-kind={link.kind}
      >
        <span class="grid h-[30px] w-[30px] place-items-center rounded-lg text-[13px] font-bold" style={iconStyle(link)} aria-hidden="true">
          {KIND_ICON[link.kind].glyph}
        </span>
        <div class="grid min-w-0 gap-px">
          <b class="truncate text-[13px] font-medium text-fg" title={link.url}>{link.title}</b>
          <span class="truncate text-[12px] text-muted {link.kind === 'preview' ? 'font-mono' : ''}">{link.detail}</span>
        </div>
        <span class="lk-chip inline-flex h-5 items-center whitespace-nowrap rounded-[5px] px-[7px] text-[11px] font-medium {chipClass(link)}">{link.chip}</span>
        <div class="lk-a flex items-center gap-1">
          {#if primary}
            <button
              type="button"
              class="inline-flex h-[26px] items-center whitespace-nowrap rounded-[7px] border border-border bg-card px-2.5 text-[12px] font-medium text-fg-2 hover:bg-hover hover:text-fg"
              onclick={(e) => run(primary, link, e)}
            >{ACTION_LABEL[primary]}</button>
          {/if}
          {#each secondary as action (action)}
            <button
              type="button"
              class="inline-flex h-[26px] items-center whitespace-nowrap rounded-[7px] px-2 text-[12px] text-fg-3 hover:bg-hover hover:text-fg"
              onclick={(e) => run(action, link, e)}
            >{ACTION_LABEL[action]}</button>
          {/each}
          {#if link.actions.includes("copy")}
            <button
              type="button"
              class="grid h-[22px] w-[22px] place-items-center rounded-[5px] text-[13px] text-fg-3 hover:bg-hover hover:text-fg"
              aria-label="Copy link"
              title="Copy link"
              onclick={(e) => run("copy", link, e)}
            >⧉</button>
          {/if}
        </div>
      </div>
    {/each}
  </div>
{/if}

<style>
  .lk { grid-template-columns: 30px minmax(0, 1fr) auto auto; }
  @media (max-width: 640px) {
    .lk { grid-template-columns: 30px minmax(0, 1fr); }
    .lk-chip, .lk-a { grid-column: 2; justify-self: start; }
  }
</style>
