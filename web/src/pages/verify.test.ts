import { readFileSync } from "node:fs";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { Anchor, HourRecord, Manifest } from "../record/types";
import { initSync, verify } from "../wasm/venin_core.js";
import { headlineScope, hourList, leftOut, ranges, rebuiltHours } from "./verify";
import { anchorFacts, anchorsIn, defaultScope, drawFact, drawFacts, RECENT, rpcList, SAMPLE } from "./verify-run";
import { answer, type Verified } from "./verify-wasm";

const root = new URL("../../../", import.meta.url);
const read = (rel: string) => readFileSync(new URL(rel, root), "utf8");

describe("scope", () => {
  it("takes the latest periods and a sample from before them", () => {
    let k = 0;
    const scope = defaultScope(99, () => (k++ * 0.37) % 1);
    expect(scope.filter((n) => n >= 100 - RECENT)).toEqual(Array.from({ length: RECENT }, (_, i) => 100 - RECENT + i));
    const earlier = scope.filter((n) => n < 100 - RECENT);
    expect(earlier).toHaveLength(SAMPLE);
    expect(new Set(earlier).size).toBe(SAMPLE);
    expect(scope).toEqual([...scope].sort((a, b) => a - b));
    expect(defaultScope(5)).toEqual([0, 1, 2, 3, 4, 5]);
    expect(defaultScope(RECENT + 2, () => 0)).toEqual(Array.from({ length: RECENT + 3 }, (_, i) => i));
  });

  it("writes hours as runs", () => {
    expect(ranges([0, 1, 2, 3, 7, 24, 25, 26])).toBe("0-3, 7, 24-26");
    expect(ranges([])).toBe("");
    expect(hourList([0])).toBe("Hour 0");
    expect(hourList([0, 1, 5])).toBe("Hours 0-1, 5");
    expect(hourList([])).toBe("");
  });

  it("names the exam hours a headline counts and each one it left out, and says nothing when all count", () => {
    const why = () => "was void";
    expect(headlineScope([0, 5, 10], [], [0, 5, 10], why)).toBeNull();
    expect(headlineScope([0, 5, 10, 15], [1, 5, 6], [0, 15], why)).toBe(
      "This scoreboard counts exam hours 0, 15; hour 10 was void; 1 of the 4 exam hours in the record was not replayed here. Replay every hour for the whole scoreboard.",
    );
    expect(headlineScope([0, 5], [0, 5], [], why)).toBe(
      "This scoreboard counts no exam hour; 2 of the 2 exam hours in the record were not replayed here. Replay every hour for the whole scoreboard.",
    );
    expect(headlineScope([0, 5], [1], [0, 5], why)).toBe(
      "All 2 exam hours in the record were replayed here, so the scoreboard is whole.",
    );
    expect(headlineScope([0, 5], [], [5], why), "every hour replayed, one exam hour out").toBe(
      "This scoreboard counts exam hour 5; hour 0 was void.",
    );
  });

  it("says why a replayed exam hour is out of the headline", () => {
    const h = (extra: Partial<HourRecord>) =>
      ({ n: 20, strain: "EAR-1.0", flags: [], outcomes: [], ...extra }) as unknown as HourRecord;
    expect(leftOut(h({ flags: ["sealed"] }), [], [])).toBe("was sealed");
    expect(leftOut(h({}), [20], [])).toBe("arrived late");
    expect(leftOut(h({}), [], ["runs/20.jsonl.gz"])).toBe("had its runs taken down");
    expect(leftOut(h({ strain: null }), [], [])).toBe("was void");
  });

  it("counts an hour as rebuilt only when its runs were there to replay", () => {
    const v = (replayed: number[], notRecomputed: number[]) =>
      ({ replayed, not_loaded: [], ok: true, report: { not_recomputed: notRecomputed } }) as unknown as Verified;
    expect(rebuiltHours(v([0, 1, 2, 3], []))).toEqual([0, 1, 2, 3]);
    expect(rebuiltHours(v([0, 1, 2, 3], [2])), "hour 2's runs were taken down").toEqual([0, 1, 3]);
  });

  it("checks the manifest and exam anchors and the scoped hours", () => {
    const a = (kind: Anchor["kind"], n: number): Anchor => ({ kind, n, tx: `0x${kind}${n}`, block: n });
    const all = [a("manifest", 0), a("exam", 0), a("hour", 0), a("hour", 1), a("hour", 2)];
    expect(anchorsIn(all, [2]).map((x) => x.tx)).toEqual(["0xmanifest0", "0xexam0", "0xhour2"]);
  });
});

