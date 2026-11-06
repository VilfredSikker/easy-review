import { tick } from "svelte";
import { reviewPage } from "$lib/stores/reviewPage.svelte";
import { rightRail } from "$lib/stores/rightRail.svelte";
import { rightPanelTab } from "$lib/stores/rightPanelTab.svelte";
import { overviewCollapse } from "$lib/stores/overviewCollapse.svelte";

/**
 * Overview sections other surfaces link into. Branch & PR, Triage & checklist
 * and Diagrams used to be right-panel tabs; their old entry points (the
 * collapsed rail, the palette, "Push to GitHub…") now land here.
 */
export type OverviewSection = "branch" | "triage" | "diagrams";

export function overviewSectionId(section: OverviewSection): string {
  return `ov-${section}`;
}

/** The fold each linked section sits in; triage folds from its own header. */
const FOLD: Partial<Record<OverviewSection, string>> = {
  branch: "aside-branch",
  diagrams: "diagrams",
};

/** Opens the Overview and brings `section` into view, expanding it if folded. */
export async function showOverviewSection(section: OverviewSection): Promise<void> {
  reviewPage.show("overview");
  if (reviewPage.current !== "overview") return;
  const fold = FOLD[section];
  if (fold) overviewCollapse.open(fold);
  await tick();
  const el = document.getElementById(overviewSectionId(section));
  if (!el) return;
  el.scrollIntoView({ block: "start", behavior: "smooth" });
}

/** Diagrams beside Tour and Diff open in the side panel, which keeps the code
 *  on screen; anywhere else they are on the Overview. */
export function showDiagrams(): void {
  const page = reviewPage.current;
  if (page === "tour" || page === "diff") {
    rightPanelTab.set("diagrams");
    rightRail.expand();
    return;
  }
  void showOverviewSection("diagrams");
}
