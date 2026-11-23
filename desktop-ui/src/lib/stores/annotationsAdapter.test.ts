import { describe, expect, it } from "bun:test";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import type { AiAnnotations } from "$lib/protocol/generated/AiAnnotations";
import type { Resource } from "$lib/protocol/generated/Resource";
import type { ResourceKey } from "$lib/protocol/generated/ResourceKey";
import type { Versioned } from "$lib/protocol/generated/Versioned";
import type { AiSnapshot, AppSnapshot, ThreadMessage, ThreadSnapshot } from "$lib/types";
import type { PendingPaint } from "./annotationWrites";
import { AnnotationsAdapter, JOBS, annotationsKey, uiAnnotationsKey } from "./annotationsAdapter";
import { diffKey } from "./diffAdapter";
import { resourceKeyId } from "./resourceStore";

const TAB = 3;
const here = dirname(fileURLToPath(import.meta.url));

const message = (id: string, body: string): ThreadMessage => ({
  id,
  author: "You",
  kind: "you",
  timestamp: "t1",
  body_markdown: body,
});

function ai(): AiAnnotations {
  const finding = {
    id: "f-1", file: "src/a.rs", line: 2, hunk_index: 0, severity: "high", confidence: "confirmed",
    arbiter_verdict: null, arbiter_rationale: null, lens_category: "", resolved: false,
    expert_label: null, agent_label: "General", raised_by: [], providers: [], security_stage: false,
    title: "A defect", message_markdown: "", promoted_to: null, thread_id: null, responses: [],
  };
  const root = { ...message("q-1", "why?"), origin: null, source: null, editable: null, deletable: null };
  const thread = {
    id: "q-1", kind: "question", file: "src/a.rs", line: 2, side: "RIGHT", source: "local",
    stale: false, resolved: false, root, replies: [], promoted_to: null, github_id: null,
  };
  const counts = { high: 1, med: 0, low: 0, local_comment_count: 0, github_comment_count: 0, comments: 0, questions: 1, notes: 0 };
  const arbiter = { arbiter_dropped: 0, arbiter_merged: 0, arbiter_unmatched: 0, arbiter_regraded: 0 };
  return {
    fresh: true, stale_reason: null, summary_markdown: null, agent_summaries: {}, ...counts, ...arbiter,
    threads: [thread], findings: [finding], file_risks: [], resolved_findings: [], dropped_findings: [],
    min_trust_default: "informational", review_created_at: null, review_diff_hash: null,
    has_review_json: true, eligible_comment_count: 0, triage: null, checklist: null, diagrams: [], diagram_presets: [],
  };
}

const THINKING = "…thinking";

/** The App's snapshot, which joins the in-flight replies itself in both builds. */
function appSnapshot(mode = "branch"): AppSnapshot {
  const joined = ai() as unknown as AiSnapshot;
  const pending: ThreadMessage = { id: "", author: "ai", kind: "ai", timestamp: "", body_markdown: THINKING };
  joined.threads[0].replies = [{ ...pending, origin: "thread_reply" }];
  const response = { ...pending, author: "AI", origin: "finding_response" as const, editable: false, deletable: false };
  joined.findings[0].responses = [response];
  return { mode, ai: joined, tour: null, other_bucket: null, ui_annotations: [] } as unknown as AppSnapshot;
}

/** A store holding `values`, each under the key it answers. */
function holding(values: [ResourceKey, Resource][]): { get: (key: ResourceKey) => Versioned<Resource> | undefined } {
  const held = new Map(values.map(([key, value]) => [resourceKeyId(key), { key, version: 1, value }]));
  return { get: (key) => held.get(resourceKeyId(key)) };
}

const inFlight: Resource = {
  kind: "jobs",
  ai_replies: [
    { target: "q-1", ai_action: "ask", started_at_ms: 1 },
    { target: "finding:f-1", ai_action: null, started_at_ms: 1 },
  ],
  arena_runs: [],
  review_flows: [],
};
const annotations: Resource = { kind: "annotations", ai: ai(), tour: { available: false, fresh: false, scope: "branch", title: "", overviewMarkdown: "", pillars: [], createdAt: "", diffHash: "" }, other_bucket: null, view: { repo_root: "/repo", pr_number: null, branch: "", mode: "branch" } };

/** `diff(tab)` showing `mode`; the adapter reads nothing else of it. */
const diffIn = (mode: string): Resource => ({ kind: "diff", mode }) as unknown as Resource;

function adapter(flagOn: boolean, values: [ResourceKey, Resource][]): AnnotationsAdapter {
  return new AnnotationsAdapter({ flagOn, store: holding(values), activeKey: () => TAB });
}

function thinking(snapshot: AppSnapshot): [number, number] {
  const replies = snapshot.ai.threads[0].replies.filter((r) => r.body_markdown === THINKING);
  const responses = snapshot.ai.findings[0].responses.filter((r) => r.body_markdown === THINKING);
  return [replies.length, responses.length];
}

