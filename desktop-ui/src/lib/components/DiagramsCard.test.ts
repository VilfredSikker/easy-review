import { describe, expect, it } from "bun:test";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const src = readFileSync(
  join(dirname(fileURLToPath(import.meta.url)), "DiagramsCard.svelte"),
  "utf8",
);

describe("DiagramsCard", () => {
  it("sends the custom prompt under the camelCase key Tauri binds", () => {
    // Tauri maps `custom_prompt: Option<String>` to the JS key `customPrompt`.
    // A snake_case key binds to None, and every custom diagram is rejected as
    // missing its prompt.
    expect(src).toContain('app.cmd("generate_diagram", { kind, customPrompt: prompt ?? null })');
    expect(src).not.toContain("custom_prompt");
  });
});
