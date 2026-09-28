import { afterAll, beforeEach, describe, expect, it, mock } from "bun:test";

// Characterisation of the global keydown handler: which action each key
// reaches, in precedence order, and whether it calls preventDefault.

const calls: string[] = [];
const log = (name: string) => () => {
  calls.push(name);
};

const state = {
  paletteOpen: false,
  dismissTopModal: false,
  popoverOpen: false,
  searchOpen: false,
  selectionActive: false,
  highlightActive: false,
  terminalOpen: false,
  modalOpen: false,
  focusedField: false,
};

mock.module("@tauri-apps/api/core", () => ({
  invoke: async (cmd: string) => {
    calls.push(`invoke:${cmd}`);
    return { kind: "opened", target: "" };
  },
}));
mock.module("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({ close: log("window.close") }),
}));
mock.module("./app.svelte", () => ({
  app: {
    snapshot: {
      active_tab: 1,
      tabs: [{}, {}],
      selected_file: 0,
      files: [
        { path: "a.ts", source_index: 0 },
        { path: "b.ts", source_index: 1 },
      ],
    },
    cmd: (name: string, args?: Record<string, unknown>) => {
      calls.push(`cmd:${name}${args && "idx" in args ? `:${String(args.idx)}` : ""}`);
      return Promise.resolve();
    },
    togglePanel: (p: string) => calls.push(`togglePanel:${p}`),
    setMainView: (v: string) => calls.push(`setMainView:${v}`),
    toggleDiffViewMode: log("toggleDiffViewMode"),
    showToast: log("showToast"),
    pushLog: log("pushLog"),
  },
}));
mock.module("./diffSelection.svelte", () => ({
  diffSel: {
    get active() {
      return state.selectionActive;
    },
    clear: log("diffSel.clear"),
  },
}));
mock.module("./referenceHighlight.svelte", () => ({
  refHighlight: {
    get popoverOpen() {
      return state.popoverOpen;
    },
    get searchOpen() {
      return state.searchOpen;
    },
    get active() {
      return state.highlightActive;
    },
    closePopover: log("refHighlight.closePopover"),
    closeSearch: log("refHighlight.closeSearch"),
    clear: log("refHighlight.clear"),
    openSearch: (prefill: string | null) => calls.push(`refHighlight.openSearch:${prefill}`),
  },
}));
mock.module("./terminal.svelte", () => ({
  terminal: {
    get open() {
      return state.terminalOpen;
    },
    toggle: log("terminal.toggle"),
  },
}));
mock.module("./browser.svelte", () => ({
  browser: {
    layout: "hidden",
    cycleLayout: log("browser.cycleLayout"),
    setLayout: (l: string) => calls.push(`browser.setLayout:${l}`),
  },
}));
mock.module("$lib/stores/prUrlModal.svelte", () => ({ openPrUrlModal: log("openPrUrlModal") }));
mock.module("./overlay.svelte", () => ({
  overlay: {
    dismissTopModal: () => {
      calls.push("overlay.dismissTopModal");
      return state.dismissTopModal;
    },
  },
}));
mock.module("./commandPalette.svelte", () => ({
  commandPalette: {
    get open() {
      return state.paletteOpen;
    },
  },
}));
mock.module("$lib/stores/fileTreeCollapse.svelte", () => ({
  fileTreeCollapse: { collapsed: new Set<string>() },
}));
mock.module("$lib/stores/diffNav.svelte", () => ({
  diffNav: { scrollToFile: (p: string) => calls.push(`diffNav.scrollToFile:${p}`) },
}));
mock.module("$lib/stores/rightRail.svelte", () => ({ rightRail: { toggle: log("rightRail.toggle") } }));

let handler: ((e: KeyboardEvent) => void) | null = null;
const fieldEl = {
  tagName: "INPUT",
  isContentEditable: false,
  blur: log("field.blur"),
  closest: () => null,
};

const realGlobals = { window: globalThis.window, document: globalThis.document };
afterAll(() => {
  Object.assign(globalThis, realGlobals);
});

Object.assign(globalThis, {
  window: {
    addEventListener: (_type: string, fn: (e: KeyboardEvent) => void) => {
      handler = fn;
    },
    removeEventListener: () => {},
    getSelection: () => null,
  },
  document: {
    get activeElement() {
      return state.focusedField ? fieldEl : null;
    },
    querySelector: (sel: string) => {
      if (sel === "[data-modal]") return state.modalOpen ? {} : null;
      calls.push(`querySelector:${sel}`);
      return null;
    },
  },
});

