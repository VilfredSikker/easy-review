import { describe, expect, it } from "bun:test";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const src = readFileSync(
  join(dirname(fileURLToPath(import.meta.url)), "ChecklistCard.svelte"),
  "utf8",
);

describe("ChecklistCard", () => {
  it("groups rows through the tested helper rather than in the template", () => {
    expect(src).toContain("groupChecklistItems(items)");
    expect(src).toContain("{#each groups as group (group.category)}");
    expect(src).toContain("checklistProgress(items)");
  });

  it("fires the toggle without waiting on the backend", () => {
    // The optimistic op paints the checkbox; awaiting would put the round trip
    // back in front of the click.
    expect(src).toContain('void app.cmd("toggle_checklist_item", { index });');
    expect(src).not.toContain('await app.cmd("toggle_checklist_item"');
    // The address is the item's position in the flat list, not its row.
    expect(src).toContain("toggle(row.index)");
  });

  it("jumps to a related file and to a related finding", () => {
    expect(src).toContain('app.cmd("select_file", { idx: f.source_index })');
    expect(src).toContain("navigateToFinding(finding)");
    expect(src).toContain("{#each row.item.related_files as path (path)}");
    expect(src).toContain("{#each row.item.related_findings as findingId (findingId)}");
  });

  it("marks a checklist generated against another diff", () => {
    expect(src).toContain("{#if checklist && !checklist.fresh}");
    expect(src).toContain(">stale</span>");
  });

  it("points at the review action when the bucket has no checklist", () => {
    expect(src).toContain("{#if items.length === 0}");
    expect(src).toContain('app.cmd("run_ai_review", { scope: reviewScope })');
    expect(src).toContain("reviewScopeFromMode(app.snapshot?.mode)");
  });
});
