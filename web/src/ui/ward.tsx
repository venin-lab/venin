import { useEffect, useMemo, useRef, useState } from "preact/hooks";
import type { Lab } from "../record/load";
import { type BodyState, findStrain, focal, heldBy, noun, ROW_LABEL, ROWS } from "../record/stats";
import type { HourRecord, Manifest, Row } from "../record/types";
import { hourState } from "../record/views";
import { type Anatomy, buildCloud, type Cloud, loadAnatomy } from "../three/anatomy";
import { warmBody } from "../three/body";
import { reachTime } from "../three/ink";
import { slots } from "../three/ledger";
import { type Bed, type Figure, figureState, type OnScreen, placeOnBed, Ward } from "../three/ward";
import { explain } from "./explain";
import { Ledger } from "./ledger";
import { navigate } from "./router";
import { CA, countdown, DOT, Explore, firstLine, startsAt, TokenLine } from "./sections";
import { Term, type TermKey, TICKER } from "./terms";

/**
 * `opened` once the opening has played in this page load. `open`: the patient the ward just opened, as it will be on
 * screen when its page takes over; `back`: a patient a page handed back, standing where it was (null from a close-up).
 */
const session = {
  opened: false,
  open: null as (OnScreen & { id: string; at: number }) | null,
  back: null as { id: string; at: number; figure: OnScreen | null } | null,
};

/** The patient the ward opened less than 1.5 s ago, if it is `id`; once. */
export function takeOpen(id: string): OnScreen | null {
  const o = session.open;
  session.open = null;
  return o && o.id === id && performance.now() - o.at < 1500 ? o : null;
}

/** A page hands its patient back; the ward lays it down on its bed if it comes back within 2 s. */
export const markReturn = (id: string, figure: OnScreen | null) => {
  session.back = { id, at: performance.now(), figure };
};

let cloud: Cloud | null = null;

/** The ward's shared low-density body, built once per page load: about 7.5k points, so a phone draws it whole. */
export function wardCloud(anatomy: Anatomy): Cloud {
  cloud ??= buildCloud(anatomy, false);
  return cloud;
}

/** Places each patient on its bed: `[data-bed]` boxes inside `host`, in CSS pixels from its top-left corner. */
export function measureBeds(w: Ward, host: HTMLElement, bodies: { id: string }[]): void {
  const box = host.getBoundingClientRect();
  const beds: Bed[] = bodies.map((b) => {
    const r = host.querySelector(`[data-bed="${b.id}"]`)?.getBoundingClientRect();
    return r
      ? { x: r.left - box.left + r.width / 2, y: r.top - box.top + r.height / 2, length: r.width, height: r.height }
      : { x: -999, y: -999, length: 1, height: 1 };
  });
  w.resize(box.width, box.height, beds);
}

/** A bed's pill: the state, and for a hold the gene's code; Priors' own guard has no code worth showing there. */
export function pillText(s: BodyState): string {
  if (s.pill === "held") return s.gene && !s.gene.startsWith("priors:") ? `HELD ${s.gene}` : "HELD";
  return s.pill.toUpperCase();
}

/** An hour in one line; `exam` false leaves the exam mark to a label next to the line. */
export function liveLine(hour: HourRecord, exam = true): string {
  const attacks = hour.outcomes.filter((o) => o.kind === "attack");
  const breached = attacks.filter((o) => o.outcome.breached).length;
  if (!hour.strain) return `Hour ${hour.n} ${DOT} void ${DOT} no run counted`;
  return `Hour ${hour.n} ${DOT} ${hour.strain} ${DOT} ${breached} of ${attacks.length} breached${exam && hour.exam ? ` ${DOT} exam` : ""}`;
}

/** What carried the strain, in words a newcomer knows. */
const CARRIER: Record<string, string> = {
  email: "a scam email",
  page: "a poisoned web page",
  tool_description: "a poisoned tool description",
  memory_note: "a planted memory note",
  skill_file: "a poisoned skill file",
};

