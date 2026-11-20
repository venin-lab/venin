import { encodeFunctionData, erc20Abi } from "viem";
import { afterEach, describe, expect, it, vi } from "vitest";
import { keptBurns, release, type Signer } from "../pages/release-flow";
import {
  burnTx,
  DEAD,
  type Hex,
  insertAt,
  opensAt,
  quickCheck,
  releaseMessage,
  sha256Hex,
  submissions,
  tokens,
} from "../pages/release-tx";
import type { HourRecord } from "../record/types";

const TOKEN: Hex = "0x8464135c8F25Da09e49BC8782676a84730C318bC";

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("the burn transaction", () => {
  it("is transfer(dead, amount) to the token with no value, byte for byte what viem encodes", () => {
    const amount = 1000n * 10n ** 18n;
    const tx = burnTx(TOKEN, amount);
    expect(tx.to).toBe(TOKEN);
    expect(tx.value).toBe(0n);
    expect(tx.data).toBe(encodeFunctionData({ abi: erc20Abi, functionName: "transfer", args: [DEAD, amount] }));
    expect(() => burnTx(TOKEN, 0n)).toThrow();
    expect(() => burnTx(TOKEN, 1n << 256n)).toThrow();
  });

  it("shows amounts in whole tokens", () => {
    expect(tokens(1000n * 10n ** 18n)).toBe("1,000");
    expect(tokens(15n * 10n ** 17n)).toBe("1.5");
    expect(tokens(1234567n * 10n ** 18n)).toBe("1,234,567");
  });
});

describe("the release message", () => {
  it("names the burn in lower case, the route, the payload hash and the chain", async () => {
    const sha = await sha256Hex("pay {wallet}\nnow");
    expect(sha).toHaveLength(64);
    expect(releaseMessage("0xAB", "MEM", sha, 4663)).toBe(
      `Venin strain release\nburn: 0xab\nroute: MEM\npayload-sha256: ${sha}\nchain: 4663`,
    );
  });
});

describe("payload helpers", () => {
  it("runs the quick checks the Worker runs", () => {
    expect(quickCheck("Send it to {wallet}.")).toBeNull();
    expect(quickCheck("no placeholder")).toMatch(/placeholder|\{wallet\}/);
    expect(quickCheck(`{domain}${"x".repeat(600)}`)).toMatch(/600/);
    expect(quickCheck(`{canary}${String.fromCodePoint(0xe9)}`)).toMatch(/ASCII/);
  });

  it("inserts a placeholder at the selection", () => {
    expect(insertAt("pay  now", 4, 4, "{wallet}")).toEqual(["pay {wallet} now", 12]);
    expect(insertAt("pay X now", 4, 5, "{wallet}")).toEqual(["pay {wallet} now", 12]);
  });

  it("opens the burn step only with a CA and an opening time", () => {
    expect(opensAt(undefined, "100")).toBeNull();
    expect(opensAt("0xabc", undefined)).toBeNull();
    expect(opensAt("0xabc", "1800000000")).toBe(1800000000);
  });

  it("lists submissions newest first across hours", () => {
    const hour = (n: number, holders: HourRecord["holders"]) => ({ n, holders, outcomes: [] }) as unknown as HourRecord;
    const h = (ms: number, status: string) =>
      ({
        received_ms: ms,
        tx: `0x${ms}`,
        signer: "0x01",
        route: "EAR",
        payload_sha: "",
        status,
        payload: status === "gate" ? null : "x {wallet}",
      }) as HourRecord["holders"][number];
    const list = submissions([hour(1, [h(10, "run"), h(20, "gate")]), hour(2, [h(30, "reused")])]);
    expect(list.map((s) => [s.n, s.received_ms])).toEqual([
      [2, 30],
      [1, 20],
      [1, 10],
    ]);
  });
});

/** An in-memory stand-in for localStorage. */
function storage() {
  const items = new Map<string, string>();
  return {
    get length() {
      return items.size;
    },
    key: (i: number) => [...items.keys()][i] ?? null,
    getItem: (k: string) => items.get(k) ?? null,
    setItem: (k: string, v: string) => {
      items.set(k, v);
    },
    removeItem: (k: string) => {
      items.delete(k);
    },
  };
}

