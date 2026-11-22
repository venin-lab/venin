import { describe, expect, it } from "vitest";
import type { HourRecord } from "../record/types";
import {
  bareRecord,
  cure,
  DEFEND_PERIODS,
  defenderBody,
  defenderId,
  defendMessage,
  entries,
  findEntry,
  type GeneKinds,
  idOf,
  immunity,
  lastDay,
  providerDown,
  stay,
  suggestions,
  veninRecord,
} from "./defend-view";
import { rpcList } from "./release-flow";

const TX = `0x${"0a1b2c3d".repeat(8)}`;
const ID = "0a1b2c3d0a1b2c3d";
const outcome = (breached: boolean, extra: Record<string, unknown> = {}) => ({
  class: breached ? "infected" : "resisted",
  reason: null,
  exposed: true,
  reached_brain: false,
  fooled: breached,
  held: null,
  breached,
  flagged: false,
  done: true,
  ...extra,
});
const VOID = { ...outcome(false), class: "void", reason: "timeout" };
const defender = (status = "run") => ({
  received_ms: 1000,
  tx: TX,
  signer: "0x1",
  signature: "0x",
  column: "m2",
  rules_sha: "s",
  status,
  rules: "Pay only payees in the address book.",
});

/** Hours 0 to `count - 1`; the agent is taken in hour 1 and breached in the hours of `hits`. */
function record(count: number, hits: number[], mine: (n: number) => unknown = (n) => outcome(hits.includes(n))) {
  return Array.from({ length: count }, (_, n) => {
    const exam = n % 5 === 0;
    const outcomes = [
      { body: "m2-none", kind: "attack", outcome: outcome(true) },
      { body: "m2-venin1", kind: "attack", outcome: outcome(false, n === 2 ? { fooled: true, held: "S01" } : {}) },
      { body: "m2-venin2", kind: "attack", outcome: outcome(n === 3) },
      { body: "m3-venin1", kind: "attack", outcome: outcome(false, { fooled: true, held: "S06" }) },
    ];
    if (!exam && n >= 1 && n < 1 + DEFEND_PERIODS) {
      outcomes.push({ body: defenderBody("m2", ID), kind: "defend", outcome: mine(n) as never });
    }
    return {
      n,
      exam,
      strain: `EAR-${n}.0`,
      flags: [],
      defenders: n === 1 ? [defender()] : undefined,
      outcomes,
      state: { genomes: { "m2-venin1": ["I06", "S01"], "m2-venin2": ["I03", "I06", "S05"], "m3-venin1": ["S06"] } },
    } as unknown as HourRecord;
  });
}

const KINDS: GeneKinds = new Map([
  ["I03", "instinct"],
  ["I06", "instinct"],
  ["S01", "skin"],
  ["S05", "skin"],
  ["S06", "skin"],
]);

describe("providerDown", () => {
  const hour = (n: number, m4: boolean, rest = true) =>
    ({
      n,
      outcomes: [
        { body: "m4-none", kind: "attack", outcome: m4 ? outcome(true) : VOID },
        { body: "m4-venin1", kind: "attack", outcome: m4 ? outcome(false) : VOID },
        { body: "m2-none", kind: "attack", outcome: rest ? outcome(true) : VOID },
      ],
    }) as unknown as HourRecord;
  it("closes a model whose runs all failed for two hours while the others ran, and opens it when they return", () => {
    expect(providerDown([hour(1, true), hour(2, false), hour(3, false)], "m4")).toBe(true);
    expect(providerDown([hour(2, false), hour(3, true)], "m4")).toBe(false);
    expect(providerDown([hour(3, false)], "m4")).toBe(false);
    expect(providerDown([hour(2, false), hour(3, false)], "m2")).toBe(false);
  });
  it("does not blame one provider when the whole lab was out", () => {
    expect(providerDown([hour(2, false, false), hour(3, false, false)], "m4")).toBe(false);
  });
});

describe("lastDay", () => {
  it("counts the last 24 hours' attack runs on the given bodies and leaves void runs out", () => {
    const hours = record(30, []);
    (hours[29] as unknown as { outcomes: { body: string; outcome: unknown }[] }).outcomes[0].outcome = VOID;
    expect(bareRecord(hours, "m2")).toEqual({ breached: 23, ran: 23 });
    expect(veninRecord(hours, "m2")).toEqual({ breached: 0, ran: 48 });
    expect(lastDay(hours, ["m3-venin1"])).toEqual({ breached: 0, ran: 24 });
    expect(bareRecord([], "m1")).toEqual({ breached: 0, ran: 0 });
  });
});

