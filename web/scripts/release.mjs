import { mkdir } from "node:fs/promises";
import { homedir } from "node:os";
import { join } from "node:path";
import { chromium } from "playwright-core";
import { createServer } from "vite";
import { faults, watch } from "./serve.mjs";

/**
 * /release on this machine: shots at 1440, 1920 and 390 with the overlap and console checks, then, when the dev
 * wallet is configured (the VITE_DEV_* values `./dev up` sets, an anvil with the dev token, the Worker at VITE_DEV_API
 * allowing this page's origin), the dev wallet button pressed until the Worker has queued the burn.
 * Usage: node scripts/release.mjs [port]
 */
const port = Number(process.argv[2] ?? 5173);
const OUT = process.env.SHOTS_DIR ?? join(homedir(), "Documents/venin-dev/shots");
const SIZES = [
  [1440, 900],
  [1920, 1080],
  [390, 844],
];

const server = await createServer({
  root: new URL("..", import.meta.url).pathname,
  logLevel: "error",
  server: { port, strictPort: true, host: "localhost" },
});
await server.listen();
const url = `http://localhost:${port}/release`;
const browser = await chromium.launch();
const problems = [];
try {
  await mkdir(OUT, { recursive: true });
  for (const [width, height] of SIZES) {
    const ctx = await browser.newContext({ viewport: { width, height }, deviceScaleFactor: width < 500 ? 2 : 1 });
    const page = await ctx.newPage();
    const errors = await watch(page);
    await page.goto(url);
    await page.waitForSelector("main.release-page", { timeout: 30000 });
    await page.waitForSelector(".rel-verdict:not(.dim)", { timeout: 30000 });
    await page.waitForTimeout(500);
    for (const f of await page.evaluate(faults)) problems.push(`${width}: ${f}`);
    await page.screenshot({ path: join(OUT, `${width}-release.png`), fullPage: true });
    for (const e of errors) problems.push(`${width} console: ${e}`);
    await ctx.close();
  }
  if (process.env.VITE_DEV_WALLET_KEY) {
    const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });
    const errors = await watch(page);
    await page.goto(url);
    const button = page.getByRole("button", { name: "Burn and release with the dev wallet" });
    await button.waitFor({ timeout: 30000 });
    await page.waitForFunction(() => !document.querySelector(".rel-dev button")?.disabled, null, { timeout: 30000 });
    await button.click();
    const queued = page.locator(".rel-steps li.done", { hasText: "Queued for the next hour" });
    const failed = page.locator(".rel-steps li.fail");
    await Promise.race([queued.waitFor({ timeout: 60000 }), failed.waitFor({ timeout: 60000 })]);
    if (await failed.count()) problems.push(`release: ${await failed.textContent()}`);
    else console.log(`queued ${await queued.locator(".rel-hash").textContent()}`);
    await page.locator(".rel-burn").screenshot({ path: join(OUT, "1440-release-queued.png") });
    for (const e of errors) problems.push(`release console: ${e}`);
  }
} finally {
  await browser.close();
  await server.close();
}
console.log(`shots in ${OUT}`);
if (problems.length) {
  console.error(problems.join("\n"));
  process.exit(1);
}
