import type {
  Anchor,
  Card,
  Exam,
  Gene,
  Head,
  HourRecord,
  Manifest,
  RunLine,
  Status,
  Strain,
  Takedown,
  Task,
} from "./types";

export const DATA_URL: string = (import.meta.env.VITE_DATA_URL ?? "/data").replace(/\/$/, "");

async function get(path: string): Promise<Response> {
  const res = await fetch(`${DATA_URL}/${path}`);
  if (!res.ok) throw new Error(`${path}: HTTP ${res.status}`);
  return res;
}

const json = async <T>(path: string): Promise<T> => (await get(path)).json() as Promise<T>;

/** Between genesis and the first hour the manifest is published and head.json is not yet. */
export class FirstHourPending extends Error {
  constructor(readonly manifest: Manifest) {
    super("The first hour has not been written yet");
  }
}

/**
 * A file that may not exist yet: null when it is missing, which a host says with a 404 and a dev server by answering
 * with the site's own page.
 */
export async function maybeJson<T>(path: string): Promise<T | null> {
  const res = await fetch(`${DATA_URL}/${path}`);
  if (res.status === 404) return null;
  if (!res.ok) throw new Error(`${path}: HTTP ${res.status}`);
  const text = await res.text();
  return text.trimStart().startsWith("<") ? null : (JSON.parse(text) as T);
}

/** Splits JSON Lines text into objects, skipping blank lines. */
export function parseLines<T>(text: string): T[] {
  return text
    .split("\n")
    .filter((line) => line.trim() !== "")
    .map((line) => JSON.parse(line) as T);
}

/** Gunzips when the bytes carry the gzip magic; a host may already have decoded them. */
export async function gunzipText(bytes: Uint8Array): Promise<string> {
  if (bytes[0] !== 0x1f || bytes[1] !== 0x8b) return new TextDecoder().decode(bytes);
  const stream = new Blob([bytes as BlobPart]).stream().pipeThrough(new DecompressionStream("gzip"));
  return new Response(stream).text();
}

export interface Lab {
  manifest: Manifest;
  exam: Exam;
  head: Head;
  status: Status | null;
  anchors: Anchor[];
  /** takedowns.json; empty when the site has none. */
  takedowns: Takedown[];
  hours: HourRecord[];
  genes: Gene[];
  strains: Strain[];
  tasks: Task[];
}

/** takedowns.json as the home page counts it: its entries that name a path; anything else lists none. */
export const takedownList = (v: unknown): Takedown[] =>
  Array.isArray(v) ? v.filter((t): t is Takedown => typeof t?.path === "string") : [];

async function all<T>(count: number, load: (i: number) => Promise<T>, width = 8): Promise<T[]> {
  const out = new Array<T>(count);
  let next = 0;
  const worker = async () => {
    while (next < count) {
      const i = next++;
      out[i] = await load(i);
    }
  };
  await Promise.all(Array.from({ length: Math.min(width, count) }, worker));
  return out;
}

export async function loadLab(): Promise<Lab> {
  const [manifest, exam, head, anchors, genes, strainsText, tasksText, status, takedowns] = await Promise.all([
    json<Manifest>("manifest.json"),
    json<Exam>("exam.json"),
    maybeJson<Head>("head.json"),
    json<Anchor[]>("anchors.json"),
    json<Gene[]>("library/genes.json"),
    get("library/strains.jsonl").then((r) => r.text()),
    get("library/tasks.jsonl").then((r) => r.text()),
    json<Status>("status.json").catch(() => null),
    json<unknown>("takedowns.json")
      .then(takedownList)
      .catch((): Takedown[] => []),
  ]);
  if (!head) throw new FirstHourPending(manifest);
  // Known limit: every hour file is fetched; past a few hundred periods this wants a published index of outcomes.
  const hours = await all(head.n + 1, (n) => json<HourRecord>(`hours/${n}.json`));
  return {
    manifest,
    exam,
    head,
    status,
    anchors,
    takedowns,
    hours,
    genes,
    strains: parseLines<Strain>(strainsText),
    tasks: parseLines<Task>(tasksText),
  };
}