const { initKeyboard } = await import("./keyboard");
initKeyboard();

interface KeyInit {
  key: string;
  code?: string;
  metaKey?: boolean;
  ctrlKey?: boolean;
  shiftKey?: boolean;
  altKey?: boolean;
  inField?: boolean;
  inTerminal?: boolean;
}

/** Dispatch one keydown; returns the calls it made and whether it prevented default. */
function press(init: KeyInit): { calls: string[]; prevented: boolean } {
  calls.length = 0;
  let prevented = false;
  const target = {
    tagName: init.inField ? "INPUT" : "DIV",
    isContentEditable: false,
    closest: (sel: string) => (sel === ".xterm" && init.inTerminal ? {} : null),
  };
  const e = {
    key: init.key,
    code: init.code ?? "",
    metaKey: init.metaKey ?? false,
    ctrlKey: init.ctrlKey ?? false,
    shiftKey: init.shiftKey ?? false,
    altKey: init.altKey ?? false,
    target,
    preventDefault: () => {
      prevented = true;
    },
    stopPropagation: log("stopPropagation"),
  } as unknown as KeyboardEvent;
  if (!handler) throw new Error("initKeyboard did not register a listener");
  handler(e);
  return { calls: [...calls], prevented };
}

beforeEach(() => {
  for (const k of Object.keys(state) as (keyof typeof state)[]) state[k] = false;
});

describe("Escape precedence", () => {
  it("leaves Escape to an open command palette", () => {
    state.paletteOpen = true;
    expect(press({ key: "Escape" })).toEqual({ calls: [], prevented: false });
  });

  it("dismisses the top modal first", () => {
    state.dismissTopModal = true;
    state.popoverOpen = true;
    expect(press({ key: "Escape" })).toEqual({ calls: ["overlay.dismissTopModal"], prevented: true });
  });

  it("then the usages popover, search bar, diff selection, identifier highlight", () => {
    state.popoverOpen = state.searchOpen = state.selectionActive = state.highlightActive = true;
    expect(press({ key: "Escape" }).calls).toEqual(["overlay.dismissTopModal", "refHighlight.closePopover"]);
    state.popoverOpen = false;
    expect(press({ key: "Escape" }).calls).toEqual(["overlay.dismissTopModal", "refHighlight.closeSearch"]);
    state.searchOpen = false;
    expect(press({ key: "Escape" }).calls).toEqual(["overlay.dismissTopModal", "diffSel.clear"]);
    state.selectionActive = false;
    expect(press({ key: "Escape" })).toEqual({
      calls: ["overlay.dismissTopModal", "refHighlight.clear"],
      prevented: true,
    });
  });

  it("then blurs a focused field", () => {
    state.focusedField = true;
    expect(press({ key: "Escape" })).toEqual({
      calls: ["overlay.dismissTopModal", "field.blur"],
      prevented: true,
    });
  });

  it("falls through without preventDefault when nothing is dismissed", () => {
    expect(press({ key: "Escape" })).toEqual({ calls: ["overlay.dismissTopModal"], prevented: false });
  });
});

