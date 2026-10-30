import { describe, expect, it } from "bun:test";
import type { ArenaFinding, ArenaRunSnapshot, Ballot, Vote } from "$lib/types/arena";
import { MOCK_ARENA_SNAPSHOT } from "./mockRun";
import {
  aliveInRound,
  ballotTrail,
  digestLine,
  endedRunNotice,
  findingLabels,
  firstSentence,
  funnelSteps,
  importableFindings,
  importedCount,
  overlayAccess,
  agentPickKey,
  lensColorVar,
  modelPickKey,
  outAfterRound,
  picksToReviewers,
  reviewerToPickKey,
  pageStats,
  reviewerColorVar,
  reviewerProgress,
  roundNumbers,
  roundsRun,
  runDigest,
  runDuration,
  runOrdinals,
  triageRecommends,
  votesAtRound,
} from "./arenaView";

const b = (reviewer: string, vote: Vote): Ballot => ({ reviewer, vote });

function finding(id: string, verdict: ArenaFinding["verdict"], rounds: Ballot[][], extra: Partial<ArenaFinding> = {}): ArenaFinding {
  return {
    id,
    file: "a.ts",
    title: id,
    body: "",
    severity_by_round: { 1: "low" },
    raised_by: ["a"],
    verdict,
    confidence: 0.5,
    rationale: "",
    rounds: rounds.map((log, i) => ({ n: i + 1, log })),
    ...extra,
  };
}

function snapshotOf(findings: ArenaFinding[], rounds = 3): ArenaRunSnapshot {
  return {
    ...MOCK_ARENA_SNAPSHOT,
    run: { ...MOCK_ARENA_SNAPSHOT.run, findings, config: { ...MOCK_ARENA_SNAPSHOT.run.config, rounds }, accepted_finding_ids: [] },
  };
}

const ROUNDS = [1, 2, 3];

describe("votes by round", () => {
  const f = finding("f", "kept", [[b("a", "propose")], [b("a", "keep"), b("b", "drop")], [b("b", "keep")]]);

  it("reads the vote cast in the chosen round, not the latest one", () => {
    expect(votesAtRound(f, 2)).toEqual({ a: "keep", b: "drop" });
    expect(votesAtRound(f, 3)).toEqual({ b: "keep" });
  });

  it("leaves a gap where a reviewer did not vote", () => {
    expect(ballotTrail(f, "b", ROUNDS).map((x) => x?.vote)).toEqual([undefined, "drop", "keep"]);
  });

  it("counts reviewer rounds and ignores the arbiter round", () => {
    const withArbiter = { ...f, rounds: [...f.rounds, { n: 255, log: [b("arb", "keep")] }] };
    expect(roundsRun([withArbiter])).toBe(3);
  });
});

describe("outAfterRound", () => {
  it("marks a dropped finding out at the first round a majority dropped it", () => {
    const f = finding("f", "dropped", [
      [b("a", "propose")],
      [b("a", "keep"), b("b", "drop"), b("c", "drop"), b("d", "abstain")],
      [b("a", "drop")],
    ]);
    expect(outAfterRound(f, ROUNDS)).toBe(2);
    expect(aliveInRound(f, 2, ROUNDS)).toBe(true);
    expect(aliveInRound(f, 3, ROUNDS)).toBe(false);
  });

  it("does not count abstentions toward the majority", () => {
    const f = finding("f", "dropped", [[b("a", "drop"), b("b", "abstain"), b("c", "abstain")]]);
    expect(outAfterRound(f, ROUNDS)).toBe(1);
  });

  it("keeps a finding the arbiter kept in every column", () => {
    const f = finding("f", "kept", [[b("a", "drop"), b("b", "drop")]]);
    expect(outAfterRound(f, ROUNDS)).toBeNull();
  });
});

describe("stats and import", () => {
  const findings = [
    finding("k", "kept", []),
    finding("e", "escalated", []),
    finding("m", { merged: { into: "k" } }, []),
    finding("d", "dropped", []),
    finding("a", "kept", [], { accepted_at: "now" }),
  ];

  it("counts proposed findings rather than producers", () => {
    expect(pageStats(findings)).toEqual({ proposed: 5, kept: 2, escalated: 1, merged: 1, dropped: 1, pending: 0 });
  });

  it("imports unaccepted kept and escalated findings minus the ones left out", () => {
    const run = { findings, accepted_finding_ids: ["e"] };
    expect(importableFindings(run).map((f) => f.id)).toEqual(["k"]);
    expect(importableFindings({ findings, accepted_finding_ids: [] }, new Set(["k"])).map((f) => f.id)).toEqual(["e"]);
    expect(importedCount(run)).toBe(2);
  });
});

describe("funnelSteps", () => {
  it("walks proposed → each round → arbiter with drop captions", () => {
    const findings = [
      finding("x", "kept", [[b("a", "propose")], [b("a", "keep")]], {}),
      finding("y", "dropped", [[b("a", "propose")], [b("a", "drop")]]),
    ];
    findings[0].rounds.push({ n: 255, log: [b("arb", "keep")] });
    const steps = funnelSteps(snapshotOf(findings, 2));
    expect(steps.map((s) => [s.label, s.count])).toEqual([
      ["Proposed", 2],
      ["After round 1", 2],
      ["After round 2", 1],
      ["After arbiter", 1],
    ]);
    expect(steps[2].caption).toBe("F2 dropped by majority");
    expect(steps[3].caption).toBe("1 dropped");
  });

  it("stops at the last round that ran", () => {
    const steps = funnelSteps(snapshotOf([finding("x", "pending", [[b("a", "propose")]])], 3));
    expect(steps.map((s) => s.label)).toEqual(["Proposed", "After round 1"]);
  });
});

