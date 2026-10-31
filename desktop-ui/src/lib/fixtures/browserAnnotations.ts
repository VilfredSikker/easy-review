import type { BrowserAnnotationsSnapshot } from "../types";

export const expected = {
  annotations: [{
    id: "ba-0123456789abcdef", number: 1, idempotency_key: "draft-1",
    created_at: "2026-10-30T20:00:00Z", author: "reviewer", text: "Fix alignment",
    page: { origin: "http://localhost:5173", path: "/settings", query: null, hash_route: null },
    locator: null,
    target: { kind: "region", role: null, name: null, text_excerpt: null, limitation: null },
    captured: { rect: { x: 10, y: 20, width: 80, height: 24 }, viewport: { width: 1024, height: 768 }, scroll: { x: 0, y: 100 }, device_scale: 2, zoom: 1.25 },
    dom_context: null, resolution: { status: "missing", current_rect: null }, image: { status: "none" },
  }],
  load_error: null,
} satisfies BrowserAnnotationsSnapshot;
