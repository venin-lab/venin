import { type Hex, recoverMessageAddress } from "viem";

/** The parts of an R2 bucket the Worker uses. */
export interface Bucket {
  head(key: string): Promise<unknown>;
  /** Resolves to null when `onlyIf` does not hold, as R2 does. */
  put(
    key: string,
    value: string,
    options?: { onlyIf?: { etagDoesNotMatch?: string; etagMatches?: string } },
  ): Promise<unknown>;
  delete(key: string): Promise<void>;
  get(key: string): Promise<{ text(): Promise<string>; etag: string; uploaded: Date } | null>;
  list(options: { prefix: string; limit: number; startAfter?: string }): Promise<{ objects: { key: string }[] }>;
}

export interface Limiter {
  limit(options: { key: string }): Promise<{ success: boolean }>;
}

export interface Env {
  QUEUE: Bucket;
  LIMITER: Limiter;
  QUEUE_TOKEN: string;
  CHAIN_ID: string;
  TOKEN: string;
  BURN_AMOUNT: string;
  RPC_URL: string;
  ALLOWED_ORIGIN: string;
}

export const MAX_BODY = 4096;
/** A defend body: up to MAX_RULES of rules, each newline escaped as two bytes. */
export const MAX_DEFEND_BODY = 8192;
export const MAX_PAYLOAD = 600;
export const MAX_RULES = 2000;
export const PAGE = 50;
/** A burn's claim still `checking` after this long belongs to a check that stopped, and a new submission may take it. */
export const CLAIM_MS = 60_000;
const QUEUE_KEY = /^\d+-0x[0-9a-f]{64}$/;
export const DEAD = "0x000000000000000000000000000000000000dead";
/** keccak256("Transfer(address,address,uint256)") */
export const TRANSFER = "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef";
const ROUTES = ["EYE", "EAR", "MEM", "SKL"] as const;
export type Route = (typeof ROUTES)[number];
export const COLUMNS = ["m1", "m2", "m3", "m4"] as const;
export type Column = (typeof COLUMNS)[number];

export interface Release {
  tx: Hex;
  route: Route;
  payload: string;
  signature: Hex;
}

/** The message a holder signs; docs/RECORD.md, "Holder intake". */
export const message = (tx: string, route: Route, payloadSha: string, chainId: string) =>
  `Venin strain release\nburn: ${tx}\nroute: ${route}\npayload-sha256: ${payloadSha}\nchain: ${chainId}`;

export interface Defend {
  tx: Hex;
  column: Column;
  rules: string;
  signature: Hex;
}

/** The message a holder signs to put their agent in the ward; docs/RECORD.md, "Defend intake". */
export const defendMessage = (tx: string, column: Column, rulesSha: string, chainId: string) =>
  `Venin defend\nburn: ${tx}\nmodel: ${column}\nrules-sha256: ${rulesSha}\nchain: ${chainId}`;

export async function sha256Hex(text: string): Promise<string> {
  const digest = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(text));
  return [...new Uint8Array(digest)].map((b) => b.toString(16).padStart(2, "0")).join("");
}

export class Refused extends Error {
  constructor(
    readonly status: number,
    reason: string,
  ) {
    super(reason);
  }
}

/** A JSON object with exactly the `fields` named, its tx and signature checked; otherwise why it is refused. */
function fields(text: string, names: string[]): Record<string, unknown> {
  let v: unknown;
  try {
    v = JSON.parse(text);
  } catch {
    throw new Refused(400, "the body is not JSON");
  }
  const o = (typeof v === "object" && v !== null ? v : {}) as Record<string, unknown>;
  if (Object.keys(o).sort().join(",") !== [...names].sort().join(","))
    throw new Refused(400, `the body needs exactly ${names.join(", ")}`);
  const { tx, signature } = o;
  if (typeof tx !== "string" || !/^0x[0-9a-fA-F]{64}$/.test(tx)) throw new Refused(400, "tx is not a transaction hash");
  if (typeof signature !== "string" || !/^0x[0-9a-fA-F]{130}$/.test(signature)) {
    throw new Refused(400, "signature is not 65 bytes of hex");
  }
  return o;
}

/** The request's fields, or why they are refused. Hex is lower-cased; the route must be upper case. */
export function parse(text: string): Release {
  const o = fields(text, ["tx", "route", "payload", "signature"]);
  const { tx, route, payload, signature } = o as Record<string, string>;
  if (!ROUTES.includes(route as Route)) throw new Refused(400, "route is not one of EYE, EAR, MEM, SKL");
  if (typeof payload !== "string") throw new Refused(400, "payload is not a string");
  checkPayload(payload);
  return {
    tx: tx.toLowerCase() as Hex,
    route: route as Route,
    payload,
    signature: signature.toLowerCase() as Hex,
  };
}

