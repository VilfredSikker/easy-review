<script lang="ts">
  import type { Snippet } from "svelte";

  // A dropdown drawn `fixed` at a position from `anchoredMenuPosition`, with a
  // full-window backdrop that closes it. `w-64` matches ANCHORED_MENU_WIDTH.
  interface Props {
    pos: { top: number; left: number };
    onClose: () => void;
    children: Snippet;
  }

  const { pos, onClose, children }: Props = $props();
</script>

<!-- The backdrop only catches outside clicks; it is not a control, so it has
     no key handler or role. -->
<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="fixed inset-0 z-40" onclick={onClose}></div>
<div
  class="fixed z-50 bg-ink-800 border border-ink-500 rounded shadow-xl w-64 py-1"
  style="top: {pos.top}px; left: {pos.left}px;"
  role="menu"
>
  {@render children()}
</div>
