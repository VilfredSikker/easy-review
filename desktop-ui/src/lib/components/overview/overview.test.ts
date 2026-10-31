import { describe, expect, it } from "bun:test";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import type { AppSnapshot, FlatFinding, OtherBucketSnapshot } from "$lib/types";
import { canPromote, confidenceWord, lensCaption, overviewFindingsView, promoteBody } from "./findingsList";
import { emptyBucketCopy } from "./emptyBucket";

function finding(over: Partial<FlatFinding>): FlatFinding {
  return {
    id: "f",
    file: "src/a.ts",
    line: 1,
    hunk_index: 0,
    severity: "low",
    confidence: "tentative",
    lens_category: "",
    resolved: false,
    expert_label: null,
    agent_label: "General",
    raised_by: [],
    providers: [],
    security_stage: false,
    title: "t",
    message_markdown: "",
    promoted_to: null,
    thread_id: null,
    ...over,
  };
}

describe("overviewFindingsView", () => {
  const open = [
    finding({ id: "low", severity: "low", confidence: "confirmed", providers: ["openai"] }),
    finding({ id: "high", severity: "high", agent_label: "Data", providers: ["openai", "anthropic"] }),
    finding({ id: "info", severity: "med", confidence: "informational" }),
  ];
  const resolved = [finding({ id: "done", severity: "high", resolved: true })];

  it("lists open findings by severity, then resolved ones, behind the gate", () => {
    const view = overviewFindingsView({ findings: open, resolved_findings: resolved }, "tentative");
    expect(view.rows.map((f) => f.id)).toEqual(["high", "low", "done"]);
    expect(view.gatedCount).toBe(1);
    expect(view.counts).toEqual({ high: 1, med: 1, low: 1 });
    expect(view.resolvedCount).toBe(1);
  });

  it("names the lenses and counts distinct providers", () => {
    const view = overviewFindingsView({ findings: open, resolved_findings: [] }, "informational");
    expect(view.lenses).toEqual(["General", "Data"]);
    expect(view.reviewerCount).toBe(2);
    expect(lensCaption(view.lenses, view.reviewerCount)).toBe("General · Data · 2 reviewers");
    expect(lensCaption(["General"], 1)).toBe("General");
  });

  it("handles a missing review", () => {
    const view = overviewFindingsView(null, "informational");
    expect(view.rows).toEqual([]);
    expect(view.lenses).toEqual([]);
  });
});

describe("finding row", () => {
  it("says resolved in place of the grade", () => {
    expect(confidenceWord({ resolved: true, confidence: "confirmed" })).toBe("resolved");
    expect(confidenceWord({ resolved: false, confidence: "confirmed" })).toBe("confirmed");
  });

  it("offers Promote only on the PR view, for an open unpromoted finding", () => {
    expect(canPromote({ resolved: false, promoted_to: null }, true)).toBe(true);
    expect(canPromote({ resolved: false, promoted_to: null }, false)).toBe(false);
    expect(canPromote({ resolved: true, promoted_to: null }, true)).toBe(false);
    expect(canPromote({ resolved: false, promoted_to: "123" }, true)).toBe(false);
  });

  it("quotes replies under the finding in the promote body", () => {
    const f = finding({
      title: "Title",
      message_markdown: "Body",
      responses: [
        { id: "r1", author: "AI", kind: "ai", timestamp: "2026-10-30T09:00:00Z", body_markdown: "line1\nline2", origin: "finding_response", editable: false, deletable: true },
        { id: "r2", author: "AI", kind: "ai", timestamp: "2026-10-30T09:01:00Z", body_markdown: "…thinking", origin: "finding_response", editable: false, deletable: true },
      ],
    });
    expect(promoteBody(f, null)).toBe("Title\n\nBody\n\n> **AI** replied:\n> line1\n> line2");
  });
});

const other: OtherBucketSnapshot = {
  view: "pr",
  has_triage: true,
  has_review: true,
  finding_count: 4,
  pillar_count: 4,
  tour_shared: false,
  same_diff: false,
};

