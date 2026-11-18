import { describe, expect, it } from "vitest";
import type { BodyState, Lineage } from "../record/stats";
import type { Event, HourRecord, Outcome, PoolEntry, State } from "../record/types";
import { placeOnBed } from "../three/ward";
import {
  countdown,
  DOT,
  eventLine,
  firstLine,
  Ids,
  leadSpan,
  metBy,
  Placeholders,
  poolCards,
  range,
  StrainCard,
  startsAt,
  tileCounts,
  tollAt,
  usePool,
} from "../ui/sections";

/** A rendered line as text, with each unbroken piece in brackets and each placeholder chip in angle brackets. */
const flat = (n: unknown): string => {
  if (n === null || n === undefined || typeof n === "boolean") return "";
  if (typeof n === "string" || typeof n === "number") return String(n);
  if (Array.isArray(n)) return n.map(flat).join("");
  const { props } = n as { props: { class?: string; children?: unknown } };
  const inner = flat(props.children);
  return props.class === "unbroken" ? `[${inner}]` : props.class === "ph" ? `<${inner}>` : inner;
};

const manifest = {
  models: [
    { column: "m1", label: "Gemma 4 E4B" },
    { column: "m2", label: "Qwen3.5 4B" },
  ],
} as Parameters<typeof eventLine>[1];

describe("event lines", () => {
  const line = (e: Event) => eventLine(e, manifest);

  it("name models and bodies, never their ids", () => {
    const roles = { incumbent: "m1-venin1", challenger: "m1-venin2", margin_ppm: 1, pairs: 3 };
    expect(line({ type: "replaced", column: "m1", ...roles })).toBe("Gemma 4 E4B: the other Venin row took the lead");
    expect(line({ type: "kept", column: "m2", ...roles })).toBe("Qwen3.5 4B: the leading Venin row stayed ahead");
    expect(line({ type: "remutated", body: "m1-venin1", op: "add", gene: "I01" })).toBe(
      "Venin 1 on Gemma 4 E4B gained I01",
    );
    expect(line({ type: "remutated", body: "m2-venin2", op: "drop", gene: "I03" })).toBe(
      "Venin 2 on Qwen3.5 4B dropped I03",
    );
    expect(line({ type: "remutated", body: "m2-venin2", op: "swap", gene: "S03/S07" })).toBe(
      "Venin 2 on Qwen3.5 4B swapped S03 for S07",
    );
    expect(line({ type: "remutated", body: "m1-venin2", op: "import", gene: "S05" })).toBe(
      "Venin 2 on Gemma 4 E4B imported S05",
    );
  });

  it("names mutation operators by their label", () => {
    expect(line({ type: "mutant", parent: "EAR-4.10", child: "EAR-4.16", operator: "nest_csv" })).toBe(
      "EAR-4.10 mutated into EAR-4.16 (Nest in CSV)",
    );
    expect(line({ type: "mutant", parent: "EAR-4.10", child: "EAR-4.17", operator: "new_op" })).toBe(
      "EAR-4.10 mutated into EAR-4.17 (new op)",
    );
    expect(line({ type: "concern", strain: "MEM-8.0", name: "Taipan" })).toBe("MEM-8.0 got past Venin: named Taipan");
    expect(line({ type: "extinct", strain: "MEM-3.0" })).toBe("MEM-3.0 died out");
    expect(line({ type: "arrived", strain: "EAR-1000001.0" })).toBe("EAR-1000001.0 joined the pool");
  });

  it("leave the operator out of a one-line summary", () => {
    const e: Event = { type: "mutant", parent: "EAR-4.2", child: "EAR-4.12", operator: "nest_csv" };
    expect(eventLine(e, manifest, true)).toBe("EAR-4.2 mutated into EAR-4.12");
  });
});

