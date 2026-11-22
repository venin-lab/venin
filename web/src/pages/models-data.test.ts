import { createHash } from "node:crypto";
import { describe, expect, it } from "vitest";
import { ROUNDS } from "./models-data";

/** A released payload's strain id, as the core names it: family 1000000 plus the first 3 bytes of its SHA-256. */
const strainId = (route: string, payload: string) =>
  `${route}-${1000000 + Number.parseInt(createHash("sha256").update(payload).digest("hex").slice(0, 6), 16)}.0`;

describe("the rounds", () => {
  it("names every released attack by its payload's strain id, and only attacks that were written", () => {
    for (const r of ROUNDS) {
      for (const a of r.asked) {
        expect(a.attack !== null).toBe(a.verdict === "wrote");
        if (!a.attack) continue;
        expect(a.attack.strain).toBe(strainId(a.attack.route, a.attack.payload));
        expect(a.attack.hour).toBe(r.hour);
        expect(a.attack.fooled).toBeLessThanOrEqual(a.attack.judged);
        expect(a.attack.breached).toBeLessThanOrEqual(a.attack.fooled);
      }
    }
  });
  it("puts each ward model at most once in a round", () => {
    for (const r of ROUNDS) {
      const cols = r.asked.flatMap((a) => (a.column ? [a.column] : []));
      expect(new Set(cols).size).toBe(cols.length);
    }
  });
});