describe("the annotations adapter", () => {
  it("shows exactly one thinking reply per in-flight reply with the flag on and with it off", () => {
    for (const flagOn of [false, true]) {
      for (const fetched of [true, false]) {
        const values: [ResourceKey, Resource][] = [[JOBS, inFlight]];
        if (fetched) values.push([annotationsKey(TAB), annotations]);
        const shown = adapter(flagOn, values).overlay(appSnapshot());
        expect(thinking(shown), `flag ${flagOn ? "on" : "off"}, annotations ${fetched ? "held" : "not held"}`).toEqual([1, 1]);
      }
    }
  });

  it("draws the in-flight replies from jobs with the flag on, and none once jobs drops them", () => {
    const running = adapter(true, [[JOBS, inFlight], [annotationsKey(TAB), annotations]]);
    const shown = running.overlay(appSnapshot());
    expect(shown.ai.threads[0].replies.at(-1)?.ai_action).toBe("ask");
    expect(running.threads()?.[0].replies.map((r) => r.body_markdown)).toEqual([THINKING]);

    const answered = adapter(true, [[JOBS, { kind: "jobs", ai_replies: [], arena_runs: [], review_flows: [] }], [annotationsKey(TAB), annotations]]);
    expect(thinking(answered.overlay(appSnapshot()))).toEqual([0, 0]);
  });

  // With the flag on a switch to a view the workspace loads never reaches the
  // App, so its mode can say History while diff(tab) shows Branch.
  it("reads annotations(tab) while diff(tab) shows a view the workspace loads, whatever the App's mode", () => {
    const held = adapter(true, [[JOBS, inFlight], [annotationsKey(TAB), annotations], [diffKey(TAB), diffIn("branch")]]);
    const raw = appSnapshot("history");
    const shown = held.overlay(raw);
    expect(shown).not.toBe(raw);
    expect(shown.ai.threads[0].replies.at(-1)?.ai_action).toBe("ask");
    expect(held.threads()?.[0].replies.map((r) => r.body_markdown)).toEqual([THINKING]);
    expect(held.fetchedTab()).toBe(TAB);
  });

  // The workspace loads every view, History included, so a visit to History
  // and back reads annotations(tab) throughout. The App's mode stays at
  // History, as a switch with the flag on never reaches it.
  it("reads the AI cards, tour and threads from annotations(tab) in History, then Branch", () => {
    const values = new Map<string, Versioned<Resource>>();
    const put = (key: ResourceKey, value: Resource) => values.set(resourceKeyId(key), { key, version: 1, value });
    const shown = new AnnotationsAdapter({ flagOn: true, store: { get: (key) => values.get(resourceKeyId(key)) }, activeKey: () => TAB });
    const fetched = ai();
    fetched.summary_markdown = "fetched";
    const tour = (annotations as { tour: unknown }).tour;
    put(JOBS, inFlight);
    put(annotationsKey(TAB), { ...annotations, ai: fetched } as Resource);
    for (const view of ["history", "branch"]) {
      put(diffKey(TAB), diffIn(view));
      const card = shown.overlay(appSnapshot("history"));
      expect(card.ai.summary_markdown, view).toBe("fetched");
      expect(card.tour, view).toEqual(tour as never);
      expect(shown.threads()?.[0].replies.map((r) => r.body_markdown), view).toEqual([THINKING]);
      expect(shown.fetchedTab(), view).toBe(TAB);
    }
  });

  it("draws the pending writes on threads() and the overlay, a row the server holds once", () => {
    const held = ai().threads[0] as unknown as ThreadSnapshot;
    const painted = (id: string, body: string): ThreadSnapshot => ({ ...held, id, root: { ...held.root, id, body_markdown: body } });
    const paint: PendingPaint = {
      threads: [painted("q-1", "painted again"), painted("q-2", "pending question")],
      replies: [{ parentId: "q-1", reply: message("q-3", "pending reply") }],
      hidden: [],
    };
    const shown = new AnnotationsAdapter({
      flagOn: true,
      store: holding([[annotationsKey(TAB), annotations]]),
      activeKey: () => TAB,
      paints: () => [paint],
    });
    const drawn = (threads: readonly ThreadSnapshot[] | null | undefined) =>
      threads?.map((t) => [t.id, t.root.body_markdown, ...t.replies.map((r) => r.body_markdown)]);
    const expected = [["q-1", "why?", "pending reply"], ["q-2", "pending question"]];
    expect(drawn(shown.threads())).toEqual(expected);
    expect(drawn(shown.overlay(appSnapshot()).ai.threads)).toEqual(expected);
  });

  it("reads the browser annotations with their DOM context parsed back", () => {
    const row = {
      id: "ui-1", url: "/", selector: null, box_x: 0, box_y: 0, box_w: 1, box_h: 1, viewport_w: 10, viewport_h: 10,
      text: "here", timestamp: "", author: "You", screenshot_path: null, stale: false, element_context: null,
      dom_context_json: '{"tag":"button"}',
    };
    const values: [ResourceKey, Resource][] = [
      [annotationsKey(TAB), annotations],
      [uiAnnotationsKey(TAB), { kind: "ui_annotations", rows: [row] }],
    ];
    const shown = adapter(true, values).overlay(appSnapshot());
    expect(shown.ui_annotations?.[0].dom_context).toEqual({ tag: "button" });
  });

  it("leaves thread order to the server", () => {
    const source = readFileSync(join(here, "annotationsAdapter.ts"), "utf8");
    expect(source).not.toMatch(/\.sort\(|\.toSorted\(/);
  });
});