describe("draw blocks", () => {
  const block = (number: number, hash: string, parent: string, ts: number) => ({
    number,
    hash,
    parent_hash: parent,
    ts,
  });

  it("give a chain fact only when the block follows its parent", () => {
    expect(drawFact({ block: block(11, "bb", "aa", 100), parent: block(10, "aa", "99", 99) })).toEqual([
      11,
      { hash: "bb", ts: 100, parent_ts: 99 },
    ]);
    expect(drawFact({ block: block(11, "bb", "cc", 100), parent: block(10, "aa", "99", 99) })).toBe(
      "draw block 11 does not follow its parent",
    );
  });

  it("are read from the draw chain, block and parent, on the manifest's chain", async () => {
    const rpcBlock = (n: number) => ({
      number: `0x${n.toString(16)}`,
      hash: `0x${n.toString(16).padStart(64, "0")}`,
      parentHash: `0x${(n - 1).toString(16).padStart(64, "0")}`,
      timestamp: `0x${(1000 + n).toString(16)}`,
    });
    vi.stubGlobal(
      "fetch",
      vi.fn(async (_url: string, init: { body: string }) => {
        const { method, params } = JSON.parse(init.body) as { method: string; params: string[] };
        const result = method === "eth_chainId" ? "0x1237" : rpcBlock(Number(params[0]));
        return { json: async () => ({ result }) };
      }),
    );
    const manifest = { draw_chain_id: 4663 } as Manifest;
    const blocks = await drawFacts(["http://draw.example"], manifest, [11, 11], () => {}, 0);
    expect(blocks).toEqual({ 11: { hash: "b".padStart(64, "0"), ts: 1011, parent_ts: 1010 } });
    await expect(
      drawFacts(["http://draw.example"], { draw_chain_id: 1 } as Manifest, [11], () => {}, 0),
    ).rejects.toThrow("the draw RPC is chain 4663; the record draws on chain 1");
    vi.unstubAllGlobals();
  });
});

describe("the chain", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("lists the configured RPCs and the dev anvil on this machine", () => {
    expect(rpcList("https://a.example/rpc, https://b.example", "venin.example")).toEqual([
      "https://a.example/rpc",
      "https://b.example",
    ]);
    expect(rpcList("", "localhost")).toEqual(["http://127.0.0.1:8546"]);
  });

  const manifest = { anchor: { address: "0xAbC", chain_id: 31337 } } as Manifest;
  const anchor: Anchor = { kind: "hour", n: 3, tx: "0xt3", block: 9 };

  function stub(answers: Record<string, unknown>, down = "") {
    const calls: string[] = [];
    vi.stubGlobal(
      "fetch",
      vi.fn(async (url: string, init: { body: string }) => {
        const { method } = JSON.parse(init.body) as { method: string };
        calls.push(`${new URL(url).host} ${method}`);
        if (down && url.includes(down)) throw new Error("refused");
        return { json: async () => ({ result: answers[method] }) };
      }),
    );
    return calls;
  }

  it("reads each anchor, its block time and the nonce, falling back past an RPC that is down", async () => {
    const calls = stub(
      {
        eth_chainId: "0x7a69",
        eth_getTransactionByHash: { blockNumber: "0x9", from: "0xABC", to: "0xAbC", input: "0xDEAD" },
        eth_getBlockByNumber: { timestamp: "0x10" },
        eth_getTransactionCount: "0x4",
      },
      "down.example",
    );
    const seen: number[] = [];
    const got = await anchorFacts(
      ["http://down.example", "http://up.example"],
      manifest,
      [anchor],
      (d) => seen.push(d),
      0,
    );
    expect(got).toEqual({
      nonce: 4,
      txs: { "0xt3": { from: "0xabc", to: "0xabc", input: "0xdead", block: 9, ts: 16 } },
      pending: [],
      absent: [],
    });
    expect(seen).toEqual([1, 2, 3, 4]);
    expect(calls.filter((c) => c.startsWith("up.example"))).toHaveLength(4);
  });

  it("names anchors the chain does not hold or has not mined", async () => {
    stub({ eth_chainId: "0x7a69", eth_getTransactionByHash: null, eth_getTransactionCount: "0x4" });
    expect(await anchorFacts(["http://x.example"], manifest, [anchor], () => {}, 0)).toEqual({
      nonce: 4,
      txs: {},
      pending: [],
      absent: ["0xt3"],
    });
    stub({ eth_chainId: "0x7a69", eth_getTransactionByHash: { blockNumber: null }, eth_getTransactionCount: "0x4" });
    expect((await anchorFacts(["http://x.example"], manifest, [anchor], () => {}, 0)).pending).toEqual(["0xt3"]);
  });

  it("refuses an RPC on another chain", async () => {
    stub({ eth_chainId: "0x1" });
    await expect(anchorFacts(["http://x.example"], manifest, [anchor], () => {}, 0)).rejects.toThrow(
      "the RPC is chain 1; the record anchors on chain 31337",
    );
  });
});

