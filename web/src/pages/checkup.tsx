import { useEffect, useMemo, useRef, useState } from "preact/hooks";
import sample from "../../../library/examples/sample-agent.json?raw";
import { DATA_URL, type Lab } from "../record/load";
import { geneStats, noun, pct } from "../record/stats";
import { type Anatomy, loadAnatomy, ORGAN_ANCHORS } from "../three/anatomy";
import { BodyView, drawStill, FINISHED, labelX, ORGANS, type Phase, spread, warmBody } from "../three/body";
import { PART } from "../three/ink";
import { DOT } from "../ui/sections";
import { Term } from "../ui/terms";
import { dropped, lesions, type Picked, picked, readFolder, scannedBody } from "./checkup-read";
import { answer, core, type Finding, type Organ, type Scan } from "./verify-wasm";
import "./checkup.css";

const ORGAN_LABEL: Record<Organ, string> = {
  eyes: "Eyes",
  ears: "Ears",
  brain: "Brain",
  hands: "Hands",
  heart: "Heart",
};
const ORGAN_NOTE: Record<Organ, string> = {
  eyes: "tool descriptions it reads",
  ears: "other files it reads",
  brain: "its rules and notes",
  hands: "skills, tools and what may run unasked",
  heart: "money it can move",
};
const CHECK_LABEL: Record<Finding["check"], string> = {
  hidden: "Hidden characters",
  comment: "Hidden text",
  order: "Planted order",
  actor: "Acts unasked",
  wide: "Too-wide permission",
  pay: "Unguarded payments",
};

/** From here on this document may only connect to its own site: the dropped folder cannot leave the browser. */
function sealConnections(): void {
  if (document.querySelector("meta[data-checkup]")) return;
  const meta = document.createElement("meta");
  meta.httpEquiv = "Content-Security-Policy";
  meta.content = "connect-src 'self'";
  meta.dataset.checkup = "";
  document.head.append(meta);
}

const reduced = () => matchMedia("(prefers-reduced-motion: reduce)").matches;

/** Chances a rule needs before its block rate is shown: below it, "0% of 3" would read as a verdict. */
const FIRM = 5;

/**
 * Where a cover stands among a lesion's covers: measured rules first, the best first; then rules too little tested to
 * tell, then untested ones; a rule that has had its chances and blocked none comes last.
 */
function coverRank(s: { chances: number; blocked: number } | undefined): number {
  if (!s?.chances) return 2;
  if (s.chances < FIRM) return 1;
  return s.blocked ? -s.blocked / s.chances : 3;
}

