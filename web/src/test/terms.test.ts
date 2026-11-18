import { describe, expect, it, vi } from "vitest";
import { type RowRate, wilson } from "../record/stats";
import type { HourRecord, Outcome, Strain } from "../record/types";
import { DOT, leadLine, stepLine } from "../ui/sections";
import { TERMS } from "../ui/terms";

describe("TERMS", () => {
  it("defines every lab word in one short, plain, ASCII sentence or two", () => {
    for (const [key, [title, text]] of Object.entries(TERMS)) {
      const words = text.split(/\s+/).length;
      expect(words, `${key}: ${words} words`).toBeLessThanOrEqual(20);
      expect(`${title} ${text}`, key).toMatch(/^[\x20-\x7e]+$/);
      expect(text, key).toMatch(/^[A-Z$0-9]/);
      expect(text, key).toMatch(/\.$/);
      expect(text.toLowerCase(), key).not.toMatch(/\b(just|simply|powerful|seamless|revolutionary|unleash)\b/);
    }
  });
});

const attack = (breached: boolean) => ({
  body: "m1-none",
  kind: "attack",
  outcome: { class: breached ? "breached" : "resisted", breached } as Outcome,
});

const hour = (n: number, extra: Partial<HourRecord> = {}): HourRecord =>
  ({
    n,
    exam: false,
    strain: "EAR-1.0",
    flags: [],
    outcomes: [...Array.from({ length: 2 }, () => attack(true)), ...Array.from({ length: 18 }, () => attack(false))],
    ...extra,
  }) as unknown as HourRecord;

describe("the hero's live line", () => {
  it("says what carried the scam in words, with no strain code, and says plainly when an hour did not run", async () => {
    // The router listens for history changes when it loads; the ward needs no other browser global to load.
    vi.stubGlobal("addEventListener", () => {});
    const { heroLine } = await import("../ui/ward");
    const lab = { strains: [{ id: "EAR-1.0", carrier: "email" }] as Strain[], hours: [] };
    expect(heroLine(lab, hour(5))).toBe(`Hour 5 ${DOT} a scam email ${DOT} 2 of 20 breached`);
    expect(heroLine(lab, hour(6, { strain: "EYE-9.0" }))).toBe(`Hour 6 ${DOT} a hidden scam ${DOT} 2 of 20 breached`);
    expect(heroLine(lab, hour(7, { strain: null, outcomes: [] }))).toBe(`Hour 7 ${DOT} did not run`);
    expect(heroLine(lab, hour(8, { flags: ["sealed"] }))).toBe(`Hour 8 ${DOT} sealed for review`);
    vi.unstubAllGlobals();
  });
});

const rate = (breached: number, attacks: number): RowRate => ({
  row: "none",
  attacks,
  breached,
  fooled: breached,
  tasks: 0,
  done: 0,
  interval: wilson(breached, attacks),
});

describe("the scoreboard's reading line", () => {
  it("says no defence leads while every range overlaps, and nothing once one row pulls ahead", () => {
    expect(leadLine([rate(0, 8), rate(2, 8)])).toBe(
      "Lower is better, but each defence has faced only 8 exam attacks so far: too few to rank them, and every range still overlaps the others.",
    );
    expect(leadLine([rate(0, 40), rate(30, 40)])).toBeNull();
    expect(leadLine([rate(0, 8), rate(1, 0)]), "one counted row").toBeNull();
    expect(leadLine([rate(0, 1), rate(1, 1)])).toContain("only 1 exam attack so far");
  });

  it("says Venin has not evolved until its rows first compare, then nothing", () => {
    const hours = (events: { type: string }[]) => [{ n: 0, events }] as unknown as HourRecord[];
    expect(stepLine(hours([]))).toBe(
      "Venin has not evolved yet: its rows still carry the few rules they were dealt at the start, and their first step comes at hour 11.",
    );
    expect(stepLine(hours([{ type: "kept" }]))).toBeNull();
    expect(stepLine(hours([{ type: "replaced" }]))).toBeNull();
  });
});
