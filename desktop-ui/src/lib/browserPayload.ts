// Parsers for messages the review browser's page script posts back. The
// payloads cross a process boundary untyped, so every field is checked here.

import type { UiDomContext } from "./types";

export type PageRect = { left: number; top: number; width: number; height: number };

export function stringField(data: Record<string, unknown>, key: string): string | null {
  const value = data[key];
  return typeof value === "string" ? value : null;
}

/** `data[key]` when it is a non-null object; the shape is trusted from the page script. */
export function objectField<T>(data: Record<string, unknown>, key: string): T | null {
  const value = data[key];
  return value && typeof value === "object" ? (value as T) : null;
}

/** An in-page composer submit: its box (24×24 at the origin when missing) and text. */
export function composerSubmission(data: Record<string, unknown>) {
  const box = data.box;
  const bbox: [number, number, number, number] = Array.isArray(box) && box.length >= 4
    ? [Number(box[0]) || 0, Number(box[1]) || 0, Number(box[2]) || 24, Number(box[3]) || 24]
    : [0, 0, 24, 24];
  return {
    bbox,
    selector: stringField(data, "selector"),
    text: stringField(data, "text") ?? "",
    elementContext: stringField(data, "element_context"),
    domContext: objectField<UiDomContext>(data, "dom_context"),
  };
}

/** The element under the pointer, or null when the page found none. */
export function hoverTarget(data: Record<string, unknown>) {
  const rect = objectField<PageRect>(data, "rect");
  if (!rect) return null;
  return {
    selector: stringField(data, "selector"),
    rect,
    element_context: stringField(data, "element_context"),
    dom_context: objectField<UiDomContext>(data, "dom_context"),
  };
}

/** Anchor updates from a reanchor pass, shaped for `update_ui_annotation_anchors`. */
export function reanchorUpdates(data: Record<string, unknown>) {
  const results = Array.isArray(data.results)
    ? (data.results as Array<{ id: string; fresh: boolean; new_box?: [number, number, number, number] }>)
    : [];
  return results.map((r) => ({
    id: r.id,
    fresh: !!r.fresh,
    new_box: r.new_box ?? null,
  }));
}

/** A click the page captured while annotate mode was on. */
export function iframeClick(data: Record<string, unknown>) {
  return {
    x: Number(data.x) || 0,
    y: Number(data.y) || 0,
    w: Number(data.w) || 0,
    h: Number(data.h) || 0,
    selector: stringField(data, "selector"),
    element_context: stringField(data, "element_context"),
    dom_context: objectField<UiDomContext>(data, "dom_context"),
  };
}
