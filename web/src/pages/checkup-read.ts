import type { BodyState } from "../record/stats";
import type { Route } from "../record/types";
import type { Finding, Organ } from "./verify-wasm";

/** Larger files are not read; the same ceiling as venin-core's scan. */
export const MAX_BYTES = 2_000_000;
const KEY = /^(\.env.*|.*\.pem|.*key.*|.*\.(jks|keystore|p12|pfx|kdbx)|utc--.*|id_(rsa|dsa|ecdsa|ed25519))$/i;
const TEXT = new Set("md mdc txt py sh js mjs cjs ts json jsonc yaml yml toml html xml".split(" ").map((e) => `.${e}`));
/** Folders that hold installed or generated files, not the agent's own. */
const PASSED = new Set([".git", "node_modules", "target", ".venv", "__pycache__"]);

/** venin-core's scan::skip: a file whose name says it may hold a key is never opened. */
export function skip(path: string): boolean {
  const name = path.split("/").pop() ?? path;
  return KEY.test(name) || path.split("/").some((p) => p.toLowerCase().includes("keystore"));
}

/** What the check-up does with a file: skip it unread, read it, or pass it by (not text, too big, installed). */
export function triage(path: string, size: number): "skip" | "read" | "pass" {
  if (path.split("/").some((p) => PASSED.has(p))) return "pass";
  if (skip(path)) return "skip";
  const dot = path.lastIndexOf(".");
  if (dot < 0 || !TEXT.has(path.slice(dot).toLowerCase()) || size > MAX_BYTES) return "pass";
  return "read";
}

/** A file met in a picked or dropped folder, read only on demand. */
export interface Picked {
  path: string;
  size: number;
  text(): Promise<string>;
}

/** The folder's text files for the scan and the names left unread; bytes are read only for files that pass triage. */
export async function readFolder(
  files: Picked[],
): Promise<{ files: { path: string; content: string }[]; skipped: string[] }> {
  const out: { path: string; content: string }[] = [];
  const skipped: string[] = [];
  for (const f of files) {
    const t = triage(f.path, f.size);
    if (t === "skip") skipped.push(f.path);
    if (t === "read") out.push({ path: f.path, content: await f.text() });
  }
  return { files: out, skipped };
}

/** Files of an `<input webkitdirectory>` pick, with the picked folder's own name taken off their paths. */
export function picked(list: FileList | File[]): Picked[] {
  return [...list].map((f) => ({
    path: (f.webkitRelativePath || f.name)
      .split("/")
      .slice(f.webkitRelativePath ? 1 : 0)
      .join("/"),
    size: f.size,
    text: () => f.text(),
  }));
}

/** Every file under a dropped entry, with paths relative to it. */
export async function dropped(entry: FileSystemEntry): Promise<Picked[]> {
  const out: Picked[] = [];
  const walk = async (e: FileSystemEntry, prefix: string): Promise<void> => {
    if (e.isFile) {
      const file = await new Promise<File>((ok, fail) => (e as FileSystemFileEntry).file(ok, fail));
      out.push({ path: `${prefix}${e.name}`, size: file.size, text: () => file.text() });
      return;
    }
    if (PASSED.has(e.name)) return;
    const reader = (e as FileSystemDirectoryEntry).createReader();
    for (;;) {
      const batch = await new Promise<FileSystemEntry[]>((ok, fail) => reader.readEntries(ok, fail));
      if (batch.length === 0) break;
      for (const child of batch) await walk(child, e === entry ? prefix : `${prefix}${e.name}/`);
    }
  };
  await walk(entry, "");
  return out;
}

const ORDER: Organ[] = ["eyes", "ears", "brain", "hands", "heart"];
const ROUTE: Partial<Record<Organ, Route>> = { eyes: "EYE", ears: "EAR", brain: "MEM", hands: "SKL" };
const REACH: Record<Organ, number> = { eyes: 1, ears: 1, brain: 3, hands: 5, heart: 7 };

/** One card: findings of one check at one file and line that open with the same clause, and every gene for them. */
export interface Lesion {
  check: Finding["check"];
  file: string;
  line: number;
  /** The finding; for several, the clause they share once, and `causes` what each one adds after its "; ". */
  says: string;
  causes: string[];
  genes: string[];
}

const opening = (says: string) => says.split("; ")[0];

function card(g: Finding[]): Lesion {
  const head = opening(g[0].says);
  const tails = [...new Set(g.map((f) => f.says.slice(head.length + 2)).filter(Boolean))];
  return {
    check: g[0].check,
    file: g[0].file,
    line: g[0].line,
    says: tails.length > 1 ? `${head}:` : g[0].says,
    causes: tails.length > 1 ? tails : [],
    genes: [...new Set(g.flatMap((f) => f.genes))],
  };
}

/**
 * Lesion cards per organ, in the scan's order. Two findings that say the same thing at one file and line ("X can pay
 * with no one asking") read as one lesion with two causes, not as a card printed twice.
 */
export function lesions(findings: Finding[]): Record<Organ, Lesion[]> {
  const groups = new Map<string, Finding[]>();
  for (const f of findings) {
    const key = [f.organ, f.check, f.file, f.line, opening(f.says)].join("\n");
    groups.set(key, [...(groups.get(key) ?? []), f]);
  }
  const out = Object.fromEntries(ORDER.map((o) => [o, [] as Lesion[]])) as Record<Organ, Lesion[]>;
  for (const g of groups.values()) out[g[0].organ].push(card(g));
  return out;
}

/**
 * The body the scan draws: ink enters at the first organ with a lesion and runs as deep as the deepest one;
 * `BodyView.inkOnly` then keeps it to the organs that have one.
 */
export function scannedBody(findings: Finding[]): BodyState {
  const hit = ORDER.filter((o) => findings.some((f) => f.organ === o));
  if (hit.length === 0) return { body: "checkup", pill: "resisted", gene: null, reach: 0, route: null };
  const entry = hit.find((o) => ROUTE[o]) ?? "hands";
  return {
    body: "checkup",
    pill: "breached",
    gene: null,
    reach: Math.max(...hit.map((o) => REACH[o])),
    route: ROUTE[entry] ?? "SKL",
  };
}
