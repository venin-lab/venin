import { open, watch } from "./serve.mjs";

const MIN = Number(process.env.FPS_MIN ?? 55);

/** Frames per second over `ms` of requestAnimationFrame, and the share of frames slower than 34 ms. */
function probe(ms) {
  return new Promise((resolve) => {
    const times = [];
    const start = performance.now();
    const step = (t) => {
      times.push(t);
      if (t - start < ms) requestAnimationFrame(step);
      else {
        const gaps = times.slice(1).map((x, i) => x - times[i]);
        resolve({ fps: (gaps.length * 1000) / (times.at(-1) - times[0]), slow: gaps.filter((g) => g > 34).length });
      }
    };
    requestAnimationFrame(step);
  });
}

const app = await open(["--use-angle=metal", "--enable-gpu", "--ignore-gpu-blocklist"]);
let failed = false;
try {
  const ctx = await app.browser.newContext({ viewport: { width: 1440, height: 900 }, deviceScaleFactor: 2 });
  const page = await ctx.newPage();
  const errors = await watch(page);
  await page.goto(`${app.url}/`);
  await page.waitForSelector(".stage.opening", { timeout: 20000 });
  const gpu = await page.evaluate(() => {
    const gl = document.createElement("canvas").getContext("webgl2");
    const info = gl?.getExtension("WEBGL_debug_renderer_info");
    return info ? gl.getParameter(info.UNMASKED_RENDERER_WEBGL) : "unknown";
  });
  const opening = await page.evaluate(probe, 4000);
  await page.waitForSelector(".stage.open", { timeout: 20000 });
  const ward = await page.evaluate(probe, 4000);
  await page.click(".cell.focal");
  await page.waitForSelector(".body-page");
  await page.waitForTimeout(800);
  const body = await page.evaluate(probe, 4000);
  const rows = { opening, ward, body };
  console.log(`renderer: ${gpu}`);
  for (const [name, r] of Object.entries(rows)) {
    console.log(`${name.padEnd(8)} ${r.fps.toFixed(1)} fps, ${r.slow} frames over 34 ms`);
    if (r.fps < MIN) failed = true;
  }
  if (errors.length) {
    console.error(errors.join("\n"));
    failed = true;
  }
} finally {
  await app.close();
}
if (failed) {
  console.error(`below ${MIN} fps or errors`);
  process.exit(1);
}
