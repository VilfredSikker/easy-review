import { describe, expect, it } from "bun:test";
import { resourceClient, type Invoke } from "$lib/protocol/client";
import type { Resource } from "$lib/protocol/generated/Resource";
import type { ResourceChanged } from "$lib/protocol/generated/ResourceChanged";
import type { ResourceKey } from "$lib/protocol/generated/ResourceKey";
import type { Versioned } from "$lib/protocol/generated/Versioned";
import { ResourceStore } from "./resourceStore";

const TABS: ResourceKey = { kind: "tabs" };

/**
 * A backend holding one `tabs` resource. It records every command invoked,
 * pushes only when a test says so, and attaches the listener when a test
 * lets it.
 */
function fakeBackend() {
  let version = 1;
  let tabs = [1];
  const calls: string[] = [];
  let handler: ((changed: ResourceChanged[]) => void) | null = null;
  let attach: () => void = () => {};
  const attached = new Promise<void>((resolve) => (attach = resolve));
  let ack: ResourceChanged[] = [];

  const run: Invoke = async <T,>(command: string, args: Record<string, unknown>): Promise<T> => {
    calls.push(command);
    const keys = (args.keys ?? []) as ResourceKey[];
    switch (command) {
      case "er_fetch":
        return keys.map((key) => ({
          key,
          version,
          value: { kind: "tabs", tabs: [...tabs], active: tabs[0] ?? null },
        })) as T;
      case "er_versions":
        return keys.map(() => version) as T;
      case "er_send":
        return { changed: ack, created: null } as T;
      default:
        throw Object.assign(new Error(`${command} is not a resource command`), { kind: "unsupported" });
    }
  };
  const client = resourceClient(run, async (_event, h) => {
    await attached;
    handler = (changed) => h({ payload: changed });
    return () => (handler = null);
  });
  return {
    client,
    calls,
    attach,
    /** The resource moves to a new version; the push is held back. */
    move(next: number[]) {
      version += 1;
      tabs = next;
      return version;
    },
    push(changed: ResourceChanged[]) {
      handler?.(changed);
    },
    ackWith(changed: ResourceChanged[]) {
      ack = changed;
    },
    get version() {
      return version;
    },
  };
}

const settle = () => new Promise((r) => setTimeout(r, 0));

function heldTabs(store: ResourceStore): Versioned<Resource> | undefined {
  return store.get(TABS);
}

describe("ResourceStore", () => {
  it("refetches a shown key only when a pushed version is above the one held", async () => {
    const backend = fakeBackend();
    const store = new ResourceStore(backend.client, 60_000);
    store.show(TABS);
    backend.attach();
    await store.start();
    await settle();
    const fetchesBefore = backend.calls.filter((c) => c === "er_fetch").length;

    backend.push([{ key: TABS, version: backend.version }]);
    backend.push([{ key: TABS, version: backend.version - 1 }]);
    await settle();
    expect(backend.calls.filter((c) => c === "er_fetch").length).toBe(fetchesBefore);

    const next = backend.move([1, 2]);
    backend.push([{ key: TABS, version: next }]);
    await settle();
    expect(backend.calls.filter((c) => c === "er_fetch").length).toBe(fetchesBefore + 1);
    expect(heldTabs(store)?.version).toBe(next);
    store.stop();
  });

  it("fetches nothing for a key no one shows", async () => {
    const backend = fakeBackend();
    const store = new ResourceStore(backend.client, 60_000);
    backend.attach();
    await store.start();
    backend.push([{ key: TABS, version: backend.move([3]) }]);
    await settle();
    expect(backend.calls).not.toContain("er_fetch");
    store.stop();
  });

  it("refreshes on the safety net through er_versions and er_fetch, never poll", async () => {
    const backend = fakeBackend();
    const store = new ResourceStore(backend.client, 5);
    store.show(TABS);
    backend.attach();
    await store.start();
    await settle();
    backend.calls.length = 0;

    const next = backend.move([1, 4]);
    await new Promise((r) => setTimeout(r, 30));
    store.stop();

    expect(backend.calls).toContain("er_versions");
    expect(backend.calls).toContain("er_fetch");
    expect(backend.calls).not.toContain("poll");
    expect(heldTabs(store)?.version).toBe(next);
  });

  it("catches a push sent before the listener attached with one version check", async () => {
    const backend = fakeBackend();
    // The safety net is far off, so only the attach-time check can catch it.
    const store = new ResourceStore(backend.client, 60_000);
    store.show(TABS);
    await settle();
    const started = store.start();
    const next = backend.move([7, 8]);
    backend.push([{ key: TABS, version: next }]);
    backend.attach();
    await started;
    await settle();

    expect(heldTabs(store)?.version).toBe(next);
    store.stop();
  });

  it("refetches what an Ack names before any push arrives", async () => {
    const backend = fakeBackend();
    const store = new ResourceStore(backend.client, 60_000);
    store.show(TABS);
    backend.attach();
    await store.start();
    await settle();

    const next = backend.move([1, 2, 3]);
    backend.ackWith([{ key: TABS, version: next }]);
    // No wire command exists yet; any command exercises the Ack path.
    await store.send(undefined as never);
    await settle();

    expect(heldTabs(store)?.version).toBe(next);
    const value = heldTabs(store)?.value;
    expect(value?.kind === "tabs" ? value.tabs : null).toEqual([1, 2, 3]);
    store.stop();
  });

  it("drops a key's content when its last holder releases it", async () => {
    const backend = fakeBackend();
    const store = new ResourceStore(backend.client, 60_000);
    const first = store.show(TABS);
    const second = store.show(TABS);
    await settle();
    first();
    expect(heldTabs(store)).toBeDefined();
    second();
    expect(heldTabs(store)).toBeUndefined();
  });
});
