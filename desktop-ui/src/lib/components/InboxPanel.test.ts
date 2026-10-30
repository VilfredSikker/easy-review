import { describe, expect, it } from "bun:test";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const dir = dirname(fileURLToPath(import.meta.url));
const panel = readFileSync(join(dir, "InboxPanel.svelte"), "utf8");
const view = readFileSync(join(dir, "InboxView.svelte"), "utf8");
const modal = readFileSync(join(dir, "TabOpenModal.svelte"), "utf8");

describe("InboxPanel nav row", () => {
  it("opens the Inbox main view instead of a popover", () => {
    expect(panel).toContain('app.setMainView("inbox")');
    expect(panel).not.toContain("ModalShell");
  });
});

describe("InboxView", () => {
  it("keeps the open, read and clear commands reachable", () => {
    // Tauri maps camelCase args to the command's snake_case `new_tab`.
    expect(view).toContain('app.cmd("open_inbox_item", { id: thread.latest.id, newTab })');
    expect(view).toContain('app.cmd("refresh_notifications")');
    expect(view).toContain('app.cmd("mark_inbox_items_read", { ids: thread.unreadIds })');
  });

  it("selects a tab already on the PR instead of opening a second one", () => {
    expect(view).toContain("findOpenTab(");
    expect(view).toContain('app.cmd("select_tab", { idx: existing.idx })');
  });

  it("asks through the tab-open modal unless a modifier or the preference says new tab", () => {
    expect(view).toContain("resolvePrOpen(");
    expect(view).toContain("<TabOpenModal");
    expect(view).toContain("saveAlwaysNewTab(true)");
  });
});

describe("TabOpenModal", () => {
  it("defaults Enter to a new tab and keeps Cmd+Enter for replacing", () => {
    expect(modal).toContain("choose(!(e.metaKey || e.ctrlKey))");
    expect(modal).toContain("Always open in a new tab, don't ask again");
    expect(modal).toContain("data-tm-primary");
  });
});