const runCache = new Map<number, Promise<RunLine[]>>();

export function loadRuns(n: number): Promise<RunLine[]> {
  let hit = runCache.get(n);
  if (!hit) {
    hit = get(`runs/${n}.jsonl.gz`)
      .then((r) => r.arrayBuffer())
      .then((b) => gunzipText(new Uint8Array(b)))
      .then((text) => parseLines<RunLine>(text));
    hit.catch(() => runCache.delete(n));
    runCache.set(n, hit);
  }
  return hit;
}

const cardCache = new Map<number, Promise<Card | null>>();

/** The runner's card of hour `n`, or null before it has written one. */
export function loadCard(n: number): Promise<Card | null> {
  let hit = cardCache.get(n);
  if (!hit) {
    hit = json<Card>(`cards/${n}.json`).catch(() => null);
    cardCache.set(n, hit);
  }
  return hit;
}

let names: Promise<string[]> | null = null;

/** The names variants of concern take, in order (library/voc-names.txt); none when the list cannot be read. */
export function vocNames(): Promise<string[]> {
  names ??= get("library/voc-names.txt")
    .then((r) => r.text())
    .then((t) => t.split("\n").flatMap((s) => (s.trim() ? [s.trim()] : [])))
    .catch(() => []);
  return names;
}

/** The anchor chain RPCs from VITE_ANCHOR_RPC, comma-separated, tried in order. */
const ANCHOR_RPCS: string[] = ((import.meta.env.VITE_ANCHOR_RPC as string | undefined) ?? "")
  .split(",")
  .map((u) => u.trim())
  .filter(Boolean);

/** Block timestamps from a JSON-RPC batch reply, by block number. */
export function blockTimes(reply: { result?: { number?: string; timestamp?: string } | null }[]): Map<number, number> {
  const out = new Map<number, number>();
  for (const r of reply) {
    if (r.result?.number && r.result.timestamp) out.set(Number(r.result.number), Number(r.result.timestamp));
  }
  return out;
}

/**
 * Unix seconds of each hour anchor's block, by hour: from the anchor chain's RPC when VITE_ANCHOR_RPC names one, from
 * the fixture's own chain file for the synthetic league, else empty (verify then decides LATE).
 */
async function anchorTimes(lab: Lab): Promise<Map<number, number>> {
  const hourly = lab.anchors.filter((a) => a.kind === "hour");
  const out = new Map<number, number>();
  if (ANCHOR_RPCS.length) {
    const blocks = [...new Set(hourly.map((a) => a.block))];
    const times = new Map<number, number>();
    for (let i = 0; i < blocks.length; i += 100) {
      const batch = blocks.slice(i, i + 100).map((b, id) => ({
        jsonrpc: "2.0",
        id,
        method: "eth_getBlockByNumber",
        params: [`0x${b.toString(16)}`, false],
      }));
      let reply: unknown = null;
      for (const url of ANCHOR_RPCS) {
        try {
          const res = await fetch(url, {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify(batch),
          });
          reply = await res.json();
          if (Array.isArray(reply)) break;
        } catch {
          reply = null;
        }
      }
      if (!Array.isArray(reply)) throw new Error("no anchor RPC answered");
      for (const [b, ts] of blockTimes(reply)) times.set(b, ts);
    }
    for (const a of hourly) {
      const ts = times.get(a.block);
      if (ts !== undefined) out.set(a.n, ts);
    }
  } else if (lab.manifest.league === "synthetic fixture") {
    const chain = await json<{ txs: Record<string, { ts: number }> }>("chain.json");
    for (const a of hourly) {
      const tx = chain.txs[a.tx];
      if (tx) out.set(a.n, tx.ts);
    }
  }
  return out;
}

let timesCache: Promise<Map<number, number>> | null = null;

export function loadAnchorTimes(lab: Lab): Promise<Map<number, number>> {
  timesCache ??= anchorTimes(lab).catch(() => new Map<number, number>());
  return timesCache;
}
