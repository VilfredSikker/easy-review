import { describe, expect, it } from "bun:test";
import { resourceClient, type Invoke } from "$lib/protocol/client";
import type { Ack } from "$lib/protocol/generated/Ack";
import type { Prs } from "$lib/protocol/generated/Prs";
import type { PrRow } from "$lib/protocol/generated/PrRow";
import type { ResourceKey } from "$lib/protocol/generated/ResourceKey";
import type { AppSnapshot, TabSummary } from "$lib/types";
import { branchInPrStack, PrAdapter, prKey, prStatusKey, statusTarget, type HeldPrs } from "./prAdapter";
import { ResourceStore } from "./resourceStore";

const settle = () => new Promise((r) => setTimeout(r, 0));
const KEY = 3;

function row(number: number, headRef: string, baseRef = "main", state = "OPEN"): PrRow {
  const rest = { headOid: "", isDraft: false, author: "", assignees: [], reviewers: [], updatedAt: "", approvedByMe: false };
  return { number, title: `#${number}`, headRef, baseRef, state, checksState: null, reviewDecision: null, mergedAt: null, ...rest };
}

function list(remote: string, rows: PrRow[]): HeldPrs {
  return { remote, prs: { rows, fetchedAtMs: null, projects: [] } as Prs };
}

function tab(over: Partial<TabSummary>): TabSummary {
  return { idx: 0, label: "", kind: "working", branch: null, pr_number: null, remote: null, repo_root: "/repo", is_active: true, change_token: "", ...over };
}

const CARD = { number: 9, title: "Card", state: "OPEN", base: "main", head: "feat", url: "u", author: "me" };
const STACK = { trunk: "main", layers: [], position: null, size: 0, unavailable: null, retryable: false, loading: false };
const STATUS = { owner: "acme", repo: "widgets", number: 9, title: "From status" };

/** A store whose `er_fetch` answers what `values` holds for each key, and whose `er_send` records the command. */
function backend(values: Map<string, unknown>, ack: Ack = { changed: [], created: null }) {
  const sent: unknown[] = [];
  const run: Invoke = async <T,>(command: string, args: Record<string, unknown>): Promise<T> => {
    if (command === "er_fetch") {
      const keys = (args.keys ?? []) as ResourceKey[];
      return keys.map((key) => ({ key, version: 4, value: values.get(JSON.stringify(key)) })) as T;
    }
    if (command === "er_send") {
      sent.push(args.command);
      return ack as T;
    }
    throw Object.assign(new Error(`unexpected ${command}`), { kind: "unsupported" });
  };
  return { sent, store: new ResourceStore(resourceClient(run, async () => () => {}), 60_000) };
}

function adapter(store: ResourceStore, flagOn: boolean, held: HeldPrs[], active: TabSummary | null = tab({ kind: "working" })) {
  return new PrAdapter({
    flagOn,
    store,
    activeKey: () => KEY,
    activeTab: () => active,
    repoRemote: () => "acme/widgets",
    heldPrs: () => held,
  });
}

const RAW = {
  branch: "feat",
  local_branch: null,
  pr: null,
  stack: null,
  github: { number: 1, title: "App's status" },
} as unknown as AppSnapshot;

describe("the PR adapter", () => {
  it("leaves the snapshot's PR fields with the flag off", () => {
    const { store } = backend(new Map());
    expect(adapter(store, false, []).overlay(RAW)).toBe(RAW);
  });

  it("with the flag on, reads the card and stack from pr(tab) and the status the card's PR names", async () => {
    const held = [list("acme/widgets", [row(9, "feat", "base"), row(8, "base")])];
    const target = { remote: "acme/widgets", number: 9 };
    const values = new Map<string, unknown>([
      [JSON.stringify(prKey(KEY)), { kind: "pr", pr: CARD, stack: STACK }],
      [JSON.stringify(prStatusKey(target)), { kind: "pr_status", status: STATUS }],
    ]);
    const { store } = backend(values);
    const releases = [store.show(prKey(KEY)), store.show(prStatusKey(target))];
    await settle();
    const shown = adapter(store, true, held).overlay(RAW);
    expect(shown.pr).toEqual(CARD);
    expect(shown.stack).toEqual({ ...STACK, likely_stacked: true });
    expect(shown.github?.title).toBe("From status");
    for (const release of releases) release();
  });

  it("shows no status when the active tab names no PR, once pr(tab) is held", async () => {
    const values = new Map<string, unknown>([[JSON.stringify(prKey(KEY)), { kind: "pr", pr: null, stack: null }]]);
    const { store } = backend(values);
    const release = store.show(prKey(KEY));
    await settle();
    const shown = adapter(store, true, [list("acme/widgets", [])]).overlay(RAW);
    expect(shown.github).toBeNull();
    expect(shown.stack).toBeNull();
    release();
  });

  it("sends the two refreshes keyed to the active tab", async () => {
    const { store, sent } = backend(new Map());
    const pr = adapter(store, true, []);
    await pr.route("refresh_stack");
    await pr.route("refresh_github_status");
    expect(sent).toEqual([
      { kind: "refresh_stack", tab: KEY },
      { kind: "refresh_github_status", tab: KEY },
    ]);
    expect(pr.route("pull_github_comments")).toBeNull();
    expect(adapter(store, false, []).route("refresh_stack")).toBeNull();
  });
});

describe("statusTarget", () => {
  const held = [list("other/repo", [row(5, "feat")]), list("acme/widgets", [row(4, "feat", "main", "CLOSED"), row(9, "feat")])];

  it("names a remote PR directly", () => {
    expect(statusTarget(tab({ kind: "remote_pr", remote: "acme/widgets", pr_number: 2 }), "x", null, [])).toEqual({ remote: "acme/widgets", number: 2 });
  });

  it("keeps a local PR's own number and finds its remote", () => {
    expect(statusTarget(tab({ pr_number: 4 }), "feat", "acme/widgets", held)).toEqual({ remote: "acme/widgets", number: 4 });
  });

  it("takes a branch's open PR in the tab's own repo", () => {
    expect(statusTarget(tab({}), "feat", "acme/widgets", held)).toEqual({ remote: "acme/widgets", number: 9 });
    expect(statusTarget(tab({}), "none", "acme/widgets", held)).toBeNull();
  });
});

describe("branchInPrStack", () => {
  it("reads a stack from the open PRs, never from a trunk", () => {
    const held = [list("acme/widgets", [row(1, "a"), row(2, "b", "a"), row(3, "c")])];
    expect(branchInPrStack("a", null, held)).toBe(true);
    expect(branchInPrStack("b", null, held)).toBe(true);
    expect(branchInPrStack("c", null, held)).toBe(false);
    expect(branchInPrStack("main", null, held)).toBe(false);
  });
});
