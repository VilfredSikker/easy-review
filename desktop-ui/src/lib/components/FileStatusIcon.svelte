<script lang="ts">
  import type { FileStatusIconKind } from "$lib/fileStatus";

  interface Props {
    kind: FileStatusIconKind;
    class?: string;
    title?: string;
    size?: number;
  }
  const { kind, class: className = "", title, size = 14 }: Props = $props();
</script>

<svg
  width={size}
  height={size}
  viewBox="0 0 24 24"
  fill="none"
  stroke="currentColor"
  stroke-width="2"
  stroke-linecap="round"
  stroke-linejoin="round"
  class="shrink-0 {className}"
  aria-hidden={title ? undefined : true}
  role={title ? "img" : undefined}
>
  {#if title}<title>{title}</title>{/if}
  {#if kind === "pencil"}
    <path d="M17 3a2.85 2.83 0 1 1 4 4L7.5 20.5 2 22l1.5-5.5Z" />
    <path d="m15 5 4 4" />
  {:else if kind === "plus-circle"}
    <circle cx="12" cy="12" r="10" />
    <path d="M8 12h8" />
    <path d="M12 8v8" />
  {:else if kind === "minus-circle"}
    <circle cx="12" cy="12" r="10" />
    <path d="M8 12h8" />
  {:else if kind === "alert"}
    <circle cx="12" cy="12" r="10" />
    <path d="M12 8v4" />
    <path d="M12 16h.01" />
  {/if}
</svg>