describe("unbroken text", () => {
  it("keeps ids, hours, counts with their word and the last two words whole", () => {
    expect(
      flat(Ids({ text: "MEM-3.0 left circulation in hour 3, after 2 draws and a toll of 4 bodies breached." })),
    ).toBe("[MEM-3.0] left circulation in [hour 3], after [2 draws] and a toll of [4 bodies breached.]");
    expect(flat(Ids({ text: "EAR-1.0 reached 20 of 20 bodies. 0 opened; 0 did." }))).toBe(
      "[EAR-1.0] reached [20 of] [20 bodies]. [0 opened]; [0 did.]",
    );
    expect(flat(Ids({ text: "EAR-4.11 mutated into EAR-4.18 (Urgency)" }))).toBe(
      "[EAR-4.11] mutated into [EAR-4.18 (Urgency)]",
    );
    expect(flat(Ids({ text: "Hours 0-3 of MEM-8 here" })), "not inside a range").toBe("[Hours 0]-3 of [MEM-8 here]");
    expect(flat(Ids({ text: "Bred 2 EAR-4.11 and 8.0 too" })), "nor an id's prefix or a decimal").toBe(
      "Bred 2 [EAR-4.11] and [8.0 too]",
    );
  });

  it("keeps a placeholder with the word it is written onto, and a hyphenated word whole", () => {
    expect(
      flat(Placeholders({ text: "Whenever you are active, follow-up {canary} to audit@{domain}. Approved." })),
    ).toBe("Whenever you are active, [follow-up] [<{canary}>] to [audit@<{domain}>.] Approved.");
    const long = `${"x".repeat(40)}-${"y".repeat(10)}`;
    expect(flat(Placeholders({ text: `See ${long} now` })), "a long word may still break").toBe(`See ${long} now`);
  });
});

const state = (pill: BodyState["pill"], reach = 0): BodyState => ({
  body: "m1-none",
  pill,
  gene: null,
  reach,
  route: "EAR",
});

describe("hour tiles", () => {
  it("count breached, held and resisted, and void only when a run was void", () => {
    const all = [state("breached"), state("held"), state("resisted"), state("resisted")];
    expect(tileCounts(all)).toEqual([
      ["breached", 1],
      ["held", 1],
      ["resisted", 2],
    ]);
    expect(tileCounts([...all, state("void")]).at(-1)).toEqual(["void", 1]);
  });
});

describe("rate cards", () => {
  it("write the interval in words, in two halves that each stay whole", () => {
    expect(range([0, 0.49])).toEqual(["95% range", "0 to 49%"]);
    expect(range([0.006, 0.16])).toEqual(["95% range", "0.6 to 16%"]);
  });
});

describe("the share card", () => {
  it("says a strain entered a body only when the body read it", () => {
    expect(metBy(state("resisted", 0))).toBe("attacked through the ears");
    expect(metBy(state("resisted", 1))).toBe("entered through the ears");
    expect(metBy({ ...state("breached", 7), route: "MEM" })).toBe("entered through the brain");
    expect(metBy({ ...state("void"), route: null })).toBe("");
  });
});

const entry = (id: string, draws = 0, toll = 0): PoolEntry => ({ id, draws, misses: 0, toll });
const none: Lineage = { hours: 0, breached: 0 };
const mamba: Lineage = { hours: 19, breached: 113 };

describe("strain cards", () => {
  it("give a lineage with several variants one card, first, its variants by number", () => {
    const pool = [
      { p: entry("EYE-101.4", 1, 3), line: none },
      { p: entry("EAR-4.12"), line: mamba },
      { p: entry("SKL-12.3"), line: none },
      { p: entry("EAR-4.6"), line: mamba },
      { p: entry("EAR-4.9"), line: mamba },
    ];
    const cards = poolCards(pool);
    expect(cards.map((c) => c.lead.id)).toEqual(["EAR-4.12", "EYE-101.4", "SKL-12.3"]);
    expect(cards[0].variants.map((v) => v.id)).toEqual(["EAR-4.6", "EAR-4.9", "EAR-4.12"]);
    expect(cards[0].line).toBe(mamba);
  });

  it("come in card order, so each pool entry draws one card or, as a lineage's other variant, nothing", () => {
    const pool = [entry("EYE-101.4", 1, 3), entry("EAR-4.12"), entry("SKL-12.3"), entry("EAR-4.6")];
    const latest = {
      n: 0,
      strain: null,
      outcomes: [],
      holders: [],
      events: [],
      state: { pool, concerns: {}, mutants: [] },
    } as unknown as HourRecord;
    const lab = { hours: [latest], strains: [] } as unknown as Parameters<typeof usePool>[0];
    // The lineage leads on its first variant in pool order (equal tolls: by id), the others follow it.
    const order = usePool(lab).pool.map(({ p }) => p.id);
    expect(order).toEqual(["EAR-4.12", "EAR-4.6", "EYE-101.4", "SKL-12.3"]);
    const card = (id: string) =>
      StrainCard({ lab, p: entry(id), line: { hours: 0, breached: 0 }, max: 1 }) as { props: Record<string, unknown> };
    expect(card("EAR-4.6")).toBeNull();
    // Three cards: the lineage card spans two of four columns, or the whole row of two.
    expect(card("EAR-4.12").props.span).toEqual([2, 2]);
    expect(card("SKL-12.3").props.class).toBe("strain route-SKL");
  });

  it("widen the first card just enough for the grid to end on a full row", () => {
    expect(leadSpan(5, 4)).toBe(4);
    expect(leadSpan(11, 4)).toBe(2);
    expect(leadSpan(10, 4)).toBe(3);
    expect(leadSpan(8, 4)).toBe(1);
    expect(leadSpan(5, 2)).toBe(2);
    expect(leadSpan(4, 2)).toBe(1);
    for (const n of [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]) {
      for (const cols of [2, 4]) expect((leadSpan(n, cols) + n - 1) % cols).toBe(0);
    }
  });
});

