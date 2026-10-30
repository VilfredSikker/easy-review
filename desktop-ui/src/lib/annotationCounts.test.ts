import { describe, expect, test } from "bun:test";
import { annotationCounts } from "./annotationCounts";
import type { FlatFinding, ThreadSnapshot } from "./types";

const t = (kind: ThreadSnapshot["kind"]) => ({ kind }) as ThreadSnapshot;
const f = (resolved: boolean) => ({ resolved }) as FlatFinding;

describe("annotationCounts", () => {
  test("counts each kind", () => {
    const c = annotationCounts(
      [t("comment"), t("comment"), t("question"), t("note")],
      [f(false), f(false)],
      [f(true)],
    );
    expect(c).toEqual({ comments: 2, questions: 1, notes: 1, findingsUnresolved: 2, findingsResolved: 1 });
  });

  test("handles missing input", () => {
    expect(annotationCounts(null, undefined)).toEqual({
      comments: 0,
      questions: 0,
      notes: 0,
      findingsUnresolved: 0,
      findingsResolved: 0,
    });
  });
});
