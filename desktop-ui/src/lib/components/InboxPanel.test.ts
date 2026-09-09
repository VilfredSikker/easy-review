import { describe, expect, it } from "bun:test";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const src = readFileSync(join(dirname(fileURLToPath(import.meta.url)), "InboxPanel.svelte"), "utf8");
const list = src.slice(src.indexOf("flex-1 min-h-0 overflow-y-auto"));

describe("InboxPanel popover list", () => {
  it("does not cap the popover list", () => {
    expect(src).not.toContain("INBOX_POPOVER_LIMIT");
    expect(list).not.toMatch(/\.slice\s*\(/);
  });

  it("keeps group mark-read and clear-read on every category including chip filters", () => {
    expect(src).not.toContain("groupedView");
    expect(list).toContain("{#each inboxGroups as group");
    expect(list).toContain('app.cmd("mark_inbox_items_read"');
    expect(list).toContain('app.cmd("clear_inbox_items"');
    expect(list).toContain("disabled={groupUnread.length === 0}");
    expect(list).toContain("disabled={groupRead.length === 0}");
    expect(list).toContain("ids: groupUnread");
    expect(list).toContain("ids: groupRead");
  });
});

describe("InboxPanel message dialog", () => {
  const dialog = src.slice(src.indexOf("{#if selectedInboxMessage}"));

  it("offers opening the target in the current tab or in a new tab", () => {
    expect(dialog).toContain("Open target");
    expect(dialog).toContain("Open in new tab");
    expect(dialog).toContain("openSelectedInboxTarget(false)");
    expect(dialog).toContain("openSelectedInboxTarget(true)");
    // Tauri maps camelCase args to the command's snake_case `new_tab`.
    expect(src).toContain('app.cmd("open_inbox_item", { id: selectedInboxMessage.id, newTab })');
  });
});
