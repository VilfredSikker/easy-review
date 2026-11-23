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

// The page view runs on runes too and is not under test. Bun keeps a module
// mock for the whole run, which is safe here: no test loads the real store,
// and keyboard.test.ts mocks it again.
void mock.module("./pageView.svelte", () => ({ pageView: { current: null, overlay: () => {}, readTabs: () => {} } }));
void mock.module("./legacySnapshot.svelte", () => ({ legacySnapshot: { tabRows: [], overlay: (view: unknown) => view } }));
void mock.module("./app.svelte", () => ({ app: { routeCmds: () => {} } }));

const { followRunProgress } = await import("./arenaResource.svelte");
const { jobsFixture } = await import("./arenaResourceFixture.svelte");

/** Lets the effects the last write scheduled run. */
const settle = () => new Promise((resolve) => setTimeout(resolve, 0));

describe("followRunProgress", () => {
  it("calls the arena on each move of jobs", async () => {
    let revisions = 0;
    const release = followRunProgress(() => (revisions += 1), jobsFixture);
    await settle();
    const first = revisions;

    jobsFixture.move();
    await settle();
    jobsFixture.move();
    await settle();
    expect(revisions).toBe(first + 2);

    release();
    jobsFixture.move();
    await settle();
    expect(revisions).toBe(first + 2);
  });
});
