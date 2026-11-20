import { invoke } from "@tauri-apps/api/core";
import type { Invoke } from "$lib/protocol/client";
import { v1ServiceRequested } from "$lib/v1ServiceFlag";
import { resources, type ResourceStore } from "./resourceStore";

export type Panel = "left" | "tree" | "right";

interface PanelMirrorDeps {
  flagOn: boolean;
  store: Pick<ResourceStore, "send">;
  invoke: Invoke;
}

/**
 * Tells the backend a side panel was toggled. The page paints its panels
 * itself (`layoutPanels`, `rightRail`); the backend keeps a mirror, through
 * the old command with the flag off and `er_send` with it on. Lives outside
 * `app.svelte.ts`, which may not import the protocol.
 */
export async function mirrorPanelToggle(
  panel: Panel,
  deps: PanelMirrorDeps = { flagOn: v1ServiceRequested(), store: resources, invoke },
): Promise<void> {
  if (deps.flagOn) {
    await deps.store.send({ kind: "toggle_panel", panel });
    return;
  }
  await deps.invoke("toggle_panel", { panel });
}