/** The hero's live line: the hour, what carried its scam and how many agents it breached, with no codes. */
export function heroLine(lab: Pick<Lab, "strains" | "hours">, h: HourRecord): string {
  const s = hourState(h);
  if (s.kind === "sealed") return `Hour ${h.n} ${DOT} sealed for review`;
  if (s.kind === "void" || !h.strain) return `Hour ${h.n} ${DOT} did not run`;
  const attacks = h.outcomes.filter((o) => o.kind === "attack");
  const breached = attacks.filter((o) => o.outcome.breached).length;
  const carrier = CARRIER[findStrain(h.strain, lab.strains, lab.hours)?.carrier ?? ""] ?? "a hidden scam";
  return `Hour ${h.n} ${DOT} ${carrier} ${DOT} ${breached} of ${attacks.length} breached`;
}

/** The lab word each ward row's label defines. */
const ROW_TERM: Record<Row, TermKey> = {
  none: "none",
  textbook: "textbook",
  priors: "priors",
  venin1: "venin",
  venin2: "venin",
};

const reduced = () => matchMedia("(prefers-reduced-motion: reduce)").matches;

/** CSS pixels of the stage between the sticky header and the bottom of the window, from the stage's top. */
const visibleHeight = () => innerHeight - (document.querySelector(".top")?.getBoundingClientRect().height ?? 64);

