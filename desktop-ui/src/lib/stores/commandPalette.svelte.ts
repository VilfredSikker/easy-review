// Single source of truth for the command palette's open/closed state.
// CommandPalette.svelte, TabStrip.svelte, and LeftSidebar.svelte all reach
// for this — keeps the prop-passing chain short.

function createCommandPaletteStore() {
  let open = $state(false);

  return {
    get open() {
      return open;
    },
    toggle() {
      open = !open;
    },
    show() {
      open = true;
    },
    close() {
      open = false;
    },
  };
}

export const commandPalette = createCommandPaletteStore();