describe("verify in WebAssembly", () => {
  initSync({ module: readFileSync(new URL("web/src/wasm/venin_core_bg.wasm", root)) });
  const golden = (rel: string) => read(`crates/core/tests/fixtures/golden/${rel}`);
  const library = (rel: string) => read(`crates/core/tests/fixtures/library/${rel}`);
  const run = (bundles: Record<string, string | null>, cards: Record<string, string | null> = {}) =>
    answer<Verified>(
      verify(
        JSON.stringify({
          manifest: golden("manifest.json"),
          exam: golden("exam.json"),
          anchors: golden("anchors.json"),
          head: golden("head.json"),
          hours: { 0: golden("hours/0.json"), 1: golden("hours/1.json") },
          cards,
          takedowns: [],
        }),
        JSON.stringify(bundles),
        golden("chain.json"),
        JSON.stringify({
          strains: library("strains.jsonl"),
          tasks: library("tasks.jsonl"),
          genes: library("genes.json"),
          rows: library("rows.json"),
          names: library("voc-names.txt"),
        }),
      ),
    );

  it("recomputes the golden record from the page's own inputs", () => {
    const v = run({ 1: golden("runs/1.jsonl") });
    expect(v.ok).toBe(true);
    expect(v.report.hours).toBe(2);
    expect(v.replayed).toEqual([1]);
    expect(v.not_loaded).toEqual([0]);
    expect(v.report.headline_hours, "exam hour 0 was not replayed").toEqual([]);
    expect(run({ 0: golden("runs/0.jsonl") }).report.headline_hours).toEqual([0]);
    const lost = run({ 0: null });
    expect(lost.ok).toBe(false);
    expect(lost.report.errors).toContain("runs/0.jsonl.gz: missing");
  });

  it("compares the cards it is given with the cards the core tells, and names one that was changed", () => {
    const runs = { 1: golden("runs/1.jsonl") };
    expect(run(runs, { 0: golden("cards/0.json"), 1: golden("cards/1.json") }).ok).toBe(true);
    expect(run(runs, { 0: null, 1: null }).ok, "a card the site does not have is not compared").toBe(true);
    const changed = run(runs, { 1: golden("cards/1.json").replace("left circulation", "died out") });
    expect(changed.ok).toBe(false);
    expect(changed.report.errors).toEqual(["cards/1.json: differs from the card of hours/1.json"]);
  });

  it("throws the core's own words on bad input", () => {
    expect(() => answer(verify("{}", "{}", "{}", "{}"))).toThrow(/^records:/);
  });
});
