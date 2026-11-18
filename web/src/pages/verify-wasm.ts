import init, { gate, mutate, replay, scan, verify } from "../wasm/venin_core.js";

let ready: Promise<unknown> | null = null;

/** venin-core compiled to WebAssembly, fetched and compiled once per page. Every export takes and returns JSON. */
export async function core() {
  ready ??= init();
  await ready;
  return { verify, replay, scan, gate, mutate };
}

export interface RowRate {
  row: string;
  attacks: number;
  fooled: number;
  breached: number;
}

/** venin-core's verify::Report. */
export interface Report {
  made_by: number | null;
  hours: number;
  late: number[];
  void_share_ppm: number;
  withheld: number;
  taken_down: string[];
  /** Hours whose runs were taken down, so their state was taken as written. */
  not_recomputed: number[];
  unchecked: number;
  headline: RowRate[];
  /** The exam hours `headline` counts. */
  headline_hours: number[];
  errors: string[];
}

export interface Verified {
  report: Report;
  ok: boolean;
  replayed: number[];
  not_loaded: number[];
}

export type Organ = "eyes" | "ears" | "brain" | "hands" | "heart";

/** venin-core's scan::Finding. */
export interface Finding {
  organ: Organ;
  check: "hidden" | "comment" | "order" | "actor" | "wide" | "pay";
  kind: string;
  file: string;
  line: number;
  says: string;
  genes: string[];
}

export interface Scan {
  read: number;
  skipped: string[];
  findings: Finding[];
}

/** Parses a core answer, throwing its `error` field as an Error. */
export function answer<T>(json: string): T {
  const v = JSON.parse(json) as T & { error?: string };
  if (typeof v.error === "string") throw new Error(v.error);
  return v;
}