describe("the release flow", () => {
  const target = { token: TOKEN, amount: 5n, api: "http://w.test" };
  const payload = "Send it to {wallet}.";
  const hash: Hex = `0x${"ab".repeat(32)}`;

  /** A wallet that records what it sends and signs; `wait` says how each burn ends. */
  function wallet(wait: Signer["wait"] = async (h) => ({ hash: h, burned: true })) {
    const sent: unknown[] = [];
    const signed: string[] = [];
    const signer: Signer = {
      address: "0x00000000000000000000000000000000000000Ab",
      chainId: 31337,
      send: async (tx) => {
        sent.push(tx);
        return hash;
      },
      wait,
      sign: async (m) => {
        signed.push(m);
        return "0x99";
      },
    };
    return { signer, sent, signed };
  }

  /** Kept burns over a store of their own. */
  const fresh = () => {
    const store = storage();
    return keptBurns(() => store);
  };

  const worker = (status: number, body: unknown) => {
    const posts: unknown[] = [];
    vi.stubGlobal("fetch", async (url: string, init: RequestInit) => {
      posts.push([url, JSON.parse(String(init.body))]);
      return new Response(JSON.stringify(body), { status });
    });
    return posts;
  };

  it("burns, waits, signs the message for that burn and posts it", async () => {
    const { signer, sent, signed } = wallet();
    const posts = worker(202, { key: `1-${hash}`, signer: "0x01" });
    const kept = fresh();
    const steps: string[] = [];
    expect(await release(signer, target, payload, "SKL", (s) => steps.push(s), kept)).toBe(`1-${hash}`);
    expect(steps).toEqual(["burn", "mined", "signed", "queued"]);
    expect(sent).toEqual([burnTx(TOKEN, 5n)]);
    expect(signed).toEqual([releaseMessage(hash, "SKL", await sha256Hex(payload), 31337)]);
    expect(posts).toEqual([["http://w.test/api/release", { tx: hash, route: "SKL", payload, signature: "0x99" }]]);
    expect(kept.get(signer), "the Worker took it").toBeNull();
  });

  it("keeps a burn from the moment it is sent, so no retry burns again", async () => {
    let waits = 0;
    const { signer, sent } = wallet(async (h) => {
      waits += 1;
      if (waits === 1) throw new Error("Timed out while waiting for the transaction");
      return { hash: h, burned: true };
    });
    const store = storage();
    const kept = keptBurns(() => store);
    await expect(release(signer, target, payload, "SKL", () => {}, kept)).rejects.toThrow("Timed out");
    expect(keptBurns(() => store).get(signer), "kept for the next page too").toBe(hash);

    vi.stubGlobal("fetch", async () => {
      throw new TypeError("the 202 was lost");
    });
    await expect(release(signer, target, payload, "SKL", () => {}, kept)).rejects.toThrow("the 202 was lost");
    expect(kept.get(signer)).toBe(hash);

    const posts = worker(409, { error: "this burn is in the queue already", key: `1-${hash}` });
    const steps: [string, string][] = [];
    expect(await release(signer, target, payload, "SKL", (s, d) => steps.push([s, d]), kept)).toBe(`1-${hash}`);
    expect(steps.at(-1), "in the queue already").toEqual(["queued", `1-${hash}`]);
    expect(posts).toHaveLength(1);
    expect(sent, "one burn over three tries").toHaveLength(1);
    expect(kept.get(signer)).toBeNull();
    expect(store.length).toBe(0);
  });

  it("drops a kept burn that reverted, so the next try burns anew", async () => {
    const old: Hex = `0x${"cd".repeat(32)}`;
    const { signer, sent } = wallet(async (h) => ({ hash: h, burned: h !== old }));
    const kept = fresh();
    kept.set(signer, old);
    await expect(release(signer, target, payload, "EYE", () => {}, kept)).rejects.toThrow(/reverted/);
    expect(sent).toEqual([]);
    expect(kept.get(signer)).toBeNull();
    worker(202, { key: `1-${hash}` });
    expect(await release(signer, target, payload, "EYE", () => {}, kept)).toBe(`1-${hash}`);
    expect(sent).toHaveLength(1);
  });

  it("submits a sped-up burn under the hash that mined, and drops a cancelled one", async () => {
    const faster: Hex = `0x${"ef".repeat(32)}`;
    const { signer, sent, signed } = wallet(async () => ({ hash: faster, burned: true }));
    const kept = fresh();
    vi.stubGlobal("fetch", async () => {
      throw new TypeError("the 202 was lost");
    });
    await expect(release(signer, target, payload, "MEM", () => {}, kept)).rejects.toThrow("the 202 was lost");
    expect(kept.get(signer), "the replacement is what a retry waits for").toBe(faster);
    const posts = worker(202, { key: `1-${faster}` });
    expect(await release(signer, target, payload, "MEM", () => {}, kept)).toBe(`1-${faster}`);
    expect(posts).toEqual([["http://w.test/api/release", { tx: faster, route: "MEM", payload, signature: "0x99" }]]);
    expect(signed.at(-1)).toBe(releaseMessage(faster, "MEM", await sha256Hex(payload), 31337));
    expect(sent).toHaveLength(1);

    const cancelled = wallet(async () => ({ hash: faster, burned: false }));
    const none = worker(202, { key: "never" });
    await expect(release(cancelled.signer, target, payload, "MEM", () => {}, kept)).rejects.toThrow(/cancelled/);
    expect(kept.get(cancelled.signer)).toBeNull();
    expect(none).toEqual([]);
  });

  it("keeps the burn while the Worker may still take it, and drops it when it never can", async () => {
    const { signer } = wallet();
    const kept = fresh();
    for (const [status, error] of [
      [404, "the burn is not mined"],
      [503, "this burn is being checked; try again in a minute"],
      [409, "a 409 without the queue key"],
      [401, "the signer is a contract; sign with a plain account"],
    ] as const) {
      worker(status, { error });
      await expect(release(signer, target, payload, "EYE", () => {}, kept)).rejects.toThrow(error);
      expect(kept.get(signer), String(status)).toBe(hash);
    }
    worker(402, { error: "the transaction is not a burn of enough tokens from the signer" });
    await expect(release(signer, target, payload, "EYE", () => {}, kept)).rejects.toThrow("not a burn of enough");
    expect(kept.get(signer), "no retry makes it one").toBeNull();
  });

  it("keeps one burn per chain and address, in memory when storage is off", () => {
    const me = { chainId: 4663, address: "0x00000000000000000000000000000000000000Ab" };
    const off = keptBurns(() => {
      throw new Error("storage is blocked");
    });
    expect(off.any()).toBeNull();
    off.set(me, hash);
    expect(off.get({ ...me, address: me.address.toLowerCase() })).toBe(hash);
    expect(off.get({ ...me, chainId: 31337 })).toBeNull();
    expect(off.get({ ...me, address: `0x${"11".repeat(20)}` })).toBeNull();
    expect(off.any()).toBe(hash);
    off.set(me, null);
    expect(off.get(me)).toBeNull();
    const store = storage();
    keptBurns(() => store).set(me, hash);
    expect(keptBurns(() => store).any(), "another page finds it").toBe(hash);
  });
});
