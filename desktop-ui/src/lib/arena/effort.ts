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

export type EffortChoice = {
  id: string;
  label: string;
  selected: boolean;
};

/** Effort rows shown after picking a model that advertises thinking levels. */
export function effortChoicesForModel(
  model: { effort_levels: string[]; is_selected?: boolean } | null | undefined,
  currentEffort: string | null | undefined,
): EffortChoice[] {
  const selected = Boolean(model?.is_selected);
  return effortLevelsForModel(model).map((id) => ({
    id,
    label: effortLabel(id),
    selected: selected && currentEffort === id,
  }));
}

export function selectedModelDescription(
  model: { is_selected: boolean; effort_levels: string[] },
  currentEffort: string | null | undefined,
): string {
  if (!model.is_selected) return "";
  if (modelSupportsEffort(model) && currentEffort) {
    return `currently selected · ${effortLabel(currentEffort)}`;
  }
  return "currently selected";
}
