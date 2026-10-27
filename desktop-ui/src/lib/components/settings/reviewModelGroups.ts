import type { ReviewModelGroups, ReviewModelRef } from "$lib/types";

export const REVIEW_MODEL_GROUPS = ["fast", "standard", "advanced"] as const;
export type ReviewModelGroup = (typeof REVIEW_MODEL_GROUPS)[number];

export function cloneReviewModelGroups(groups: ReviewModelGroups): ReviewModelGroups {
  return {
    fast: groups.fast.map(cloneRef),
    standard: groups.standard.map(cloneRef),
    advanced: groups.advanced.map(cloneRef),
  };
}

export function sameReviewModelGroups(a: ReviewModelGroups, b: ReviewModelGroups): boolean {
  return REVIEW_MODEL_GROUPS.every((group) => sameRefs(a[group], b[group]));
}

export function toggleReviewModel(
  groups: ReviewModelGroups,
  group: ReviewModelGroup,
  ref: ReviewModelRef,
): ReviewModelGroups {
  const current = groups[group];
  const exists = current.some((item) => sameRef(item, ref));
  return {
    ...groups,
    [group]: exists ? current.filter((item) => !sameRef(item, ref)) : [...current, cloneRef(ref)],
  };
}

export function hasReviewModel(groups: ReviewModelGroups, group: ReviewModelGroup, ref: ReviewModelRef) {
  return groups[group].some((item) => sameRef(item, ref));
}

export function reviewModelKey(ref: ReviewModelRef): string {
  return `${ref.provider_id}\u0000${ref.model_id}`;
}

function sameRefs(a: ReviewModelRef[], b: ReviewModelRef[]): boolean {
  const keys = new Set(b.map(reviewModelKey));
  return a.length === b.length
    && new Set(a.map(reviewModelKey)).size === keys.size
    && a.every((ref) => keys.has(reviewModelKey(ref)));
}

function sameRef(a: ReviewModelRef, b: ReviewModelRef): boolean {
  return a.provider_id === b.provider_id && a.model_id === b.model_id;
}

function cloneRef(ref: ReviewModelRef): ReviewModelRef {
  return { provider_id: ref.provider_id, model_id: ref.model_id };
}
