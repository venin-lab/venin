/// <reference types="vitest/config" />
import { createReadStream, existsSync, statSync } from "node:fs";
import type { ServerResponse } from "node:http";
import { homedir } from "node:os";
import { extname, join, normalize } from "node:path";
import { defineConfig, loadEnv, type Plugin } from "vite";

const TYPES: Record<string, string> = {
  ".json": "application/json",
  ".jsonl": "application/x-ndjson",
  ".gz": "application/gzip",
  ".txt": "text/plain",
};

/** Serves the runner's data directory at /data in dev and preview; production reads VITE_DATA_URL. */
function data(dir: string): Plugin {
  const serve = (url: string | undefined, res: ServerResponse, next: () => void) => {
    const path = normalize(decodeURIComponent((url ?? "/").split("?")[0]));
    if (path.includes("..")) return next();
    const file = join(dir, path);
    try {
      if (!statSync(file).isFile()) return next();
    } catch {
      return next();
    }
    res.setHeader("Content-Type", TYPES[extname(file)] ?? "application/octet-stream");
    res.setHeader("Cache-Control", "no-store");
    createReadStream(file).pipe(res);
  };
  return {
    name: "venin-data",
    configureServer(server) {
      server.middlewares.use("/data", (req, res, next) => serve(req.url, res, next));
    },
    configurePreviewServer(server) {
      server.middlewares.use("/data", (req, res, next) => serve(req.url, res, next));
    },
  };
}

/** The origin of an absolute URL, as a CSP source; nothing for a path on the site itself. */
const originOf = (url: string) => (/^https?:/.test(url) ? ` ${new URL(url).origin}` : "");

/** What Reown AppKit reaches on /release once a project id is set: WalletConnect's relay, RPC and wallet list. */
const WALLETCONNECT = {
  connect:
    " https://rpc.walletconnect.org https://relay.walletconnect.org wss://relay.walletconnect.org" +
    " https://api.web3modal.org https://pulse.walletconnect.org https://keys.walletconnect.org",
  img: " https://api.web3modal.org",
  frame: " https://verify.walletconnect.org",
};

/** Content Security Policy for built pages: scripts and styles from the site; data, the RPCs and the release Worker. */
function csp(dataUrl: string, rpcs: string[], wallet: boolean): Plugin {
  const origin = [dataUrl, ...rpcs].map(originOf).join("");
  const wc = (part: keyof typeof WALLETCONNECT) => (wallet ? WALLETCONNECT[part] : "");
  const policy = [
    "default-src 'self'",
    "script-src 'self' 'wasm-unsafe-eval'",
    "style-src 'self' 'unsafe-inline'",
    `img-src 'self' data: blob:${originOf(dataUrl)}${wc("img")}`,
    "font-src 'self'",
    `connect-src 'self'${origin}${wc("connect")}`,
    wallet ? `frame-src${WALLETCONNECT.frame}` : "frame-src 'none'",
    "object-src 'none'",
    "base-uri 'self'",
  ].join("; ");
  return {
    name: "venin-csp",
    apply: "build",
    transformIndexHtml: (html) =>
      html.replace(
        '<meta charset="utf-8" />',
        `<meta charset="utf-8" />\n    <meta http-equiv="Content-Security-Policy" content="${policy}" />`,
      ),
  };
}

/** The dev league's data once its genesis is written, else the synthetic fixture. */
function devData(): string {
  const dev = join(homedir(), "Documents/venin-dev/data");
  return existsSync(join(dev, "manifest.json")) ? dev : join(homedir(), "Documents/venin-dev/synthetic");
}

export default defineConfig(({ mode }) => {
  const env = loadEnv(mode, process.cwd(), "VITE_");
  return {
    plugins: [
      data(process.env.VENIN_DATA_DIR ?? devData()),
      csp(
        env.VITE_DATA_URL ?? "/data",
        [
          ...(env.VITE_ANCHOR_RPC ?? "").split(","),
          ...(env.VITE_DRAW_RPC ?? "").split(","),
          ...(env.VITE_RELEASE_RPC ?? "").split(","),
          env.VITE_RELEASE_API ?? "",
        ],
        Boolean(env.VITE_REOWN_PROJECT_ID),
      ),
    ],
    oxc: { jsx: { runtime: "automatic", importSource: "preact" } },
    build: { sourcemap: false, chunkSizeWarningLimit: 900 },
    server: { port: 5173 },
    test: { include: ["src/**/*.test.{ts,mjs}"] },
  };
});
