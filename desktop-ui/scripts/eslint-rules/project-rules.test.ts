import { describe, expect, it } from "bun:test";
import { ESLint } from "eslint";

// Runs the real eslint.config.js, so a selector typo or a file override that
// switches a convention off shows up here rather than as silence in the gate.
const eslint = new ESLint({ cwd: new URL("../..", import.meta.url).pathname });

async function restricted(code: string, filePath = "src/lib/probe.ts"): Promise<string[]> {
  const [result] = await eslint.lintText(code, { filePath });
  return result.messages.filter((m) => m.ruleId === "no-restricted-syntax").map((m) => m.message);
}

describe("hard-coded hex classes", () => {
  it("flags hex in a string, a template and a Svelte class attribute", async () => {
    /* eslint-disable no-restricted-syntax -- these strings are the hex fixtures the rule is tested on */
    expect(await restricted(`export const c = "px-2 bg-[#1e1e1e]";`)).toHaveLength(1);
    // eslint-disable-next-line no-template-curly-in-string -- source code under test
    expect(await restricted("export const c = (x: string) => `hover:text-[#fff] ${x}`;")).toHaveLength(1);
    const svelte = `<div class="p-1 border-[#abcdef]"></div>\n`;
    /* eslint-enable no-restricted-syntax */
    expect(await restricted(svelte, "src/lib/components/Probe.svelte")).toHaveLength(1);
  });

  it("allows theme tokens and arbitrary values that are not hex", async () => {
    expect(await restricted(`export const c = "bg-[var(--arena-orange)] text-[12px] bg-ink-650";`)).toEqual([]);
    expect(await restricted(`export const c = "#1e1e1e";`)).toEqual([]);
  });
});

describe("snake_case Tauri command args", () => {
  it("flags a snake_case key passed to app.cmd or invoke", async () => {
    expect(await restricted(`app.cmd("generate_diagram", { kind, custom_prompt: null });`)).toHaveLength(1);
    expect(await restricted(`invoke("get_log", { task_id: 1 });`)).toHaveLength(1);
  });

  it("allows camelCase keys and snake_case command names", async () => {
    expect(await restricted(`app.cmd("generate_diagram", { kind, customPrompt: null });`)).toEqual([]);
    expect(await restricted(`invoke("list_available_branches");`)).toEqual([]);
    expect(await restricted(`other("x", { snake_key: 1 });`)).toEqual([]);
  });
});
