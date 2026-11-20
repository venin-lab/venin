import { describe, expect, it } from "vitest";
import type { HourRecord, RunLine } from "../record/types";
import { attribute, holderStrainId, submissions } from "./release-tx";

const holder = (tx: string, status = "run") => ({
  received_ms: 1,
  tx,
  signer: "0x1",
  route: "MEM",
  status,
  payload: "p",
  payload_sha: "s",
  signature: "0x",
});
const run = (breached: boolean) => ({ body: "m1-venin1", kind: "holder", outcome: { breached } });

describe("submissions", () => {
  it("counts the breaches of a release that ran alone in its hour", () => {
    const h = {
      n: 27,
      holders: [holder("0xa")],
      outcomes: [run(false), run(true), { body: "m1-none", kind: "attack", outcome: { breached: true } }],
    };
    expect(submissions([h as unknown as HourRecord])[0].outcome).toEqual({ breached: 1, of: 2 });
  });
  it("gives every release of an hour 0 when no holder run breached", () => {
    const h = {
      n: 29,
      holders: [holder("0xa"), holder("0xb")],
      outcomes: [run(false), run(false), run(false), run(false)],
    };
    expect(submissions([h as unknown as HourRecord]).map((s) => s.outcome)).toEqual([
      { breached: 0, of: 2 },
      { breached: 0, of: 2 },
    ]);
  });
  it("leaves the outcome unknown when two releases shared the hour, or one did not run", () => {
    const h = { n: 28, holders: [holder("0xa"), holder("0xb"), holder("0xc", "gate")], outcomes: [run(true)] };
    expect(submissions([h as unknown as HourRecord]).map((s) => s.outcome)).toEqual([null, null, null]);
  });
});

describe("attribute", () => {
  it("names a holder strain the way the lab does, from the payload hash", () => {
    // Hour 46's first release: the lab named MEM-5189519.0 as it arrived.
    expect(holderStrainId("MEM", "3fed4fcf6cbe")).toBe("MEM-5189519.0");
  });
  it("reads each release's breaches from the run log when releases shared an hour and one breached", () => {
    const a = { ...holder("0xa"), payload_sha: "3fed4f00" };
    const b = { ...holder("0xb"), payload_sha: "216153aa" };
    const h = { n: 46, holders: [a, b], outcomes: [run(true), run(false), run(false), run(false)] };
    const list = submissions([h as unknown as HourRecord]);
    expect(list.map((s) => s.outcome)).toEqual([null, null]);
    const line = (strain: string, breached: boolean) =>
      ({ kind: "holder", strain, outcome: { breached } }) as unknown as RunLine;
    const runs = new Map([
      [
        46,
        [
          line("MEM-5189519.0", true),
          line("MEM-5189519.0", false),
          line("MEM-3187603.0", false),
          line("MEM-3187603.0", false),
        ],
      ],
    ]);
    const out = attribute(list, runs);
    expect(out.find((s) => s.tx === "0xa")?.outcome).toEqual({ breached: 1, of: 2 });
    expect(out.find((s) => s.tx === "0xb")?.outcome).toEqual({ breached: 0, of: 2 });
  });
});
