import { describe, expect, it } from "bun:test";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { resourceClient, type Invoke } from "$lib/protocol/client";
import type { Ack } from "$lib/protocol/generated/Ack";
import type { ResourceKey } from "$lib/protocol/generated/ResourceKey";
import type { TabReviewed } from "$lib/protocol/generated/TabReviewed";
import type { AppSnapshot } from "$lib/types";
import { isOptimisticCommand } from "../optimisticLocal";
import { ResourceStore } from "./resourceStore";
import { ReviewedAdapter, reviewedKey } from "./reviewedAdapter";

const settle = () => new Promise((r) => setTimeout(r, 0));
const KEY = 7;
const VIEW = { repo_root: "/repo", pr_number: null, branch: "", mode: "branch" };

function marks(paths: string[], count: number, mode = "branch"): TabReviewed {
  return { paths, reviewed_count: count, total_count: 2, view: { ...VIEW, mode } };
}

/** The App's snapshot: two unreviewed files and a count only the App holds. */
function snapshot(): AppSnapshot {
  const file = (path: string) => ({ path, reviewed: false });
  return { files: [file("src/a.rs"), file("src/b.rs")], reviewed_count: 5 } as unknown as AppSnapshot;
}

/**
 * A backend whose `er_fetch` answers `reviewed(tab)` at the version and
 * marks it holds, at once or, after `holdFetches`, when the test releases
 * it; and whose `er_send` waits until the test answers it.
 */
function backend(held: { version: number; value: TabReviewed }, { shown = true } = {}) {
  const sent: unknown[] = [];
  let answer: { ack?: Ack; error?: unknown; resolve?: (ack: Ack) => void; reject?: (e: unknown) => void } = {};
  let gate: Promise<void> | null = null;
  let open = () => {};
  const run: Invoke = async <T,>(command: string, args: Record<string, unknown>): Promise<T> => {
    if (command === "er_fetch") {
      if (gate) await gate;
      const keys = (args.keys ?? []) as ResourceKey[];
      return keys.map((key) => ({ key, version: held.version, value: { kind: "reviewed", ...held.value } })) as T;
    }
    if (command === "er_send") {
      sent.push(args.command);
      return new Promise<Ack>((resolve, reject) => {
        answer = { resolve, reject };
      }) as Promise<T>;
    }
    throw Object.assign(new Error(`unexpected ${command}`), { kind: "unsupported" });
  };
  const store = new ResourceStore(resourceClient(run, async () => () => {}), 60_000);
  if (shown) store.show(reviewedKey(KEY));
  return {
    sent,
    store,
    held,
    /** Holds every `er_fetch` from here until `releaseFetches`. */
    holdFetches() {
      gate = new Promise((resolve) => {
        open = resolve;
      });
    },
    releaseFetches() {
      gate = null;
      open();
    },
    /** Shows `reviewed(KEY)`, which fetches it, as the page does on a tab switch. */
    show() {
      store.show(reviewedKey(KEY));
    },
    /** Lands the write: the backend holds `value` at `version` and the `Ack` names it. */
    ack(version: number, value: TabReviewed) {
      Object.assign(held, { version, value });
      answer.resolve?.({ changed: [{ key: reviewedKey(KEY), version }], created: null });
    },
    refuse() {
      answer.reject?.({ kind: "refused", message: "The active view changed; the annotation was not written" });
    },
  };
}

function adapter(store: ResourceStore, flagOn = true) {
  return new ReviewedAdapter({ flagOn, store, activeKey: () => KEY });
}

const flags = (snap: AppSnapshot) => snap.files.map((f) => [f.path, f.reviewed]);

