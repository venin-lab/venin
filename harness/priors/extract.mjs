// One-off, run by hand when a Priors version joins the ward: writes Priors' exact tool list (names, descriptions,
// input schemas) and server instructions to library/priors/tools-<version>.json. The package is installed with its
// dependencies into a temp dir (scripts disabled, a temp HOME), and its server factory is called in-process under
// `node --permission` with no network permission, a fetch that throws and a throwaway key, configured with the
// settings of library/priors/settings.json.
// Usage: node harness/priors/extract.mjs

import { execFileSync } from "node:child_process";
import { randomBytes } from "node:crypto";
import { mkdirSync, mkdtempSync, readFileSync, realpathSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { download, VERSION } from "./fetch.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const library = join(here, "../../library/priors");

/** JSON with every character above U+007E written as a lowercase \uXXXX escape. */
export function asciiJson(value) {
  return JSON.stringify(value, null, 2).replace(
    /[\u007f-\uffff]/g,
    (c) => `\\u${c.charCodeAt(0).toString(16).padStart(4, "0")}`,
  );
}

/** The environment the settings file names, as Priors reads it. */
export function settingsEnv(settings) {
  return Object.fromEntries(settings.env.map((s) => [s.name, s.value]));
}

// Runs inside the permission sandbox: the factory, an in-memory MCP client, the list on stdout.
const DUMP = `
import { createPriorsMcpServer } from "@priors/mcp";
import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { InMemoryTransport } from "@modelcontextprotocol/sdk/inMemory.js";
const env = JSON.parse(process.env.DUMP_ENV);
const fetchImpl = () => { throw new Error("no network in the sandbox"); };
const server = await createPriorsMcpServer({ env, fetchImpl });
const [a, b] = InMemoryTransport.createLinkedPair();
const client = new Client({ name: "extract", version: "1" });
await Promise.all([server.connect(a), client.connect(b)]);
const { tools } = await client.listTools();
process.stdout.write(JSON.stringify({ instructions: client.getInstructions(), tools }));
process.exit(0);
`;

function main() {
  const settings = JSON.parse(readFileSync(join(library, "settings.json"), "utf8"));
  const tmp = realpathSync(mkdtempSync(join(tmpdir(), "venin-extract-")));
  const home = join(tmp, "home");
  const app = join(tmp, "app");
  mkdirSync(home);
  mkdirSync(app);
  const clean = { PATH: process.env.PATH, HOME: home, npm_config_cache: join(tmp, "cache") };
  return download(tmp)
    .then((tgz) => {
      execFileSync("npm", ["install", "--ignore-scripts", "--no-audit", "--no-fund", "--prefix", app, tgz], {
        env: clean,
        cwd: app,
        stdio: ["ignore", "ignore", "inherit"],
      });
      writeFileSync(join(app, "dump.mjs"), DUMP);
      const env = {
        ...settingsEnv(settings),
        PRIORS_KEY: `0x${randomBytes(32).toString("hex")}`,
        PRIORS_STATE_DIR: join(tmp, "state"),
      };
      const out = execFileSync("node", ["--permission", `--allow-fs-read=${tmp}`, `--allow-fs-write=${join(tmp, "state")}`, join(app, "dump.mjs")], {
        env: { PATH: process.env.PATH, HOME: home, DUMP_ENV: JSON.stringify(env) },
        cwd: app,
        stdio: ["ignore", "pipe", "inherit"],
        timeout: 60_000,
      });
      const dump = JSON.parse(out.toString("utf8"));
      const file = join(library, `tools-${VERSION}.json`);
      const record = {
        package: "@priors/mcp",
        version: VERSION,
        label: `Sandbox copy of Priors' public package v${VERSION} (tool list, fence, host allowlist and caps; network and chain stubbed). Unaffiliated.`,
        method: "server factory called in-process under node --permission with no network permission",
        settings: "library/priors/settings.json",
        instructions: dump.instructions,
        tools: dump.tools.map((t) => ({ name: t.name, description: t.description, inputSchema: t.inputSchema })),
      };
      writeFileSync(file, `${asciiJson(record)}\n`);
      console.log(`${record.tools.length} tools in ${file}`);
    })
    .finally(() => rmSync(tmp, { recursive: true, force: true }));
}

if (import.meta.main) {
  main().catch((e) => {
    console.error(`extract: ${e.message}`);
    process.exit(1);
  });
}
