import type { AppSnapshot, FileSnapshot, PillarSnapshot, RelatedFileRef, TourFileRef } from "$lib/types";
import {
  fileExperimentTemplate,
  fileMediaCombobox,
  filePageTest,
  fileVariantWarningCopy,
  fileVariantWarningTest,
  richSnapshot,
} from "./fixtures";

const ref = (path: string, related: RelatedFileRef[] = []): TourFileRef => ({
  path,
  reason: "",
  findingIds: [],
  related,
});

/** A three-pillar tour over `richSnapshot`'s files: a nested test, a pillar
 *  whose narrative names its files, and one with a markdown file link. */
export const tourPillarsFixture: PillarSnapshot[] = [
  {
    id: "p-1",
    title: "Variant warning copy",
    descriptionMarkdown:
      "The warning text moves into `variant-warning-copy.ts`, with its test beside it. Check the plural forms: the old copy read **1 variants**.",
    summaryMarkdown: "Warning copy moves into one module.",
    importance: 90,
    foundation: true,
    files: [ref(fileVariantWarningCopy.path, [{ path: fileVariantWarningTest.path, kind: "test", reason: "" }])],
    reviewedCount: 0,
    totalCount: 2,
  },
  {
    id: "p-2",
    title: "Template resolution",
    descriptionMarkdown:
      "Templates now resolve before the page renders; see [the resolver](packages/discovery-platform/src/lib/experiment-template-resolution.ts) and the page test.",
    summaryMarkdown: "Template lookup moves ahead of render.",
    importance: 60,
    foundation: false,
    files: [ref(fileExperimentTemplate.path), ref(filePageTest.path)],
    reviewedCount: 1,
    totalCount: 2,
  },
  {
    id: "p-3",
    title: "Media combobox",
    descriptionMarkdown: "A new combobox for picking media in experiments.",
    importance: 40,
    foundation: false,
    files: [ref(fileMediaCombobox.path)],
    reviewedCount: 0,
    totalCount: 1,
  },
];

export function tourSnapshot(fresh = true): AppSnapshot {
  const snap: AppSnapshot = JSON.parse(JSON.stringify(richSnapshot));
  snap.mode = "tour";
  snap.tour = {
    available: true,
    fresh,
    scope: "pr",
    title: "Variant warnings",
    overviewMarkdown: "",
    pillars: tourPillarsFixture,
  };
  return snap;
}

/** A timestamped migration, to show the rail leading with its descriptive name. */
export const migrationFile: FileSnapshot = {
  ...fileExperimentTemplate,
  path: "supabase/migrations/20261029093502_add_variant_warning.sql",
  additions: 22,
  deletions: 0,
};
