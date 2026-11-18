import { useEffect, useState } from "preact/hooks";

const listeners = new Set<() => void>();

export function navigate(path: string, replace = false): void {
  if (path === location.pathname + location.search) return;
  if (replace) history.replaceState(null, "", path);
  else history.pushState(null, "", path);
  for (const l of listeners) l();
}

addEventListener("popstate", () => {
  for (const l of listeners) l();
});

export function usePath(): string {
  const [path, setPath] = useState(location.pathname);
  useEffect(() => {
    const update = () => {
      setPath(location.pathname);
    };
    listeners.add(update);
    return () => {
      listeners.delete(update);
    };
  }, []);
  return path;
}

/** Splits a path into the route and its parameters: /body/m1-none/eyes -> ["body", "m1-none", "eyes"]. */
export const segments = (path: string) => path.split("/").filter(Boolean).map(decodeURIComponent);

/** The kind of page a path opens: "" for the ward, "strains", "strain", "body" and so on. */
const room = (path: string) => segments(path.split("?")[0] ?? "")[0] ?? "";

/**
 * Follows same-site links through the router; modified clicks keep the browser's behaviour. Into a new kind of page
 * the change moves back in depth under the still header (styles.css, "Motion"). It changes at once between two pages
 * of one kind (hour to hour, strain to strain), to or from a body page, whose ward flies the patient itself, and under
 * reduced motion.
 * Known limit: the browser's back and forward buttons change at once; the browser restores their scroll itself.
 */
export function onLink(e: MouseEvent): void {
  const a = (e.target as Element).closest("a");
  if (!a || e.defaultPrevented || e.button !== 0 || e.metaKey || e.ctrlKey || e.shiftKey || e.altKey) return;
  const href = a.getAttribute("href");
  if (!href?.startsWith("/") || a.target) return;
  e.preventDefault();
  const [path, hash] = href.split("#");
  const to = path || "/";
  const land = (smooth: boolean) => {
    const el = hash ? document.getElementById(hash) : null;
    if (el) el.scrollIntoView({ behavior: smooth ? "smooth" : "instant" });
    else window.scrollTo(0, 0);
  };
  const [from, into] = [room(location.pathname), room(to)];
  const depth =
    from !== into &&
    from !== "body" &&
    into !== "body" &&
    "startViewTransition" in document &&
    !matchMedia("(prefers-reduced-motion: reduce)").matches;
  if (!depth) {
    navigate(to);
    requestAnimationFrame(() => land(true));
    return;
  }
  document.startViewTransition(() => {
    navigate(to);
    // The new page renders in a microtask, so it stands by the next task: it is captured there, at its top.
    return new Promise<void>((done) =>
      setTimeout(() => {
        land(false);
        done();
      }),
    );
  });
}
