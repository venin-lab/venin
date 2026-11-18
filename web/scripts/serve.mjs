import { chromium } from "playwright-core";
import { createServer } from "vite";

/**
 * Starts the dev server on the dev data (synthetic until the dev league has a genesis), or uses the one already
 * serving at SHOTS_URL, and a Playwright headless shell (never the desktop Chrome).
 */
export async function open(args = []) {
  const running = process.env.SHOTS_URL?.replace(/\/$/, "");
  const server = running
    ? null
    : await createServer({ root: new URL("..", import.meta.url).pathname, logLevel: "error" });
  await server?.listen(0);
  const url = running ?? server.resolvedUrls.local[0].replace(/\/$/, "");
  const browser = await chromium.launch({ args });
  return {
    url,
    browser,
    async close() {
      await browser.close();
      await server?.close();
    },
  };
}

/** Collects console errors and page errors; newer Chrome returns a Promise from scrollTo, so the shell does too. */
export async function watch(page) {
  const errors = [];
  page.on("console", (m) => {
    if (m.type() === "error") errors.push(m.text());
  });
  page.on("pageerror", (e) => errors.push(e.message));
  await page.addInitScript(() => {
    const scrollTo = window.scrollTo.bind(window);
    window.scrollTo = (...a) => {
      scrollTo(...a);
      return Promise.resolve();
    };
  });
  return errors;
}

/** Text boxes that overlap another text box, and pages that scroll sideways. */
export function faults() {
  // Text cut off by a scrolling or clipping box is not on screen past that box's edges.
  const clip = (el, box) => {
    const out = { ...box };
    for (let a = el; a && a !== document.body; a = a.parentElement) {
      const cs = getComputedStyle(a);
      if (cs.overflowX === "visible" && cs.overflowY === "visible") continue;
      const c = a.getBoundingClientRect();
      out.left = Math.max(out.left, c.left);
      out.right = Math.min(out.right, c.right);
      out.top = Math.max(out.top, c.top);
      out.bottom = Math.min(out.bottom, c.bottom);
    }
    return out.right - out.left > 1 && out.bottom - out.top > 1 ? out : null;
  };
  const boxes = [];
  // With a modal dialog open only its text is on screen; the page sits inert under the backdrop.
  const modal = document.querySelector("dialog:modal");
  const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
  for (let node = walker.nextNode(); node; node = walker.nextNode()) {
    const el = node.parentElement;
    if (!node.textContent.trim() || !el || el.closest("[aria-hidden=true]") || (modal && !modal.contains(el))) continue;
    // Scrolled, the sticky header sits over the page on purpose; its own layout is checked at the top of each page.
    if (scrollY > 0 && el.closest("header.top")) continue;
    if (!el.checkVisibility({ opacityProperty: true, visibilityProperty: true })) continue;
    const range = document.createRange();
    range.selectNodeContents(node);
    // A line box is taller than its glyphs; compare the band the letters actually cover.
    // The computed font size ignores CSS zoom (the card scales itself to fit); the rects do not.
    const band = Number.parseFloat(getComputedStyle(el).fontSize) * 0.36 * (el.currentCSSZoom ?? 1);
    for (const r of range.getClientRects()) {
      if (r.width <= 1 || r.height <= 1) continue;
      const mid = r.top + r.height / 2;
      const box = clip(el, { left: r.left, right: r.right, top: mid - band, bottom: mid + band });
      if (box) boxes.push({ r: box, el, text: node.textContent.trim().slice(0, 40) });
    }
  }
  const out = [];
  for (let i = 0; i < boxes.length; i++) {
    for (let j = i + 1; j < boxes.length; j++) {
      const a = boxes[i];
      const b = boxes[j];
      // An open popover (a lab word's meaning) is drawn over the page on purpose and covers what lies under it.
      if (a.el === b.el || a.el.closest(":popover-open") || b.el.closest(":popover-open")) continue;
      const w = Math.min(a.r.right, b.r.right) - Math.max(a.r.left, b.r.left);
      const h = Math.min(a.r.bottom, b.r.bottom) - Math.max(a.r.top, b.r.top);
      if (w > 2 && h > 2) out.push(`overlap: "${a.text}" / "${b.text}"`);
    }
  }
  // The page's own width, not innerWidth: an emulated phone zooms out to a wider page, and innerWidth follows it.
  const { scrollWidth, clientWidth } = document.documentElement;
  if (scrollWidth > clientWidth + 1) out.push(`sideways scroll: ${scrollWidth} > ${clientWidth}`);
  return out;
}
