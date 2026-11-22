import type { Column, Defender, HourRecord, Outcome } from "../record/types";

/** Periods an agent stays in the ward: the core's DEFEND_PERIODS. */
export const DEFEND_PERIODS = 24;
/** The core's gate::DEFEND_MAX. */
export const MAX_RULES = 2000;
export const COLUMNS: Column[] = ["m1", "m2", "m3", "m4"];

/** Attack runs on the given bodies over the last 24 written hours, void runs left out. */
export function lastDay(hours: HourRecord[], bodies: string[]): { breached: number; ran: number } {
  const out = { breached: 0, ran: 0 };
  for (const h of hours.slice(-24)) {
    for (const o of h.outcomes) {
      if (o.kind !== "attack" || !bodies.includes(o.body) || o.outcome.class === "void") continue;
      out.ran += 1;
      out.breached += Number(o.outcome.breached);
    }
  }
  return out;
}
/** The bare model, no defence, for the model picker and the models page. */
export const bareRecord = (hours: HourRecord[], column: Column) => lastDay(hours, [`${column}-none`]);
/** Venin's two agents on the model. */
export const veninRecord = (hours: HourRecord[], column: Column) =>
  lastDay(hours, [`${column}-venin1`, `${column}-venin2`]);

/** The message a holder signs to put their agent in the ward; docs/RECORD.md, "Defend intake". */
export const defendMessage = (tx: string, column: Column, rulesSha: string, chainId: number) =>
  `Venin defend\nburn: ${tx}\nmodel: ${column}\nrules-sha256: ${rulesSha}\nchain: ${chainId}`;

/** The id a burn gives the agent it puts in the ward: the first 16 hex digits of the transaction. */
export const defenderId = (tx: string) => tx.slice(2, 18).toLowerCase();
export const defenderBody = (column: string, id: string) => `${column}-defender-${id}`;

/**
 * A model whose every run failed in each of the last two written hours while the other models ran: its provider is
 * down, and an agent sent to it would collect only void runs. The picker closes it until its runs come back.
 */
export function providerDown(hours: HourRecord[], column: Column): boolean {
  const last = hours.slice(-2);
  const mine = (body: string) => body.startsWith(`${column}-`);
  return (
    last.length === 2 &&
    last.every((h) => {
      const own = h.outcomes.filter((o) => mine(o.body));
      const rest = h.outcomes.filter((o) => !mine(o.body));
      return (
        own.length > 0 && own.every((o) => o.outcome.class === "void") && rest.some((o) => o.outcome.class !== "void")
      );
    })
  );
}

/** An id from a page path: the 16 hex digits, or a whole burn transaction. */
export function idOf(param: string): string | null {
  const p = param.toLowerCase();
  if (/^[0-9a-f]{16}$/.test(p)) return p;
  if (/^0x[0-9a-f]{64}$/.test(p)) return defenderId(p);
  return null;
}

/** A defend entry of the record with the hour that took it. */
export interface Entry extends Defender {
  n: number;
  id: string;
}

/** Every defend entry of the record, newest first. */
export const entries = (hours: HourRecord[]): Entry[] =>
  hours
    .flatMap((h) => (h.defenders ?? []).map((d) => ({ ...d, n: h.n, id: defenderId(d.tx) })))
    .sort((a, b) => b.received_ms - a.received_ms || (a.tx < b.tx ? 1 : -1));

/** The entry that put agent `id` in the ward, or the latest entry of its burn when none ran. */
export function findEntry(hours: HourRecord[], id: string): Entry | null {
  const all = entries(hours).filter((e) => e.id === id);
  return all.find((e) => e.status === "run") ?? all[0] ?? null;
}

export type VisitState = "held" | "breached" | "void" | "exam" | "pending" | "sealed" | "missed";

/** One hour of an agent's stay, with the bodies on its model that met the same strain. */
export interface Visit {
  n: number;
  state: VisitState;
  strain: string | null;
  outcome: Outcome | null;
  /** The model with no defence. */
  bare: Outcome | null;
  /** The two Venin bodies on the model, by body id. */
  venin: [string, Outcome | null][];
}

export interface Stay {
  entry: Entry;
  visits: Visit[];
  held: number;
  breached: number;
  /** Hours whose runs were void or did not take place although the agent was in the ward. */
  lost: number;
  /** Training hours still to come. */
  left: number;
  /** Over the hours the agent ran and was judged: the bare model's and the Venin bodies' results on the same strain. */
  bare: { held: number; ran: number };
  veninHeld: { held: number; ran: number };
}

const attackOf = (h: HourRecord, body: string): Outcome | null =>
  h.outcomes.find((o) => o.body === body && o.kind === "attack")?.outcome ?? null;

const judged = (o: Outcome | null): o is Outcome => o !== null && o.class !== "void";

