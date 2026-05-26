<script lang="ts">
  import { onExternalLinkClick } from "$lib/openExternalUrl";

  interface Props {
    text: string;
    className?: string;
  }
  const { text, className = "" }: Props = $props();

  type Node =
    | { t: "p"; v: string }
    | { t: "h"; l: number; v: string }
    | { t: "ul"; items: string[] }
    | { t: "ol"; items: string[] }
    | { t: "bq"; v: string }
    | { t: "code"; lang: string; v: string };

  function parse(md: string): Node[] {
    const lines = md.replace(/\r\n/g, "\n").split("\n");
    const out: Node[] = [];
    let i = 0;
    while (i < lines.length) {
      const line = lines[i];
      if (!line.trim()) {
        i++;
        continue;
      }
      const hm = line.match(/^(#{1,6})\s+(.*)$/);
      if (hm) {
        out.push({ t: "h", l: hm[1].length, v: hm[2] });
        i++;
        continue;
      }
      const cm = line.match(/^```(\w+)?\s*$/);
      if (cm) {
        const lang = cm[1] ?? "";
        i++;
        const code: string[] = [];
        while (i < lines.length && !lines[i].startsWith("```")) code.push(lines[i++]);
        if (i < lines.length) i++;
        out.push({ t: "code", lang, v: code.join("\n") });
        continue;
      }
      if (line.startsWith("> ")) {
        const q: string[] = [];
        while (i < lines.length && lines[i].startsWith("> ")) q.push(lines[i++].slice(2));
        out.push({ t: "bq", v: q.join("\n") });
        continue;
      }
      const um = line.match(/^\s*[-*]\s+(.+)$/);
      if (um) {
        const items: string[] = [];
        while (i < lines.length) {
          const m = lines[i].match(/^\s*[-*]\s+(.+)$/);
          if (!m) break;
          items.push(m[1]);
          i++;
        }
        out.push({ t: "ul", items });
        continue;
      }
      const om = line.match(/^\s*\d+\.\s+(.+)$/);
      if (om) {
        const items: string[] = [];
        while (i < lines.length) {
          const m = lines[i].match(/^\s*\d+\.\s+(.+)$/);
          if (!m) break;
          items.push(m[1]);
          i++;
        }
        out.push({ t: "ol", items });
        continue;
      }
      const p: string[] = [line];
      i++;
      while (i < lines.length && lines[i].trim()) {
        if (/^(#{1,6})\s+/.test(lines[i]) || /^```/.test(lines[i])) break;
        p.push(lines[i++]);
      }
      out.push({ t: "p", v: p.join("\n") });
    }
    return out;
  }

  const nodes = $derived(parse(text || ""));

  function inline(s: string): string {
    const escaped = s
      .replace(/&/g, "&amp;")
      .replace(/</g, "&lt;")
      .replace(/>/g, "&gt;");
    return escaped
      .replace(/`([^`]+)`/g, "<code>$1</code>")
      .replace(/\*\*([^*]+)\*\*/g, "<strong>$1</strong>")
      .replace(/\*([^*]+)\*/g, "<em>$1</em>")
      .replace(/\[([^\]]+)\]\((https?:\/\/[^\s)]+)\)/g, '<a href="$2" rel="noreferrer">$1</a>');
  }
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
  .markdown-text :global(blockquote) { margin: 0.3rem 0; padding-left: 0.7rem; border-left: 2px solid rgba(148,163,184,.45); }
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
    border: 1px solid rgba(148,163,184,.25);
    border-radius: 6px;
    overflow-x: auto;
    max-width: 100%;
    white-space: pre-wrap;
    word-break: break-word;
  }
  .markdown-text :global(a) { text-decoration: underline; }
</style>
