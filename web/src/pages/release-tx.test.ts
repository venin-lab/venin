import { describe, expect, it } from "vitest";
import type { HourRecord } from "../record/types";
import { submissions } from "./release-tx";

const holder = (tx: string, status = "run") => ({ received_ms: 1, tx, signer: "0x1", route: "MEM", status, payload: "p", payload_sha: "s", signature: "0x" });
const run = (breached: boolean) => ({ body: "m1-venin1", kind: "holder", outcome: { breached } });

describe("submissions", () => {
  it("counts the breaches of a release that ran alone in its hour", () => {
    const h = { n: 27, holders: [holder("0xa")], outcomes: [run(false), run(true), { body: "m1-none", kind: "attack", outcome: { breached: true } }] };
    expect(submissions([h as unknown as HourRecord])[0].outcome).toEqual({ breached: 1, of: 2 });
  });
  it("gives every release of an hour 0 when no holder run breached", () => {
    const h = { n: 29, holders: [holder("0xa"), holder("0xb")], outcomes: [run(false), run(false), run(false), run(false)] };
    expect(submissions([h as unknown as HourRecord]).map((s) => s.outcome)).toEqual([{ breached: 0, of: 2 }, { breached: 0, of: 2 }]);
  });
  it("leaves the outcome unknown when two releases shared the hour, or one did not run", () => {
    const h = { n: 28, holders: [holder("0xa"), holder("0xb"), holder("0xc", "gate")], outcomes: [run(true)] };
    expect(submissions([h as unknown as HourRecord]).map((s) => s.outcome)).toEqual([null, null, null]);
  });
});