describe("ReviewedAdapter", () => {
  it("leaves the snapshot alone and sends nothing with the flag off", async () => {
    const { store, sent } = backend({ version: 3, value: marks(["src/a.rs"], 1) });
    await settle();
    const reviewed = adapter(store, false);
    const snap = snapshot();
    expect(reviewed.overlay(snap)).toBe(snap);
    expect(reviewed.route("mark_reviewed", { path: "src/a.rs" })).toBeNull();
    expect(sent).toEqual([]);
  });

  it("takes the marks and the count from reviewed(tab), and the App's count in a view the App loads", async () => {
    const { store, held } = backend({ version: 3, value: marks(["src/b.rs"], 1) });
    await settle();
    const shown = adapter(store).overlay(snapshot());
    expect(flags(shown)).toEqual([["src/a.rs", false], ["src/b.rs", true]]);
    expect(shown.reviewed_count).toBe(1);

    Object.assign(held, { version: 4, value: marks([], 0, "history") });
    store.applyChanges([{ key: reviewedKey(KEY), version: 4 }]);
    await settle();
    expect(adapter(store).overlay(snapshot()).reviewed_count).toBe(5);
  });

  it("shows the checkmark until the Ack's versions are held, then reads the marks", async () => {
    const backendState = backend({ version: 3, value: marks([], 0) });
    const { store, sent } = backendState;
    await settle();
    const reviewed = adapter(store);

    const write = reviewed.route("mark_reviewed", { path: "src/a.rs" });
    expect(sent).toEqual([{ kind: "mark_reviewed", tab: KEY, path: "src/a.rs", reviewed: true }]);
    expect(flags(reviewed.overlay(snapshot()))[0]).toEqual(["src/a.rs", true]);
    expect(reviewed.overlay(snapshot()).reviewed_count).toBe(0);

    // The Ack lands before the fetch of the version it names: the checkmark stays.
    backendState.holdFetches();
    let done = false;
    void write?.then(() => {
      done = true;
    });
    backendState.ack(4, marks(["src/a.rs"], 1));
    await settle();
    expect(done).toBe(false);
    expect(flags(reviewed.overlay(snapshot()))[0]).toEqual(["src/a.rs", true]);
    expect(reviewed.overlay(snapshot()).reviewed_count).toBe(0);

    backendState.releaseFetches();
    await write;
    expect(reviewed.overlay(snapshot()).reviewed_count).toBe(1);
    // The paint is gone: a later read without the mark shows none.
    Object.assign(backendState.held, { version: 5, value: marks([], 0) });
    store.applyChanges([{ key: reviewedKey(KEY), version: 5 }]);
    await settle();
    expect(flags(reviewed.overlay(snapshot()))[0]).toEqual(["src/a.rs", false]);
  });

  it("sends no single-file write before reviewed(tab) is held, so a click lands only on the tab the page paints", async () => {
    // Right after a tab switch: the page shows the new key, and its fetch has not answered.
    const backendState = backend({ version: 3, value: marks([], 0) }, { shown: false });
    backendState.holdFetches();
    backendState.show();
    await settle();
    const reviewed = adapter(backendState.store);
    expect(reviewed.held()).toBeNull();

    for (const [command, args] of [
      ["mark_reviewed", { path: "src/a.rs" }],
      ["unmark_reviewed", { path: "src/a.rs" }],
      ["toggle_reviewed", { path: "src/a.rs" }],
    ] as const) {
      const write = reviewed.route(command, args);
      expect(write).not.toBeNull();
      await expect(write ?? Promise.resolve()).rejects.toThrow("not loaded yet");
    }
    expect(backendState.sent).toEqual([]);
    expect(flags(reviewed.overlay(snapshot()))[0]).toEqual(["src/a.rs", false]);

    backendState.releaseFetches();
    await settle();
    const write = reviewed.route("mark_reviewed", { path: "src/a.rs" });
    expect(backendState.sent).toEqual([{ kind: "mark_reviewed", tab: KEY, path: "src/a.rs", reviewed: true }]);
    backendState.ack(4, marks(["src/a.rs"], 1));
    await write;
  });

  it("takes the checkmark back when the write is refused", async () => {
    const backendState = backend({ version: 3, value: marks([], 0) });
    await settle();
    const reviewed = adapter(backendState.store);

    const write = reviewed.route("mark_reviewed", { path: "src/a.rs" });
    backendState.refuse();

    await expect(write ?? Promise.resolve()).rejects.toThrow("The active view changed");
    await settle();
    expect(flags(reviewed.overlay(snapshot()))[0]).toEqual(["src/a.rs", false]);
  });

  it("sends a bulk review with the tab key and the view reviewed(tab) names", async () => {
    const backendState = backend({ version: 3, value: marks([], 0) });
    await settle();

    const write = adapter(backendState.store).route("unbulk_review_pillar", { pillarId: "p-1" });
    backendState.ack(3, marks([], 0));
    await write;

    expect(backendState.sent).toEqual([
      { kind: "bulk_review_pillar", tab: KEY, view: VIEW, pillarId: "p-1", reviewed: false },
    ]);
  });
});

describe("the bulk reviews with the flag on", () => {
  it("leave the optimistic path, so a bulk review paints nothing until reviewed(tab) moves", () => {
    for (const command of ["bulk_review_pillar", "unbulk_review_pillar"]) {
      expect(isOptimisticCommand(command, true)).toBe(false);
      expect(isOptimisticCommand(command, false)).toBe(true);
    }
  });
});

describe("the reviewed count with the flag on", () => {
  const here = dirname(fileURLToPath(import.meta.url));
  const code = (file: string) =>
    readFileSync(join(here, file), "utf8")
      .replace(/\/\*[\s\S]*?\*\//g, "")
      .replace(/\/\/.*$/gm, "");

  it("is the server's: no flag-on reviewed module counts marks", () => {
    for (const file of ["reviewedAdapter.ts", "reviewedResource.svelte.ts"]) {
      const source = code(file);
      expect(source).not.toMatch(/\.length\b|\.reduce\(|\+=|-=|\+\+|--/);
      expect(source).not.toMatch(/reviewed_count\s*[+-]/);
    }
  });
});
