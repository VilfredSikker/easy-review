/**
 * Open a headed Playwright Chromium window pointed at a Storybook story.
 * Usage: `bun run scripts/open-storybook.ts [storyId]`
 * Default story: pages-mainlayout--full
 */
import { chromium } from "playwright";

const storyId = process.argv[2] ?? "pages-mainlayout--full";
const url = `http://localhost:6006/iframe.html?id=${storyId}&viewMode=story`;

const browser = await chromium.launch({ headless: false });
const ctx = await browser.newContext({ viewport: { width: 1440, height: 900 } });
const page = await ctx.newPage();
await page.goto(url);
console.log(`Opened ${url}`);
console.log("Close the browser window to exit.");
// Keep the process alive until the browser closes.
await new Promise<void>((resolve) => browser.on("disconnected", resolve));
