import type { Meta, StoryObj } from "@storybook/svelte";
import TourRail from "$lib/components/TourRail.svelte";
import TourRailHarness from "./TourRailHarness.svelte";
import { railPillars } from "$lib/tourModel";
import type { PillarSnapshot } from "$lib/types";
import {
  fileExperimentTemplate,
  fileMediaCombobox,
  filePageTest,
  fileVariantWarningCopy,
  fileVariantWarningTest,
} from "./fixtures";
import { migrationFile, tourPillarsFixture } from "./tourFixtures";

const noop = () => {};

const files = [
  migrationFile,
  fileVariantWarningCopy,
  fileVariantWarningTest,
  fileExperimentTemplate,
  filePageTest,
  fileMediaCombobox,
];

// The first pillar also owns the migration, so the rail shows a timestamped name.
const pillars: PillarSnapshot[] = tourPillarsFixture.map((p, i) =>
  i === 0 ? { ...p, files: [{ path: migrationFile.path, reason: "", findingIds: [] }, ...p.files], totalCount: 3 } : p,
);

const meta = {
  title: "Components/TourRail",
  component: TourRail,
  parameters: { layout: "fullscreen" },
} satisfies Meta<typeof TourRail>;

export default meta;
type Story = StoryObj<typeof meta>;

const base = {
  pillars: railPillars(pillars, files),
  activePillarId: "p-1",
  currentPath: fileVariantWarningCopy.path,
  onpillar: noop,
  onjump: noop,
  ontogglefile: noop,
  onreviewpillar: noop,
};

/** The first pillar open: summary, progress, Review all and its files, with a
 *  migration led by its descriptive name and a test nested under its source. */
export const FirstPillarOpen: Story = { args: base };

/** Scroll-spy moved on: the second pillar expands, the first folds to its count. */
export const SecondPillarOpen: Story = {
  args: { ...base, activePillarId: "p-2", currentPath: fileExperimentTemplate.path },
};

/** Every file in the pillar reviewed: the button reads Clear. */
export const PillarDone: Story = {
  args: {
    ...base,
    pillars: railPillars(
      pillars.map((p) => (p.id === "p-1" ? { ...p, reviewedCount: 3 } : p)),
      files.map((f) => ({ ...f, reviewed: true })),
    ),
  },
};

/** Mounted as in the Tour, inside a full-height flex row like `.tour-split`:
 *  the ruler takes its own strip beside the rail, vertically centred, with
 *  ticks grouped by pillar. Hover it for the list. */
export const WithRuler: Story = {
  args: { ...base, rulerPillars: pillars, rulerFiles: files },
  render: (args) => ({ Component: TourRailHarness, props: args }),
};