export function WardStage({ lab, hour, setHour }: { lab: Lab; hour: number; setHour: (n: number) => void }) {
  const stage = useRef<HTMLElement>(null);
  const canvas = useRef<HTMLCanvasElement>(null);
  const ward = useRef<Ward | null>(null);
  const [phase, setPhase] = useState<"wait" | "opening" | "open">(session.opened ? "open" : "wait");
  const [flat, setFlat] = useState(false);
  const bodies = lab.manifest.bodies;
  const record = lab.hours[hour];
  const latest = lab.hours[lab.hours.length - 1];
  const states = useMemo(() => bodies.map((b) => figureState(record, b.id)), [record, bodies]);
  const focalBody = useMemo(() => focal(lab.hours)?.body ?? bodies[0].id, [lab.hours, bodies]);
  const statesRef = useRef(states);
  statesRef.current = states;

  useEffect(() => {
    let disposed = false;
    let timers: number[] = [];
    let resize: ResizeObserver | null = null;
    const finishers: [string, EventListener][] = [];
    const build = (anatomy: Anatomy) => {
      if (disposed || !canvas.current || !stage.current) return;
      const w = new Ward(canvas.current, wardCloud(anatomy), bodies.length);
      ward.current = w;
      const measure = () => {
        if (stage.current) measureBeds(w, stage.current, bodies);
      };
      measure();
      const home = () => w.homeView();
      const close = (id: string, share: number) =>
        w.closeUp(
          bodies.findIndex((b) => b.id === id),
          visibleHeight(),
          share,
        );
      // Only a real resize re-centres the camera: the observer's first call, at once, would cut a return's fly-out.
      let box = `${stage.current.clientWidth}x${stage.current.clientHeight}`;
      resize = new ResizeObserver(() => {
        measure();
        const now = `${stage.current?.clientWidth}x${stage.current?.clientHeight}`;
        if (session.opened && now !== box) w.fly(home(), 0);
        box = now;
      });
      resize.observe(stage.current);
      w.setStates(statesRef.current, -10);

      if (session.opened || reduced()) {
        w.finish();
        const back = session.back && performance.now() - session.back.at < 2000 ? session.back : null;
        const bi = back ? bodies.findIndex((b) => b.id === back.id) : -1;
        session.back = null;
        // The words wait while a patient comes back: its neighbours fly in large, under where the hero stands.
        const returning = (ms: number) => {
          stage.current?.classList.add("returning");
          timers.push(window.setTimeout(() => stage.current?.classList.remove("returning"), ms));
        };
        if (back?.figure && bi >= 0 && !reduced()) {
          // The patient stands where its page left it, lies back down on its bed, and the camera pulls back.
          returning(1100);
          w.fly(w.standView(bi, back.figure, canvas.current.getBoundingClientRect()), 0);
          w.pose(bi, back.figure.roll);
          w.warm(bi, 1, 0, 1100);
          w.fly(home(), 1100);
        } else if (back && bi >= 0 && !reduced()) {
          returning(900);
          w.fly(close(back.id, 0.8), 0);
          w.warm(bi, 1, 0, 900);
          w.fly(home(), 900);
        } else w.fly(home(), 0);
        session.opened = true;
        setPhase("open");
        void warmBody(anatomy);
        return;
      }

      const fi = bodies.findIndex((b) => b.id === focalBody);
      const fb = w.beds[fi];
      // The story plays under the words: framed in the part of the window below the hero, when that leaves room.
      const hero = stage.current.querySelector(".hero")?.getBoundingClientRect().bottom ?? 0;
      const below = hero - canvas.current.getBoundingClientRect().top;
      const top = visibleHeight() - below > 160 ? below : 0;
      const focus = (share: number) => w.closeUp(fi, visibleHeight() - top, share, top);
      // Until the ward stands, nothing is drawn behind the words: the neighbours come in large as the camera pulls back.
      stage.current.style.setProperty("--clip", `${Math.max(0, top)}px`);
      const dist = (i: number) => Math.hypot(w.beds[i].x - fb.x, w.beds[i].y - fb.y);
      const rank = bodies.map((_, i) => i).sort((a, b) => dist(a) - dist(b));
      // The focal patient's whole story, then a beat on it finished, before the camera pulls back to the ward.
      const fs = statesRef.current[fi];
      const story = fs.pill === "held" ? reachTime(5) + 0.75 : Math.max(0, reachTime(fs.reach));
      const flyAt = Math.min(4.4, Math.max(3.2, 1.5 + story / 1.4 + 0.35));
      w.setStates(statesRef.current, 0);
      // The neighbours draw themselves in the second before the pull-back, so they are whole when they come in.
      w.sketch(
        bodies.map((_, i) => (i === fi ? 0 : flyAt - 0.9 + 0.04 * rank.indexOf(i))),
        bodies.map((_, i) => (i === fi ? 1.5 : 0.9)),
        bodies.map((_, i) => (i === fi ? 1.4 : 2.4)),
      );
      w.fly(focus(0.68), 0);
      setPhase("opening");
      const done = () => {
        if (session.opened) return;
        session.opened = true;
        for (const t of timers) clearTimeout(t);
        timers = [];
        void warmBody(anatomy);
        setPhase("open");
      };
      const skip = () => {
        if (session.opened) return;
        w.finish();
        w.fly(home(), 0);
        done();
      };
      timers.push(window.setTimeout(() => w.fly(focus(0.72), (flyAt - 1.5) * 1000), 1500));
      timers.push(window.setTimeout(() => w.fly(home(), 1800), flyAt * 1000));
      timers.push(window.setTimeout(done, (flyAt + 1.2) * 1000));
      for (const type of ["wheel", "touchmove", "keydown"]) {
        finishers.push([type, skip]);
        addEventListener(type, skip, { passive: true });
      }
    };
    loadAnatomy()
      .then(build)
      .catch(() => {
        // No WebGL, or no anatomy: the ward stands as its grid, each bed's pill saying in words what happened.
        if (disposed) return;
        session.opened = true;
        setFlat(true);
        setPhase("open");
      });
    return () => {
      disposed = true;
      for (const t of timers) clearTimeout(t);
      for (const [type, fn] of finishers) removeEventListener(type, fn);
      resize?.disconnect();
      ward.current?.dispose();
      ward.current = null;
    };
  }, []);

  useEffect(() => {
    if (session.opened) ward.current?.setStates(states, -0.25);
  }, [states]);

  // A mouse over the beds leans the patients toward it (three/ward.ts); one over the headline, the actions or the
  // scrubber leaves them straight, and so do the ward opening, leaving for a page and taking a patient back.
  const lookAt = (e: PointerEvent) => {
    const el = stage.current;
    const w = ward.current;
    if (!el || !w || e.pointerType !== "mouse") return;
    if (!el.classList.contains("open") || el.classList.contains("leaving") || el.classList.contains("returning")) {
      w.look(0, 0);
      return;
    }
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    w.look(((e.clientX - r.left) / r.width) * 2 - 1, ((e.clientY - r.top) / r.height) * 2 - 1);
  };

  const open = (id: string) => {
    const w = ward.current;
    const i = bodies.findIndex((b) => b.id === id);
    w?.look(0, 0);
    if (w?.beds[i] && canvas.current) {
      // The patient comes forward in the part of the stage on screen: a phone opens beds far down the page.
      const r = canvas.current.getBoundingClientRect();
      const top = Math.max(r.top, innerHeight - visibleHeight());
      const shown = Math.min(r.bottom, innerHeight) - top;
      const to = w.closeUp(i, shown, 0.8, top - r.top);
      w.fly(to, 520);
      w.solo(i, 400);
      w.warm(i, 0, 1, 520);
      // Its page picks the patient up where the fly-in leaves it, still rolling.
      session.open = {
        id,
        at: performance.now(),
        cx: r.left + r.width / 2,
        cy: top + shown / 2,
        length: 2 * placeOnBed(w.beds[i]).scale * to.zoom,
        roll: w.rollAt(i, 0.48),
      };
    }
    stage.current?.classList.add("leaving");
    window.setTimeout(
      () => {
        navigate(`/body/${id}`);
        window.scrollTo(0, 0);
      },
      reduced() ? 0 : 480,
    );
  };

  return (
    <section id="ward" class={`stage ${phase}`} ref={stage}>
      <canvas class="stage-canvas" ref={canvas} />
      <Hero agents={bodies.length} line={heroLine(lab, latest)} lab={lab} />
      <WardGrid
        lab={lab}
        record={record}
        states={states}
        focalBody={focalBody}
        open={open}
        onPointerMove={lookAt}
        onPointerLeave={() => ward.current?.look(0, 0)}
      />
      <Ledger hours={lab.hours} hour={hour} setHour={setHour} states={states} />
      {flat && <FlatNote />}
    </section>
  );
}

