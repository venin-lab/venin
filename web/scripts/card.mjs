// Renders /card/<n> of the built site to <data dir>/cards/<n>.png, next to the runner's cards/<n>.json.
// Usage: node web/scripts/card.mjs <n>; the data dir is VENIN_DATA (set by the runner) or VENIN_DATA_DIR.
// Known limit: the site must be built with the default VITE_DATA_URL ("/data"), which this script serves.
import { createReadStream, statSync } from "node:fs";
import { mkdir, rename, writeFile } from "node:fs/promises";
import { createServer } from "node:http";
import { homedir } from "node:os";
import { extname, join, normalize } from "node:path";
import { chromium } from "playwright-core";

const TYPES = {
  ".html": "text/html",
  ".js": "text/javascript",
  ".css": "text/css",
  ".json": "application/json",
  ".jsonl": "application/x-ndjson",
  ".gz": "application/gzip",
  ".svg": "image/svg+xml",
  ".png": "image/png",
  ".webp": "image/webp",
  ".woff2": "font/woff2",
  ".bin": "application/octet-stream",
  ".txt": "text/plain",
};

const isFile = (path) => {
  try {
    return statSync(path).isFile();
  } catch {
    return false;
  }
};

/** The file a request path maps to: /data/... from the data dir, the rest from the build, else the app page. */
export function fileFor(url, dist, data) {
  let raw;
  try {
    raw = decodeURIComponent(url.split("?")[0]);
  } catch {
    return null;
  }
  if (raw.includes("..")) return null;
  const path = normalize(raw);
  if (path.startsWith("/data/")) return join(data, path.slice("/data/".length));
  const file = join(dist, path);
  return isFile(file) ? file : join(dist, "index.html");
}

function serve(dist, data) {
  const server = createServer((req, res) => {
    const file = fileFor(req.url ?? "/", dist, data);
    if (!file || !isFile(file)) {
      res.writeHead(404).end();
      return;
    }
    res.writeHead(200, { "Content-Type": TYPES[extname(file)] ?? "application/octet-stream" });
    createReadStream(file).pipe(res);
  });
  return new Promise((done) => server.listen(0, "127.0.0.1", () => done(server)));
}

async function main() {
  const n = process.argv[2] ?? "";
  if (!/^\d+$/.test(n)) throw new Error("usage: card.mjs <n>");
  const dist = new URL("../dist", import.meta.url).pathname;
  const data = process.env.VENIN_DATA ?? process.env.VENIN_DATA_DIR ?? join(homedir(), "Documents/venin-dev/synthetic");
  if (!isFile(join(dist, "index.html"))) throw new Error(`no build at ${dist}; run pnpm build first`);
  if (!isFile(join(data, `hours/${n}.json`))) throw new Error(`no hours/${n}.json in ${data}`);
  const server = await serve(dist, data);
  const origin = `http://127.0.0.1:${server.address().port}`;
  const browser = await chromium.launch();
  try {
    const page = await browser.newPage({ viewport: { width: 1600, height: 900 }, deviceScaleFactor: 1 });
    const errors = [];
    page.on("pageerror", (e) => errors.push(e.message));
    await page.route("**/*", (route) =>
      new URL(route.request().url()).origin === origin ? route.continue() : route.abort(),
    );
    await page.goto(`${origin}/card/${n}`);
    await page.waitForSelector("[data-ready]", { timeout: 30000 });
    await page.waitForTimeout(500);
    if (errors.length) throw new Error(`card ${n}: ${errors.join("; ")}`);
    await mkdir(join(data, "cards"), { recursive: true });
    const out = join(data, "cards", `${n}.png`);
    // Written beside the card and renamed into place, so the uploader and the poster never meet half a picture.
    await writeFile(`${out}.tmp`, await page.screenshot({ type: "png" }));
    await rename(`${out}.tmp`, out);
    console.log(out);
  } finally {
    await browser.close();
    server.close();
  }
}

if (process.argv[1] === new URL(import.meta.url).pathname) {
  main().catch((e) => {
    console.error(e instanceof Error ? e.message : String(e));
    process.exit(1);
  });
}