function bucket(over: Partial<AppSnapshot> = {}): Pick<AppSnapshot, "mode" | "ai" | "tour" | "other_bucket"> {
  return {
    mode: "branch",
    ai: { triage: null, has_review_json: false, summary_markdown: null, findings: [] } as unknown as AppSnapshot["ai"],
    tour: undefined,
    other_bucket: other,
    ...over,
  };
}

describe("emptyBucketCopy", () => {
  it("points the local branch view at the PR diff view's artifacts", () => {
    const copy = emptyBucketCopy(bucket());
    expect(copy?.title).toBe("No triage, review or tour in the Local branch view");
    expect(copy?.body).toBe(
      "Review artifacts belong to the view that produced them. The PR diff view has a triage, 4 findings and a 4-pillar tour; this view has its own, empty bucket.",
    );
    expect(copy?.switchLabel).toBe("Switch to PR diff");
  });

  it("never claims no tour while a shared tour is on screen (ADR 0025)", () => {
    const copy = emptyBucketCopy(
      bucket({
        tour: { available: true, fresh: true, scope: "pr", title: "", overviewMarkdown: "", pillars: [] },
        other_bucket: { ...other, tour_shared: true, same_diff: true },
      }),
    );
    expect(copy?.title).toBe("No triage or review in the Local branch view");
    expect(copy?.body).toContain("has a triage and 4 findings;");
    expect(copy?.body).not.toContain("pillar");
    expect(copy?.body).toContain("The tour is shared");
  });

  it("is absent once this view has a triage or a review", () => {
    const withTriage = bucket();
    withTriage.ai = { ...withTriage.ai, triage: {} as AppSnapshot["ai"]["triage"] };
    expect(emptyBucketCopy(withTriage)).toBeNull();
    const withReview = bucket();
    withReview.ai = { ...withReview.ai, has_review_json: true };
    expect(emptyBucketCopy(withReview)).toBeNull();
  });

  it("is absent without another bucket, or when on that bucket's own view", () => {
    expect(emptyBucketCopy(bucket({ other_bucket: null }))).toBeNull();
    expect(emptyBucketCopy(bucket({ mode: "pr" }))).toBeNull();
  });

  it("says when the other view is empty too", () => {
    const copy = emptyBucketCopy(
      bucket({ other_bucket: { ...other, has_triage: false, has_review: false, pillar_count: 0 } }),
    );
    expect(copy?.body).toContain("The PR diff view has no artifacts either;");
  });

  it("works from the PR diff view toward the local branch view", () => {
    const copy = emptyBucketCopy(
      bucket({ mode: "pr", other_bucket: { ...other, view: "branch", finding_count: 1, pillar_count: 0 } }),
    );
    expect(copy?.title).toBe("No triage, review or tour in the PR diff view");
    expect(copy?.body).toContain("The Local branch view has a triage and 1 finding;");
    expect(copy?.switchLabel).toBe("Switch to Local branch");
  });
});

const read = (name: string) => readFileSync(join(dirname(fileURLToPath(import.meta.url)), name), "utf8");

