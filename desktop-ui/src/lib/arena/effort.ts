/** Canonical labels used by catalog metadata and arena controls. */
export const EFFORT_LEVELS = ["low", "medium", "high", "xhigh", "max"] as const;

export type EffortLevel = (typeof EFFORT_LEVELS)[number];

/** Effort levels are supplied by the shared Rust catalog metadata. */
export function effortLevelsForModel(
  model: { effort_levels: string[] } | null | undefined,
): readonly string[] {
  return model?.effort_levels ?? [];
}

export function modelSupportsEffort(
  model: { effort_levels: string[] } | null | undefined,
): boolean {
  return effortLevelsForModel(model).length > 0;
}

export function effortLabel(level: string): string {
  if (level === "xhigh") return "XHigh";
  return level.charAt(0).toUpperCase() + level.slice(1);
}
