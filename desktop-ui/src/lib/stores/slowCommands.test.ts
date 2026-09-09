import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const src = readFileSync(
  join(dirname(fileURLToPath(import.meta.url)), "app.svelte.ts"),
  "utf8",
);

/** Commands that paint locally first and confirm in the background. */
const LOCAL_FIRST_COMMANDS = [
  "add_comment",
  "add_question",
  "add_note",
  "reply_to_thread",
  "resolve_thread",
  "delete_thread",
  "update_thread_message",
  "dismiss_finding",
  "promote_to_comment",
  "promote_to_note",
  "bulk_review_pillar",
  "unbulk_review_pillar",
  "add_ui_annotation",
  "delete_ui_annotation",
  "remove_finding_thread",
  "promote_finding_to_comment",
  "delete_finding_response",
  "update_finding_response",
  "reply_to_finding",
  "mark_inbox_item_read",
  "mark_inbox_items_read",
  "mark_all_inbox_read",
  "clear_read_inbox_items",
  "clear_inbox_items",
  "save_pr",
  "unsave_pr",
  "dismiss_remote_pr",
  "undismiss_remote_pr",
];

/** The composer guard: refuse to paint, keep the draft, say why. */
const PAINT_GUARD = "if (!app.canPaintOptimistic()) return app.explainPaintBlocked(";

describe("SLOW_COMMANDS", () => {
  it("does not treat local-first writes as slow overlay commands", () => {
    const block = src.slice(src.indexOf("const SLOW_COMMANDS"), src.indexOf("const VOID_COMMANDS"));
    for (const command of LOCAL_FIRST_COMMANDS) {
      expect(block.includes(`"${command}"`)).toBe(false);
    }
  });

  it("routes local sidecar writes through the optimistic writer", () => {
    expect(src).toContain("if (isOptimisticCommand(command))");
    expect(src).toContain("return this.cmdOptimistic(command, args ?? {});");
    expect(src).toContain("applyOptimisticOp(snap, op);");
    expect(src).toContain("rollbackOptimisticOp(originSnap, op);");
    expect(src).toContain("this.snapshot !== originSnap");
    expect(src).toContain("this.keepOptimisticOps();");
    // A blocked paint explains itself instead of silently dropping the click.
    expect(src).toContain("if (global ? this.switching : !this.canPaintOptimistic()) {");
    expect(src).toContain("this.explainPaintBlocked(command);");
    expect(src).toContain("!this.pendingTabSwitch && !this.switching");
    expect(src).toContain("snapshotViewParts(snap)");
    expect(src).toContain("const stillHere =");
    expect(src).toContain("snapshotViewIdentity(returned) === viewAtStart");
  });

  it("confirms global chrome ops with a chrome merge, never a diff replace", () => {
    const fn = src.slice(
      src.indexOf("private mergeGlobalConfirm("),
      src.indexOf("private async cmdReviewed("),
    );
    expect(fn).toContain('mergeChromeSnapshot(current, returned, "prev")');
    expect(fn).toContain("this.snapshotGeneration += 1;");
    expect(fn).toContain("this.keepOptimisticOps();");
    expect(fn).not.toContain("ingestCommandSnapshot");
    // cmdReviewed's own chrome merge takes inbox/projects from the backend, so it
    // must reapply in-flight global ops too.
    const reviewed = src.slice(
      src.indexOf("private async cmdReviewed("),
      src.indexOf("async cmd(command: string"),
    );
    expect(reviewed).toContain("this.keepOptimisticOps();");
  });
});

function component(name: string): string {
  return readFileSync(
    join(dirname(fileURLToPath(import.meta.url)), "..", "components", name),
    "utf8",
  );
}

