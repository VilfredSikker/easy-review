import { chromium } from "playwright";
import { mkdir } from "node:fs/promises";

await mkdir("screenshots", { recursive: true });
const browser = await chromium.launch();
const ctx = await browser.newContext({ viewport: { width: 1440, height: 900 }, deviceScaleFactor: 2 });
const page = await ctx.newPage();
await page.goto("http://localhost:6006/iframe.html?id=pages-mainlayout--full&viewMode=story", { waitUntil: "networkidle" });
await page.waitForTimeout(500);

const sidebar = page.locator("aside").first();
await sidebar.screenshot({ path: "screenshots/Sidebar-Full.png" });
console.log("saved screenshots/Sidebar-Full.png");

await browser.close();
