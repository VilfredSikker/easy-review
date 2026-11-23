import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const src = readFileSync(
  join(dirname(fileURLToPath(import.meta.url)), "app.svelte.ts"),
  "utf8",
);

function component(name: string): string {
  return readFileSync(
    join(dirname(fileURLToPath(import.meta.url)), "..", "components", name),
    "utf8",
  );
}

describe("optimistic local-write call sites", () => {
  it("closes composers before firing local writes", () => {
    const thread = component("InlineThread.svelte");
    // Local question/note replies stay optimistic and close at once.
    for (const command of [
      "reply_to_thread",
      "delete_thread",
      "resolve_thread",
      "update_thread_message",
      "promote_to_note",
    ]) {
      expect(thread).toContain(`void app.cmd("${command}"`);
      expect(thread).not.toContain(`await app.cmd("${command}"`);
    }
    // GitHub-kind paths post first: the draft clears only after the await,
    // and a failure keeps it.
    // A thread is routed on its GitHub id, not on source or kind.
    expect(thread).toContain("thread.github_id != null");
    expect(thread).toContain('"reply_to_github_thread"');
    expect(thread.indexOf("thread.github_id != null")).toBeLessThan(
      thread.indexOf('"reply_to_github_thread"'),
    );
    expect(thread).toContain('"promote_to_comment"');
    expect(thread).toContain("{ throwOnError: true }");
    expect(thread).not.toContain('void app.cmd("promote_to_comment"');
    const awaitIdx = thread.indexOf("await app.cmd(");
    const clearIdx = thread.indexOf('draft.replyText = "";');
    expect(awaitIdx).toBeGreaterThan(-1);
    expect(clearIdx).toBeGreaterThan(awaitIdx);

    const bar = component("ReplyActionBar.svelte");
    expect(bar).toContain('void app.cmd("resolve_thread"');
    expect(bar).not.toContain('await app.cmd("resolve_thread"');

    const finding = component("InlineFinding.svelte");
    for (const command of [
      "dismiss_finding",
      "update_thread_message",
      "update_finding_response",
      "delete_finding_response",
      "delete_thread",
      "remove_finding_thread",
      "reply_to_finding",
    ]) {
      expect(finding).toContain(`void app.cmd("${command}"`);
    }
    for (const command of [
      "dismiss_finding",
      "update_thread_message",
      "update_finding_response",
      "delete_finding_response",
      "delete_thread",
      "remove_finding_thread",
    ]) {
      expect(finding).not.toContain(`await app.cmd("${command}"`);
    }
    // A promoted finding is posted first; the finding disappears only once
    // GitHub holds the comment.
    expect(finding).toContain('"promote_finding_to_comment"');
    expect(finding).toContain("{ throwOnError: true }");
    expect(finding).not.toContain('void app.cmd("promote_finding_to_comment"');

    const composer = component("PreviewComposer.svelte");
    expect(composer).toContain('"add_comment"');
    expect(composer).toContain("{ throwOnError: true }");
    expect(composer.indexOf("await app.cmd(")).toBeLessThan(
      composer.indexOf('text = "";'),
    );

    const overview = component("overview/OverviewFindings.svelte");
    expect(overview).toContain('"promote_finding_to_comment"');
    expect(overview).toContain("{ throwOnError: true }");
    expect(overview).not.toContain('void app.cmd("promote_finding_to_comment"');

    const browser = component("BrowserView.svelte");
    expect(browser).toContain('void app.cmd("add_ui_annotation"');
    expect(browser).not.toContain('await app.cmd("add_ui_annotation"');

    const anns = component("UiAnnotationsCard.svelte");
    expect(anns).toContain('void app.cmd("delete_ui_annotation"');
    expect(anns).not.toContain('await app.cmd("delete_ui_annotation"');
  });

  it("fires app-wide chrome writes without waiting on the backend", () => {
    // The inbox reads and clears go through the inbox adapter, which sends
    // the old optimistic command with the flag off and er_send with it on.
    const inbox = component("InboxView.svelte");
    for (const method of ["markItemsRead", "clearItems"]) {
      expect(inbox).toContain(`void inboxResource.${method}(`);
      expect(inbox).not.toContain(`await inboxResource.${method}(`);
    }

    // The PR pins and Ignore go through the projects adapter, which sends
    // the old optimistic command with the flag off and er_send with it on.
    const sidebar = component("LeftSidebar.svelte");
    const card = component("BranchCard.svelte");
    for (const method of ["savePr", "unsavePr"]) {
      expect(sidebar).toContain(`void projectsResource.${method}(`);
      expect(sidebar).not.toContain(`await projectsResource.${method}(`);
      expect(card).toContain(`void projectsResource.${method}(`);
      expect(card).not.toContain(`await projectsResource.${method}(`);
    }
    for (const method of ["dismissPr", "undismissPr"]) {
      expect(sidebar).toContain(`void projectsResource.${method}(`);
      expect(sidebar).not.toContain(`await projectsResource.${method}(`);
    }
  });

});

describe("panel chrome is local-first", () => {
  it("paints layout before invoking toggle_panel", () => {
    const fn = src.slice(src.indexOf("togglePanel("), src.indexOf("setMainView"));
    expect(fn.indexOf("layoutPanels.toggle")).toBeGreaterThan(-1);
    expect(fn.indexOf("rightRail.toggle()")).toBeGreaterThan(-1);
    expect(fn.indexOf("layoutPanels.toggle")).toBeLessThan(fn.indexOf("mirrorPanelToggle("));
    expect(fn.indexOf("rightRail.toggle()")).toBeLessThan(fn.indexOf("mirrorPanelToggle("));
    expect(fn).not.toContain("ingestCommandSnapshot");
    expect(fn).not.toContain("await invoke");
  });

  it("command palette toggles all three chrome panels through togglePanel", () => {
    // The View & Layout menu is built in commandPaletteMenus.ts.
    const palette = readFileSync(
      join(dirname(fileURLToPath(import.meta.url)), "..", "commandPaletteMenus.ts"),
      "utf8",
    );
    expect(palette).toContain('id: "toggle-tree"');
    expect(palette).toContain('app.togglePanel("left")');
    expect(palette).toContain('app.togglePanel("tree")');
    expect(palette).toContain('app.togglePanel("right")');
  });

  it("opens effort levels before activating an effort-capable model", () => {
    const palette = component("CommandPalette.svelte");
    expect(palette).toContain("effortChoicesForModel");
    expect(palette).toContain("function modelItem(");
    expect(palette).toContain("pickAi(providerId, m.id, choice.id)");
    expect(palette).toContain("if (choices.length === 0)");
    expect(palette).toContain("function goBack()");
    expect(palette).toContain("if (activeSubmenu) goBack()");
    expect(palette).toContain("if (item.submenuItems || item.view) pushSubmenu(item);");
  });
});
