<script lang="ts">
  import { onExternalLinkClick } from "$lib/openExternalUrl";
  import { parseMarkdown, renderInline as inline } from "$lib/markdown";

  interface Props {
    text: string;
    className?: string;
  }
  const { text, className = "" }: Props = $props();

  const nodes = $derived(parseMarkdown(text || ""));
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class={`markdown-text min-w-0 max-w-full ${className}`}
  onclick={onExternalLinkClick}
>
  {#each nodes as n}
    {#if n.t === "h"}
      {#if n.l === 1}<h1>{@html inline(n.v)}</h1>
      {:else if n.l === 2}<h2>{@html inline(n.v)}</h2>
      {:else if n.l === 3}<h3>{@html inline(n.v)}</h3>
      {:else if n.l === 4}<h4>{@html inline(n.v)}</h4>
      {:else if n.l === 5}<h5>{@html inline(n.v)}</h5>
      {:else}<h6>{@html inline(n.v)}</h6>{/if}
    {:else if n.t === "p"}
      <p>{@html inline(n.v)}</p>
    {:else if n.t === "ul"}
      <ul>{#each n.items as it}<li>{@html inline(it)}</li>{/each}</ul>
    {:else if n.t === "ol"}
      <ol>{#each n.items as it}<li>{@html inline(it)}</li>{/each}</ol>
    {:else if n.t === "bq"}
      <blockquote>{@html inline(n.v)}</blockquote>
    {:else if n.t === "code"}
      <pre><code class={n.lang ? `language-${n.lang}` : ""}>{n.v}</code></pre>
    {:else if n.t === "table"}
      <div class="md-table-wrap">
        <table>
          <thead>
            <tr>
              {#each n.header as cell, c}
                <th style={n.align[c] ? `text-align:${n.align[c]}` : ""}>{@html inline(cell)}</th>
              {/each}
            </tr>
          </thead>
          <tbody>
            {#each n.rows as row}
              <tr>
                {#each n.header as _, c}
                  <td style={n.align[c] ? `text-align:${n.align[c]}` : ""}>{@html inline(row[c] ?? "")}</td>
                {/each}
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    {/if}
  {/each}
</div>

<style>
  .markdown-text {
    overflow-wrap: anywhere;
    word-break: break-word;
  }
  .markdown-text :global(p),
  .markdown-text :global(li),
  .markdown-text :global(blockquote),
  .markdown-text :global(h1),
  .markdown-text :global(h2),
  .markdown-text :global(h3),
  .markdown-text :global(h4),
  .markdown-text :global(h5),
  .markdown-text :global(h6) {
    overflow-wrap: anywhere;
    word-break: break-word;
    max-width: 100%;
  }
  .markdown-text :global(p) { margin: 0 0 0.4rem 0; }
  .markdown-text :global(h1), .markdown-text :global(h2), .markdown-text :global(h3),
  .markdown-text :global(h4), .markdown-text :global(h5), .markdown-text :global(h6) {
    margin: 0.5rem 0 0.3rem 0;
    font-weight: 600;
  }
  .markdown-text :global(ul), .markdown-text :global(ol) { margin: 0.3rem 0 0.45rem 1.1rem; }
  .markdown-text :global(blockquote) { margin: 0.3rem 0; padding-left: 0.7rem; border-left: 2px solid color-mix(in srgb, var(--color-fg-3) 45%, transparent); }
  .markdown-text :global(code) {
    font-family: "JetBrains Mono", monospace;
    font-size: .9em;
    overflow-wrap: anywhere;
    word-break: break-word;
    white-space: pre-wrap;
  }
  .markdown-text :global(pre) {
    margin: 0.4rem 0;
    padding: 0.55rem;
    border: 1px solid color-mix(in srgb, var(--color-fg-3) 25%, transparent);
    border-radius: 6px;
    overflow-x: auto;
    max-width: 100%;
    white-space: pre-wrap;
    word-break: break-word;
  }
  .markdown-text :global(a) { text-decoration: underline; }
  .markdown-text :global(.md-table-wrap) {
    margin: 0.45rem 0;
    max-width: 100%;
    overflow-x: auto;
  }
  .markdown-text :global(table) {
    border-collapse: collapse;
    font-size: 0.9em;
    width: max-content;
    max-width: 100%;
  }
  .markdown-text :global(th),
  .markdown-text :global(td) {
    border: 1px solid color-mix(in srgb, var(--color-fg-3) 25%, transparent);
    padding: 0.28rem 0.55rem;
    text-align: left;
    vertical-align: top;
    overflow-wrap: anywhere;
    word-break: break-word;
  }
  .markdown-text :global(th) {
    font-weight: 600;
    background: color-mix(in srgb, var(--color-fg-3) 12%, transparent);
  }
  .markdown-text :global(tbody tr:nth-child(even) td) {
    background: color-mix(in srgb, var(--color-fg-3) 6%, transparent);
  }
</style>
