import { describe, expect, it } from "bun:test";
import {
  canonicalizeBrowserUrl,
  BLANK_BROWSER_URL,
  defaultDevUrl,
  fromProxyUrl,
  annotationMatchesPage,
  pageKey,
  sameBrowserUrl,
  toProxyUrl,
  urlPath,
  DEFAULT_DEV_URL,
} from "./browserUrl";

describe("defaultDevUrl", () => {
  it("falls back to the vite dev port when nothing is known", async () => {
    expect(await defaultDevUrl()).toBe(DEFAULT_DEV_URL);
    // No Tauri runtime in unit tests — `invoke` rejects, so we fall back.
    expect(await defaultDevUrl("/tmp/some-repo")).toBe(DEFAULT_DEV_URL);
  });

  it("returns a localhost URL", async () => {
    expect(await defaultDevUrl()).toMatch(/^http:\/\/localhost:\d+$/);
  });
});

describe("urlPath", () => {
  it("strips query and hash from a full URL", () => {
    expect(urlPath("http://localhost:5173/dashboard?x=1#frag")).toBe("/dashboard");
  });

  it("returns '/' for a bare origin", () => {
    expect(urlPath("http://localhost:5173")).toBe("/");
  });

  it("preserves a path-only input", () => {
    expect(urlPath("/foo/bar")).toBe("/foo/bar");
  });

  it("strips query from a path-only input", () => {
    expect(urlPath("/foo?bar=1")).toBe("/foo");
  });
});

describe("browser proxy URLs", () => {
  it("uses a true blank iframe URL when no browser URL is set", () => {
    expect(toProxyUrl("")).toBe(BLANK_BROWSER_URL);
    expect(toProxyUrl("   ")).toBe(BLANK_BROWSER_URL);
    expect(fromProxyUrl(BLANK_BROWSER_URL)).toBe(BLANK_BROWSER_URL);
  });

  it("round-trips https URLs through erps", () => {
    const proxied = toProxyUrl("https://google.com/search?q=x");
    expect(proxied).toBe("erps://google.com/search?q=x");
    expect(fromProxyUrl(proxied)).toBe("https://google.com/search?q=x");
  });

  it("round-trips http localhost URLs through erp", () => {
    const proxied = toProxyUrl("http://localhost:6006/iframe.html");
    expect(proxied).toBe("erp://localhost:6006/iframe.html");
    expect(fromProxyUrl(proxied)).toBe("http://localhost:6006/iframe.html");
  });

  it("leaves data story URLs alone", () => {
    const storyUrl = "data:text/html,<h1>Story</h1>";
    expect(toProxyUrl(storyUrl)).toBe(storyUrl);
    expect(fromProxyUrl(storyUrl)).toBe(storyUrl);
  });
});

describe("canonicalizeBrowserUrl / sameBrowserUrl", () => {
  it("treats bare origin and trailing-slash root as the same page", () => {
    expect(sameBrowserUrl("http://localhost:5173", "http://localhost:5173/")).toBe(true);
  });

  it("treats the erp:// echo of the same URL as equal", () => {
    expect(sameBrowserUrl("http://localhost:5173/", "erp://localhost:5173/")).toBe(true);
  });

  it("normalizes erps:// to https:// for comparison", () => {
    expect(sameBrowserUrl("https://app.example.com/x", "erps://app.example.com/x")).toBe(true);
  });

  it("keeps query significant but ignores hash for reload decisions", () => {
    expect(sameBrowserUrl("http://localhost:5173/", "http://localhost:5173/?x=1")).toBe(false);
    expect(sameBrowserUrl("http://localhost:5173/#a", "http://localhost:5173/#b")).toBe(true);
  });

  it("treats different paths as different pages", () => {
    expect(sameBrowserUrl("http://localhost:5173/", "http://localhost:5173/dashboard")).toBe(false);
  });

  it("canonicalizes host casing", () => {
    expect(canonicalizeBrowserUrl("HTTP://LocalHost:5173/")).toBe("http://localhost:5173/");
  });

  it("canonicalizes the blank page without pretending it is http", () => {
    expect(canonicalizeBrowserUrl(BLANK_BROWSER_URL)).toBe(BLANK_BROWSER_URL);
    expect(sameBrowserUrl(BLANK_BROWSER_URL, BLANK_BROWSER_URL)).toBe(true);
  });
});

describe("pageKey / annotationMatchesPage", () => {
  it("uses origin and path while ignoring query and hash", () => {
    expect(pageKey("http://LOCALHOST:5173/dashboard?x=1#frag")).toBe(
      "http://localhost:5173/dashboard",
    );
  });

  it("keeps identical paths on different origins separate", () => {
    expect(pageKey("http://localhost:5173/dashboard")).not.toBe(
      pageKey("https://app.example.com/dashboard"),
    );
  });

  it("matches legacy path-only annotations for the current page", () => {
    expect(annotationMatchesPage("/dashboard", "http://localhost:5173/dashboard?x=1")).toBe(true);
  });
});
