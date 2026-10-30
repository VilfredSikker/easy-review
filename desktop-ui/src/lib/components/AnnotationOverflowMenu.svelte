<script lang="ts">
  import AnchoredMenu from "$lib/components/ui/AnchoredMenu.svelte";
  import { anchoredMenuPosition } from "$lib/anchoredMenu";
  import type { OverflowItem } from "$lib/annotationActions";

  interface Props {
    items: OverflowItem[];
    label?: string;
  }

  const { items, label = "More actions" }: Props = $props();

  let open = $state(false);
  let pos = $state({ top: 0, left: 0 });

  function toggle(e: MouseEvent) {
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    pos = anchoredMenuPosition(r, window.innerWidth);
    open = !open;
  }

  function pick(item: OverflowItem) {
    if (item.disabled) return;
    open = false;
    item.onselect();
  }
</script>

<button
  type="button"
  onclick={toggle}
  title={label}
  aria-label={label}
  aria-haspopup="menu"
  aria-expanded={open}
  class="px-1.5 py-0.5 rounded text-[12px] leading-none text-fg-3 hover:bg-hover hover:text-fg-2 {open ? 'bg-hover text-fg-2' : ''}"
>⋯</button>

{#if open}
  <AnchoredMenu {pos} onClose={() => (open = false)}>
    {#each items as item (item.id)}
      <button
        type="button"
        role="menuitem"
        title={item.title}
        disabled={item.disabled}
        onclick={() => pick(item)}
        class="w-full text-left px-3 py-1.5 text-[12px] hover:bg-hover disabled:opacity-40 {item.danger ? 'text-del-fg' : 'text-fg-2'}"
      >{item.label}</button>
    {/each}
  </AnchoredMenu>
{/if}
