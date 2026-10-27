import { describe, expect, it } from "bun:test";
import type { ReviewModelGroups } from "$lib/types";
import {
  cloneReviewModelGroups,
  hasReviewModel,
  sameReviewModelGroups,
  toggleReviewModel,
} from "./reviewModelGroups";

const initial: ReviewModelGroups = {
  fast: [{ provider_id: "haiku", model_id: "haiku-4" }],
  standard: [{ provider_id: "sonnet", model_id: "sonnet-4" }],
  advanced: [{ provider_id: "opus", model_id: "opus-4" }],
};

describe("review model group drafts", () => {
  it("clones server values so edits cannot mutate the loaded config", () => {
    const draft = cloneReviewModelGroups(initial);
    draft.fast[0]!.model_id = "haiku-4.1";
    expect(initial.fast[0]?.model_id).toBe("haiku-4");
  });

  it("toggles an exact provider and model pair independently in each group", () => {
    const ref = { provider_id: "haiku", model_id: "haiku-4" };
    const draft = toggleReviewModel(initial, "fast", ref);
    expect(hasReviewModel(draft, "fast", ref)).toBe(false);
    expect(hasReviewModel(draft, "standard", ref)).toBe(false);
    expect(hasReviewModel(toggleReviewModel(draft, "advanced", ref), "advanced", ref)).toBe(true);
    expect(hasReviewModel(initial, "fast", { provider_id: "other", model_id: "haiku-4" })).toBe(false);
  });

  it("compares group contents and exact pair values", () => {
    expect(sameReviewModelGroups(initial, cloneReviewModelGroups(initial))).toBe(true);
    expect(
      sameReviewModelGroups(
        initial,
        toggleReviewModel(initial, "standard", { provider_id: "sonnet", model_id: "sonnet-4" }),
      ),
    ).toBe(false);
  });

  it("recognizes a reverted checkbox selection across multiple models", () => {
    const second = { provider_id: "codex", model_id: "luna" };
    const configured = toggleReviewModel(initial, "fast", second);
    const first = configured.fast[0]!;
    const unchecked = toggleReviewModel(configured, "fast", first);
    expect(sameReviewModelGroups(configured, unchecked)).toBe(false);
    const restored = toggleReviewModel(unchecked, "fast", first);
    expect(sameReviewModelGroups(configured, restored)).toBe(true);
  });
});
