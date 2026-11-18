import { useEffect, useMemo, useRef, useState } from "preact/hooks";
import type { BodyState } from "../record/stats";
import type { HourRecord } from "../record/types";
import { hourState } from "../record/views";
import { indexAt, playIndex, slots, tick } from "../three/ledger";
import { DOT, tileCounts } from "./sections";
import { Term } from "./terms";

/** The slider's keys, as a slider's keys go: a step, five steps a page, and the two ends. */
const KEYS = new Map<string, (hour: number, last: number) => number>([
  ["ArrowLeft", (h) => h - 1],
  ["ArrowDown", (h) => h - 1],
  ["ArrowRight", (h) => h + 1],
  ["ArrowUp", (h) => h + 1],
  ["PageDown", (h) => h - 5],
  ["PageUp", (h) => h + 5],
  ["Home", () => 0],
  ["End", (_, last) => last],
]);

/**
 * One tick per period; drag to recolour the ward through history, play runs the record in about 20 s. The head
 * counts the ward's `states` for the hour shown, all twenty, in the legend's colours. A tick is grey unless the hour
 * breached a defended agent, then pink by how many; exam hours carry a graphite mark under the tick.
 */
export function Ledger({
  hours,
  hour,
  setHour,
  states,
}: {
  hours: HourRecord[];
  hour: number;
  setHour: (n: number) => void;
  states: BodyState[];
}) {
  const ticks = useMemo(() => hours.map(tick), [hours]);
  const strip = useRef<HTMLDivElement>(null);
  const [playing, setPlaying] = useState(false);
  const last = hours.length - 1;
  const width = slots(hours.length);

  useEffect(() => {
    if (!playing) return;
    const t0 = performance.now();
    let id = 0;
    const step = () => {
      const i = playIndex((performance.now() - t0) / 1000, hours.length);
      setHour(i);
      if (i >= last) setPlaying(false);
      else id = requestAnimationFrame(step);
    };
    id = requestAnimationFrame(step);
    return () => {
      cancelAnimationFrame(id);
    };
  }, [playing]);

  const scrub = (e: PointerEvent) => {
    const el = strip.current;
    if (!el) return;
    const r = el.getBoundingClientRect();
    setPlaying(false);
    setHour(Math.min(last, indexAt(e.clientX - r.left, r.width, width)));
  };

  const t = ticks[hour];
  const h = hours[hour];
  const kind = hourState(h).kind;
  const breached = states.filter((s) => s.pill === "breached").length;
  const told =
    kind === "ok" ? `${breached} of ${states.length} breached` : kind === "void" ? "did not run" : "sealed for review";
  return (
    <div class="ledger">
      <button
        type="button"
        class={`btn ink small play${playing ? " on" : ""}`}
        disabled={last < 1}
        onClick={() => setPlaying(!playing)}
        aria-pressed={playing}
      >
        {playing ? "Pause" : "Play past hours"}
      </button>
      <div class="ledger-body">
        <div class="ledger-head mono">
          {/* No strain code here: the hero says what carried the scam, and the hour page names it. */}
          <span>
            Hour <b>{t.n}</b> {DOT} {hour === last ? "latest" : `of ${last}`}
            {t.exam && (
              <em class="exam-tag">
                <Term k="exam">EXAM</Term>
              </em>
            )}
          </span>
          {kind === "ok" ? (
            <span class="ledger-tally">
              {tileCounts(states).map(([pill, n]) => (
                <span key={pill} class="unbroken">
                  <i class={`sw ${pill}`} aria-hidden="true" />
                  {n} <Term k={pill}>{pill}</Term>
                </span>
              ))}
            </span>
          ) : (
            <span class="dim">
              {kind === "void" ? <Term k="void">Did not run</Term> : <Term k="sealed">Sealed for review</Term>}
            </span>
          )}
        </div>
        <div
          class="ticks"
          ref={strip}
          role="slider"
          tabIndex={0}
          aria-label="Hour of the record"
          aria-valuemin={0}
          aria-valuemax={last}
          aria-valuenow={hour}
          aria-valuetext={`Hour ${t.n}${t.exam ? ", exam" : ""}, ${told}`}
          onPointerDown={(e) => {
            (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
            scrub(e);
          }}
          onPointerMove={(e) => {
            if ((e.currentTarget as HTMLElement).hasPointerCapture(e.pointerId)) scrub(e);
          }}
          onKeyDown={(e) => {
            const to = KEYS.get(e.key)?.(hour, last);
            if (to === undefined) return;
            e.preventDefault();
            setPlaying(false);
            setHour(Math.max(0, Math.min(last, to)));
          }}
        >
          {ticks.map((k, i) => (
            <i
              key={k.n}
              class={`tick${k.void ? " void" : ""}${k.hit ? " hit" : ""}${k.exam ? " exam" : ""}${i === hour ? " at" : ""}`}
              style={{ "--b": k.defended ? k.breached / k.defended : 0, "--h": k.defended ? k.held / k.defended : 0 }}
            />
          ))}
          {Array.from({ length: width - ticks.length }, (_, i) => (
            <i key={`next-${i}`} class="tick next" aria-hidden="true" />
          ))}
        </div>
      </div>
      <button type="button" class="btn ghost small" disabled={hour === last && !playing} onClick={() => setHour(last)}>
        Latest
      </button>
    </div>
  );
}
