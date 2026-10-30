import { describe, expect, it } from "bun:test";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const src = readFileSync(
  join(dirname(fileURLToPath(import.meta.url)), "TriageCard.svelte"),
  "utf8",
);

/** The source of one Disclosure row, from its label to its closing tag. */
function row(label: string): string {
  const start = src.indexOf(`label="${label}"`);
  expect(start).toBeGreaterThan(-1);
  const end = src.indexOf("</Disclosure>", start);
  expect(end).toBeGreaterThan(start);
  return src.slice(start, end);
}

function occurrences(marker: string): number {
  return src.split(marker).length - 1;
}

describe("TriageCard", () => {
  it("summarises the verdict as pills built by the tested helper", () => {
    expect(src).toContain("summaryPills(triage)");
    expect(src).toContain("{#each pills as pill (pill.label)}");
    expect(src).toContain("<Pill tone={pill.tone}");
  });

  it("starts every row collapsed", () => {
    for (const flag of ["verdictOpen", "filesOpen"]) {
      expect(src).toContain(`let ${flag} = $state(false);`);
      expect(src).toContain(`bind:open={${flag}}`);
    }
  });

  it("keeps every paragraph inside its row", () => {
    expect(row("Verdict")).toContain("{verdictSummary}");
    expect(row("Verdict")).toContain("{triage.rationale}");
    expect(row("Priority files")).toContain("navigateToPath(pf.path)");
    // Each paragraph is rendered once, so "inside its row" is also "nowhere else".
    for (const marker of ["{triage.rationale}", "<MarkdownText"]) {
      expect(occurrences(marker)).toBe(1);
    }
  });

  it("gives each collapsed row a one-line teaser", () => {
    expect(src).toContain("preview={previewLine(triage.rationale) || verdictSummary}");
    expect(src).toContain("preview={filesPreview(triage.priority_files)}");
    expect(src).toContain("badge={triage.priority_files.length}");
  });

  it("jumps the diff to a priority file and names its risk without hiding the reason", () => {
    expect(src).toContain('app.cmd("select_file", { idx: f.source_index })');
    expect(src).toContain("onclick={() => navigateToPath(pf.path)}");
    expect(src).toContain("{priorityDotClass(pf.risk)}");
    // Screen readers get the risk as text; an aria-label would replace the
    // path and reason as the button's name.
    expect(row("Priority files")).toContain('<span class="sr-only">{pf.risk} risk</span>');
    expect(row("Priority files")).not.toContain("aria-label=");
  });

  it("routes the follow-up through the tested mapping", () => {
    expect(src).toContain("followUpFor(triage, reviewScope)");
    expect(src).toContain('if (next.kind === "arena")');
    expect(src).toContain("arena.openLauncher()");
    expect(src).toContain("await app.cmd(next.command, next.args)");
  });

  it("states the impression and the takeaways from the tested helper, not from raw fields", () => {
    expect(src).toContain("const rows = $derived(takeaways(triage));");
    expect(src).toContain("{#each rows as row (row.key)}");
    expect(src).toContain("text={triage.first_impression}");
    expect(src).not.toContain("triage.guard");
    expect(src).not.toContain("triage.reach_reason");
  });

  it("shows the fresh or stale built-from chip beside the label", () => {
    expect(src).toContain("triageFreshness(triage, app.snapshot?.diff_stale?.built_from_oid)");
    expect(src).toContain("{freshness.text}");
  });

  it("lists domains as a list, so the label reaches assistive tech", () => {
    expect(src).toMatch(/<ul[^>]*aria-label="Domains"/);
    expect(src).not.toMatch(/<div[^>]*aria-label=/);
  });

  it("marks stale triage while routing its default follow-up through the review flow", () => {
    expect(src).toContain('app.cmd("start_review_flow", { scope: reviewScope, erDir: snap.review_bucket_er_dir })');
    expect(src).toContain("flowControls.confirm");
    expect(src).toContain("flowControls.busy");
    expect(src).toContain("flowControls.openRunId");
  });
});
