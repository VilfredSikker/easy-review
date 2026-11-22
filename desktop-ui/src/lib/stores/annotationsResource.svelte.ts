import { v1ServiceRequested } from "$lib/v1ServiceFlag";
import { AnnotationsAdapter, JOBS, annotationsKey, uiAnnotationsKey } from "./annotationsAdapter";
import { app } from "./app.svelte";
import { resources } from "./resourceStore";
import { tabsResource } from "./tabsResource.svelte";

const flagOn = v1ServiceRequested();

/** The active tab's annotations, from the snapshot or `annotations(tab)`, `ui_annotations(tab)` and `jobs` by build. */
export const annotationsResource = new AnnotationsAdapter({
  flagOn,
  store: resources,
  activeKey: () => tabsResource.activeKey,
});

// With the flag on the page reads the AI fields from the resources: the
// active tab's annotations and browser annotations are shown, and so is
// `jobs`, whose in-flight replies join the threads and findings.
if (flagOn) {
  app.overlaySnapshot((raw) => annotationsResource.overlay(raw));
  $effect.root(() => {
    $effect(() => resources.show(JOBS));
    $effect(() => {
      const key = tabsResource.activeKey;
      if (key === null) return;
      const releases = [resources.show(annotationsKey(key)), resources.show(uiAnnotationsKey(key))];
      return () => {
        for (const release of releases) release();
      };
    });
  });
}
