import type { Column, Route, Strain } from "../record/types";
import { defendMessage } from "./defend-view";
import { burnTx, type Hex, releaseMessage, sha256Hex } from "./release-tx";
import { core as wasm } from "./verify-wasm";

/** The core's `gate` export: whether a payload passes, the strain id it would get and the organ it enters by. */
export interface GateResult {
  ok: boolean;
  reason: string | null;
  strain_preview: string | null;
  organ: string | null;
}

export interface Core {
  gate(payload: string, route: Route): GateResult;
  mutate(strain: Strain, operator: string, seedHex: string): Strain;
  /** The core's rules gate for a defended agent. */
  defendGate(rules: string): { ok: boolean; reason: string | null };
}

/** The core's `gate` and `mutate`, shared with /verify's load of the WebAssembly build. */
export async function loadCore(): Promise<Core> {
  const m = await wasm();
  return {
    gate: (payload, route) => JSON.parse(m.gate(payload, route)) as GateResult,
    mutate: (strain, operator, seedHex) => JSON.parse(m.mutate(JSON.stringify(strain), operator, seedHex)) as Strain,
    defendGate: (rules) => JSON.parse(m.defend_gate(rules)) as { ok: boolean; reason: string | null },
  };
}

export function randomHex(bytes = 32): string {
  return [...crypto.getRandomValues(new Uint8Array(bytes))].map((b) => b.toString(16).padStart(2, "0")).join("");
}

/** What the release flow needs from a wallet. */
export interface Signer {
  address: Hex;
  chainId: number;
  send(tx: { to: Hex; data: Hex; value: bigint }): Promise<Hex>;
  /**
   * Resolves once the transaction, or one that took its place, is mined: the hash mined, and whether it burned. It did
   * when it succeeded and was the burn itself or the burn sent again at another fee, not a cancel or another call.
   */
  wait(hash: Hex): Promise<{ hash: Hex; burned: boolean }>;
  sign(message: string): Promise<Hex>;
}

export interface Target {
  token: Hex;
  amount: bigint;
  /** Where the Worker answers, without the trailing `/api/release`. */
  api: string;
}

export type Step = "burn" | "mined" | "signed" | "queued";

/** What kept burns need of `localStorage`. */
type Store = Pick<Storage, "getItem" | "setItem" | "removeItem" | "key" | "length">;
type Wallet = { chainId: number; address: string };

const KEPT = "venin.burn.";

/**
 * Burns sent and not yet taken by the Worker, one per chain and address, so a retry waits for the burn it sent instead
 * of burning again. Each lives in `storage` when the browser allows it and in this page's memory always.
 */
export function keptBurns(storage: () => Store = () => localStorage) {
  const memory = new Map<string, Hex>();
  const key = (w: Wallet) => `${KEPT}${w.chainId}.${w.address.toLowerCase()}`;
  const stored = (read: (s: Store) => string | null): Hex | null => {
    try {
      return read(storage()) as Hex | null;
    } catch {
      return null;
    }
  };
  return {
    get: (w: Wallet): Hex | null => stored((s) => s.getItem(key(w))) ?? memory.get(key(w)) ?? null,
    set(w: Wallet, hash: Hex | null) {
      if (hash) memory.set(key(w), hash);
      else memory.delete(key(w));
      try {
        if (hash) storage().setItem(key(w), hash);
        else storage().removeItem(key(w));
      } catch {
        // Storage may be off; the hash then lives as long as the page.
      }
    },
    /** A kept burn of any wallet, for the page to mention before a wallet connects. */
    any(): Hex | null {
      const first = stored((s) => {
        for (let i = 0; i < s.length; i++) {
          const k = s.key(i);
          if (k?.startsWith(KEPT)) return s.getItem(k);
        }
        return null;
      });
      return first ?? memory.values().next().value ?? null;
    },
  };
}

export type Kept = ReturnType<typeof keptBurns>;

/**
 * Burns, signs and submits one release; `step` hears each stage. Returns the queue key, also when the Worker had this
 * burn queued already. The burn's hash is in `kept` from the moment it is sent until the Worker takes it, so a retry
 * waits for that burn and submits it instead of burning again. A burn that reverted, was cancelled or was replaced is
 * dropped, and so is one the Worker finds is not a burn of enough tokens from this wallet (402), which no retry mends.
 */
export async function release(
  signer: Signer,
  target: Target,
  payload: string,
  route: Route,
  step: (s: Step, detail: string) => void,
  kept: Kept,
): Promise<string> {
  const sha = await sha256Hex(payload);
  return submit(signer, target, step, kept, "release", (tx) => releaseMessage(tx, route, sha, signer.chainId), {
    route,
    payload,
  });
}

/** Burns, signs and submits one agent for the ward, as `release` does a strain. */
export async function defend(
  signer: Signer,
  target: Target,
  rules: string,
  column: Column,
  step: (s: Step, detail: string) => void,
  kept: Kept,
): Promise<string> {
  const sha = await sha256Hex(rules);
  return submit(signer, target, step, kept, "defend", (tx) => defendMessage(tx, column, sha, signer.chainId), {
    column,
    rules,
  });
}

