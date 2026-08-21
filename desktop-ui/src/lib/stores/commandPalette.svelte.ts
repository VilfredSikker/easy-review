// Single source of truth for the command palette's open/closed state.
// CommandPalette.svelte, TabStrip.svelte, and LeftSidebar.svelte all reach
// for this — keeps the prop-passing chain short.

export type CommandPaletteView = "root" | "ai-providers";

function createCommandPaletteStore() {
  let open = $state(false);
  let pendingView = $state<CommandPaletteView>("root");

  return {
    get open() {
      return open;
    },
    get pendingView() {
      return pendingView;
    },
    toggle() {
      if (open) {
        this.close();
      } else {
        this.show();
      }
    },
    show() {
      pendingView = "root";
      open = true;
    },
    showAiProviders() {
      pendingView = "ai-providers";
      open = true;
    },
    consumePendingView(): CommandPaletteView {
      const view = pendingView;
      pendingView = "root";
      return view;
    },
    close() {
      open = false;
      pendingView = "root";
    },
  };
}

export const commandPalette = createCommandPaletteStore();
