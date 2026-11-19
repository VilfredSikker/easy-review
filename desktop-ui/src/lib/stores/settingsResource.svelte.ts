import { invoke } from "@tauri-apps/api/core";
import { v1ServiceRequested } from "$lib/v1ServiceFlag";
import { app } from "./app.svelte";
import { resources } from "./resourceStore";
import { RiskAdapter } from "./riskAdapter";
import { SettingsAdapter } from "./settingsAdapter";

/** The Settings page's data and writes, from the old commands or the `settings` resource by build. */
export const settingsResource = new SettingsAdapter({
  flagOn: v1ServiceRequested(),
  store: resources,
  invoke,
  notify: (level, message) => app.showToast(level, message),
});

/** One repo's risk rules for Settings and the Risks tab, by build. */
export const riskResource = new RiskAdapter({ flagOn: v1ServiceRequested(), store: resources, invoke });
