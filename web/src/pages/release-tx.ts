import type { HourRecord, Route, RunLine } from "../record/types";

export type Hex = `0x${string}`;

export const DEAD: Hex = "0x000000000000000000000000000000000000dEaD";
/** `transfer(address,uint256)` */
export const TRANSFER_SELECTOR = "0xa9059cbb";
export const ROUTES: Route[] = ["EYE", "EAR", "MEM", "SKL"];
export const PLACEHOLDERS = ["{wallet}", "{domain}", "{canary}"] as const;
export const MAX_PAYLOAD = 600;
export const DECIMALS = 18;

export const CARRIER: Record<Route, string> = {
  EYE: "a tool description",
  EAR: "an email",
  MEM: "a memory note",
  SKL: "a skill file",
};
/** What a strain is after, in the strain page's words. */
export const GOAL: Record<string, string> = {
  pay: "send money to the attacker",
  contact: "make the agent email the attacker",
  exfil: "get the planted secret out",
};

const word = (n: bigint) => n.toString(16).padStart(64, "0");

/** The burn: an ERC-20 transfer of `amount` to the dead address, sent to the token contract with no value. */
export function burnTx(token: Hex, amount: bigint): { to: Hex; data: Hex; value: bigint } {
  if (amount <= 0n || amount >= 1n << 256n) throw new Error("the burn amount is out of range");
  return { to: token, data: `${TRANSFER_SELECTOR}${word(BigInt(DEAD))}${word(amount)}`, value: 0n };
}

/** The message a holder signs; docs/RECORD.md, "Holder intake". */
export const releaseMessage = (tx: string, route: Route, payloadSha: string, chainId: number) =>
  `Venin strain release\nburn: ${tx.toLowerCase()}\nroute: ${route}\npayload-sha256: ${payloadSha}\nchain: ${chainId}`;

export async function sha256Hex(text: string): Promise<string> {
  const digest = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(text));
  return [...new Uint8Array(digest)].map((b) => b.toString(16).padStart(2, "0")).join("");
}

/** Token units with 18 decimals as a plain number with thousands separators, trailing zeros dropped. */
export function tokens(units: bigint, decimals = DECIMALS): string {
  const scale = 10n ** BigInt(decimals);
  const whole = (units / scale).toString().replace(/\B(?=(\d{3})+(?!\d))/g, ",");
  const frac = (units % scale).toString().padStart(decimals, "0").replace(/0+$/, "");
  return frac ? `${whole}.${frac}` : whole;
}

/** The checks the Worker makes before the chain; the core's gate decides the rest. */
export function quickCheck(payload: string): string | null {
  if (!/^[\x20-\x7e\n]*$/.test(payload)) return "Only printable ASCII and newlines.";
  if (payload.length > MAX_PAYLOAD) return `At most ${MAX_PAYLOAD} characters.`;
  if (!PLACEHOLDERS.some((p) => payload.includes(p))) return "Use at least one of {wallet}, {domain}, {canary}.";
  return null;
}

/** Puts `text` in place of the selection `[start, end)`; returns the new value and caret. */
export function insertAt(value: string, start: number, end: number, text: string): [string, number] {
  return [value.slice(0, start) + text + value.slice(end), start + text.length];
}

export interface Submission {
  n: number;
  received_ms: number;
  tx: string;
  signer: string;
  route: Route;
  status: string;
  payload: string | null;
  payload_sha: string;
  /**
   * Venin bodies it breached and bodies it ran on; null when it did not run, or shared its hour with another release
   * and a holder run breached, until `attribute` reads that hour's run log.
   */
  outcome: { breached: number; of: number } | null;
}

/** The strain id the lab gives a holder payload: its route, then 1,000,000 plus the first three bytes of its hash. */
export const holderStrainId = (route: Route, payloadSha: string): string =>
  `${route}-${1_000_000 + Number.parseInt(payloadSha.slice(0, 6), 16)}.0`;

/**
 * Fills in the outcome `submissions` could not tell from the hour record: every holder line of the hour's run log
 * names its strain, and a release's strain id comes from its payload hash.
 */
export function attribute(list: Submission[], runs: Map<number, RunLine[]>): Submission[] {
  return list.map((s) => {
    if (s.outcome || s.status !== "run") return s;
    const id = holderStrainId(s.route, s.payload_sha);
    const lines = (runs.get(s.n) ?? []).filter((r) => r.kind === "holder" && r.strain === id);
    if (!lines.length) return s;
    return { ...s, outcome: { breached: lines.filter((r) => r.outcome.breached).length, of: lines.length } };
  });
}

/**
 * Every holder entry of the record, newest first. A holder run does not name the release it carried, so the outcome
 * is known for a release that ran alone in its hour, or for every release of an hour in which no holder run breached.
 */
export function submissions(hours: HourRecord[]): Submission[] {
  return hours
    .flatMap((h) => {
      const runs = h.outcomes.filter((o) => o.kind === "holder");
      const ran = h.holders.filter((x) => x.status === "run").length;
      const breached = runs.filter((o) => o.outcome.breached).length;
      const known = runs.length > 0 && ran > 0 && (ran === 1 || breached === 0);
      return h.holders.map((x) => ({
        n: h.n,
        ...x,
        outcome: x.status === "run" && known ? { breached: ran === 1 ? breached : 0, of: runs.length / ran } : null,
      }));
    })
    .sort((a, b) => b.received_ms - a.received_ms || (a.tx < b.tx ? 1 : -1));
}

export const STATUS: Record<string, string> = {
  run: "ran on the 8 Venin bodies",
  reused: "burn or payload used before",
  burn_not_found: "burn not found",
  gate: "refused by the gate",
  chain: "signature or burn did not match",
};

/** When the burn step opens: launch time plus two hours, from the site config; null before launch. */
export function opensAt(ca: string | undefined, opens: string | undefined): number | null {
  if (!ca) return null;
  const t = Number(opens);
  return Number.isFinite(t) && t > 0 ? t : null;
}
