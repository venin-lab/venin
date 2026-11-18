/**
 * The motion that needs a script (styles.css, "Motion", has the rest): cards rise into place the first time they come
 * into the window, cards lean toward a pointer that can hover, and the footer's mark rests out of sight. Reduced motion
 * turns the rise and the lean off.
 */

/**
 * What rises: the cards of a grid, each on its own, as a list comes in. Single plates and the brand's fields stand
 * still, so the page has one moving moment per screen rather than every band entering the same way. A target inside
 * another rises with it; one that holds a canvas draws itself in instead, and its 3D layer measures the page, which a
 * tipped card would skew.
 */
const RISE = ".rates > .rate, .tiles > *, .strain-grid > *, .genes > .gene, .sheet";

/** What leans: cards you can open, and the header's link, whose mark leans for it. Plates that open nothing stay put. */
const LEAN = ".tile:not(.next), .strain:not(.line), .gene, .brand";

/** Starts both on `root`, the app's element, before anything renders into it. */
export function startMotion(root: HTMLElement): void {
  const still = matchMedia("(prefers-reduced-motion: reduce)");
  const fine = matchMedia("(hover: hover) and (pointer: fine)");

  // Nothing waits to rise until the reader first scrolls, taps or types: a page nobody scrolls shows whole, and so
  // does a capture of it.
  let armed = false;
  /** Targets last seen under the window that have not risen yet. */
  const below = new Set<HTMLElement>();

  const rise = (el: HTMLElement, delay: number) => {
    el.style.animationDelay = `${delay}ms`;
    el.dataset.rise = "in";
    const done = (e: AnimationEvent) => {
      if (e.target !== el || e.animationName !== "rise") return;
      el.removeEventListener("animationend", done);
      el.removeEventListener("animationcancel", done);
      el.removeAttribute("data-rise");
      el.style.animationDelay = "";
    };
    el.addEventListener("animationend", done);
    el.addEventListener("animationcancel", done);
  };

  const io = new IntersectionObserver(
    (entries) => {
      const coming: IntersectionObserverEntry[] = [];
      for (const e of entries) {
        const el = e.target as HTMLElement;
        if (!e.isIntersecting && e.boundingClientRect.top >= innerHeight) {
          below.add(el);
          if (armed && !still.matches) el.dataset.rise = "wait";
          continue;
        }
        // In the window, or passed above it unseen: either way it has had its turn.
        io.unobserve(el);
        below.delete(el);
        if (e.isIntersecting && el.dataset.rise === "wait") coming.push(e);
      }
      // A row that comes in together goes left to right, each card a little sooner after the one before.
      coming.sort(
        (a, b) =>
          a.boundingClientRect.top - b.boundingClientRect.top || a.boundingClientRect.left - b.boundingClientRect.left,
      );
      coming.forEach((e, i) => {
        rise(e.target as HTMLElement, Math.round(330 * (1 - 0.8 ** i)));
      });
    },
    // A waiting card sits 32px low; the margin offsets that, so a card starts up as its place enters the window and
    // the window's bottom edge is never bare paper over cards still waiting.
    { rootMargin: "0px 0px 32px 0px" },
  );

  // The first input comes before the scroll it starts, so every target still under the window waits in time; no
  // layout is read here, where it would hold up that first frame.
  const arm = () => {
    if (armed) return;
    armed = true;
    if (still.matches) return;
    for (const el of below) el.dataset.rise = "wait";
  };
  for (const type of ["wheel", "touchstart", "keydown", "pointerdown"]) {
    addEventListener(type, arm, { once: true, passive: true });
  }

  // The footer's mark sways only while it is in the window (styles.css, "Motion"); out of it, it holds still.
  const sway = new IntersectionObserver((entries) => {
    for (const e of entries) e.target.classList.toggle("off", !e.isIntersecting);
  });

  const watch = (node: Node) => {
    if (!(node instanceof HTMLElement)) return;
    const found = node.matches(RISE) ? [node] : [];
    found.push(...node.querySelectorAll<HTMLElement>(RISE));
    for (const el of found) {
      if (!el.parentElement?.closest(RISE) && !el.querySelector("canvas")) io.observe(el);
    }
    for (const el of node.matches(".foot-mark") ? [node] : node.querySelectorAll(".foot-mark")) sway.observe(el);
  };
  new MutationObserver((records) => {
    for (const r of records) for (const n of r.addedNodes) watch(n);
    for (const el of below) {
      if (el.isConnected) continue;
      io.unobserve(el);
      below.delete(el);
    }
  }).observe(root, { childList: true, subtree: true });

  // Turned on mid-visit, reduced motion shows whatever was still waiting.
  still.addEventListener("change", () => {
    if (!still.matches) return;
    for (const el of root.querySelectorAll<HTMLElement>("[data-rise]")) {
      el.removeAttribute("data-rise");
      el.style.animationDelay = "";
    }
  });

  let leaning: HTMLElement | null = null;
  let px = 0;
  let py = 0;
  let frame = 0;
  const mover = (el: HTMLElement) =>
    el.classList.contains("brand") ? el.querySelector<HTMLElement>(".brand-mark") : el;
  const settle = () => {
    const m = leaning && mover(leaning);
    if (m) m.style.transform = "";
    leaning = null;
  };
  // The card under the pointer rises 2px and tips toward it by at most 2 degrees: a hint of depth, so a plate of data
  // never sways with the pointer. One wider than 600px stays flat, its shadow alone lifting it, since a flat card
  // raised 2px would slide off a pointer at its bottom edge and flicker. The header's mark, 32px, leans up to 10 degrees.
  const TIP = 2;
  const lean = () => {
    frame = 0;
    const m = leaning && mover(leaning);
    if (!leaning || !m) return;
    const r = leaning.getBoundingClientRect();
    const x = Math.max(-1, Math.min(1, ((px - r.left) / r.width) * 2 - 1));
    const y = Math.max(-1, Math.min(1, ((py - r.top) / r.height) * 2 - 1));
    const mark = m !== leaning;
    if (!mark && r.width > 600) return;
    const rx = (mark ? 10 : TIP) * y;
    const ry = -(mark ? 10 : TIP) * x;
    const depth = mark ? "perspective(128px)" : "perspective(900px) translateY(-2px)";
    m.style.transform = `${depth} rotateX(${rx.toFixed(2)}deg) rotateY(${ry.toFixed(2)}deg)`;
  };
  document.addEventListener(
    "pointermove",
    (e) => {
      if (e.pointerType === "touch" || still.matches || !fine.matches) return;
      const el = (e.target as Element).closest<HTMLElement>(LEAN);
      if (el !== leaning) settle();
      // A card still rising keeps the rise's move.
      if (!el || el.closest("[data-rise]")) return;
      leaning = el;
      px = e.clientX;
      py = e.clientY;
      if (!frame) frame = requestAnimationFrame(lean);
    },
    { passive: true },
  );
  document.documentElement.addEventListener("pointerleave", settle);
  addEventListener("scroll", settle, { passive: true });
}