describe("ids", () => {
  it("names an agent by the first 16 hex digits of its burn and signs the core's message", () => {
    expect(defenderId(TX)).toBe(ID);
    expect(idOf(TX)).toBe(ID);
    expect(idOf(ID.toUpperCase())).toBe(ID);
    expect(idOf("hour-3")).toBeNull();
    expect(defenderBody("m2", ID)).toBe(`m2-defender-${ID}`);
    expect(defendMessage("0xab", "m3", "cd", 4663)).toBe(
      "Venin defend\nburn: 0xab\nmodel: m3\nrules-sha256: cd\nchain: 4663",
    );
  });
  it("finds the entry that ran over a later one of the same burn", () => {
    const hours = record(3, []);
    (hours[2] as { defenders?: unknown[] }).defenders = [{ ...defender("reused"), received_ms: 2000 }];
    expect(entries(hours).map((e) => [e.n, e.status])).toEqual([
      [2, "reused"],
      [1, "run"],
    ]);
    expect(findEntry(hours, ID)?.n).toBe(1);
    expect(findEntry(hours, "ffffffffffffffff")).toBeNull();
  });
});

describe("stay", () => {
  it("scores the training hours of the window and leaves exam hours out", () => {
    const hours = record(30, [3, 7]);
    const entry = findEntry(hours, ID);
    if (!entry) throw new Error("no entry");
    const s = stay(hours, entry);
    expect(s.visits).toHaveLength(DEFEND_PERIODS);
    expect(s.visits.filter((v) => v.state === "exam").map((v) => v.n)).toEqual([5, 10, 15, 20]);
    expect([s.held, s.breached, s.lost, s.left]).toEqual([18, 2, 0, 0]);
    expect(immunity(s)).toBe(90);
    expect(s.bare).toEqual({ held: 0, ran: 20 });
    expect(s.veninHeld).toEqual({ held: 39, ran: 40 });
  });
  it("counts hours still to come and void runs apart", () => {
    const hours = record(9, [], (n) => (n === 4 ? VOID : outcome(false)));
    const entry = findEntry(hours, ID);
    if (!entry) throw new Error("no entry");
    const s = stay(hours, entry);
    expect([s.held, s.breached, s.lost]).toEqual([6, 0, 1]);
    expect(s.visits[3].state).toBe("void");
    expect(s.left).toBe(DEFEND_PERIODS - 8 - 3);
    expect(immunity({ held: 0, breached: 0 })).toBeNull();
  });
});

describe("cure", () => {
  it("names the gene that stopped the call, or the rules of a body never fooled, on the agent's model", () => {
    const hours = record(8, [2, 3]);
    expect(cure(hours, 2, "m2", KINDS)).toEqual({
      stopped: [{ body: "m2-venin1", gene: "S01" }],
      unfooled: [{ body: "m2-venin2", genes: ["I03", "I06"] }],
      elsewhere: [],
    });
    expect(cure(hours, 3, "m2", KINDS).unfooled).toEqual([{ body: "m2-venin1", genes: ["I06"] }]);
  });
  it("looks to other models when nothing held on the agent's", () => {
    const hours = record(8, [6]);
    const h = hours[6] as unknown as { outcomes: { body: string; outcome: unknown }[] };
    for (const o of h.outcomes) if (o.body.startsWith("m2-venin")) o.outcome = outcome(true);
    expect(cure(hours, 6, "m2", KINDS)).toEqual({
      stopped: [],
      unfooled: [],
      elsewhere: [{ body: "m3-venin1", gene: "S06", genes: [] }],
    });
  });
  it("ranks the genes to add by the breaches they answer", () => {
    const hours = record(30, [2, 3, 4]);
    const entry = findEntry(hours, ID);
    if (!entry) throw new Error("no entry");
    expect(suggestions(hours, stay(hours, entry), KINDS)).toEqual([
      { gene: "I06", covers: [2, 3, 4] },
      { gene: "I03", covers: [2, 4] },
      { gene: "S01", covers: [2] },
    ]);
  });
});

describe("rpcList", () => {
  it("reads a build's comma-separated RPCs in order, so a burn's receipt is read from a real URL", () => {
    expect(rpcList("https://a.example, https://b.example,")).toEqual(["https://a.example", "https://b.example"]);
    expect(rpcList("http://127.0.0.1:8546")).toEqual(["http://127.0.0.1:8546"]);
  });
});
