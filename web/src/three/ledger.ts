import { DEFENDED, rowOf } from "../record/stats";
import type { HourRecord } from "../record/types";

/** One tick of the generation ledger. */
export interface Tick {
  n: number;
  exam: boolean;
  void: boolean;
  /** Defended agents breached, out of `defended`. */
  breached: number;
  held: number;
  defended: number;
  /** Any agent breached, the undefended rows included: the hour's tick is pink. */
  hit: boolean;
}

export function tick(hour: HourRecord): Tick {
  const all = hour.outcomes.filter((o) => o.kind === "attack");
  const attacks = all.filter((o) => DEFENDED.includes(rowOf(o.body)));
  return {
    n: hour.n,
    exam: hour.exam,
    void: hour.strain === null || attacks.every((o) => o.outcome.class === "void"),
    breached: attacks.filter((o) => o.outcome.breached).length,
    held: attacks.filter((o) => o.outcome.held !== null && !o.outcome.breached).length,
    defended: attacks.length,
    hit: all.some((o) => o.outcome.breached),
  };
}

/** The tick index under a pointer `x` pixels into a strip `width` wide holding `count` ticks. */
export function indexAt(x: number, width: number, count: number): number {
  if (count <= 1 || width <= 0) return 0;
  return Math.min(count - 1, Math.max(0, Math.floor((x / width) * count)));
}

/** Slots in the strip: a day's worth until the record is longer, so a young record reads as the start of a day. */
export const slots = (count: number, day = 24) => Math.max(day, count);

/** Playback runs the whole record in about `total` seconds, at least `min` seconds a tick. */
export function playIndex(elapsed: number, count: number, total = 20, min = 0.12): number {
  const per = Math.max(min, total / Math.max(1, count));
  return Math.min(count - 1, Math.floor(elapsed / per));
}
