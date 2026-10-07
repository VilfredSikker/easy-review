import { describe, expect, it } from "vitest";
import { readFileSync, readdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

// Registering a command in main.rs is not enough: Tauri's ACL rejects any
// invoke missing from this allowlist with "<name> not allowed. Command not found".
const here = dirname(fileURLToPath(import.meta.url));
const srcDir = join(here, "..");
const desktopCrate = join(here, "../../../crates/er-desktop");
const allowlist = readFileSync(join(desktopCrate, "permissions/app-commands.toml"), "utf8");

function allowedCommands(): Set<string> {
  const block = allowlist.match(/commands\.allow\s*=\s*\[([\s\S]*?)\]/);
  if (!block) throw new Error("commands.allow not found in app-commands.toml");
  return new Set([...block[1].matchAll(/"([a-z0-9_]+)"/g)].map((m) => m[1]));
}

// The UI reaches commands through invoke, app.cmd and injected invoke
// functions, so scanning call sites misses some. Every registered handler is
// reachable from the webview, so the registered list is what must be allowed.
function registeredCommands(): string[] {
  const main = readFileSync(join(desktopCrate, "src/main.rs"), "utf8");
  const block = main.match(/generate_handler!\[([\s\S]*?)\]/);
  if (!block) throw new Error("generate_handler! not found in main.rs");
  return [...block[1].matchAll(/(?:[a-z0-9_]+::)*([a-z0-9_]+)\s*,?/g)].map((m) => m[1]);
}

function invokedCommands(): Map<string, string> {
  const found = new Map<string, string>();
  const files = readdirSync(srcDir, { recursive: true, encoding: "utf8" }).filter(
    (f) => /\.(ts|svelte)$/.test(f) && !/\.test\.ts$/.test(f),
  );
  for (const file of files) {
    const text = readFileSync(join(srcDir, file), "utf8");
    for (const m of text.matchAll(/\b(?:invoke(?:<[^(]*>)?|cmd)\(\s*["']([a-z0-9_]+)["']/g)) {
      if (!found.has(m[1])) found.set(m[1], file);
    }
  }
  return found;
}

describe("IPC allowlist", () => {
  it("allows every command registered in main.rs", () => {
    const allowed = allowedCommands();
    const registered = registeredCommands();
    expect(registered.length).toBeGreaterThan(100);
    expect(registered.filter((name) => !allowed.has(name))).toEqual([]);
  });

  it("allows every command the UI invokes", () => {
    const allowed = allowedCommands();
    const invoked = invokedCommands();
    expect(invoked.size).toBeGreaterThan(50);
    const missing = [...invoked]
      .filter(([name]) => !allowed.has(name))
      .map(([name, file]) => `${name} (${file})`);
    expect(missing).toEqual([]);
  });
});
