import { describe, expect, it } from "bun:test";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const dir = dirname(fileURLToPath(import.meta.url));
const panel = readFileSync(join(dir, "InboxPanel.svelte"), "utf8");
const view = readFileSync(join(dir, "InboxView.svelte"), "utf8");
const modal = readFileSync(join(dir, "TabOpenModal.svelte"), "utf8");

describe("InboxPanel nav row", () => {
  it("toggles the Inbox main view instead of opening a popover", () => {
    expect(panel).toContain("app.toggleInbox()");
    expect(panel).not.toContain("ModalShell");
  });

  it("leaves Esc to full-window overlays open over the Inbox", () => {
    expect(view).toContain("commandPalette.open || arena.overlayOpen || app.showEmptyState");
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

  it("offers a way back from 'always open in a new tab'", () => {
    expect(view).toContain("saveAlwaysNewTab(false)");
    expect(view).toContain("Ask before opening");
  });

  it("keeps the selected row while marking it read drops it from the Unread filter", () => {
    expect(view).toContain("keepSelectedListed(");
  });

  it("marks read and clears only the threads shown, not the whole inbox", () => {
    expect(view).not.toContain("mark_all_inbox_read");
    expect(view).not.toContain("clear_read_inbox_items");
    expect(view).toContain('app.cmd("clear_inbox_items", { ids: readThreadItemIds(visible) })');
  });

  it("builds the modal request once per open, not on every snapshot", () => {
    expect(view).toContain("const openReq = $derived(");
    expect(view).toContain("key: thread.key");
  });
});

describe("TabOpenModal", () => {
  it("routes Enter through tabOpenKeyChoice so other buttons keep their own Enter", () => {
    expect(modal).toContain("tabOpenKeyChoice(");
    expect(modal).toContain("Always open in a new tab, don't ask again");
    expect(modal).toContain("data-tm-primary");
  });

  it("resets its choices on a new open, not on a new request object", () => {
    expect(modal).toContain("request?.key");
  });
});