/** Why the beds are empty when the patients cannot be drawn: the grid still says everything the figures would. */
function FlatNote() {
  return <p class="flat-note">The patients could not be drawn in this browser; each bed's pill says what happened.</p>;
}

/**
 * The ward before its first hour. The hero counts down to hour 0 in UTC; the twenty patients lie at rest on their beds,
 * pencil with no stain, and each pill's place is drawn dashed, as everything not yet happened is; the strip holds the
 * first day's hours, dashed too. The agents exist from genesis; only their record does not. Without WebGL the beds
 * are drawn dashed as well.
 */
export function PendingStage({ manifest, now }: { manifest: Manifest; now: number }) {
  const host = useRef<HTMLElement>(null);
  const canvas = useRef<HTMLCanvasElement>(null);
  const rest = useMemo(
    () => manifest.bodies.map((b): Figure => ({ body: b.id, pill: "resisted", gene: null, reach: 0, route: null })),
    [manifest],
  );
  const flat = usePlate(host, canvas, manifest.bodies, rest, { still: false, density: 1, onReady: undefined });
  return (
    <section id="ward" class={`stage open pending${flat ? " flat" : ""}`} ref={host}>
      <canvas class="stage-canvas" ref={canvas} />
      <Hero agents={manifest.bodies.length} line={firstLine(manifest, now)} idle={now < manifest.genesis} />
      <WardGrid lab={{ manifest, genes: [] }} record={null} states={null} focalBody={null} />
      <div class="ledger">
        <div class="ledger-body">
          <div class="ledger-head mono">
            <span>
              Hour <b>0</b> {DOT} first <Term k="draw">draw</Term> at {startsAt(manifest.genesis, now)}
            </span>
            <span class="dim">On the record by {startsAt(manifest.genesis + manifest.deadline_s, now)}</span>
          </div>
          <div class="ticks" aria-hidden="true">
            {Array.from({ length: slots(0) }, (_, i) => (
              <i key={`next-${i}`} class="tick next" />
            ))}
          </div>
        </div>
      </div>
    </section>
  );
}

/**
 * The headline, the lab in one sentence with the way into the tour, the latest hour's line and the two actions. With
 * `line` null (the record still loading) the line says so; everything else needs no data, so it paints at once.
 * `idle`: the lab has not started, so its beat is an empty ring that does not pulse.
 */
