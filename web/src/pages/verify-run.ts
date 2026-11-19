import { DATA_URL, gunzipText } from "../record/load";
import type { Anchor, Manifest } from "../record/types";

/** Periods replayed by default: the latest ones, then a few drawn at random from before them. */
export const RECENT = 24;
export const SAMPLE = 8;

/** The last `RECENT` periods up to `head` and up to `SAMPLE` earlier ones picked by `random`, in order. */
export function defaultScope(head: number, random: () => number = Math.random): number[] {
  const first = Math.max(0, head - RECENT + 1);
  const earlier = Array.from({ length: first }, (_, i) => i);
  const picked: number[] = [];
  while (picked.length < SAMPLE && earlier.length > 0) {
    picked.push(earlier.splice(Math.floor(random() * earlier.length), 1)[0]);
  }
  const recent = Array.from({ length: head - first + 1 }, (_, i) => first + i);
  return [...picked, ...recent].sort((a, b) => a - b);
}

/** No such file: a 404, or the page itself, which a dev server sends for any path it does not know. */
const absent = (res: Response) => res.status === 404 || (res.headers.get("content-type") ?? "").startsWith("text/html");

/** A published file's exact text, or null when the site does not have it. */
async function text(path: string): Promise<string | null> {
  const res = await fetch(`${DATA_URL}/${path}`);
  if (absent(res)) return null;
  if (!res.ok) throw new Error(`${path}: HTTP ${res.status}`);
  return res.text();
}

async function need(path: string): Promise<string> {
  const t = await text(path);
  if (t === null) throw new Error(`${path}: missing`);
  return t;
}

export interface Records {
  manifest: string;
  exam: string;
  anchors: string;
  head: string;
  hours: Record<string, string>;
  takedowns: unknown[];
}

export interface Library {
  strains: string;
  tasks: string;
  genes: string;
  rows: string;
  names: string;
}

/**
 * The record files and the library, byte for byte as the site serves them. The hours are the ones the head read here
 * names, not the page's: a page left open past an upload would pair the new anchors.json with its old hours.
 */
export async function readRecord(): Promise<{ records: Records; library: Library }> {
  const [manifest, exam, anchors, head, takedowns, strains, tasks, genes, rows, names] = await Promise.all([
    need("manifest.json"),
    need("exam.json"),
    need("anchors.json"),
    need("head.json"),
    text("takedowns.json"),
    need("library/strains.jsonl"),
    need("library/tasks.jsonl"),
    need("library/genes.json"),
    need("library/rows.json"),
    need("library/voc-names.txt"),
  ]);
  const hours: Record<string, string> = {};
  const ns = Array.from({ length: (JSON.parse(head) as { n: number }).n + 1 }, (_, n) => n);
  const texts = await Promise.all(ns.map((n) => need(`hours/${n}.json`)));
  ns.forEach((n, i) => {
    hours[n] = texts[i];
  });
  return {
    records: { manifest, exam, anchors, head, hours, takedowns: takedowns ? JSON.parse(takedowns) : [] },
    library: { strains, tasks, genes, rows, names },
  };
}

/** Each period's card file, or null when the site has none; verify compares the cards it is given. */
export async function readCards(periods: number[], cache: Map<number, string | null>): Promise<void> {
  await Promise.all(periods.filter((n) => !cache.has(n)).map(async (n) => cache.set(n, await text(`cards/${n}.json`))));
}

/** Each period's runs file after gunzip, or null when it was asked for and not found. */
export async function readRuns(periods: number[], cache: Map<number, string | null>): Promise<void> {
  await Promise.all(
    periods
      .filter((n) => !cache.has(n))
      .map(async (n) => {
        const res = await fetch(`${DATA_URL}/runs/${n}.jsonl.gz`);
        if (absent(res)) cache.set(n, null);
        else if (!res.ok) throw new Error(`runs/${n}.jsonl.gz: HTTP ${res.status}`);
        else cache.set(n, await gunzipText(new Uint8Array(await res.arrayBuffer())));
      }),
  );
}

export interface ChainBlock {
  hash: string;
  ts: number;
  parent_ts: number;
}

export interface Tx {
  from: string;
  to: string;
  input: string;
  block: number;
  ts: number;
}

/**
 * venin-core's verify::Chain: what the chains say, as far as it was read. A transaction the chain answered for is in
 * `txs`, `pending` (known, not mined) or `absent` (unknown to it).
 */
export interface Chain {
  nonce: number | null;
  txs: Record<string, Tx>;
  blocks: Record<string, ChainBlock>;
  pending: string[];
  absent: string[];
}

interface SavedBlock {
  number: number;
  hash: string;
  parent_hash: string;
  ts: number;
}

/** A draw block and its parent as the draw chain gives them; the chain fact they give, or why they do not. */
export function drawFact(saved: { block: SavedBlock; parent: SavedBlock }): [number, ChainBlock] | string {
  const { block, parent } = saved;
  if (parent.number + 1 !== block.number || block.parent_hash !== parent.hash) {
    return `draw block ${block.number} does not follow its parent`;
  }
  return [block.number, { hash: block.hash, ts: block.ts, parent_ts: parent.ts }];
}

/** RPCs to try in order: the configured list, then the dev anvil when the page is served from this machine. */
export function rpcList(configured: string, hostname: string): string[] {
  const list = configured
    .split(",")
    .map((s) => s.trim())
    .filter(Boolean);
  if (hostname === "localhost" || hostname === "127.0.0.1") list.push("http://127.0.0.1:8546");
  return [...new Set(list)];
}

const pause = (ms: number) => new Promise((r) => setTimeout(r, ms));
const quantity = (hex: unknown) => Number(BigInt(String(hex)));

