import { tick } from "svelte";
import { reviewPage } from "$lib/stores/reviewPage.svelte";

/**
 * Overview sections other surfaces link into. Branch & PR, Triage & checklist
 * and Diagrams used to be right-panel tabs; their old entry points (the
 * collapsed rail, the palette, "Push to GitHub…") now land here.
 */
export type OverviewSection = "branch" | "triage" | "diagrams";

export function overviewSectionId(section: OverviewSection): string {
  return `ov-${section}`;
}

/** Opens the Overview and brings `section` into view, expanding it if folded. */
export async function showOverviewSection(section: OverviewSection): Promise<void> {
  reviewPage.show("overview");
  if (reviewPage.current !== "overview") return;
  await tick();
  const el = document.getElementById(overviewSectionId(section));
  if (!el) return;
  if (el instanceof HTMLDetailsElement) el.open = true;
  el.scrollIntoView({ block: "start", behavior: "smooth" });
}
