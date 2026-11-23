import { resources } from "./resourceStore";
import { RiskAdapter } from "./riskAdapter";
import { SettingsAdapter } from "./settingsAdapter";

/** The Settings page's data and writes, from the `settings` resource. */
export const settingsResource = new SettingsAdapter({ store: resources });

/** One repo's risk rules for Settings and the Risks tab. */
export const riskResource = new RiskAdapter({ store: resources });
