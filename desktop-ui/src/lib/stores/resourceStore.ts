import { SvelteMap } from "svelte/reactivity";
import { resourceClient, type ResourceClient } from "$lib/protocol/client";
import type { Ack } from "$lib/protocol/generated/Ack";
import type { Command } from "$lib/protocol/generated/Command";
import type { ErError } from "$lib/protocol/generated/ErError";
import type { Resource } from "$lib/protocol/generated/Resource";
import type { ResourceChanged } from "$lib/protocol/generated/ResourceChanged";
import type { ResourceKey } from "$lib/protocol/generated/ResourceKey";
import type { Versioned } from "$lib/protocol/generated/Versioned";

/**
 * How often the store asks for the versions of what it shows when no push
 * came. A missed push is the backend's bug to fix; this only bounds how long
 * one can leave the page stale (ADR 0072).
 */
export const SAFETY_NET_MS = 30_000;

/** One string per key, so keys that are equal as values share an entry. */
export function resourceKeyId(key: ResourceKey): string {
  const entries = Object.entries(key).sort(([a], [b]) => a.localeCompare(b));
  return JSON.stringify(entries);
}

interface Shown {
  key: ResourceKey;
  holders: number;
}

/**
 * The resources the page shows, each at the version it was fetched.
 *
 * A key is fetched when it is first shown, and again only when a push, an
 * `Ack` or a version check names a version above the one held. The version
 * check runs once the push listener is attached, since a push sent before
 * that is lost, and then on the safety net.
 */
export class ResourceStore {
  private readonly values = new SvelteMap<string, Versioned<Resource>>();
  private readonly shown = new Map<string, Shown>();
  private readonly held = new Map<string, number>();
  private readonly inFlight = new Set<string>();
  /** The highest version announced for a key while its fetch was in flight. */
  private readonly announced = new Map<string, number>();
  private unlisten: (() => void) | null = null;
  private timer: ReturnType<typeof setTimeout> | null = null;
  private running = false;

  constructor(
    private readonly client: ResourceClient,
    private readonly safetyNetMs = SAFETY_NET_MS,
    private readonly report: (e: ErError) => void = (e) =>
      console.warn(`[resources] ${e.kind}: ${e.message}`),
  ) {}

  /** The key's content at the version held; undefined until the first fetch answers. */
  get(key: ResourceKey): Versioned<Resource> | undefined {
    return this.values.get(resourceKeyId(key));
  }

  /** Shows a key until the returned release is called; the first holder fetches it. */
  show(key: ResourceKey): () => void {
    const id = resourceKeyId(key);
    const entry = this.shown.get(id);
    if (entry) {
      entry.holders += 1;
    } else {
      this.shown.set(id, { key, holders: 1 });
      void this.refetch([key]);
    }
    let released = false;
    return () => {
      if (released) return;
      released = true;
      this.release(id);
    };
  }

  /** Attaches the push listener, checks the versions once, then arms the safety net. */
  async start(): Promise<void> {
    if (this.running) return;
    this.running = true;
    const unlisten = await this.client.onChanged((changed) => this.applyChanges(changed));
    if (!this.running) {
      unlisten();
      return;
    }
    this.unlisten = unlisten;
    await this.checkVersions();
    this.arm();
  }

  stop(): void {
    this.running = false;
    if (this.timer !== null) {
      clearTimeout(this.timer);
      this.timer = null;
    }
    this.unlisten?.();
    this.unlisten = null;
  }

  /** Sends a command and refetches what its `Ack` names, without waiting for the push. */
  async send(command: Command): Promise<Ack> {
    const ack = await this.client.send(command);
    this.applyChanges(ack.changed);
    return ack;
  }

  /** Refetches each shown key whose announced version is above the one held. */
  applyChanges(changed: readonly ResourceChanged[]): void {
    const due: ResourceKey[] = [];
    for (const { key, version } of changed) {
      const id = resourceKeyId(key);
      if (!this.shown.has(id)) continue;
      if (this.inFlight.has(id)) {
        this.announced.set(id, Math.max(version, this.announced.get(id) ?? version));
        continue;
      }
      const held = this.held.get(id);
      if (held === undefined || version > held) due.push(key);
    }
    void this.refetch(due);
  }

  private release(id: string): void {
    const entry = this.shown.get(id);
    if (!entry) return;
    entry.holders -= 1;
    if (entry.holders > 0) return;
    this.shown.delete(id);
    this.held.delete(id);
    this.announced.delete(id);
    this.values.delete(id);
  }

  private arm(): void {
    this.timer = setTimeout(() => {
      void this.checkVersions().then(() => {
        if (this.running) this.arm();
      });
    }, this.safetyNetMs);
  }

  private async checkVersions(): Promise<void> {
    const keys = [...this.shown.values()].map((entry) => entry.key);
    if (keys.length === 0) return;
    try {
      const versions = await this.client.versions(keys);
      this.applyChanges(keys.map((key, i) => ({ key, version: versions[i] })));
    } catch (e) {
      this.report(e as ErError);
    }
  }

  private async refetch(keys: readonly ResourceKey[]): Promise<void> {
    const asked = keys.filter((key) => !this.inFlight.has(resourceKeyId(key)));
    if (asked.length === 0) return;
    for (const key of asked) this.inFlight.add(resourceKeyId(key));
    try {
      for (const entry of await this.client.fetch([...asked])) this.accept(entry);
    } catch (e) {
      this.report(e as ErError);
    } finally {
      this.settle(asked);
    }
  }

  private accept(entry: Versioned<Resource>): void {
    const id = resourceKeyId(entry.key);
    if (!this.shown.has(id)) return;
    const held = this.held.get(id);
    // Replies can land out of order; an older one never replaces a newer one.
    if (held !== undefined && entry.version <= held) return;
    this.held.set(id, entry.version);
    this.values.set(id, entry);
  }

  /** Ends the fetches of `asked`, and fetches again any key announced past what came back. */
  private settle(asked: readonly ResourceKey[]): void {
    const again: ResourceChanged[] = [];
    for (const key of asked) {
      const id = resourceKeyId(key);
      this.inFlight.delete(id);
      const version = this.announced.get(id);
      this.announced.delete(id);
      if (version !== undefined) again.push({ key, version });
    }
    if (again.length > 0) this.applyChanges(again);
  }
}

/** The app's store, over the Tauri commands. Started by the poll loop with the flag on. */
export const resources = new ResourceStore(resourceClient());