function Patient({
  findings,
  onOrgan,
  focus,
}: {
  findings: Finding[];
  onOrgan: (o: Organ) => void;
  focus: Organ | null;
}) {
  const canvas = useRef<HTMLCanvasElement>(null);
  const host = useRef<HTMLDivElement>(null);
  const view = useRef<BodyView | null>(null);
  const spots = useRef<Partial<Record<Organ, HTMLButtonElement | null>>>({});
  const dots = useRef<Partial<Record<Organ, HTMLElement | null>>>({});
  const leads = useRef<Partial<Record<Organ, SVGPolylineElement | null>>>({});
  const per = lesions(findings);
  const state = useMemo(() => scannedBody(findings), [findings]);
  // Only organs with a lesion take ink: a clean brain stays umber even when the hands and heart are hit.
  const inked = ORGANS.filter((o) => per[o].length > 0).map((o) => PART[o]);
  const latest = useRef({ state, inked });
  latest.current = { state, inked };
  /** Without WebGL: "still", the patient drawn once on a 2D canvas; "bare", no anatomy either, the labels alone. */
  const [fallback, setFallback] = useState<"" | "still" | "bare">("");
  const redraw = useRef<(() => void) | null>(null);

  useEffect(() => {
    let alive = true;
    let ro: ResizeObserver | null = null;
    const project = (at: Record<Organ, [number, number]>, phase: Record<Organ, Phase>) => {
      const width = host.current?.clientWidth ?? 0;
      const narrow = width < 520;
      const lx = labelX(at, width, host.current?.clientHeight ?? 0, narrow ? 24 : 60, narrow ? 90 : 150);
      const ys = spread(at, narrow ? 32 : 40);
      for (const o of ORGANS) {
        const [x, y] = at[o];
        dots.current[o]?.style.setProperty("transform", `translate(${x}px, ${y}px)`);
        spots.current[o]?.style.setProperty("transform", `translate(${lx}px, ${ys[o]}px)`);
        leads.current[o]?.setAttribute("points", `${x},${y} ${lx - 14},${ys[o]} ${lx - 4},${ys[o]}`);
        // A label shows once the pencil has drawn its organ.
        for (const el of [dots.current[o], spots.current[o], leads.current[o]])
          el?.style.setProperty("opacity", phase[o].drawn ? "" : "0");
      }
    };
    // Without WebGL the patient is drawn once, finished; without the anatomy its organ buttons stand alone.
    const still = (anatomy: Anatomy | null) => {
      if (!alive) return;
      setFallback(anatomy ? "still" : "bare");
      if (!anatomy) return;
      const anchors = ORGAN_ANCHORS(anatomy);
      void warmBody(anatomy).then((cloud) => {
        const stage = host.current;
        if (!alive || !stage) return;
        redraw.current = () => {
          const el = canvas.current;
          const at = el && drawStill(el, cloud, anchors, latest.current.state, { only: latest.current.inked });
          if (at) project(at, FINISHED);
          else setFallback("bare");
        };
        redraw.current();
        ro = new ResizeObserver(() => redraw.current?.());
        ro.observe(stage);
      });
    };
    loadAnatomy().then(
      (anatomy) => {
        if (!alive || !canvas.current || !host.current) return;
        let v: BodyView;
        try {
          v = new BodyView(canvas.current, anatomy, project);
        } catch {
          still(anatomy);
          return;
        }
        view.current = v;
        const size = () => {
          const r = host.current?.getBoundingClientRect();
          if (r) v.resize(r.width, r.height);
        };
        size();
        ro = new ResizeObserver(size);
        ro.observe(host.current);
        v.focus(null, false, 0);
        v.inkOnly(latest.current.inked);
        // Lesions are findings, not a story: inked from the start, they appear as the pencil draws each organ.
        v.show(latest.current.state, reduced() ? -100 : 0, -100);
      },
      () => still(null),
    );
    return () => {
      alive = false;
      ro?.disconnect();
      redraw.current = null;
      view.current?.dispose();
      view.current = null;
    };
  }, []);

  useEffect(() => {
    view.current?.inkOnly(inked);
    view.current?.show(state, -100, 0);
    redraw.current?.();
  }, [state]);

  return (
    <div class={`checkup-stage${fallback ? ` ${fallback}` : ""}`} ref={host}>
      <canvas ref={canvas} />
      {fallback === "bare" && <p class="stage-note">The figure could not be drawn in this browser.</p>}
      <svg class="leads" aria-hidden="true">
        {ORGANS.map((o) => (
          <polyline
            key={o}
            ref={(el) => {
              leads.current[o] = el;
            }}
            class={per[o].length ? "ink" : ""}
            style={{ opacity: 0 }}
          />
        ))}
      </svg>
      {ORGANS.map((o) => (
        <i
          key={o}
          ref={(el) => {
            dots.current[o] = el;
          }}
          class={`dot${per[o].length ? " ink" : ""}`}
          aria-hidden="true"
          style={{ opacity: 0 }}
        />
      ))}
      {ORGANS.map((o) => (
        <button
          type="button"
          key={o}
          ref={(el) => {
            spots.current[o] = el;
          }}
          class={`spot${per[o].length ? " ink" : ""}${focus === o ? " on" : ""}`}
          style={{ opacity: 0 }}
          onClick={() => onOrgan(o)}
        >
          {ORGAN_LABEL[o]} {per[o].length}
        </button>
      ))}
    </div>
  );
}

