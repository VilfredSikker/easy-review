import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { Ack } from "./generated/Ack";
import type { Command } from "./generated/Command";
import type { ErError } from "./generated/ErError";
import type { ErErrorKind } from "./generated/ErErrorKind";
import type { Resource } from "./generated/Resource";
import type { ResourceChanged } from "./generated/ResourceChanged";
import type { ResourceKey } from "./generated/ResourceKey";
import type { ResourceVersion } from "./generated/ResourceVersion";
import type { Versioned } from "./generated/Versioned";

/** The event the backend emits with the resources whose version moved. */
export const RESOURCES_EVENT = "er://resources";

/** Tauri's `invoke`, narrowed to what the client calls. */
export type Invoke = <T>(command: string, args: Record<string, unknown>) => Promise<T>;

/** Tauri's `listen`, narrowed to the one event the client hears. */
export type ListenResources = (
  event: string,
  handler: (event: { payload: ResourceChanged[] }) => void,
) => Promise<() => void>;

/**
 * The three resource commands and the push, typed by hand over the generated
 * wire types (ADR 0072). Every call rejects with an `ErError`.
 */
export interface ResourceClient {
  /** Applies a command; the `Ack` names the versions it moved. */
  send(command: Command): Promise<Ack>;
  /** Each key's content at its published version, in the order asked. */
  fetch(keys: ResourceKey[]): Promise<Versioned<Resource>[]>;
  /** Each key's published version, in the order asked. */
  versions(keys: ResourceKey[]): Promise<ResourceVersion[]>;
  /** Calls `handler` with each pushed list; resolves once the listener is attached. */
  onChanged(handler: (changed: ResourceChanged[]) => void): Promise<() => void>;
}

const ERROR_KINDS: readonly ErErrorKind[] = [
  "not_found",
  "moved",
  "stale",
  "refused",
  "timeout",
  "disconnected",
  "github",
  "invalid",
  "unsupported",
  "internal",
];

/**
 * The backend rejects with an `ErError`; anything else (a missing Tauri
 * runtime, a command the ACL refused) becomes `internal` with its text.
 */
export function asErError(e: unknown): ErError {
  if (typeof e === "object" && e !== null) {
    const { kind, message } = e as Partial<ErError>;
    if (typeof kind === "string" && ERROR_KINDS.includes(kind) && typeof message === "string") {
      return { kind, message };
    }
  }
  return { kind: "internal", message: e instanceof Error ? e.message : String(e) };
}

async function call<T>(run: Invoke, command: string, args: Record<string, unknown>): Promise<T> {
  try {
    return await run<T>(command, args);
  } catch (e) {
    throw asErError(e);
  }
}

export function resourceClient(
  run: Invoke = invoke,
  listenTo: ListenResources = listen<ResourceChanged[]>,
): ResourceClient {
  return {
    send: (command) => call<Ack>(run, "er_send", { command }),
    fetch: (keys) => call<Versioned<Resource>[]>(run, "er_fetch", { keys }),
    versions: (keys) => call<ResourceVersion[]>(run, "er_versions", { keys }),
    onChanged: (handler) => listenTo(RESOURCES_EVENT, (event) => handler(event.payload)),
  };
}