/** Each of the 24 hours from the one that took the agent, as the record tells them so far. */
export function stay(hours: HourRecord[], entry: Entry): Stay {
  const body = defenderBody(entry.column, entry.id);
  const out: Stay = {
    entry,
    visits: [],
    held: 0,
    breached: 0,
    lost: 0,
    left: 0,
    bare: { held: 0, ran: 0 },
    veninHeld: { held: 0, ran: 0 },
  };
  for (let n = entry.n; n < entry.n + DEFEND_PERIODS; n++) {
    const h = hours[n];
    const venin: [string, Outcome | null][] = [`${entry.column}-venin1`, `${entry.column}-venin2`].map((b) => [
      b,
      h ? attackOf(h, b) : null,
    ]);
    const visit: Visit = { n, state: "pending", strain: h?.strain ?? null, outcome: null, bare: null, venin };
    out.visits.push(visit);
    if (!h) {
      if (n % 5 !== 0) out.left += 1;
      else visit.state = "exam";
      continue;
    }
    visit.bare = attackOf(h, `${entry.column}-none`);
    if (h.exam) {
      visit.state = "exam";
      continue;
    }
    if (h.flags.includes("sealed")) {
      visit.state = "sealed";
      out.lost += 1;
      continue;
    }
    const mine = h.outcomes.find((o) => o.body === body && o.kind === "defend")?.outcome ?? null;
    visit.outcome = mine;
    if (!mine) {
      visit.state = "missed";
      out.lost += 1;
      continue;
    }
    if (mine.class === "void") {
      visit.state = "void";
      out.lost += 1;
      continue;
    }
    visit.state = mine.breached ? "breached" : "held";
    out[mine.breached ? "breached" : "held"] += 1;
    if (judged(visit.bare)) {
      out.bare.ran += 1;
      out.bare.held += Number(!visit.bare.breached);
    }
    for (const [, o] of venin) {
      if (!judged(o)) continue;
      out.veninHeld.ran += 1;
      out.veninHeld.held += Number(!o.breached);
    }
  }
  return out;
}

/** Held over judged, as a whole percentage; null before the first judged hour. */
export const immunity = (s: Pick<Stay, "held" | "breached">): number | null =>
  s.held + s.breached ? Math.round((100 * s.held) / (s.held + s.breached)) : null;

/** What held a strain that breached the agent, on Venin bodies of the same hour. */
export interface Cure {
  /** A Venin body on the agent's model a gene stopped the attacker's call on: the gene. */
  stopped: { body: string; gene: string }[];
  /** A Venin body on the agent's model the strain never fooled: the rules (instinct genes) it carried. */
  unfooled: { body: string; genes: string[] }[];
  /** When nothing held on the agent's model: Venin bodies on other models that held, the same way. */
  elsewhere: { body: string; gene: string | null; genes: string[] }[];
}

/** Instinct genes are the rules in a body's prompt; skin genes act on tool calls and hold only by stopping one. */
export type GeneKinds = ReadonlyMap<string, "instinct" | "skin">;

/** What held hour `n`'s strain on the Venin bodies, starting with those on `column`. */
export function cure(hours: HourRecord[], n: number, column: Column, kinds: GeneKinds): Cure {
  const h = hours[n];
  const out: Cure = { stopped: [], unfooled: [], elsewhere: [] };
  if (!h) return out;
  // The genome a run carried is the one in the state before its hour.
  const genomes = (hours[n - 1] ?? h).state.genomes;
  const rules = (body: string) => (genomes[body] ?? []).filter((g) => kinds.get(g) === "instinct");
  const held = (body: string) => {
    const o = attackOf(h, body);
    if (!o || o.class === "void" || o.breached) return null;
    return { gene: o.held, fooled: o.fooled };
  };
  for (const c of COLUMNS) {
    for (const row of ["venin1", "venin2"]) {
      const body = `${c}-${row}`;
      const r = held(body);
      if (!r) continue;
      if (c === column) {
        if (r.gene) out.stopped.push({ body, gene: r.gene });
        else if (!r.fooled) out.unfooled.push({ body, genes: rules(body) });
      } else if (r.gene || !r.fooled) {
        out.elsewhere.push({ body, gene: r.gene, genes: r.gene ? [] : rules(body) });
      }
    }
  }
  if (out.stopped.length || out.unfooled.length) out.elsewhere = [];
  return out;
}

/**
 * Genes to add, from every breach: the genes that stopped a call, and the rules carried by bodies that were never
 * fooled, each with the breaches it answers, most first.
 */
export function suggestions(hours: HourRecord[], s: Stay, kinds: GeneKinds): { gene: string; covers: number[] }[] {
  const by = new Map<string, Set<number>>();
  for (const v of s.visits) {
    if (v.state !== "breached") continue;
    const c = cure(hours, v.n, s.entry.column, kinds);
    const genes = [
      ...c.stopped.map((x) => x.gene),
      ...c.unfooled.flatMap((x) => x.genes),
      ...c.elsewhere.flatMap((x) => (x.gene ? [x.gene] : x.genes)),
    ];
    for (const g of genes) {
      if (g.startsWith("priors:")) continue;
      const set = by.get(g) ?? new Set();
      set.add(v.n);
      by.set(g, set);
    }
  }
  return [...by.entries()]
    .map(([gene, set]) => ({ gene, covers: [...set].sort((a, b) => a - b) }))
    .sort((a, b) => b.covers.length - a.covers.length || a.gene.localeCompare(b.gene, "en", { numeric: true }));
}
