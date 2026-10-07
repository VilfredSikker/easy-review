import { describe, expect, it } from "bun:test";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const src = readFileSync(join(here, "PrMergeBox.svelte"), "utf8");
const card = readFileSync(join(here, "BranchCard.svelte"), "utf8");

describe("PrMergeBox", () => {
  it("derives every state through the tested view-model", () => {
    expect(src).toContain("mergeBoxModel(github)");
    expect(src).toContain("pickMethod(model.methods, rememberedMethod)");
    expect(src).toContain("mergeHeadNote(github, prDiffHead)");
  });

  it("sends every action through the one backend command, naming the PR", () => {
    expect(src).toContain('await app.cmd("run_github_pr_action", { pr, action });');
    expect(src.match(/app\.cmd\(/g)?.length).toBe(1);
  });

  it("runs a confirmed action against the PR the confirm was built for", () => {
    expect(src).toContain("confirmTarget = prTarget(github);");
    expect(src).toContain("void run(action, confirmTarget);");
  });

  it("asks for confirmation before merging", () => {
    // The merge button only stages the request; confirmNow runs it.
    const start = src.slice(src.indexOf("function startMerge"), src.indexOf("function confirmNow"));
    expect(start).toContain("ask(mergeRequest(");
    expect(start).not.toContain("run(");
    expect(src).toContain("onclick={startMerge}");
    expect(src).toContain("onclick={confirmNow}");
  });

  it("pins merges to the reviewed diff and blocks one GitHub would refuse", () => {
    expect(src).toContain("mergeRequest(github, confirming.method, confirming.auto, del, prDiffHead)");
    expect(src).toContain("disabled={busy !== null || mergeBlocked}");
    expect(src).toContain("if (!confirming || !confirmTarget || mergeBlocked) return;");
  });

  it("asks twice for destructive menu items", () => {
    expect(src).toContain("if (item.destructive) ask(item.action);");
  });

  it("does not cancel a pending confirm on every poll, but does on another PR", () => {
    expect(src).toContain("const confirmKey = $derived(");
    expect(src).toMatch(/\$\{github\.owner\}\/\$\{github\.repo\}#\$\{github\.number\}/);
    expect(src).toContain("void confirmKey;");
  });

  it("draws its menus through the shared fixed-position menu", () => {
    expect(src).toContain("anchoredMenuPosition(el.getBoundingClientRect(), window.innerWidth)");
    expect(src.match(/<AnchoredMenu /g)?.length).toBe(2);
    expect(src).not.toContain('class="fixed z-50');
  });

  it("guards browser storage", () => {
    expect(src).toMatch(/try \{\s*rememberedMethod = localStorage\.getItem/);
    expect(src).toMatch(/try \{\s*localStorage\.setItem/);
  });

  it("is mounted inside the BranchCard GitHub card", () => {
    expect(card).toContain("<PrMergeBox {github} />");
  });

  it("shares the menu with the BranchCard stack dropdown", () => {
    expect(card).toContain("<AnchoredMenu pos={stackMenuPos}");
    expect(card).not.toContain('class="fixed z-50');
  });
});
