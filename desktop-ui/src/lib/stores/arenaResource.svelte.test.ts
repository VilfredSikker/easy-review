import { plugin } from "bun";
import { describe, expect, it, mock } from "bun:test";
import { compileModule } from "svelte/compiler";

// The arena resource runs on runes, which only the svelte compiler turns into
// code, so this test compiles it and its fixture; bun loads every other
// module as plain TypeScript.
plugin({
  name: "arena-resource-runes",
  setup(build) {
    build.onLoad({ filter: /arenaResource(Fixture)?\.svelte\.ts$/ }, async ({ path }) => {
      const ts = await Bun.file(path).text();
      const js = new Bun.Transpiler({ loader: "ts" }).transformSync(ts);
      return { contents: compileModule(js, { generate: "client", filename: path }).js.code, loader: "js" };
    });
  },
});

// Runes modules the arena resource reads at load; bun cannot load them.
void mock.module("./app.svelte", () => ({ app: { overlaySnapshot: () => {} } }));
void mock.module("./tabsResource.svelte", () => ({ tabsResource: { activeKey: null } }));

const { followRunProgress } = await import("./arenaResource.svelte");
const { jobsFixture } = await import("./arenaResourceFixture.svelte");

/** Lets the effects the last write scheduled run. */
const settle = () => new Promise((resolve) => setTimeout(resolve, 0));

/** A feed over the fixture's `jobs` that records what the desktop listener was asked for. */
function feed(flagOn: boolean) {
  const listened: string[] = [];
  const handlers: (() => void)[] = [];
  const listen = (event: string, handler: () => void) => {
    listened.push(event);
    handlers.push(handler);
    return Promise.resolve(() => {});
  };
  return { feed: { flagOn, adapter: jobsFixture, listen }, listened, handlers };
}

describe("followRunProgress", () => {
  it("calls the arena on each move of jobs with the flag on, and never listens for er://revision", async () => {
    const { feed: on, listened } = feed(true);
    let revisions = 0;
    const release = await followRunProgress(() => (revisions += 1), on);
    await settle();
    const first = revisions;

    jobsFixture.move();
    await settle();
    jobsFixture.move();
    await settle();
    expect(revisions).toBe(first + 2);
    expect(listened).toEqual([]);

    release();
    jobsFixture.move();
    await settle();
    expect(revisions).toBe(first + 2);
  });

  it("listens for er://revision with the flag off, and a move of jobs does not call the arena", async () => {
    const { feed: off, listened, handlers } = feed(false);
    let revisions = 0;
    await followRunProgress(() => (revisions += 1), off);
    jobsFixture.move();
    await settle();
    expect(revisions).toBe(0);
    expect(listened).toEqual(["er://revision"]);

    for (const handler of handlers) handler();
    expect(revisions).toBe(1);
  });
});
