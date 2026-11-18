import { mkdir, mkdtemp, readFile, writeFile } from "node:fs/promises";
import { homedir, tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { faults, open, watch } from "./serve.mjs";

const OUT = process.env.SHOTS_DIR ?? join(homedir(), "Documents/venin-dev/shots");

/** Pages past the ward, by shot name, at the latest hour of whatever data the server holds. */
async function pages(url) {
  const get = async (path) => (await fetch(`${url}/data/${path}`)).json();
  const { n } = await get("head.json");
  const hour = await get(`hours/${n}.json`);
  const [gene] = await get("library/genes.json");
  return [
    ["hour", `/hour/${n}`],
    ["record", "/record"],
    ["strains", "/strains"],
    ["strain", `/strain/${hour.strain}`],
    ["genes", "/genes"],
    ["gene", `/gene/${gene.id}`],
    ["card", `/card/${n}`],
    ["release", "/release"],
    ["verify", "/verify"],
    ["checkup", "/checkup"],
  ];
}

/** The check-up's sample agent written out as a folder, to pick the way a visitor would. */
async function folder() {
  const root = join(await mkdtemp(join(tmpdir(), "venin-shots-")), "ledgerlark");
  const sample = JSON.parse(await readFile(new URL("../../library/examples/sample-agent.json", import.meta.url)));
  for (const f of sample.files) {
    await mkdir(dirname(join(root, f.path)), { recursive: true });
    await writeFile(join(root, f.path), f.content);
  }
  return root;
}
const SIZES = [
  [1440, 900],
  [1920, 1080],
  [390, 844],
];

async function shoot(page, name, problems, full = false) {
  // A whole-page capture first passes the page through the window and goes back to the top: plates waiting to rise
  // (ui/motion.ts) rise, and figures that draw themselves on view (the How figure) draw.
  if (full) {
    await page.evaluate(async () => {
      for (let y = 0; y < document.documentElement.scrollHeight; y += innerHeight / 2) {
        window.scrollTo(0, y);
        await new Promise((done) => setTimeout(done, 60));
      }
      // The How figure inks organ by organ while it is in view; the capture waits for the last one.
      const figure = document.querySelector(".how-figure");
      if (figure?.querySelector("[data-wait]")) {
        figure.scrollIntoView({ block: "center" });
        for (let t = 0; t < 12000 && figure.querySelector("[data-wait]"); t += 200) {
          await new Promise((done) => setTimeout(done, 200));
        }
      }
      window.scrollTo(0, 0);
    });
    await page.waitForTimeout(1000);
  }
  const found = await page.evaluate(faults);
  for (const f of found) problems.push(`${name}: ${f}`);
  // A record that does not verify is a problem of the data, which no capture of it can vouch for.
  const refused = await page.evaluate(() => document.querySelector(".verify-page .verdict.bad")?.textContent ?? null);
  if (refused) problems.push(`${name}: ${refused}`);
  // A phone's whole page at twice the pixels runs past the capture limit and comes out blank; take it at CSS size.
  await page.screenshot({ path: join(OUT, `${name}.png`), fullPage: full, scale: full ? "css" : "device" });
}

const app = await open();
await mkdir(OUT, { recursive: true });
const PAGES = await pages(app.url);
const picked = await folder();
const problems = [];
try {
  for (const [width, height] of SIZES) {
    // A phone is emulated as one: with isMobile a page wider than the screen zooms out instead of scrolling, which
    // is how a phone shows it (the share card once overflowed only there).
    const ctx = await app.browser.newContext({
      viewport: { width, height },
      deviceScaleFactor: width < 500 ? 2 : 1,
      hasTouch: width < 500,
      isMobile: width < 500,
    });
    const page = await ctx.newPage();
    const errors = await watch(page);
    const tag = `${width}`;
    await page.goto(`${app.url}/`);
    await page.waitForSelector(".stage.opening", { timeout: 20000 });
    await page.waitForTimeout(1800);
    await page.screenshot({ path: join(OUT, `${tag}-opening.png`) });
    await page.waitForSelector(".stage.open", { timeout: 20000 });
    await page.waitForTimeout(2600);
    await shoot(page, `${tag}-home`, problems);
    // The band's figure draws itself when it comes into view: sketch, then the ink from ear to heart.
    await page.locator("#how .how-figure").scrollIntoViewIfNeeded();
    await page.waitForTimeout(5600);
    await shoot(page, `${tag}-how`, problems);
    await page.evaluate(() => window.scrollTo(0, 0));
    await page.waitForTimeout(400);
    await shoot(page, `${tag}-full`, problems, true);
    await page.click(".cell.focal");
    await page.waitForSelector(".body-page");
    // The patient stands up (0.9 s), its ink lifts and runs again from 1.45 s, and the story takes 3.6 s.
    await page.waitForTimeout(5200);
    await shoot(page, `${tag}-body`, problems);
    await page.click(".spot.entry", { force: true });
    await page.waitForTimeout(3600);
    await shoot(page, `${tag}-organ`, problems);
    await page.keyboard.press("Escape");
    await page.waitForTimeout(400);
    await page.keyboard.press("Escape");
    await page.waitForSelector(".stage.open");
    await page.waitForTimeout(1200);
    await shoot(page, `${tag}-return`, problems);
    // The tour as a post's link opens it, screen by screen; then a lab word, tapped on a phone, hovered on a desktop.
    await page.goto(`${app.url}/#what`);
    await page.waitForSelector("dialog.explain[open]");
    await page.waitForTimeout(3200);
    for (let i = 1; i <= 5; i++) {
      if (i > 1) {
        await page.keyboard.press("ArrowRight");
        await page.waitForTimeout(2400);
      }
      await shoot(page, `${tag}-home-explain-${i}`, problems);
    }
    await page.keyboard.press("Escape");
    if (width !== 1920) {
      await page.waitForSelector(".stage.open");
      const word = page.locator(".ward-row .term-word", { hasText: "Venin 1" });
      await (width < 500 ? word.tap() : word.hover());
      await page.waitForTimeout(600);
      const box = await page.evaluate(() => document.querySelector(".term-pop:popover-open")?.getBoundingClientRect());
      if (!box || box.left < 0 || box.top < 0 || box.right > width || box.bottom > height) {
        problems.push(`${tag}-home-term: the definition is not inside the window (${JSON.stringify(box)})`);
      }
      await shoot(page, `${tag}-home-term`, problems);
      // The header's league pill, open, so a reader of the shots sees what "practice copy" means.
      await page.keyboard.press("Escape");
      const pill = page.locator(".league .term-word");
      if (await pill.count()) {
        await page.evaluate(() => window.scrollTo(0, 0));
        await (width < 500 ? pill.tap() : pill.hover());
        await page.waitForTimeout(600);
        await shoot(page, `${tag}-league-term`, problems);
        await page.keyboard.press("Escape");
      }
    }
    for (const [name, path] of PAGES) {
      await page.goto(`${app.url}${path}`);
      const ready = ["card", "verify", "checkup"].includes(name);
      await page.waitForSelector(ready ? "[data-ready]" : "main.page", { timeout: 20000 });
      await page.waitForTimeout(name.startsWith("hour") ? 2600 : name === "checkup" ? 3200 : 600);
      await shoot(page, `${tag}-${name}`, problems, name !== "card");
    }
    await page.setInputFiles(".checkup-page input[type=file]", picked);
    await page.waitForSelector(".checkup-body:not(.sample)");
    await page.waitForSelector("[data-ready]");
    await page.waitForTimeout(2600);
    await shoot(page, `${tag}-checkup-folder`, problems, true);
    for (const e of errors) problems.push(`${tag} console: ${e}`);
    await ctx.close();
  }
  // The narrowest phone, where the header's row of links is tightest: the home page only.
  const ctx = await app.browser.newContext({
    viewport: { width: 360, height: 740 },
    deviceScaleFactor: 2,
    hasTouch: true,
    isMobile: true,
  });
  const page = await ctx.newPage();
  const errors = await watch(page);
  await page.goto(`${app.url}/`);
  await page.waitForSelector(".stage.open", { timeout: 20000 });
  await page.waitForTimeout(2600);
  await shoot(page, "360-home", problems);
  await shoot(page, "360-full", problems, true);
  for (const e of errors) problems.push(`360 console: ${e}`);
  await ctx.close();
} finally {
  await app.close();
}
console.log(`shots in ${OUT}`);
if (problems.length) {
  console.error(problems.join("\n"));
  process.exit(1);
}
