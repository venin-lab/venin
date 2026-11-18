// Builds venin-core as WebAssembly into src/wasm/ (build output, not tracked): cargo with the repo's remapped paths
// (scripts/env), then wasm-bindgen for the web target. Fails above the size budget or when the output fails
// `scripts/check artifacts`.
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { gzipSync } from "node:zlib";

/** The gzipped wasm may not grow past this. */
const BUDGET = 1.5 * 1024 * 1024;

const root = fileURLToPath(new URL("../..", import.meta.url));
const out = "web/src/wasm";
const wasm = "target/wasm32-unknown-unknown/release/venin_core.wasm";
const sh = (script) => execFileSync("bash", ["-c", `. scripts/env && ${script}`], { cwd: root, stdio: "inherit" });

sh("cargo build -p venin-core --release --target wasm32-unknown-unknown --quiet");
sh(`wasm-bindgen --target web --out-dir ${out} --out-name venin_core ${wasm}`);
const size = gzipSync(readFileSync(`${root}${out}/venin_core_bg.wasm`), { level: 9 }).length;
if (size > BUDGET) {
  console.error(`wasm: ${size} bytes gzipped, over the ${BUDGET} byte budget`);
  process.exit(1);
}
sh(`scripts/check artifacts ${out}`);