/** A defend request's fields, or why they are refused. Hex is lower-cased; the column is `m1` to `m4`. */
export function parseDefend(text: string): Defend {
  const o = fields(text, ["tx", "column", "rules", "signature"]);
  const { tx, column, rules, signature } = o as Record<string, string>;
  if (!COLUMNS.includes(column as Column)) throw new Refused(400, "column is not one of m1, m2, m3, m4");
  if (typeof rules !== "string") throw new Refused(400, "rules is not a string");
  checkRules(rules);
  return {
    tx: tx.toLowerCase() as Hex,
    column: column as Column,
    rules,
    signature: signature.toLowerCase() as Hex,
  };
}

/** The Worker's cheap part of the rules gate; the runner runs the whole gate again. */
export function checkRules(rules: string): void {
  if (!/^[\x20-\x7e\n]*$/.test(rules)) throw new Refused(422, "the rules must be printable ASCII and newlines");
  if (rules.length > MAX_RULES) throw new Refused(422, `the rules are longer than ${MAX_RULES} bytes`);
  if (!rules.trim()) throw new Refused(422, "the rules are empty");
}

/** The Worker's cheap part of the gate; the runner runs the whole gate again. */
export function checkPayload(payload: string): void {
  if (!/^[\x20-\x7e\n]*$/.test(payload)) throw new Refused(422, "the payload must be printable ASCII and newlines");
  if (payload.length > MAX_PAYLOAD) throw new Refused(422, `the payload is longer than ${MAX_PAYLOAD} bytes`);
  if (!/\{(wallet|domain|canary)\}/.test(payload))
    throw new Refused(422, "the payload needs {wallet}, {domain} or {canary}");
}

/** One JSON-RPC call; RPC_URL may list several endpoints, comma-separated, tried in order until one answers. */
async function rpc(env: Env, method: string, params: unknown[]): Promise<unknown> {
  let last = new Refused(502, "no RPC configured");
  for (const url of env.RPC_URL.split(",")
    .map((u) => u.trim())
    .filter(Boolean)) {
    try {
      const res = await fetch(url, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ jsonrpc: "2.0", id: 1, method, params }),
      });
      if (!res.ok) throw new Refused(502, `the chain answered HTTP ${res.status}`);
      const body = (await res.json()) as { result?: unknown; error?: unknown };
      if (body.error !== undefined) throw new Refused(502, "the chain refused the request");
      return body.result ?? null;
    } catch (e) {
      last = e instanceof Refused ? e : new Refused(502, "the chain could not be reached");
    }
  }
  throw last;
}

interface Log {
  address: string;
  topics: string[];
  data: string;
}

const topic = (address: string) => `0x${address.slice(2).toLowerCase().padStart(64, "0")}`;

/** True when the receipt holds a successful transfer of at least `amount` of `token` from `from` to the dead address. */
export function burns(
  receipt: { status?: string; logs?: Log[] },
  token: string,
  from: string,
  amount: bigint,
): boolean {
  if (receipt.status !== "0x1") return false;
  return (receipt.logs ?? []).some((log) => {
    const t = log.topics.map((x) => x.toLowerCase());
    return (
      log.address.toLowerCase() === token.toLowerCase() &&
      t.length === 3 &&
      t[0] === TRANSFER &&
      t[1] === topic(from) &&
      t[2] === topic(DEAD) &&
      /^0x[0-9a-fA-F]{1,64}$/.test(log.data) &&
      BigInt(log.data) >= amount
    );
  });
}

/** Byte-wise comparison that does not stop at the first difference. */
function same(a: string, b: string): boolean {
  const x = new TextEncoder().encode(a);
  const y = new TextEncoder().encode(b);
  let diff = x.length ^ y.length;
  for (let i = 0; i < Math.max(x.length, y.length); i++) diff |= (x[i] ?? 0) ^ (y[i] ?? 0);
  return diff === 0;
}

export function reply(env: Env, status: number, body: unknown, extra: Record<string, string> = {}): Response {
  const headers: Record<string, string> = { "content-type": "application/json", "cache-control": "no-store", ...extra };
  if (env.ALLOWED_ORIGIN) {
    headers["access-control-allow-origin"] = env.ALLOWED_ORIGIN;
    headers["access-control-allow-methods"] = "POST";
    headers["access-control-allow-headers"] = "content-type";
    headers.vary = "origin";
  }
  return new Response(status === 204 ? null : JSON.stringify(body), { status, headers });
}

async function body(request: Request, env: Env, max: number): Promise<string> {
  if (!env.TOKEN || !env.BURN_AMOUNT) throw new Refused(503, "releases are not open");
  const length = Number(request.headers.get("content-length") ?? "0");
  if (length > max) throw new Refused(413, `the body is over ${max} bytes`);
  const text = await request.text();
  if (new TextEncoder().encode(text).length > max) throw new Refused(413, `the body is over ${max} bytes`);
  return text;
}

