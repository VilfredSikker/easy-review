/**
 * Screenshot every Storybook story we care about and save PNGs to
 * `screenshots/` for visual diffing against the mock HTML files.
 *
 * Run: `bun run scripts/screenshot-stories.ts`
 * Requires: storybook dev server running on http://localhost:6006
 */
import { chromium } from "playwright";
import { mkdir } from "node:fs/promises";

interface Story {
  id: string;
  name: string;
}

const STORIES: Story[] = [
  { id: "pages-mainlayout--full", name: "MainLayout-Full" },
  { id: "pages-mainlayout--ai-review-with-findings", name: "MainLayout-AiReview" },
  { id: "pages-mainlayout--git-hub-sync-state", name: "MainLayout-GitHub" },
  { id: "pages-mainlayout--multi-folder", name: "MainLayout-MultiFolder" },
  { id: "pages-mainlayout--multi-project", name: "MainLayout-MultiProject" },
  { id: "pages-mainlayout--sparse-data", name: "MainLayout-Sparse" },
  { id: "pages-emptystate--first-launch", name: "EmptyState" },
];

const OUT_DIR = "screenshots";
const VIEWPORT = { width: 1440, height: 900 };

async function main() {
  await mkdir(OUT_DIR, { recursive: true });
  const browser = await chromium.launch();
  const ctx = await browser.newContext({ viewport: VIEWPORT, deviceScaleFactor: 2 });
  const page = await ctx.newPage();

  for (const story of STORIES) {
    const url = `http://localhost:6006/iframe.html?id=${story.id}&viewMode=story`;
    console.log(`→ ${story.name}: ${url}`);
    await page.goto(url, { waitUntil: "networkidle" });
    // Give fonts a beat to render.
    await page.waitForTimeout(500);
    const out = `${OUT_DIR}/${story.name}.png`;
    await page.screenshot({ path: out, fullPage: false });
    console.log(`  saved ${out}`);
  }

  await browser.close();
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
