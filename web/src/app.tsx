import { Component, type ComponentChildren } from "preact";
import { useEffect, useRef, useState } from "preact/hooks";
import { CheckupPage } from "./pages/checkup";
import { DefendPage, DefendResult } from "./pages/defend";
import { idOf } from "./pages/defend-view";
import { ModelsPage } from "./pages/models";
import { ReleasePage } from "./pages/release";
import { VerifyPage } from "./pages/verify";
import { FirstHourPending, type Lab, loadLab } from "./record/load";
import { bodyName } from "./record/stats";
import type { Manifest } from "./record/types";
import { parseHour } from "./record/views";
import { BodyPage } from "./ui/body";
import { CardPage } from "./ui/card";
import { Explainer, explain } from "./ui/explain";
import { Footer } from "./ui/footer";
import { GenePage, GenesPage } from "./ui/genes";
import { focusPage, Header } from "./ui/header";
import { Home } from "./ui/home";
import { HourPage } from "./ui/hour";
import { NotFound } from "./ui/not-found";
import { RecordPage } from "./ui/record";
import { navigate, onLink, segments, usePath } from "./ui/router";
import { DOT, firstLine, HowItWorks, startsAt } from "./ui/sections";
import { StrainPage, StrainsPage } from "./ui/strains";
import { Hero, PendingStage } from "./ui/ward";

/**
 * A page that could not be shown, in plain words, with the ways on. What the browser said stays folded away: a reader
 * needs the way back, a builder can still open the cause.
 */
function Fault({ title, line, cause }: { title: string; line?: string; cause: string }) {
  return (
    <main class="fault">
      <h1>{title}</h1>
      {line && <p>{line}</p>}
      <div class="actions">
        <button type="button" class="btn ink" onClick={() => location.reload()}>
          Reload
        </button>
        {/* A whole page load, not the router's: the ward reads the record again instead of meeting this fault. */}
        {location.pathname !== "/" && (
          <a class="btn ghost" href="/" target="_self">
            Back to the ward
          </a>
        )}
      </div>
      <details class="fault-cause">
        <summary>What the browser said</summary>
        <p>{cause}</p>
      </details>
    </main>
  );
}

class Boundary extends Component<{ children: ComponentChildren }, { error: string | null }> {
  state = { error: null as string | null };

  static getDerivedStateFromError(e: unknown) {
    return { error: e instanceof Error ? e.message : String(e) };
  }

  render() {
    if (this.state.error === null) return this.props.children;
    return <Fault title="The plate did not load." line="Something broke on this page." cause={this.state.error} />;
  }
}

function Loading({ error }: { error: string | null }) {
  if (error !== null) {
    return <Fault title="The record could not be read just now." cause={error} />;
  }
  return (
    <main class="loading" aria-busy="true">
      <p class="mono">Reading the record</p>
    </main>
  );
}

/** The home page before the record arrives: its words need no data, so they stand where the ward will put them. */
function HomeLoading() {
  return (
    <main class="home" aria-busy="true">
      <section id="ward" class="stage wait">
        <Hero agents={20} line={null} />
      </section>
    </main>
  );
}

/** Seconds since the epoch, read again every second while mounted: the countdown's clock. */
function useNow(): number {
  const [now, setNow] = useState(() => Date.now() / 1000);
  useEffect(() => {
    const t = window.setInterval(() => setNow(Date.now() / 1000), 1000);
    return () => {
      clearInterval(t);
    };
  }, []);
  return now;
}

/**
 * Between genesis and the first hour. The home page is the ward waiting for its first patients, with how an attack runs
 * under it; any other page says when the lab starts and leads back to the ward or into the tour.
 */
function FirstHour({ manifest, home }: { manifest: Manifest; home: boolean }) {
  const now = useNow();
  if (home) {
    return (
      <main class="home">
        <PendingStage manifest={manifest} now={now} />
        <HowItWorks />
      </main>
    );
  }
  const started = now >= manifest.genesis;
  // The day and the time each stay on one line: "1 Jan" or "00:00 UTC" split in two reads as two numbers.
  const [time, day] = startsAt(manifest.genesis, now).split(", ").reverse();
  return (
    <main class="later first">
      <h1>
        The lab {started ? "started" : "starts"} at {day && <span class="unbroken">{day},</span>}{" "}
        <span class="unbroken">{time}.</span>
      </h1>
      <p>The record appears here as soon as its first hour is written.</p>
      <p class="live mono">
        <i class={started ? "beat" : "beat idle"} aria-hidden="true" />
        {firstLine(manifest, now)}
      </p>
      <div class="actions">
        <a class="btn ink" href="/">
          Back to the ward
        </a>
        <button type="button" class="btn ghost" aria-haspopup="dialog" onClick={() => explain.open()}>
          What is this? <span class="mono">30 sec</span>
        </button>
      </div>
    </main>
  );
}

/** Page names for the tab, by route; the home page keeps the site's own title. */
const PAGE = new Map([
  ["record", "Record"],
  ["strains", "Strains"],
  ["genes", "Genes"],
  ["release", "Release a strain"],
  ["defend", "Defend your agent"],
  ["verify", "Verify"],
  ["checkup", "Check-up"],
  ["models", "Both sides"],
]);

