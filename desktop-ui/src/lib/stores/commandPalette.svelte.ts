// Single source of truth for the command palette's open/closed state.
// CommandPalette.svelte, TabStrip.svelte, and LeftSidebar.svelte all reach
// for this — keeps the prop-passing chain short.

/** A root menu the palette can open straight into. */
export type CommandPaletteMenu =
  | "menu-actions"
  | "menu-navigate"
  | "menu-view"
  | "menu-ai"
  | "menu-pr"
  | "menu-files";

export type CommandPaletteView = "root" | "ai-providers" | CommandPaletteMenu;

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
    /**
     * Opens on one menu, with ← leading back to the root. Lets a control that
     * stands for a group of actions (a breadcrumb, an overflow button) hand
     * over to the palette instead of growing its own menu.
     */
    showMenu(menu: CommandPaletteMenu) {
      pendingView = menu;
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
