import { profileLog } from "$lib/profileLog";
import { app } from "./app.svelte";
import { InboxAdapter } from "./inboxAdapter";
import { resources } from "./resourceStore";

/** The inbox, from the `inbox` resource. */
export const inboxResource = new InboxAdapter({
  store: resources,
  report: (command, e) => app.showToast("error", `${command}: ${e.message}`),
  profile: (command, ms) => profileLog("inbox_write_painted", { command, ms: Math.round(ms) }),
});