describe("run naming", () => {
  it("numbers runs by creation order", () => {
    const m = runOrdinals([
      { id: "b", created_at: "2026-01-02T00:00:00Z" },
      { id: "a", created_at: "2026-01-01T00:00:00Z" },
    ]);
    expect(m.get("a")).toBe(1);
    expect(m.get("b")).toBe(2);
  });

  it("summarises a run as rounds and survivors", () => {
    const snap = snapshotOf([
      finding("x", "kept", [[b("a", "propose")], [b("a", "keep")]]),
      finding("y", "dropped", [[b("a", "propose")]]),
    ]);
    const d = runDigest(snap);
    expect(d).toEqual({ rounds: 2, reviewers: snap.run.reviewers.length, survived: 1, total: 2 });
    expect(digestLine(2, { rounds: 3, reviewers: 4, survived: 4, total: 9 })).toBe(
      "Run #2 · 4 reviewers · 3 rounds · 4 of 9 findings survived",
    );
  });

  it("labels findings F1… in run order", () => {
    expect([...findingLabels([{ id: "p" }, { id: "q" }]).values()]).toEqual(["F1", "F2"]);
  });

  it("offers at least the configured rounds", () => {
    expect(roundNumbers(snapshotOf([], 3))).toEqual([1, 2, 3]);
  });
});

describe("reviewerProgress", () => {
  it("fills a round's share at a time", () => {
    expect(reviewerProgress("queued", 1, 3, false)).toBe(0);
    expect(reviewerProgress("running", 1, 4, false)).toBe(13);
    expect(reviewerProgress("done", 2, 4, false)).toBe(50);
    expect(reviewerProgress("done", 3, 3, false)).toBe(100);
  });

  it("is complete while the arbiter rules", () => {
    expect(reviewerProgress("queued", 2, 3, true)).toBe(100);
  });
});

describe("formatting", () => {
  it("formats durations", () => {
    expect(runDuration("2026-01-01T00:00:00Z", "2026-01-01T00:02:18Z")).toBe("2m 18s");
    expect(runDuration("2026-01-01T00:00:00Z", "2026-01-01T00:00:45Z")).toBe("45s");
    expect(runDuration("2026-01-01T00:00:00Z", undefined)).toBeNull();
  });

  it("takes the ruling's first sentence", () => {
    expect(firstSentence("Keep · med · confirmed. Reproduced against a DB.")).toBe("Keep · med · confirmed.");
    expect(firstSentence("No period here")).toBe("No period here");
  });

  it("colours lenses by token and models by position", () => {
    expect(lensColorVar("expert:security", 0)).toBe("var(--color-error)");
    expect(lensColorVar("security", 0)).toBe("var(--color-error)");
    expect(lensColorVar(undefined, 1)).toMatch(/^var\(--color-/);
    expect(reviewerColorVar({ id: "general" }, 3)).toBe("var(--color-accent)");
    expect(lensColorVar("unknown", 0)).not.toContain("#");
  });

  it("turns launcher picks into one mixed reviewer list", () => {
    const agentModel = { provider_id: "claude", model_id: "sonnet" };
    const keys = [modelPickKey("codex", "gpt"), agentPickKey("expert:security")];
    expect(picksToReviewers(keys, agentModel)).toEqual([
      { provider_id: "codex", model_id: "gpt" },
      { provider_id: "claude", model_id: "sonnet", agent_kind: "expert:security" },
    ]);
    expect(picksToReviewers(keys, null)).toHaveLength(1);
    expect(reviewerToPickKey({ provider_id: "x", model_id: "y", agent_kind: "general" })).toBe("a:general");
  });

  it("matches Triage's expert names to lens kinds", () => {
    expect(triageRecommends("expert:security", ["security"])).toBe(true);
    expect(triageRecommends("general", ["general"])).toBe(true);
    expect(triageRecommends("expert:testing", ["security"])).toBe(false);
  });

  it("explains a cancelled run", () => {
    const snap = snapshotOf([finding("x", "pending", [[b("a", "propose")]])]);
    snap.run.status = "cancelled";
    expect(endedRunNotice(snap, 1)?.title).toBe("Run #1 was cancelled after round 1");
    snap.run.status = "complete";
    expect(endedRunNotice(snap, 1)).toBeNull();
  });
});

describe("overlayAccess", () => {
  it("offers Keep anyway on an ordinary arena run", () => {
    const a = overlayAccess({ flowAcceptance: false, runKind: "models", readOnlyVerification: false });
    expect(a).toEqual({ flowManaged: false, readOnlyFlow: false, readOnly: false, keepAnyway: true });
  });

  it("hides Keep anyway during flow acceptance, which stays editable for import", () => {
    const a = overlayAccess({ flowAcceptance: true, runKind: "review_flow", readOnlyVerification: false });
    expect(a.readOnly).toBe(false);
    expect(a.keepAnyway).toBe(false);
  });

  it("treats a flow run outside acceptance as read only", () => {
    const a = overlayAccess({ flowAcceptance: false, runKind: "review_flow", readOnlyVerification: false });
    expect(a.readOnlyFlow).toBe(true);
    expect(a.keepAnyway).toBe(false);
  });

  it("hides Keep anyway on a read-only verification run", () => {
    const a = overlayAccess({ flowAcceptance: false, runKind: "seeded", readOnlyVerification: true });
    expect(a.readOnly).toBe(true);
    expect(a.keepAnyway).toBe(false);
  });
});