/** What the tab says a page is, so history, bookmarks and a screen reader tell pages apart. */
function pageName(lab: Lab | null, path: string): string | null {
  const [route, id, organ] = segments(path);
  if (!route) return null;
  // Before the record is read, a page that names a body or an hour cannot yet tell whether it exists.
  if (!lab && (route === "body" || route === "hour" || route === "card")) return null;
  if (route === "body" && id && lab?.manifest.bodies.some((b) => b.id === id)) {
    const body = bodyName(lab.manifest, id);
    return organ ? `${organ.charAt(0).toUpperCase()}${organ.slice(1)} of ${body}` : body;
  }
  if ((route === "hour" || route === "card") && lab && parseHour(id, lab.hours.length) !== null) {
    return route === "hour" ? `Hour ${id}` : `Card of hour ${id}`;
  }
  if (route === "strain" && id) return id;
  if (route === "gene" && id) return lab?.genes.find((g) => g.id === id)?.name ?? id;
  if (route === "defend" && id && idOf(id)) return `Agent ${idOf(id)}`;
  return id ? "Not found" : (PAGE.get(route) ?? "Not found");
}

/** The title index.html gives the home page. */
const SITE = document.title;

function Routes({ lab, hour, setHour }: { lab: Lab; hour: number; setHour: (n: number) => void }) {
  const path = usePath();
  const [route, id, organ] = segments(path);
  const arrived = useRef(false);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      // An open sheet or lab word takes Esc for itself: it closes and the body page stays.
      if (e.key !== "Escape" || route !== "body" || !id || document.querySelector("dialog[open], :popover-open")) {
        return;
      }
      navigate(organ ? `/body/${id}` : "/");
    };
    addEventListener("keydown", onKey);
    return () => {
      removeEventListener("keydown", onKey);
    };
  }, [route, id, organ]);
  useEffect(() => {
    const at = parseHour(new URLSearchParams(location.search).get("hour"), lab.hours.length);
    if (at !== null) setHour(at);
    // After a move inside the site, not on the first load, the reader starts at the new page's heading.
    if (arrived.current) focusPage();
    arrived.current = true;
  }, [path]);
  if (!route) return <Home lab={lab} hour={hour} setHour={setHour} />;
  if (route === "body" && id && lab.manifest.bodies.some((b) => b.id === id)) {
    return <BodyPage lab={lab} id={id} organ={organ ?? null} hour={hour} setHour={setHour} />;
  }
  const n = parseHour(id, lab.hours.length);
  if (route === "hour" && n !== null) return <HourPage key={n} lab={lab} n={n} />;
  if (route === "record" && !id) return <RecordPage lab={lab} />;
  if (route === "strains" && !id) return <StrainsPage lab={lab} />;
  if (route === "strain" && id) return <StrainPage key={id} lab={lab} id={id} />;
  if (route === "genes" && !id) return <GenesPage lab={lab} />;
  if (route === "gene" && id) return <GenePage key={id} lab={lab} id={id} />;
  if (route === "release" && !id) return <ReleasePage lab={lab} />;
  if (route === "defend" && !id) return <DefendPage lab={lab} />;
  const agent = route === "defend" && id ? idOf(id) : null;
  if (agent) return <DefendResult key={agent} lab={lab} id={agent} />;
  if (route === "verify" && !id) return <VerifyPage lab={lab} />;
  if (route === "checkup" && !id) return <CheckupPage lab={lab} />;
  if (route === "models" && !id) return <ModelsPage lab={lab} />;
  return <NotFound />;
}

export function App() {
  const path = usePath();
  const [lab, setLab] = useState<Lab | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [pending, setPending] = useState<Manifest | null>(null);
  const [hour, setHour] = useState(-1);
  const read = () =>
    loadLab().then(
      (l) => {
        setPending(null);
        setLab(l);
        setHour(l.hours.length - 1);
      },
      (e: unknown) => {
        if (e instanceof FirstHourPending) setPending(e.manifest);
        else setError(e instanceof Error ? e.message : String(e));
      },
    );
  useEffect(() => {
    document.addEventListener("click", onLink);
    void read();
    return () => {
      document.removeEventListener("click", onLink);
    };
  }, []);
  // Once the first hour is due, the waiting page looks for its record every half minute and shows it when written.
  useEffect(() => {
    if (!pending) return;
    const due = pending.genesis + pending.deadline_s;
    const t = window.setInterval(() => {
      if (Date.now() / 1000 >= due) void read();
    }, 30_000);
    return () => {
      clearInterval(t);
    };
  }, [pending]);
  useEffect(() => {
    const name = pageName(lab, path);
    document.title = name ? `${name} ${DOT} Venin` : SITE;
  }, [lab, path]);
  const [route, id] = segments(path);
  // The card is a picture page of its own. Before the first hour it is the site's waiting page, and a card of no
  // hour is the site's not-found page.
  const n = lab ? parseHour(id, lab.hours.length) : null;
  if (route === "card" && !pending && (!lab || n !== null)) {
    return <Boundary>{lab && n !== null ? <CardPage lab={lab} n={n} /> : <Loading error={error} />}</Boundary>;
  }
  return (
    <Boundary>
      <div class="site">
        <Header league={(lab?.manifest ?? pending)?.league ?? null} path={path} />
        {lab && hour >= 0 ? (
          <Routes lab={lab} hour={hour} setHour={setHour} />
        ) : pending ? (
          <FirstHour manifest={pending} home={!route} />
        ) : !route && error === null ? (
          <HomeLoading />
        ) : (
          <Loading error={error} />
        )}
        <Footer />
        <Explainer />
      </div>
    </Boundary>
  );
}
