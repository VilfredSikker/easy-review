import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const src = readFileSync(
  join(dirname(fileURLToPath(import.meta.url)), "app.svelte.ts"),
  "utf8",
);

describe("SLOW_COMMANDS", () => {
  it("does not treat local-first comment writes as slow overlay commands", () => {
    const block = src.slice(src.indexOf("const SLOW_COMMANDS"), src.indexOf("const VOID_COMMANDS"));
    for (const command of [
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
    ]) {
      expect(block.includes(`"${command}"`)).toBe(false);
    }
  });

  it("routes local sidecar writes through the optimistic writer", () => {
    expect(src).toContain("if (isOptimisticCommand(command))");
    expect(src).toContain("return this.cmdOptimistic(command, args ?? {});");
    expect(src).toContain("applyOptimisticOp(snap, op);");
    expect(src).toContain("rollbackOptimisticOp(this.snapshot, op);");
    expect(src).toContain("optimisticInvokeArgs(command, args, op)");
    expect(src).toContain("if (!snap || this.pendingTabSwitch) return;");
    expect(src).toContain("if (!op) return;");
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
    expect(finding).toContain('void app.cmd("dismiss_finding"');
    expect(finding).not.toContain('await app.cmd("dismiss_finding"');
    expect(finding).toContain('void app.cmd("update_thread_message"');
    expect(finding).not.toContain('await app.cmd("update_thread_message"');

    const browser = component("BrowserView.svelte");
    expect(browser).toContain('void app.cmd("add_ui_annotation"');
    expect(browser).not.toContain('await app.cmd("add_ui_annotation"');

    const anns = component("UiAnnotationsCard.svelte");
    expect(anns).toContain('void app.cmd("delete_ui_annotation"');
    expect(anns).not.toContain('await app.cmd("delete_ui_annotation"');
  });
});