/** The burn, the signature over `message` and the POST of `fields` to the Worker's `/api/<path>`. */
async function submit(
  signer: Signer,
  target: Target,
  step: (s: Step, detail: string) => void,
  kept: Kept,
  path: "release" | "defend",
  message: (tx: Hex) => string,
  fields: Record<string, string>,
): Promise<string> {
  // Known limit: a kept burn the chain never mines makes each retry wait on it until viem gives up; burning again
  // from that wallet in this browser then needs its site data cleared.
  const sent = kept.get(signer) ?? (await signer.send(burnTx(target.token, target.amount)));
  kept.set(signer, sent);
  step("burn", sent);
  const mined = await signer.wait(sent);
  if (!mined.burned) {
    kept.set(signer, null);
    throw new Error("the burn reverted or was cancelled or replaced, so no tokens were burned; try again to burn anew");
  }
  const hash = mined.hash;
  kept.set(signer, hash);
  step("mined", hash);
  const tx = hash.toLowerCase() as Hex;
  const signature = await signer.sign(message(tx));
  step("signed", signature);
  const res = await fetch(`${target.api}/api/${path}`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ tx, ...fields, signature }),
  });
  const body = (await res.json().catch(() => ({}))) as { key?: string; error?: string };
  if ((res.ok || res.status === 409) && body.key) {
    kept.set(signer, null);
    step("queued", body.key);
    return body.key;
  }
  if (res.status === 402) kept.set(signer, null);
  throw new Error(body.error ?? `the Worker answered HTTP ${res.status}`);
}

/** The RPC URLs of a build setting that may list several, comma-separated, in the order to try them. */
export const rpcList = (rpc: string): string[] =>
  rpc
    .split(",")
    .map((u) => u.trim())
    .filter(Boolean);

/** A viem wallet over a local key or an EIP-1193 provider, on one chain; reads go to the RPCs in turn. */
async function viemSigner(
  chainId: number,
  rpc: string,
  account: { key: Hex } | { provider: unknown; address: Hex },
): Promise<Signer> {
  const viem = await import("viem");
  const urls = rpcList(rpc);
  const chain = viem.defineChain({
    id: chainId,
    name: chainId === 4663 ? "Robinhood Chain" : "Local anvil",
    nativeCurrency: { name: "Ether", symbol: "ETH", decimals: 18 },
    rpcUrls: { default: { http: urls } },
  });
  const reads = viem.fallback(urls.map((u) => viem.http(u)));
  const pub = viem.createPublicClient({ chain, transport: reads });
  const wait = async (hash: Hex) => {
    // viem resolves with the receipt of a transaction that took the burn's nonce; only a repriced burn still burned.
    const replaced: { reason?: string } = {};
    const receipt = await pub.waitForTransactionReceipt({
      hash,
      onReplaced: (r) => {
        replaced.reason = r.reason;
      },
    });
    const burned = receipt.status === "success" && (replaced.reason ?? "repriced") === "repriced";
    return { hash: receipt.transactionHash, burned };
  };
  if ("key" in account) {
    const { privateKeyToAccount } = await import("viem/accounts");
    const local = privateKeyToAccount(account.key);
    const wallet = viem.createWalletClient({ account: local, chain, transport: reads });
    return {
      address: local.address,
      chainId,
      send: (tx) => wallet.sendTransaction(tx),
      wait,
      sign: (message) => wallet.signMessage({ message }),
    };
  }
  const wallet = viem.createWalletClient({
    account: account.address,
    chain,
    transport: viem.custom(account.provider as Parameters<typeof viem.custom>[0]),
  });
  return {
    address: account.address,
    chainId,
    send: (tx) => wallet.sendTransaction(tx),
    wait,
    sign: (message) => wallet.signMessage({ message }),
  };
}

export interface DevWallet {
  key: Hex;
  rpc: string;
  target: Target;
}

/** The anvil wallet `./dev up` hands the dev server; only on this machine and never in a production build. */
export function devWallet(): DevWallet | null {
  if (!import.meta.env.DEV || !["localhost", "127.0.0.1"].includes(location.hostname)) return null;
  const env = import.meta.env;
  const key = env.VITE_DEV_WALLET_KEY as Hex | undefined;
  const { VITE_DEV_RPC: rpc, VITE_DEV_TOKEN: token, VITE_DEV_AMOUNT: amount, VITE_DEV_API: api } = env;
  if (!key || !rpc || !token || !amount || !api) return null;
  return { key, rpc, target: { token: token as Hex, amount: BigInt(amount), api } };
}

export const devSigner = (dev: DevWallet) => viemSigner(31337, dev.rpc, { key: dev.key });

/** Opens Reown AppKit (loaded only here, only when a project id is set) and resolves with the connected wallet. */
export async function connectWallet(projectId: string, chainId: number, rpc: string): Promise<Signer> {
  const [{ createAppKit }, { defineChain }, { WagmiAdapter }] = await Promise.all([
    import("@reown/appkit"),
    import("@reown/appkit/networks"),
    import("@reown/appkit-adapter-wagmi"),
  ]);
  const network = defineChain({
    id: chainId,
    caipNetworkId: `eip155:${chainId}`,
    chainNamespace: "eip155",
    name: "Robinhood Chain",
    nativeCurrency: { name: "Ether", symbol: "ETH", decimals: 18 },
    rpcUrls: { default: { http: rpcList(rpc) } },
  });
  const networks: [typeof network] = [network];
  // The adapter's published types fail under exactOptionalPropertyTypes; at run time it is what AppKit takes.
  const adapter = new WagmiAdapter({ networks, projectId }) as never;
  const modal = createAppKit({
    adapters: [adapter],
    networks,
    projectId,
    metadata: {
      name: "Venin",
      description: "Release a strain into the ward",
      url: location.origin,
      icons: [`${location.origin}/icon-512.png`],
    },
    features: { analytics: false, email: false, socials: false, swaps: false, onramp: false },
  });
  await modal.open();
  const address = await new Promise<Hex>((resolve) => {
    const stop = modal.subscribeAccount((a) => {
      if (a.isConnected && a.address) {
        stop();
        resolve(a.address as Hex);
      }
    });
  });
  return viemSigner(chainId, rpc, { provider: modal.getWalletProvider(), address });
}
