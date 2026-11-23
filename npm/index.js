// The check-up behind venin.io/checkup, built from crates/core. Importing this module compiles the scanner once.
import { readFileSync } from "node:fs";
import { initSync } from "./venin_core.js";

initSync({ module: readFileSync(new URL("./venin_core_bg.wasm", import.meta.url)) });

export { scan } from "./venin_core.js";