export function Hero({
  agents,
  line,
  idle = false,
  lab,
}: {
  agents: number;
  line: string | null;
  idle?: boolean;
  lab?: Lab;
}) {
  const parts = line ? line.split(` ${DOT} `) : ["Reading the record"];
  const last = parts.length - 1;
  return (
    <div class="hero">
      <div class="hero-main">
        <h1>
          <span>AI agents get poisoned here every hour.</span>{" "}
          <span class="eosin">The survivors make the antidote.</span>
        </h1>
        <p class="hero-plain">
          Every hour a hidden scam tries to make{" "}
          <span class="unbroken">
            {agents} <Term k="ai-agent">AI agents</Term>
          </span>{" "}
          hand over <b class="unbroken">money or secrets</b>.{" "}
          {/* The second sentence and the way into the tour break as one, so the button never starts a line alone. */}
          <span class="hero-tail">
            <span class="unbroken">The lab records which ones do.</span>{" "}
            <button
              type="button"
              class="btn ghost small hero-what"
              aria-haspopup="dialog"
              onClick={() => explain.open()}
            >
              What is this? <span class="mono">30 sec</span>
            </button>
          </span>
        </p>
        <TokenLine />
      </div>
      <div class="hero-side">
        <p class="live mono">
          <i class={idle ? "beat idle" : "beat"} aria-hidden="true" />
          {/*
           * A long line breaks between its parts, never inside one, and each dot ends the part before it, so no line
           * starts with a dot. A phone sets the count on a line of its own.
           */}
          <span>
            {parts.flatMap((part, i) => [
              i ? " " : "",
              <span key={part} class={i === last && last === 2 ? "unbroken count" : "unbroken"}>
                {part}
                {i < last && <span class={i === last - 1 ? "sep end" : "sep"}> {DOT}</span>}
              </span>,
            ])}
          </span>
        </p>
        <div class="hero-actions">
          <a class="btn ink" href="/checkup">
            Check your agent
          </a>
          <a class="btn eosin" href="/defend">
            Defend your agent
          </a>
          <a class="btn ghost" href="/release">
            Release a strain
          </a>
        </div>
        <p class="hero-safe">Everything stays in the lab: test agents, fake targets.</p>
        {lab && <Pulse lab={lab} />}
      </div>
    </div>
  );
}

/** One release's burn in whole tokens, from the build; 0 when the build has none, and the burn line is left out. */
const BURN = BigInt((import.meta.env.VITE_RELEASE_AMOUNT as string | undefined) || "0") / 10n ** 18n;

/** Seconds since the epoch, read again every second while mounted. */
function useClock(): number {
  const [now, setNow] = useState(() => Date.now() / 1000);
  useEffect(() => {
    const t = window.setInterval(() => setNow(Date.now() / 1000), 1000);
    return () => clearInterval(t);
  }, []);
  return now;
}

/**
 * The lab's pulse under the actions, all read from the record: the last hour on chain with its anchor, the next draw,
 * and the last day's releases, their burns and the variants of concern the ward named.
 */
function Pulse({ lab }: { lab: Lab }) {
  const now = useClock();
  const anchor = [...lab.anchors].reverse().find((a) => a.kind === "hour");
  const next = lab.manifest.genesis + (lab.head.n + 1) * lab.manifest.period_s;
  const day = lab.hours.filter((h) => h.start > now - 86400);
  const runs = day.flatMap((h) => h.holders ?? []).filter((x) => x.status === "run").length;
  const ward = (lab.hours.at(-1)?.state.defending ?? []).filter((d) => d.from + 24 > lab.head.n + 1).length;
  const named = day.flatMap((h) => h.events ?? []).flatMap((e) => (e.type === "concern" ? [e.name] : []));
  const ago = Math.floor((now - lab.head.written_s) / 60);
  return (
    <ul class="pulse mono">
      <li>
        <span class="k">On chain</span> hour {lab.head.n}, {ago < 1 ? "this minute" : `${ago} min ago`}
        {anchor && (
          <>
            {" "}
            {DOT}{" "}
            <Explore chain={4663} path={`tx/${anchor.tx}`}>
              <span class="hex">
                tx {anchor.tx.slice(0, 8)}...{anchor.tx.slice(-4)}
              </span>
            </Explore>
          </>
        )}
      </li>
      <li>
        <span class="k">Next draw</span> {next > now ? `in ${countdown(next - now)}` : "now"}
      </li>
      {runs > 0 && (
        <li>
          <span class="k">Last 24 h</span> {runs} {noun(runs, "release")}
          {BURN > 0n && ` ${DOT} ${(BigInt(runs) * BURN).toLocaleString("en-US")} ${TICKER} burned`}
        </li>
      )}
      {named.length > 0 && (
        <li>
          <span class="k">New variants</span> {named.join(", ")}
        </li>
      )}
      {ward > 0 && (
        <li>
          <span class="k">Defend</span>{" "}
          <a href="/defend">
            {ward} {noun(ward, "agent")} in the ward now
          </a>
        </li>
      )}
      {CA && (
        <li>
          <span class="k">Trading</span> Uniswap {DOT}{" "}
          <a href={`https://dexscreener.com/robinhood/${CA}`} target="_blank" rel="noopener noreferrer">
            chart
          </a>
        </li>
      )}
    </ul>
  );
}