/** One JSON-RPC call, trying each RPC in turn until one answers. */
async function call(rpcs: string[], method: string, params: unknown[]): Promise<unknown> {
  let last = "no RPC configured";
  for (const url of rpcs) {
    try {
      const res = await fetch(url, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ jsonrpc: "2.0", id: 1, method, params }),
      });
      const body = (await res.json()) as { result?: unknown; error?: { message?: string } };
      if (body.error) throw new Error(body.error.message ?? "RPC error");
      return body.result ?? null;
    } catch (e) {
      last = `${new URL(url).host}: ${e instanceof Error ? e.message : String(e)}`;
    }
  }
  throw new Error(last);
}

/** The anchors a scope checks: the manifest and exam anchors and the hour anchors of the scoped periods. */
export function anchorsIn(anchors: Anchor[], periods: number[]): Anchor[] {
  const scoped = new Set(periods);
  return anchors.filter((a) => a.kind !== "hour" || scoped.has(a.n));
}

/**
 * What the anchor chain says about `anchors` and the anchor address's nonce, one request at a time, `paceMs` apart.
 * `progress` hears how many requests are done.
 */
export async function anchorFacts(
  rpcs: string[],
  manifest: Manifest,
  anchors: Anchor[],
  progress: (done: number) => void,
  paceMs = 1000,
): Promise<Pick<Chain, "nonce" | "txs" | "pending" | "absent">> {
  let done = 0;
  const ask = async (method: string, params: unknown[]) => {
    if (done > 0) await pause(paceMs);
    const result = await call(rpcs, method, params);
    progress(++done);
    return result;
  };
  const id = quantity(await ask("eth_chainId", []));
  if (id !== manifest.anchor.chain_id) {
    throw new Error(`the RPC is chain ${id}; the record anchors on chain ${manifest.anchor.chain_id}`);
  }
  const times = new Map<number, number>();
  const txs: Record<string, Tx> = {};
  const pending: string[] = [];
  const absent: string[] = [];
  for (const a of anchors) {
    const tx = (await ask("eth_getTransactionByHash", [a.tx])) as Record<string, string> | null;
    if (tx === null) {
      absent.push(a.tx);
      continue;
    }
    if (!tx.blockNumber) {
      pending.push(a.tx);
      continue;
    }
    const block = quantity(tx.blockNumber);
    if (!times.has(block)) {
      const b = (await ask("eth_getBlockByNumber", [`0x${block.toString(16)}`, false])) as { timestamp: string };
      times.set(block, quantity(b.timestamp));
    }
    const low = (k: string) => (tx[k] ?? "").toLowerCase();
    txs[a.tx] = { from: low("from"), to: low("to"), input: low("input"), block, ts: times.get(block) ?? 0 };
  }
  const nonce = quantity(await ask("eth_getTransactionCount", [manifest.anchor.address, "latest"]));
  return { nonce, txs, pending, absent };
}

/**
 * The transaction count to hold `listed`, the anchors.json this pass verifies, to. The runner sends an hour's anchor
 * a minute or so before it publishes the list naming it, so for that minute the address has sent more than `listed`
 * holds. Then `now`, the list as published now, decides: when it starts with `listed` and names every transaction
 * sent, the extra ones are newer anchors and `listed` is whole up to them. Otherwise the chain's count stands, so a
 * transaction the lab never listed still fails the check.
 */
export function listedNonce(nonce: number, listed: Anchor[], now: Anchor[] | null): number {
  if (nonce <= listed.length || now === null || now.length < nonce) return nonce;
  return listed.every((a, i) => now[i]?.tx === a.tx) ? listed.length : nonce;
}

/** anchors.json as published this moment, past the browser's cached copy; null when it cannot be read. */
export async function anchorsNow(): Promise<Anchor[] | null> {
  try {
    const res = await fetch(`${DATA_URL}/anchors.json`, { cache: "no-store" });
    return res.ok ? ((await res.json()) as Anchor[]) : null;
  } catch {
    return null;
  }
}

/**
 * Each draw block in `numbers` and its parent as the draw chain gives them, `paceMs` apart; the runner's saved draws
 * are never taken as chain facts. `progress` hears how many requests are done.
 */
export async function drawFacts(
  rpcs: string[],
  manifest: Manifest,
  numbers: number[],
  progress: (done: number) => void,
  paceMs = 1000,
): Promise<Record<string, ChainBlock>> {
  let done = 0;
  const ask = async (method: string, params: unknown[]) => {
    if (done > 0) await pause(paceMs);
    const result = await call(rpcs, method, params);
    progress(++done);
    return result;
  };
  const id = quantity(await ask("eth_chainId", []));
  if (id !== manifest.draw_chain_id) {
    throw new Error(`the draw RPC is chain ${id}; the record draws on chain ${manifest.draw_chain_id}`);
  }
  const get = async (n: number): Promise<SavedBlock> => {
    const b = (await ask("eth_getBlockByNumber", [`0x${n.toString(16)}`, false])) as Record<string, string> | null;
    if (!b) throw new Error(`the draw chain has no block ${n}`);
    const hex = (k: string) => (b[k] ?? "").replace(/^0x/, "").toLowerCase();
    return { number: quantity(b.number), hash: hex("hash"), parent_hash: hex("parentHash"), ts: quantity(b.timestamp) };
  };
  const blocks: Record<string, ChainBlock> = {};
  for (const n of [...new Set(numbers)]) {
    const fact = drawFact({ block: await get(n), parent: await get(n - 1) });
    if (typeof fact === "string") throw new Error(fact);
    blocks[fact[0]] = fact[1];
  }
  return blocks;
}

/** The synthetic league's chain file, which stands in for both chains. */
export async function fixtureChain(): Promise<Chain> {
  return JSON.parse(await need("chain.json")) as Chain;
}
