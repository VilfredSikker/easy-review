/** Reasoning effort levels (mirrors er-engine `config::EFFORT_LEVELS`). */
export const EFFORT_LEVELS = ["low", "medium", "high", "xhigh", "max"] as const;

export type EffortLevel = (typeof EFFORT_LEVELS)[number];

const OPUS_46_SONNET = ["low", "medium", "high", "max"] as const;

/** Effort levels supported for an ai_hub model id (empty when effort does not apply). */
export function effortLevelsForModel(modelId: string): readonly string[] {
  if (
    modelId.startsWith("sonnet-5") ||
    modelId.includes("sonnet-5") ||
    modelId.startsWith("gpt-5.6-sol") ||
    modelId.startsWith("gpt-5.6-terra") ||
    modelId.startsWith("gpt-5.6-luna")
  ) {
    return EFFORT_LEVELS;
  }
  if (
    modelId.startsWith("opus-4.7") ||
    modelId.startsWith("opus-4.8") ||
    modelId.includes("opus-4-7") ||
    modelId.includes("opus-4-8")
  ) {
    return EFFORT_LEVELS;
  }
  if (
    modelId.startsWith("opus-4.6") ||
    modelId.startsWith("sonnet-4.6") ||
    modelId.includes("opus-4-6") ||
    modelId.includes("sonnet-4-6")
  ) {
    return OPUS_46_SONNET;
  }
  return [];
}

export function modelSupportsEffort(modelId: string): boolean {
  return effortLevelsForModel(modelId).length > 0;
}

export function effortLabel(level: string): string {
  if (level === "xhigh") return "XHigh";
  return level.charAt(0).toUpperCase() + level.slice(1);
}