describe("modifier shortcuts", () => {
  it("Ctrl+Q closes the window without preventDefault", () => {
    expect(press({ key: "q", ctrlKey: true })).toEqual({ calls: ["window.close"], prevented: false });
  });

  it("Cmd+K is left to the palette", () => {
    expect(press({ key: "k", metaKey: true })).toEqual({ calls: [], prevented: false });
  });

  it("Cmd+F opens search even from a field", () => {
    expect(press({ key: "f", metaKey: true, inField: true })).toEqual({
      calls: ["stopPropagation", "refHighlight.openSearch:null"],
      prevented: true,
    });
  });

  it("Cmd+B cycles the browser, but not from a field unless a modal is open", () => {
    expect(press({ key: "b", metaKey: true }).calls).toEqual(["browser.cycleLayout"]);
    expect(press({ key: "b", metaKey: true, inField: true }).calls).toEqual([]);
    state.modalOpen = true;
    expect(press({ key: "b", metaKey: true, inField: true }).calls).toEqual(["browser.cycleLayout"]);
  });

  it("Cmd+Shift+B toggles fullscreen browser", () => {
    expect(press({ key: "B", metaKey: true, shiftKey: true }).calls).toEqual(["browser.setLayout:fullscreen"]);
  });

  it("Cmd+Shift+E opens the export view", () => {
    expect(press({ key: "e", metaKey: true, shiftKey: true }).calls).toEqual(["setMainView:export-review"]);
  });

  it("Cmd+O opens a worktree, Cmd+Shift+O the PR URL modal", () => {
    expect(press({ key: "o", metaKey: true }).calls).toEqual(["cmd:open_worktree"]);
    expect(press({ key: "O", metaKey: true, shiftKey: true }).calls).toEqual(["openPrUrlModal"]);
  });

  it("Cmd+P focuses the sidebar search, falling back to the file filter", () => {
    expect(press({ key: "p", metaKey: true }).calls).toEqual([
      "querySelector:[data-left-sidebar-search-input]",
      'querySelector:input[placeholder^="Filter files"]',
    ]);
  });

  it("Cmd+T toggles the terminal; from a field only while it is open", () => {
    expect(press({ key: "t", metaKey: true }).calls).toEqual(["terminal.toggle"]);
    expect(press({ key: "t", metaKey: true, inField: true }).calls).toEqual([]);
    state.terminalOpen = true;
    expect(press({ key: "t", metaKey: true, inField: true }).calls).toEqual(["terminal.toggle"]);
  });

  it("inside the terminal only Cmd+T gets through", () => {
    expect(press({ key: "j", inTerminal: true }).calls).toEqual([]);
    expect(press({ key: "t", metaKey: true, inTerminal: true }).calls).toEqual(["terminal.toggle"]);
  });

  it("Cmd+Shift+T opens a tab, Cmd+W closes the active one", () => {
    expect(press({ key: "t", metaKey: true, shiftKey: true }).calls).toEqual(["cmd:new_tab"]);
    expect(press({ key: "w", metaKey: true }).calls).toEqual(["cmd:close_tab:1"]);
  });

  it("Cmd+digit selects a tab, and swallows an out-of-range digit without preventDefault", () => {
    expect(press({ key: "2", metaKey: true })).toEqual({ calls: ["cmd:select_tab:1"], prevented: true });
    expect(press({ key: "9", metaKey: true })).toEqual({ calls: [], prevented: false });
  });

  it("Cmd+R force-refreshes the diff", () => {
    expect(press({ key: "r", metaKey: true }).calls).toEqual(["cmd:force_refresh_diff"]);
  });
});

describe("panel toggles", () => {
  it("[ ] \\ toggle panels without modifiers", () => {
    expect(press({ key: "[" }).calls).toEqual(["togglePanel:left"]);
    expect(press({ key: "]" }).calls).toEqual(["rightRail.toggle"]);
    expect(press({ key: "\\" }).calls).toEqual(["togglePanel:tree"]);
  });

  it("are blocked by a modal unless Cmd is held", () => {
    state.modalOpen = true;
    expect(press({ key: "[" }).calls).toEqual([]);
    expect(press({ key: "[", metaKey: true }).calls).toEqual(["togglePanel:left"]);
  });
});

describe("bare keys", () => {
  it("backtick toggles the terminal", () => {
    expect(press({ key: "`" })).toEqual({ calls: ["terminal.toggle"], prevented: true });
  });

  it("j/k move through files in tree order", async () => {
    expect(press({ key: "j" }).calls).toEqual(["cmd:select_file:1"]);
    await Promise.resolve();
    expect(calls).toEqual(["cmd:select_file:1", "diffNav.scrollToFile:b.ts"]);
    expect(press({ key: "k" }).calls).toEqual(["cmd:select_file:1"]);
  });

  it("d, R, / and e reach their actions", () => {
    expect(press({ key: "d" })).toEqual({ calls: ["toggleDiffViewMode"], prevented: true });
    expect(press({ key: "R" }).calls).toEqual(["cmd:refresh_diff"]);
    expect(press({ key: "/" }).calls).toEqual(['querySelector:input[placeholder^="Filter files"]']);
    expect(press({ key: "e" }).calls).toEqual(["invoke:open_in_vscode"]);
  });

  it("are ignored in a field, under a modal, or with a modifier", () => {
    expect(press({ key: "d", inField: true }).calls).toEqual([]);
    state.modalOpen = true;
    expect(press({ key: "d" }).calls).toEqual([]);
    state.modalOpen = false;
    expect(press({ key: "d", altKey: true }).calls).toEqual([]);
  });
});
