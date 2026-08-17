import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const src = readFileSync(
  join(dirname(fileURLToPath(import.meta.url)), "app.svelte.ts"),
  "utf8",
);

describe("SLOW_COMMANDS", () => {
  it("does not treat local-first comment writes as slow overlay commands", () => {
    const block = src.slice(src.indexOf("const SLOW_COMMANDS"), src.indexOf("const VOID_COMMANDS"));
    for (const command of ["add_comment", "add_question", "add_note", "reply_to_thread"]) {
      expect(block.includes(`"${command}"`)).toBe(false);
    }
  });
});
