import type { SettingsTab } from "$lib/types";

// The engine sends one flat list of titled sections for both front ends; the
// desktop alone splits it across tabs, so the split lives here and the TUI's
// config hub keeps its single list.
const SECTION_TABS: Record<string, SettingsTab> = {
  Agent: "ai",
  Commands: "review",
};

/** A section the map does not name lands on General, so a new one is never hidden. */
export function tabForSection(title: string | null): SettingsTab {
  return (title && SECTION_TABS[title]) || "general";
}
