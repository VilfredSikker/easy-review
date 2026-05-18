/**
 * Render the mock HTML files in chromium and save PNGs so we can compare
 * pixel-for-pixel with the corresponding Storybook stories.
 *
 * Run: `bun run scripts/screenshot-mocks.ts`
 */
import { chromium } from "playwright";
import { mkdir } from "node:fs/promises";
import { pathToFileURL } from "node:url";
import { resolve } from "node:path";

interface Mock {
  file: string;
  name: string;
}

const MOCKS: Mock[] = [
  { file: "../.work/EasyReview-as-Tauri-App/mocks/01-main.html", name: "Mock-01-main" },
  { file: "../.work/EasyReview-as-Tauri-App/mocks/03-palette.html", name: "Mock-03-palette" },
  { file: "../.work/EasyReview-as-Tauri-App/mocks/04-github.html", name: "Mock-04-github" },
  { file: "../.work/EasyReview-as-Tauri-App/mocks/05-empty.html", name: "Mock-05-empty" },
];

const OUT_DIR = "screenshots";
const VIEWPORT = { width: 1440, height: 900 };

async function main() {
  await mkdir(OUT_DIR, { recursive: true });
  const browser = await chromium.launch();
  const ctx = await browser.newContext({ viewport: VIEWPORT, deviceScaleFactor: 2 });
  const page = await ctx.newPage();

  for (const mock of MOCKS) {
    const abs = resolve(mock.file);
    const url = pathToFileURL(abs).href;
    console.log(`→ ${mock.name}: ${url}`);
    await page.goto(url, { waitUntil: "networkidle" });
    // Mock pages load Alpine + Tailwind from CDN — give them time.
    await page.waitForTimeout(2000);
    const out = `${OUT_DIR}/${mock.name}.png`;
    await page.screenshot({ path: out, fullPage: false });
    console.log(`  saved ${out}`);
  }

  await browser.close();
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