export function CheckupPage({ lab }: { lab: Lab }) {
  const [report, setReport] = useState<Scan | null>(null);
  const [name, setName] = useState<string | null>(null);
  const [busy, setBusy] = useState("Reading the sample");
  const [fault, setFault] = useState<string | null>(null);
  const [over, setOver] = useState(false);
  const [focus, setFocus] = useState<Organ | null>(null);
  const input = useRef<HTMLInputElement>(null);
  const stats = useMemo(() => new Map(geneStats(lab.hours, lab.genes).map((s) => [s.gene.id, s])), [lab]);

  const show = async (
    folder: { files: { path: string; content: string }[]; skipped?: string[] },
    label: string | null,
  ) => {
    const c = await core();
    const r = answer<Scan>(c.scan(JSON.stringify({ files: folder.files })));
    r.skipped = [...r.skipped, ...(folder.skipped ?? [])].sort();
    setReport(r);
    setName(label);
    setFocus(null);
    setBusy("");
  };

  const fail = (e: unknown) => {
    setFault(e instanceof Error ? e.message : String(e));
    setBusy("");
  };

  useEffect(() => {
    sealConnections();
    void show(JSON.parse(sample), null).catch(fail);
    return () => {
      if (/^https?:/.test(DATA_URL)) location.reload();
    };
  }, []);

  const open = async (files: Picked[], label: string) => {
    setFault(null);
    setBusy(`Reading ${label} in this browser`);
    try {
      await show(await readFolder(files), label);
    } catch (e) {
      fail(e);
    }
  };

  const onDrop = async (e: DragEvent) => {
    e.preventDefault();
    setOver(false);
    const entries = [...(e.dataTransfer?.items ?? [])]
      .map((i) => i.webkitGetAsEntry())
      .filter((x): x is FileSystemEntry => x !== null);
    if (entries.length === 0) return;
    const files = (await Promise.all(entries.map(dropped))).flat();
    const dir =
      entries.length === 1 && entries[0].isDirectory
        ? entries[0].name
        : `${files.length} ${noun(files.length, "file")}`;
    await open(files, dir);
  };

  const findings = report?.findings ?? [];
  const per = lesions(findings);
  const count = ORGANS.reduce((n, o) => n + per[o].length, 0);
  const shown = focus ? ORGANS.filter((o) => o === focus) : ORGANS.filter((o) => per[o].length > 0);

  return (
    <main class="page checkup-page" data-ready={report && !busy ? "" : undefined}>
      <header class="page-head split">
        <div>
          <h1>Your agent, as a patient</h1>
          <p class="lead">
            For anyone who runs an AI agent. Drop its <Term k="agent-folder">folder</Term>: rules, skills, tool settings
            and <Term k="mcp">MCP config</Term>. The lab's own code reads it here, in your browser. Nothing is uploaded,
            and files named like keys are never opened.
          </p>
        </div>
        <div class="head-actions">
          <button type="button" class="btn ink" onClick={() => input.current?.click()}>
            Pick a folder
          </button>
          <p class="dim">
            From a terminal: <code>venin scan &lt;dir&gt;</code>
          </p>
          <input
            ref={(el) => {
              input.current = el;
              el?.setAttribute("webkitdirectory", "");
            }}
            type="file"
            multiple
            hidden
            onChange={(e) => {
              const list = (e.currentTarget as HTMLInputElement).files;
              if (list?.length) void open(picked(list), list[0].webkitRelativePath.split("/")[0] || "your folder");
            }}
          />
        </div>
      </header>
      <div class="checkup-grid">
        <section
          class={`checkup-body${over ? " over" : ""}${name === null ? " sample" : ""}`}
          onDragOver={(e) => {
            e.preventDefault();
            setOver(true);
          }}
          onDragLeave={() => setOver(false)}
          onDrop={(e) => void onDrop(e)}
          aria-label="The scanned agent as a body"
        >
          <p class="source mono">
            {name === null ? (
              <>
                <b>Sample</b> {DOT} a made-up bookkeeping agent
              </>
            ) : (
              <>
                <b>Your folder</b> {DOT} {name}
              </>
            )}
          </p>
          {report ? (
            <Patient findings={findings} focus={focus} onOrgan={(o) => setFocus(focus === o ? null : o)} />
          ) : (
            <div class="checkup-stage" />
          )}
          {/* A touch screen cannot drop a folder: there the hint points to the pick button. */}
          <p class="drop">
            <b>{matchMedia("(pointer: coarse)").matches ? "Pick" : "Drop"} your folder</b>
            <span class="mono">read here, never uploaded</span>
          </p>
        </section>
        <section class="checkup-findings" aria-live="polite">
          {busy && <p class="verdict busy">{busy}...</p>}
          {fault && <p class="verdict bad">The check-up stopped: {fault}</p>}
          {report && !busy && (
            <>
              <p class={`verdict ${count ? "bad" : "good"}`}>
                {count ? `${count} ` : "No "}
                <Term k="lesion">{noun(count, "lesion")}</Term> in {report.read} {noun(report.read, "file")}
                {report.skipped.length ? ` ${DOT} ${report.skipped.length} key files left unread` : ""}
              </p>
              {focus && (
                <button type="button" class="btn ghost small" onClick={() => setFocus(null)}>
                  Show every organ
                </button>
              )}
              {shown.map((o) => (
                <div key={o} class="organ-group">
                  <h2>
                    {ORGAN_LABEL[o]} <span class="mono dim">{ORGAN_NOTE[o]}</span>
                  </h2>
                  {per[o].length === 0 && <p class="dim">Nothing found here.</p>}
                  <ol class="lesions">
                    {per[o].map((f) => (
                      <li key={`${f.check}:${f.file}:${f.line}`}>
                        <p class="mono dim">
                          {CHECK_LABEL[f.check]}
                          <span class="sep"> {DOT} </span>
                          <span class="where">
                            {f.file}
                            {f.line ? `:${f.line}` : ""}
                          </span>
                        </p>
                        <p>{f.says}</p>
                        {f.causes.length > 0 && (
                          <ul class="causes">
                            {f.causes.map((c) => (
                              <li key={c}>{c}</li>
                            ))}
                          </ul>
                        )}
                        <p class="covers">
                          {[...f.genes]
                            .sort((a, b) => coverRank(stats.get(a)) - coverRank(stats.get(b)))
                            .map((id) => {
                              const s = stats.get(id);
                              // Each rule by its code and name; its rate only once it has been tried enough to tell.
                              const idle = Boolean(s && s.chances >= FIRM && !s.blocked);
                              const rate = !s?.chances
                                ? "not tested yet"
                                : s.chances < FIRM
                                  ? `tested ${s.chances} ${noun(s.chances, "time")}`
                                  : idle
                                    ? "has not blocked one yet"
                                    : `blocks ${pct(s.blocked / s.chances)} of ${s.chances}`;
                              return (
                                <a key={id} class={`chip gene-chip${idle ? " idle" : ""}`} href={`/gene/${id}`}>
                                  <span class="mono">{id}</span> {s?.gene.name}
                                  <small class="dim">{rate}</small>
                                </a>
                              );
                            })}
                        </p>
                      </li>
                    ))}
                  </ol>
                </div>
              ))}
              {report.skipped.length > 0 && <p class="mono dim skipped">Left unread: {report.skipped.join(", ")}</p>}
            </>
          )}
        </section>
      </div>
    </main>
  );
}
