import { invoke } from "@tauri-apps/api/core";
import { v1ServiceRequested } from "$lib/v1ServiceFlag";
import { app } from "./app.svelte";
import { resources } from "./resourceStore";
import { SettingsAdapter } from "./settingsAdapter";

/** The Settings page's data and writes, from the old commands or the `settings` resource by build. */
export const settingsResource = new SettingsAdapter({
  flagOn: v1ServiceRequested(),
  store: resources,
  invoke,
  notify: (level, message) => app.showToast(level, message),
});