describe("overview writes", () => {
  const PAINT_GUARD = "if (!app.canPaintOptimistic()) return app.explainPaintBlocked(";

  it("guards promote and resolve before painting, and keeps Promote behind the modal", () => {
    const src = read("OverviewFindings.svelte");
    const promoteFn = src.slice(src.indexOf("function submitPromote"), src.indexOf("</script>"));
    expect(promoteFn.indexOf(PAINT_GUARD)).toBeGreaterThan(-1);
    expect(promoteFn.indexOf('void app.cmd("promote_finding_to_comment"')).toBeLessThan(promoteFn.indexOf("promoting = null;"));
    expect(src).toContain("<PromoteModal");
    expect(src).not.toContain('onclick={() => void app.cmd("promote_finding_to_comment"');
    const resolveFn = src.slice(src.indexOf("function toggleResolved"), src.indexOf("let promoting"));
    expect(resolveFn).toContain(PAINT_GUARD);
  });

  it("keeps the comment draft when posting fails", () => {
    const src = read("OverviewActivity.svelte");
    const fn = src.slice(src.indexOf("async function comment"), src.indexOf("</script>"));
    expect(fn).toContain("{ throwOnError: true }");
    expect(fn.indexOf('draft = "";')).toBeGreaterThan(fn.indexOf('await app.cmd("post_github_pr_comment"'));
    expect(fn.indexOf('draft = "";')).toBeLessThan(fn.indexOf("catch"));
  });

  it("scrolls to a pillar only after the tour mode has landed", () => {
    const src = read("OverviewTour.svelte");
    const fn = src.slice(src.indexOf("async function openPillar"), src.indexOf("</script>"));
    const setMode = fn.indexOf('await app.cmd("set_mode", { mode: "tour" })');
    expect(setMode).toBeGreaterThan(-1);
    expect(fn).not.toContain('reviewPage.show("tour")');
    expect(fn.indexOf('if (!reviewPage.available("tour")) return;')).toBeGreaterThan(-1);
    expect(fn.indexOf('app.snapshot?.mode !== "tour") return;')).toBeGreaterThan(setMode);
    expect(fn.indexOf("await diffNav.scrollToFile(first)")).toBeGreaterThan(setMode);
    const row = src.slice(src.indexOf('data-testid="overview-tour-row"'));
    expect(row.slice(0, row.indexOf("onclick"))).toContain("disabled={!tourAvailable}");
  });

  it("draws avatars from theme tokens, never hex (ADR 0030)", () => {
    for (const name of ["OverviewActivity.svelte", "OverviewFindings.svelte", "OverviewTour.svelte", "EmptyBucket.svelte"]) {
      expect(read(name)).not.toMatch(/#[0-9a-fA-F]{6}\b/);
    }
  });
});

describe("overview jumps", () => {
  const card = (name: string) => readFileSync(join(dirname(fileURLToPath(import.meta.url)), "..", name), "utf8");
  const SHOW_DIFF = 'reviewPage.show("diff");';

  /** The body of one function, from its declaration to the next one. */
  function fnBody(src: string, decl: string): string {
    const start = src.indexOf(decl);
    expect(start).toBeGreaterThan(-1);
    const next = src.indexOf("function ", start + decl.length);
    return src.slice(start, next === -1 ? undefined : next);
  }

  // These cards used to sit beside the diff; on the Overview no diff is
  // mounted, so a jump that only selects a file changes nothing on screen.
  it.each([
    ["TriageCard.svelte", "async function navigateToPath", 'await app.cmd("select_file"'],
    ["FileRisksCard.svelte", "async function jumpTo(", 'await app.cmd("select_file"'],
    ["ChecklistCard.svelte", "async function jumpToFile", 'await app.cmd("select_file"'],
    ["ChecklistCard.svelte", "async function jumpToFinding", "await navigateToFinding(finding)"],
    ["overview/OverviewFindings.svelte", "async function open(", "await navigateToFinding(finding)"],
    ["overview/OverviewAside.svelte", "async function openFile", 'void app.cmd("select_file"'],
  ])("%s switches the main view to the diff before %s jumps", (name, decl, jump) => {
    const fn = fnBody(card(name), decl);
    const show = fn.indexOf(SHOW_DIFF);
    const settle = fn.indexOf("await tick();", show);
    expect(show).toBeGreaterThan(-1);
    expect(settle).toBeGreaterThan(show);
    expect(fn.indexOf(jump)).toBeGreaterThan(settle);
  });

  it("keeps the active tab's agent output reachable from the Overview", () => {
    const src = read("OverviewView.svelte");
    expect(src).toContain('import AgentOutputCard from "$lib/components/AgentOutputCard.svelte";');
    expect(src).toContain("<AgentOutputCard />");
  });
});
