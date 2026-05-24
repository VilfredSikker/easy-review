import { describe, expect, it } from "bun:test";
import { langForPath } from "./extToLang";

describe("langForPath", () => {
  it("maps .svelte to typescript (SFC diff fragments)", () => {
    expect(langForPath("src/routes/+page.svelte")).toBe("typescript");
    expect(langForPath("packages/app/Component.svelte")).toBe("typescript");
  });

  it("maps .vue and .astro to typescript", () => {
    expect(langForPath("App.vue")).toBe("typescript");
    expect(langForPath("pages/index.astro")).toBe("typescript");
  });

  it("keeps native highlighters for plain TS/JS", () => {
    expect(langForPath("foo.ts")).toBe("typescript");
    expect(langForPath("foo.tsx")).toBe("tsx");
    expect(langForPath("foo.js")).toBe("javascript");
  });
});