describe("optimistic local-write call sites", () => {
  it("closes composers before firing local writes", () => {
    const thread = component("InlineThread.svelte");
    for (const command of [
      "reply_to_thread",
      "delete_thread",
      "resolve_thread",
      "update_thread_message",
      "promote_to_comment",
      "promote_to_note",
    ]) {
      expect(thread).toContain(`void app.cmd("${command}"`);
      expect(thread).not.toContain(`await app.cmd("${command}"`);
    }

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
      "promote_finding_to_comment",
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
      "promote_finding_to_comment",
    ]) {
      expect(finding).not.toContain(`await app.cmd("${command}"`);
    }

    const browser = component("BrowserView.svelte");
    expect(browser).toContain('void app.cmd("add_ui_annotation"');
    expect(browser).not.toContain('await app.cmd("add_ui_annotation"');

    const anns = component("UiAnnotationsCard.svelte");
    expect(anns).toContain('void app.cmd("delete_ui_annotation"');
    expect(anns).not.toContain('await app.cmd("delete_ui_annotation"');
  });

  it("fires app-wide chrome writes without waiting on the backend", () => {
    const inbox = component("InboxPanel.svelte");
    for (const command of [
      "mark_inbox_item_read",
      "mark_inbox_items_read",
      "mark_all_inbox_read",
      "clear_read_inbox_items",
      "clear_inbox_items",
    ]) {
      expect(inbox).toContain(`void app.cmd("${command}"`);
    }

    const sidebar = component("LeftSidebar.svelte");
    const card = component("BranchCard.svelte");
    for (const command of ["save_pr", "unsave_pr"]) {
      expect(sidebar).toContain(`void app.cmd("${command}"`);
      expect(sidebar).not.toContain(`await app.cmd("${command}"`);
      expect(card).toContain(`void app.cmd("${command}"`);
      expect(card).not.toContain(`await app.cmd("${command}"`);
    }
    for (const command of ["dismiss_remote_pr", "undismiss_remote_pr"]) {
      expect(sidebar).toContain(`void app.cmd("${command}"`);
      expect(sidebar).not.toContain(`await app.cmd("${command}"`);
    }
  });

  it("keeps composer drafts until an optimistic write can paint", () => {
    const composer = component("DiffComposer.svelte");
    expect(composer).toContain(PAINT_GUARD);
    const cmdIdx = composer.indexOf("void app.cmd(command, cmdArgs)");
    const clearIdx = composer.indexOf("diffSel.clear();");
    expect(cmdIdx).toBeGreaterThan(-1);
    expect(clearIdx).toBeGreaterThan(cmdIdx);

    const thread = component("InlineThread.svelte");
    const replyFn = thread.slice(
      thread.indexOf("function submitReply"),
      thread.indexOf("function buildPromoteBody"),
    );
    expect(replyFn).toContain(PAINT_GUARD);
    expect(replyFn.indexOf('void app.cmd("reply_to_thread"')).toBeLessThan(
      replyFn.indexOf('replyText = "";'),
    );
    // Deleting guards before it consumes the two-step confirm click.
    const deleteFn = thread.slice(
      thread.indexOf("function deleteThread"),
      thread.indexOf("function deleteReply"),
    );
    expect(deleteFn.indexOf(PAINT_GUARD)).toBeGreaterThan(-1);
    expect(deleteFn.indexOf(PAINT_GUARD)).toBeLessThan(
      deleteFn.indexOf("confirmingDelete = false;"),
    );

    const finding = component("InlineFinding.svelte");
    expect(finding).toContain(PAINT_GUARD);
    expect(finding.indexOf('void app.cmd("update_thread_message"')).toBeLessThan(
      finding.indexOf("editMessageId = null;"),
    );
    const promoteFn = finding.slice(
      finding.indexOf("function submitPromote"),
      finding.indexOf("const targetLineLabel"),
    );
    expect(promoteFn.indexOf(PAINT_GUARD)).toBeGreaterThan(-1);
    expect(promoteFn.indexOf('void app.cmd("promote_finding_to_comment"')).toBeLessThan(
      promoteFn.indexOf("showPromote = false;"),
    );
    const replyFinding = finding.slice(
      finding.indexOf("function reply()"),
      finding.indexOf("async function askAi"),
    );
    expect(replyFinding.indexOf('void app.cmd("reply_to_finding"')).toBeLessThan(
      replyFinding.indexOf('replyText = "";'),
    );

    const annotation = component("AnnotationComposer.svelte");
    const saveFn = annotation.slice(
      annotation.indexOf("function saveComposer"),
      annotation.indexOf("async function captureScreenshot"),
    );
    expect(saveFn).toContain(PAINT_GUARD);
    expect(saveFn.indexOf("onSave(")).toBeGreaterThan(saveFn.indexOf(PAINT_GUARD));
    expect(saveFn.lastIndexOf("composer = null;")).toBeGreaterThan(saveFn.indexOf("onSave("));

    const browser = component("BrowserView.svelte");
    expect(browser).toContain(PAINT_GUARD);
  });
});

describe("panel chrome is local-first", () => {
  it("paints layout before invoking toggle_panel", () => {
    const fn = src.slice(src.indexOf("togglePanel("), src.indexOf("setMainView"));
    expect(fn.indexOf("layoutPanels.toggle")).toBeGreaterThan(-1);
    expect(fn.indexOf("rightRail.toggle()")).toBeGreaterThan(-1);
    expect(fn.indexOf("layoutPanels.toggle")).toBeLessThan(fn.indexOf('invoke("toggle_panel"'));
    expect(fn.indexOf("rightRail.toggle()")).toBeLessThan(fn.indexOf('invoke("toggle_panel"'));
    expect(fn).not.toContain("ingestCommandSnapshot");
    expect(fn).not.toContain("await invoke");
  });

  it("command palette toggles all three chrome panels through togglePanel", () => {
    const palette = component("CommandPalette.svelte");
    expect(palette).toContain('id: "toggle-tree"');
    expect(palette).toContain('app.togglePanel("left")');
    expect(palette).toContain('app.togglePanel("tree")');
    expect(palette).toContain('app.togglePanel("right")');
  });

  it("opens effort levels before activating an effort-capable model", () => {
    const palette = component("CommandPalette.svelte");
    expect(palette).toContain("effortChoicesForModel");
    expect(palette).toContain("function modelItem(");
    expect(palette).toContain("effort: choice.id");
    expect(palette).toContain("if (choices.length === 0)");
    expect(palette).toContain("function goBack()");
    expect(palette).toContain("if (activeSubmenu) goBack()");
    expect(palette).toContain("if (item.submenuItems || item.view) pushSubmenu(item);");
  });
});
