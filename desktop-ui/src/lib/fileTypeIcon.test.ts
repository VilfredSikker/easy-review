import { describe, expect, it } from "bun:test";
import { fileTypeIcon } from "./fileTypeIcon";

describe("fileTypeIcon", () => {
  it("resolves common extensions", () => {
    expect(fileTypeIcon("src/index.ts").icon).toBe("typescript");
    expect(fileTypeIcon("app/page.svelte").icon).toBe("svelte");
    expect(fileTypeIcon("lib/parser.rs").icon).toBe("rust");
  });

  it("resolves special basenames", () => {
    expect(fileTypeIcon("Dockerfile").icon).toBe("docker");
    expect(fileTypeIcon("package.json").icon).toBe("json");
  });

  it("falls back to extension monogram for unknown types", () => {
    const spec = fileTypeIcon("foo/bar.xyz");
    expect(spec.label).toBe("XYZ");
    expect(spec.icon).toBe("generic");
  });
});
