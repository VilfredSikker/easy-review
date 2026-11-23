import { AiAsks } from "./aiAsks";
import { AnnotationsAdapter, JOBS, annotationsKey, uiAnnotationsKey } from "./annotationsAdapter";
import { AnnotationWrites } from "./annotationWrites";
import { app } from "./app.svelte";
import { pageView } from "./pageView.svelte";
import { resources } from "./resourceStore";
import { tabsResource } from "./tabsResource.svelte";

/** The annotation writes, through `er_send`. */
export const annotationWrites = new AnnotationWrites({
  store: resources,
  activeKey: () => tabsResource.activeKey,
  shownAi: () => pageView.current?.ai ?? null,
});

/** The card asks, through `er_send`. */
export const aiAsks = new AiAsks({ store: resources, activeKey: () => tabsResource.activeKey });

/** The active tab's annotations, from `annotations(tab)`, `ui_annotations(tab)` and `jobs`. */
export const annotationsResource = new AnnotationsAdapter({
  store: resources,
  activeKey: () => tabsResource.activeKey,
  paints: () => annotationWrites.paints(),
});

// The page reads the AI fields from the resources: the active tab's
// annotations and browser annotations are shown, and so is `jobs`, whose
// in-flight replies join the threads and findings.
pageView.overlay((raw) => annotationsResource.overlay(raw));
app.routeCmds((command, args) => annotationWrites.route(command, args));
app.routeCmds((command, args) => aiAsks.route(command, args));
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
