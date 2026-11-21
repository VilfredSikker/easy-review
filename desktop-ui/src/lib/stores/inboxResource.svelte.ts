import { invoke } from "@tauri-apps/api/core";
import { profileLog } from "$lib/profileLog";
import { v1ServiceRequested } from "$lib/v1ServiceFlag";
import { app } from "./app.svelte";
import { InboxAdapter } from "./inboxAdapter";
import { resources } from "./resourceStore";

/** The inbox, from the snapshot or the `inbox` resource by build. */
export const inboxResource = new InboxAdapter({
  flagOn: v1ServiceRequested(),
  store: resources,
  snapshot: () => app.snapshot,
  command: (name, args) => app.cmd(name, args),
  invoke,
  report: (command, e) => app.showToast("error", `${command}: ${e.message}`),
  profile: (command, ms) => profileLog("inbox_write_painted", { command, ms: Math.round(ms) }),
});
