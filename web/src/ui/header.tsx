import { Mark } from "./mark";
import { Term } from "./terms";

/**
 * Moves focus to the page's main heading, as after a page load, so a keyboard or a screen reader carries on from the
 * new page instead of the link that left the old one (app.tsx, after every move inside the site, and the skip button
 * below). `scroll` also brings the heading into view.
 */
export function focusPage(scroll = false): void {
  const heading = document.querySelector<HTMLElement>("main h1") ?? document.querySelector<HTMLElement>("main");
  if (!heading) return;
  if (!heading.hasAttribute("tabindex")) heading.tabIndex = -1;
  heading.focus({ preventScroll: !scroll });
}

/** Label, link, and the pages it stands for: the ward holds its patients, the record its hours. */
const NAV: [string, string, (path: string) => boolean][] = [
  ["Ward", "/#ward", (p) => p === "/" || p.startsWith("/body")],
  ["What is this?", "/#what", () => false],
  ["Strains", "/strains", (p) => p.startsWith("/strain")],
  ["Genes", "/genes", (p) => p.startsWith("/gene")],
  ["Record", "/record", (p) => p.startsWith("/record") || p.startsWith("/hour")],
  ["Defend", "/defend", (p) => p.startsWith("/defend")],
];

/** In words that need no tap: a copy of the lab on one machine is a practice copy. */
const PILLS: Record<string, string> = { "synthetic fixture": "SYNTHETIC", "local league": "PRACTICE COPY" };

/** The header pill for a league that is not the public one (deploy/dev/venin.toml names the dev league). */
export function leaguePill(league: string | null): string | null {
  return (league && PILLS[league]) ?? null;
}

export function Header({ league, path }: { league: string | null; path: string }) {
  const pill = leaguePill(league);
  return (
    <header class="top">
      {/* For the keyboard: past the header (and on the home page past the twenty beds) to the page's heading. */}
      <button type="button" class="skip" onClick={() => focusPage(true)}>
        Skip to the page
      </button>
      <a class="brand" href="/" aria-label="Venin, home">
        {/* The mark leans toward a pointer over the link (ui/motion.ts) and is otherwise still, its cut exact. */}
        <span class="brand-mark">
          <Mark size={32} />
        </span>
        <img class="brand-word" src="/brand/venin-wordmark-dark.svg" alt="venin" width="74" height="20" />
      </a>
      <nav class="nav" aria-label="Sections">
        {NAV.map(([label, href, current]) => (
          <a
            key={href}
            href={href}
            aria-current={current(path) ? "page" : undefined}
            // The tour opens over the page it is clicked on (ui/explain.tsx).
            aria-haspopup={href === "/#what" ? "dialog" : undefined}
          >
            {label}
          </a>
        ))}
        {/* On a phone the check-up button gives way to this link at the end of the row. */}
        <a class="nav-checkup" href="/checkup" aria-current={path.startsWith("/checkup") ? "page" : undefined}>
          Check-up
        </a>
      </nav>
      <div class="top-end">
        {pill && (
          <span class="league mono">
            <Term k={league === "local league" ? "local-league" : "synthetic"}>{pill}</Term>
          </span>
        )}
        <a class="btn ink small" href="/checkup">
          Check your agent
        </a>
      </div>
    </header>
  );
}
