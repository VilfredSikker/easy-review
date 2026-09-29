import { describe, expect, it } from "bun:test";
import { Linter } from "eslint";
import svelteParser from "svelte-eslint-parser";
import tseslint from "typescript-eslint";
import classCohesion from "./class-cohesion.js";
import moduleCohesion from "./module-cohesion.js";

const linter = new Linter({ configType: "flat" });
const plugin = { rules: { "module-cohesion": moduleCohesion, "class-cohesion": classCohesion } };

function lint(code: string, filename = "m.ts", options = { maxGroups: 1, minGroupLines: 3 }) {
  const svelte = filename.endsWith(".svelte");
  return linter
    .verify(
      code,
      [
        {
          files: ["**/*.ts", "**/*.svelte"],
          plugins: { t: plugin },
          languageOptions: svelte
            ? { parser: svelteParser, parserOptions: { parser: tseslint.parser } }
            : { parser: tseslint.parser },
          rules: { "t/module-cohesion": ["warn", options], "t/class-cohesion": ["warn", options] },
        },
      ],
      filename,
    )
    .map((m) => m.message);
}

/** A function body `lines` long, so groups clear the size floor. */
const body = (lines: number) =>
  Array.from({ length: lines }, (_, i) => `\tconst v${i} = ${i};`).join("\n");

describe("er/module-cohesion", () => {
  it("passes a module whose declarations all connect", () => {
    const code = `
function parse(s: string) {\n${body(3)}\n\treturn s;\n}
function validate(s: string) {\n${body(3)}\n\treturn parse(s);\n}
export function load(s: string) {\n${body(3)}\n\treturn validate(s);\n}`;
    expect(lint(code)).toEqual([]);
  });

  it("flags unrelated groups and names them", () => {
    const code = `
export function formatDate(d: Date) {\n${body(3)}\n\treturn d;\n}
export function sendEmail(to: string) {\n${body(3)}\n\treturn to;\n}`;
    const [message] = lint(code);
    expect(message).toContain("2 unrelated groups");
    expect(message).toContain("[formatDate]");
    expect(message).toContain("[sendEmail]");
  });

  it("does not count imports or types as links", () => {
    const code = `
import { z } from 'zod';
type Shared = { id: string };
export function a(x: Shared) {\n${body(3)}\n\treturn z.string().parse(x.id);\n}
export function b(x: Shared) {\n${body(3)}\n\treturn z.string().parse(x.id);\n}`;
    expect(lint(code)[0]).toContain("2 unrelated groups");
  });

  it("ignores groups below the size floor", () => {
    const code = `export const a = 1;\nexport const b = 2;\nexport const c = 3;`;
    expect(lint(code)).toEqual([]);
  });

  it("reads only the script of a Svelte component, not the markup", () => {
    const code = `<script lang="ts">
	let draft = $state('');
	function save() {\n${body(3)}\n\t\treturn draft;\n\t}
	let zoom = $state(1);
	function zoomIn() {\n${body(3)}\n\t\tzoom += 1;\n\t}
</script>

<button type="button" onclick={save}>{draft}</button>
<button type="button" onclick={zoomIn}>{zoom}</button>`;
    const [message] = lint(code, "C.svelte");
    expect(message).toContain("component's script holds 2 unrelated groups");
    expect(message).toContain("subcomponent");
  });
});

describe("er/class-cohesion", () => {
  it("passes a class whose members use each other", () => {
    const code = `
export class Cart {
	#items: string[] = [];
	add(item: string) {\n${body(3)}\n\t\tthis.#items.push(item);\n\t}
	count() {\n${body(3)}\n\t\treturn this.#items.length;\n\t}
}`;
    expect(lint(code)).toEqual([]);
  });

  it("flags members that fall into separate groups", () => {
    const code = `
export class Mixed {
	#items: string[] = [];
	#zoom = 1;
	add(item: string) {\n${body(3)}\n\t\tthis.#items.push(item);\n\t}
	zoomIn() {\n${body(3)}\n\t\tthis.#zoom += 1;\n\t}
}`;
    const [message] = lint(code, "m.ts", { maxGroups: 1, minGroupLines: 3 });
    expect(message).toContain("Mixed holds 2 groups");
  });

  it("ignores the constructor, which touches everything", () => {
    const code = `
export class Mixed {
	#items: string[];
	#zoom: number;
	constructor() { this.#items = []; this.#zoom = 1; }
	add(item: string) {\n${body(3)}\n\t\tthis.#items.push(item);\n\t}
	zoomIn() {\n${body(3)}\n\t\tthis.#zoom += 1;\n\t}
}`;
    expect(lint(code)[0]).toContain("Mixed holds 2 groups");
  });

  it("skips classes that implement a contract or extend a base", () => {
    const members = `
	add(item: string) {\n${body(3)}\n\t\treturn item;\n\t}
	zoomIn() {\n${body(3)}\n\t\treturn 1;\n\t}`;
    expect(lint(`interface Port {}\nexport class A implements Port {${members}\n}`)).toEqual([]);
    expect(lint(`class Base {}\nexport class B extends Base {${members}\n}`)).toEqual([]);
  });
});