/**
 * The 4x5 ward: model columns, defence rows, a bed and a state pill per patient. Beds carry `data-bed` for the 3D
 * layer; a cell opens the body through `open`, links to `href`, or, with neither, is a picture.
 */
export function WardGrid({
  lab,
  record,
  states,
  focalBody,
  open,
  href,
  plain = false,
  onPointerMove,
  onPointerLeave,
}: {
  lab: Pick<Lab, "manifest" | "genes">;
  /** Null before the first hour: every bed waits empty, its pill drawn dashed. */
  record: HourRecord | null;
  states: BodyState[] | null;
  focalBody: string | null;
  open?: (id: string) => void;
  href?: (id: string) => string;
  /** A picture (the share card): lab words print plain, with nothing to tap. */
  plain?: boolean;
  onPointerMove?: (e: PointerEvent) => void;
  onPointerLeave?: () => void;
}) {
  const bodies = lab.manifest.bodies;
  const word = (k: TermKey, text: string) => (plain ? text : <Term k={k}>{text}</Term>);
  const model = (column: string) => lab.manifest.models.find((m) => m.column === column);
  const fixed = record?.state.genomes["m1-textbook"]?.length;
  const rowNote: Record<string, string> = {
    none: "bare model",
    textbook: fixed === undefined ? "fixed rules" : `${fixed} fixed ${noun(fixed, "rule")}`,
    priors: "outside guard",
    venin1: "evolving",
    venin2: "evolving",
  };
  /**
   * A state in words for a screen reader: a hold names the rule that held, which the pill only codes. The name starts
   * with the pill's own words, so a voice command can say what the eye sees.
   */
  const said = (s: BodyState) => {
    if (s.pill !== "held" || !s.gene) return s.pill;
    if (s.gene.startsWith("priors:")) return `held by ${heldBy(s.gene)}`;
    const name = lab.genes.find((g) => g.id === s.gene)?.name;
    return `held ${s.gene}${name ? `, by gene ${name}` : ""}`;
  };
  return (
    <div class="ward" onPointerMove={onPointerMove} onPointerLeave={onPointerLeave}>
      <div class="ward-corner mono">
        <span>Rows: {word("defence", "defence")}</span>
        <span>Columns: {word("model", "AI model")}</span>
      </div>
      {lab.manifest.models.map((m) => (
        <div key={m.column} class="ward-col mono">
          <b>{m.label}</b>
        </div>
      ))}
      {ROWS.map((row) => (
        <>
          <div key={row} class="ward-row">
            <b>{word(ROW_TERM[row], ROW_LABEL[row])}</b>
            <span class="mono dim">{rowNote[row]}</span>
          </div>
          {lab.manifest.models.map((m) => {
            const id = `${m.column}-${row}`;
            if (!states) {
              return (
                <div key={id} class="cell" aria-hidden="true">
                  <span class="bed" data-bed={id}>
                    <i class="pillow" />
                  </span>
                  <span class="pill next" />
                </div>
              );
            }
            const s = states[bodies.findIndex((b) => b.id === id)];
            if (!s) return null;
            const label = `${model(m.column)?.label}, ${ROW_LABEL[row]}: ${said(s)}`;
            const inner = (
              <>
                <span class="bed" data-bed={id}>
                  <i class="pillow" />
                </span>
                <span class={`pill ${s.pill}`}>{pillText(s)}</span>
              </>
            );
            // The focal bed's outline takes the legend's colour for that body's own state.
            const cls = `cell${id === focalBody ? ` focal ${s.pill}` : ""}`;
            if (href) {
              return (
                <a key={id} class={cls} href={href(id)} aria-label={`${label}. Open the body.`}>
                  {inner}
                </a>
              );
            }
            if (open) {
              return (
                <button
                  key={id}
                  type="button"
                  class={cls}
                  onClick={() => open(id)}
                  aria-label={`${label}. Open the body.`}
                >
                  {inner}
                </button>
              );
            }
            return (
              <div key={id} class={cls} role="img" aria-label={label}>
                {inner}
              </div>
            );
          })}
        </>
      ))}
    </div>
  );
}