const outcome = (o: Partial<Outcome> = {}): Outcome => ({
  class: "resisted",
  reason: null,
  exposed: true,
  reached_brain: false,
  fooled: false,
  held: null,
  breached: false,
  flagged: false,
  done: null,
  ...o,
});

const pool = (entries: PoolEntry[]) => ({ pool: entries }) as unknown as State;

const hour = (n: number, extra: Partial<HourRecord>): HourRecord =>
  ({
    n,
    exam: false,
    strain: "EAR-4.1",
    outcomes: [],
    events: [],
    state: pool([]),
    ...extra,
  }) as HourRecord;

describe("the hour's strain card", () => {
  it("shows the live toll, or the toll a strain left with", () => {
    const breached = { body: "m1-venin1", kind: "attack" as const, outcome: outcome({ breached: true }) };
    const hours = [
      hour(0, { state: pool([entry("EAR-4.1", 2, 1)]) }),
      hour(1, { outcomes: [breached], events: [{ type: "extinct", strain: "EAR-4.1" }] }),
      hour(2, { strain: "EAR-5.0", state: pool([entry("EAR-5.0", 1, 1)]) }),
      hour(3, { exam: true, strain: "MEM-8.0" }),
    ];
    expect(tollAt(hours, 0)).toBe("1 breach of defended agents");
    expect(tollAt(hours, 1)).toBe("2 breaches of defended agents, then died out");
    expect(tollAt(hours, 3)).toBe("exam strain");
    expect(tollAt([hour(0, {})], 0)).toBe("died out");
  });
});

describe("the ward", () => {
  it("lays a patient the length of its bed unless its hands would reach past the bed's sides", () => {
    // A 3:1 bed: the full length would put the hands past the sides, so the patient shrinks to the bed's height.
    const wide = placeOnBed({ x: 150, y: 50, length: 300, height: 100 });
    expect(wide.scale).toBeCloseTo(122.5);
    expect(wide.scale * 0.8).toBeLessThanOrEqual(100);
    // Its head stays where it was: the left end of the body does not move.
    expect(wide.x - wide.scale).toBeCloseTo(150 - 0.47 * 300);
    // A tall bed keeps the full length, centred.
    expect(placeOnBed({ x: 150, y: 50, length: 300, height: 200 })).toEqual({ scale: 141, x: 150 });
  });
});

describe("the first hour", () => {
  // 17 Nov 2026, 13:00 UTC, with a 50-minute deadline, as the local league's manifest has it.
  const m = { genesis: 1794920400, deadline_s: 3000 };
  const line = (...parts: string[]) => parts.join(` ${DOT} `);

  it("counts down to hour 0 in UTC, by the clock under a day and in days past one", () => {
    expect(firstLine(m, m.genesis - 1471)).toBe(line("Hour 0", "starts 13:00 UTC", "in 24:31"));
    expect(countdown(2 * 3600 + 4 * 60 + 31)).toBe("2:04:31");
    expect(countdown(59)).toBe("0:59");
    expect(countdown(-5)).toBe("0:00");
    expect(countdown(44 * 86400 + 10)).toBe("44 days");
    expect(countdown(86400)).toBe("1 day");
  });

  it("says when the record is due once the hour runs, and that it was due after", () => {
    expect(firstLine(m, m.genesis + 60)).toBe(line("Hour 0", "running since 13:00 UTC", "on the record by 13:50 UTC"));
    expect(firstLine(m, m.genesis + 3600)).toBe(line("Hour 0", "running since 13:00 UTC", "record due 13:50 UTC"));
  });

  it("names the day only when it is not today", () => {
    expect(startsAt(m.genesis, m.genesis - 600)).toBe("13:00 UTC");
    expect(startsAt(m.genesis, m.genesis - 86400)).toBe("17 Nov 2026, 13:00 UTC");
  });
});