async function signerOf(text: string, signature: Hex): Promise<string> {
  try {
    return (await recoverMessageAddress({ message: text, signature })).toLowerCase();
  } catch {
    throw new Refused(401, "the signature does not recover");
  }
}

/**
 * Checks a release in the order of docs/RECORD.md and queues it. One submission of a burn at a time holds its claim
 * while it is checked; a burn queued before is answered 409 with its queue key, one still being checked 503.
 */
export async function release(request: Request, env: Env, now: number): Promise<Response> {
  const r = parse(await body(request, env, MAX_BODY));
  const sha = await sha256Hex(r.payload);
  const signer = await signerOf(message(r.tx, r.route, sha, env.CHAIN_ID), r.signature);
  return queueBurn(env, now, r.tx, signer, "queue", { route: r.route, payload: r.payload, signature: r.signature });
}

/** Checks a defend submission as a release is checked and queues it under `defend/`; a burn serves one or the other. */
export async function defend(request: Request, env: Env, now: number): Promise<Response> {
  const d = parseDefend(await body(request, env, MAX_DEFEND_BODY));
  const sha = await sha256Hex(d.rules);
  const signer = await signerOf(defendMessage(d.tx, d.column, sha, env.CHAIN_ID), d.signature);
  return queueBurn(env, now, d.tx, signer, "defend", { column: d.column, rules: d.rules, signature: d.signature });
}

/** Claims the burn, checks it on chain and queues `entry` with it under `prefix`. */
async function queueBurn(
  env: Env,
  now: number,
  tx: Hex,
  signer: string,
  prefix: "queue" | "defend",
  entry: Record<string, string>,
): Promise<Response> {
  const burn = `burns/${tx}`;
  let claimed = await env.QUEUE.put(burn, "checking", { onlyIf: { etagDoesNotMatch: "*" } });
  if (!claimed) {
    const held = await env.QUEUE.get(burn);
    const value = held ? await held.text() : "";
    if (QUEUE_KEY.test(value)) return reply(env, 409, { error: "this burn is in the queue already", key: value });
    if (held && now - held.uploaded.getTime() > CLAIM_MS) {
      // Known limit: a check slower than CLAIM_MS can overlap the one that takes its claim, and both may queue the
      // burn; the runner records the second entry as reused.
      claimed = await env.QUEUE.put(burn, "checking", { onlyIf: { etagMatches: held.etag } });
    }
    if (!claimed) {
      return reply(env, 503, { error: "this burn is being checked; try again in a minute" }, { "retry-after": "60" });
    }
  }
  try {
    const code = await rpc(env, "eth_getCode", [signer, "latest"]);
    if (code !== "0x" && code !== null) throw new Refused(401, "the signer is a contract; sign with a plain account");
    const receipt = (await rpc(env, "eth_getTransactionReceipt", [tx])) as { status?: string; logs?: Log[] } | null;
    if (receipt === null) throw new Refused(404, "the burn is not mined");
    if (!burns(receipt, env.TOKEN, signer, BigInt(env.BURN_AMOUNT))) {
      throw new Refused(402, "the transaction is not a burn of enough tokens from the signer");
    }
  } catch (e) {
    await env.QUEUE.delete(burn);
    throw e;
  }
  const key = `${now}-${tx}`;
  const item = { received_ms: now, tx, signer, ...entry };
  await env.QUEUE.put(`${prefix}/${key}.json`, JSON.stringify(item));
  await env.QUEUE.put(burn, key);
  return reply(env, 202, { key, signer });
}

/** Up to `PAGE` entries under `prefix` (`queue` for releases, `defend` for agents) after the cursor, for the runner. */
export async function queue(request: Request, env: Env, prefix: "queue" | "defend" = "queue"): Promise<Response> {
  const auth = request.headers.get("authorization") ?? "";
  if (!env.QUEUE_TOKEN || !same(auth, `Bearer ${env.QUEUE_TOKEN}`)) throw new Refused(401, "a bearer token is needed");
  const after = new URL(request.url).searchParams.get("after") ?? "";
  if (!/^(\d+-0x[0-9a-f]{64})?$/.test(after)) throw new Refused(400, "after is not a queue key");
  const listed = await env.QUEUE.list({
    prefix: `${prefix}/`,
    limit: PAGE,
    ...(after ? { startAfter: `${prefix}/${after}.json` } : {}),
  });
  const items = [];
  for (const { key } of listed.objects) {
    const object = await env.QUEUE.get(key);
    if (object) items.push(JSON.parse(await object.text()));
  }
  return reply(env, 200, { items });
}
