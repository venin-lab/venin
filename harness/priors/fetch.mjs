// Fetches the three pure files of Priors' public package from the npm registry into vendor/ (gitignored): the
// tarball must match the integrity npm publishes for this version, and each file the hash in vendor.sha256. Plain tar
// extraction; nothing is installed and no package script runs.
// Usage: node harness/priors/fetch.mjs

import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { copyFileSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

export const VERSION = "0.20.0";
export const TARBALL = `https://registry.npmjs.org/@priors/mcp/-/mcp-${VERSION}.tgz`;
// `npm view @priors/mcp@0.20.0 dist.integrity`
export const INTEGRITY =
  "sha512-0yQHyb7t01kl5PmGLDlfodNWEKNtCIiSOzIdLnARzRNgq2HktVk2AUOSi0SAngXOuBlQu1A/d1MI/b3jS+7AxQ==";
export const FILES = ["fence.mjs", "pay-hosts.mjs", "daily.mjs"];

const here = dirname(fileURLToPath(import.meta.url));

/** True when `bytes` match an npm integrity string (`sha512-<base64>`). */
export function integrityOk(bytes, integrity) {
  const [algo, digest] = integrity.split("-", 2);
  return algo === "sha512" && createHash("sha512").update(bytes).digest("base64") === digest;
}

/** The expected SHA-256 of each file, from `vendor.sha256` text. */
export function parseSums(text) {
  const sums = new Map();
  for (const line of text.split("\n").filter(Boolean)) {
    const m = /^([0-9a-f]{64}) {2}(\S+)$/.exec(line);
    if (!m) throw new Error(`vendor.sha256: bad line ${JSON.stringify(line)}`);
    sums.set(m[2], m[1]);
  }
  return sums;
}

/** Throws unless every file in `dir` named in FILES has its hash in `sums`. */
export function checkFiles(dir, sums) {
  for (const name of FILES) {
    const sha = createHash("sha256")
      .update(readFileSync(join(dir, name)))
      .digest("hex");
    if (sha !== sums.get(name)) throw new Error(`${name}: sha256 ${sha} is not the recorded ${sums.get(name)}`);
  }
}

/** Downloads the tarball into `dir` and checks its integrity; returns its path. */
export async function download(dir) {
  const res = await fetch(TARBALL, { redirect: "error" });
  if (!res.ok) throw new Error(`${TARBALL}: HTTP ${res.status}`);
  const bytes = Buffer.from(await res.arrayBuffer());
  if (!integrityOk(bytes, INTEGRITY)) throw new Error(`${TARBALL}: integrity differs from ${INTEGRITY}`);
  const path = join(dir, `mcp-${VERSION}.tgz`);
  writeFileSync(path, bytes);
  return path;
}

async function main() {
  const tmp = mkdtempSync(join(tmpdir(), "venin-priors-"));
  try {
    const tgz = await download(tmp);
    execFileSync("tar", ["-xzf", tgz, "-C", tmp, ...FILES.map((f) => `package/src/${f}`)]);
    const sums = parseSums(readFileSync(join(here, "vendor.sha256"), "utf8"));
    checkFiles(join(tmp, "package/src"), sums);
    const vendor = join(here, "vendor");
    mkdirSync(vendor, { recursive: true });
    for (const name of FILES) copyFileSync(join(tmp, "package/src", name), join(vendor, name));
    console.log(`priors ${VERSION}: ${FILES.join(", ")} in ${vendor}`);
  } finally {
    rmSync(tmp, { recursive: true, force: true });
  }
}

if (import.meta.main) {
  main().catch((e) => {
    console.error(`fetch: ${e.message}`);
    process.exit(1);
  });
}