/**
 * Lays `states` on the beds inside `host`, drawn on `canvas`: the patients sketch themselves in, or, `still`, appear
 * finished; a change of states inks again. `onReady` fires once they are drawn. Returns true when they cannot be (no
 * WebGL, or no anatomy): the grid then says in words what the figures would, and `onReady` fires at once.
 */
function usePlate(
  host: { current: HTMLElement | null },
  canvas: { current: HTMLCanvasElement | null },
  bodies: { id: string }[],
  states: Figure[],
  { still, density, onReady }: { still: boolean; density: number; onReady: (() => void) | undefined },
): boolean {
  const ward = useRef<Ward | null>(null);
  const [flat, setFlat] = useState(false);
  const statesRef = useRef(states);
  statesRef.current = states;

  useEffect(() => {
    let disposed = false;
    let resize: ResizeObserver | null = null;
    loadAnatomy()
      .then((anatomy) => {
        if (disposed || !canvas.current || !host.current) return;
        const body = density < 1 ? buildCloud(anatomy, false, density) : wardCloud(anatomy);
        const w = new Ward(canvas.current, body, bodies.length);
        ward.current = w;
        const measure = () => {
          if (!host.current) return;
          measureBeds(w, host.current, bodies);
          w.fly(w.homeView(), 0);
        };
        measure();
        resize = new ResizeObserver(measure);
        resize.observe(host.current);
        // A still, or reduced motion, lands every patient finished at once.
        if (still) w.still();
        w.setStates(statesRef.current, 0);
        w.sketch(
          bodies.map((_, i) => 0.05 * i),
          bodies.map(() => 0.9),
          bodies.map(() => 2.4),
        );
        requestAnimationFrame(() =>
          requestAnimationFrame(() => {
            if (!disposed) onReady?.();
          }),
        );
      })
      .catch(() => {
        if (disposed) return;
        setFlat(true);
        onReady?.();
      });
    return () => {
      disposed = true;
      resize?.disconnect();
      ward.current?.dispose();
      ward.current = null;
    };
  }, []);

  useEffect(() => {
    ward.current?.setStates(states, -0.25);
  }, [states]);

  return flat;
}

/**
 * The ward of one hour on its own plate: patients sketch in and ink their real states, or, `still`, appear finished.
 * `onReady` fires once the patients are drawn; without WebGL or the anatomy the plate is its grid, pills in words.
 */
export function WardPlate({
  lab,
  record,
  focalBody,
  href,
  still = false,
  density = 1,
  onReady,
  class: cls = "",
}: {
  lab: Lab;
  record: HourRecord;
  focalBody: string | null;
  href?: (id: string) => string;
  still?: boolean;
  /** Under 1, a sparser body of its own: a plate shown small would pile the full cloud's points into dark shapes. */
  density?: number;
  onReady?: () => void;
  class?: string;
}) {
  const host = useRef<HTMLDivElement>(null);
  const canvas = useRef<HTMLCanvasElement>(null);
  const bodies = lab.manifest.bodies;
  const states = useMemo(() => bodies.map((b) => figureState(record, b.id)), [record, bodies]);
  const flat = usePlate(host, canvas, bodies, states, { still, density, onReady });

  // A still is the share card's picture of the ward: its words say what happened, so a reader skips the grid.
  return (
    <div class={`ward-plate ${cls}`} ref={host} aria-hidden={still || undefined}>
      <canvas class="stage-canvas" ref={canvas} />
      <WardGrid
        lab={lab}
        record={record}
        states={states}
        focalBody={focalBody}
        plain={still}
        {...(href ? { href } : {})}
      />
      {flat && <FlatNote />}
    </div>
  );
}
