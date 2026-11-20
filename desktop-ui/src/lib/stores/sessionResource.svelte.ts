import { invoke } from "@tauri-apps/api/core";
import { v1ServiceRequested } from "$lib/v1ServiceFlag";
import { app } from "./app.svelte";
import { resources } from "./resourceStore";
import { SessionAdapter } from "./sessionAdapter";

/** The app chrome and the AI picker, from the snapshot or the `settings` and `session` resources by build. */
export const sessionResource = new SessionAdapter({
  flagOn: v1ServiceRequested(),
  store: resources,
  invoke,
  snapshot: () => app.snapshot,
  cmd: (command, args) => app.cmd(command, args),
});
