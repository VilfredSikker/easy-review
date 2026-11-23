import { resources, type ResourceStore } from "./resourceStore";

export type Panel = "left" | "tree" | "right";

interface PanelMirrorDeps {
  store: Pick<ResourceStore, "send">;
}

/**
 * Tells the backend a side panel was toggled. The page paints its panels
 * itself (`layoutPanels`, `rightRail`); the backend keeps a mirror, written
 * through `er_send`. Lives outside `app.svelte.ts`, which may not import the
 * protocol.
 */
export async function mirrorPanelToggle(panel: Panel, deps: PanelMirrorDeps = { store: resources }): Promise<void> {
  await deps.store.send({ kind: "toggle_panel", panel });
}
