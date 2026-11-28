#!/usr/bin/env node
// genes proxy --policy <file> -- <command> [args...]: a stdio MCP proxy that runs every tools/call through the guard
// and every message from the server through its read filters, and withholds or strips third-party tool, prompt,
// resource and server text. genes rules: prints the default instinct rules.
import { spawn } from "node:child_process";
import { readFileSync } from "node:fs";
import { createInterface } from "node:readline";
import { createGuard, rules } from "./index.js";

const USAGE = "usage: genes proxy --policy <file> -- <command> [args...]\n       genes rules\n";

/** Fields of a content item that name or carry data rather than text the model reads. */
const ITEM = ["type", "uri", "mimeType", "blob", "data"];

/**
 * The fields that route a message or name its content, by the exact path of the object that holds them (`#` stands
 * for an array index): the top of a message, of a result and of params, a progress token in `params._meta`, the
 * content items of a result, a resource and a message, and a message's role. Nowhere else is a field exempt, so a
 * `type` or `uri` inside `structuredContent` is cleaned like any other text.
 */
const ROUTES = [
  [[], ["jsonrpc", "id", "method"]],
  [["result"], ["protocolVersion", "nextCursor"]],
  [["params"], ["cursor", "progressToken", "requestId", "uri"]],
  [["params", "_meta"], ["progressToken"]],
  [["result", "content", "#"], ITEM],
  [["result", "content", "#", "resource"], ITEM],
  [["result", "contents", "#"], ITEM],
  [["params", "messages", "#"], ["role"]],
  [["params", "messages", "#", "content"], ITEM],
  [["result", "messages", "#"], ["role"]],
  [["result", "messages", "#", "content"], ITEM],
  [["result", "messages", "#", "content", "resource"], ITEM],
];

/** Whether the field `key` of the object at `path` routes the message: it keeps its text but loses the characters
 * that render as nothing. */
function routing(path, key) {
  return ROUTES.some(
    ([at, keys]) =>
      at.length === path.length &&
      at.every((p, i) => (p === "#" ? typeof path[i] === "number" : p === path[i])) &&
      keys.includes(key),
  );
}

/** Results that list what the server offers, and the field that holds the list. */
const LISTS = {
  "tools/list": "tools",
  "prompts/list": "prompts",
  "resources/list": "resources",
  "resources/templates/list": "resourceTemplates",
};

function fail(message) {
  process.stderr.write(`genes: ${message}\n`);
  process.exit(2);
}

function warn(message) {
  process.stderr.write(`genes: ${message}\n`);
}

/** Keeps every number exactly as written, so ids and values past 2^53 survive the round trip. */
function exact(_key, value, context) {
  return typeof value === "number" && context && String(value) !== context.source
    ? JSON.rawJSON(context.source)
    : value;
}

function send(stream, message) {
  stream.write(`${JSON.stringify(message)}\n`);
}

/** An id as the proxy matches it: by JSON value, so 1 and 1.0 are one id and 1 and "1" are two. */
function idKey(id) {
  if (typeof id === "string") return `s:${id}`;
  const text = JSON.isRawJSON(id) ? JSON.stringify(id) : typeof id === "number" ? String(id) : undefined;
  if (text === undefined) return `o:${JSON.stringify(id)}`;
  const whole = /^(-?\d+)(\.0*)?$/.exec(text);
  if (whole) return `n:${BigInt(whole[1])}`;
  const n = Number(text);
  return `n:${Number.isSafeInteger(n) ? BigInt(n) : n}`;
}

/** What a JSON-RPC message is, or undefined for one that is none of them (a request that also answers, say). */
function kind(m) {
  if (!m || typeof m !== "object" || Array.isArray(m)) return undefined;
  const answers = "result" in m || "error" in m;
  if ("method" in m) {
    if (typeof m.method !== "string" || answers) return undefined;
    return "id" in m ? "request" : "notification";
  }
  return "id" in m && "result" in m !== "error" in m ? "response" : undefined;
}

/** Calls `each(message)` for every JSON line of `stream`, a batch as an array; a line that is not JSON is dropped. */
function lines(stream, each) {
  createInterface({ input: stream, crlfDelay: Number.POSITIVE_INFINITY }).on("line", (line) => {
    if (line.trim() === "") return;
    let message;
    try {
      message = JSON.parse(line, exact);
    } catch {
      warn("dropped a line that is not JSON");
      return;
    }
    each(message);
  });
}

/**
 * `message` after the guard's read filters, in place: every string and every object key, except the routing fields
 * (`routing`), which only lose the characters that render as nothing. A key that cleans into one already there is
 * dropped. Two calls in all, however large the message.
 */
function cleanMessage(guard, message) {
  const values = [];
  const routed = [];
  const keyed = [];
  const path = [];
  const walk = (v) => {
    if (JSON.isRawJSON(v)) return;
    const array = Array.isArray(v);
    for (const k of Object.keys(v)) {
      const key = array ? Number(k) : k;
      const child = v[k];
      if (typeof child === "string") (routing(path, key) ? routed : values).push([v, k]);
      else if (child && typeof child === "object") {
        path.push(key);
        walk(child);
        path.pop();
      }
    }
    if (!array && path.length > 0) keyed.push(v);
  };
  walk(message);
  const put = (slots, texts) => {
    slots.forEach(([holder, key], i) => {
      holder[key] = texts[i];
    });
  };
  if (values.length > 0) put(values, guard.cleanAll(values.map(([h, k]) => h[k])));
  if (routed.length > 0) put(routed, guard.visibleAll(routed.map(([h, k]) => h[k])));
  const names = keyed.flatMap((o) => Object.keys(o));
  const cleaned = names.length > 0 ? guard.cleanAll(names) : [];
  let i = 0;
  for (const o of keyed) {
    const entries = Object.entries(o);
    const renamed = entries.map(([, v]) => [cleaned[i++], v]);
    if (renamed.every(([k], j) => k === entries[j][0])) continue;
    for (const [k] of entries) delete o[k];
    for (const [k, v] of renamed) {
      if (!Object.hasOwn(o, k))
        Object.defineProperty(o, k, { value: v, enumerable: true, writable: true, configurable: true });
    }
  }
  return message;
}

function proxy(argv) {
  const at = argv.indexOf("--");
  const flags = at < 0 ? argv : argv.slice(0, at);
  const command = at < 0 ? [] : argv.slice(at + 1);
  const file = flags[flags.indexOf("--policy") + 1];
  if (!flags.includes("--policy") || !file || command.length === 0) fail(`bad arguments\n${USAGE}`);
  let guard;
  try {
    guard = createGuard(JSON.parse(readFileSync(file, "utf8")));
  } catch (e) {
    fail(`refusing to start: ${e.message}`);
  }

  const child = spawn(command[0], command.slice(1), { stdio: ["pipe", "pipe", "inherit"] });
  child.on("error", (e) => fail(`could not start ${command[0]}: ${e.message}`));
  child.on("close", (code, signal) => {
    process.exitCode = code ?? (signal ? 1 : 0);
    process.stdin.destroy();
  });
  process.stdin.on("end", () => child.stdin.end());

  /** The method of every client request the server has yet to answer, by id. */
  const pending = new Map();
  const listed = new Set();
  let warned = false;

  /** One client message as the server may see it, or undefined when the proxy answers or drops it. */
  function fromClient(m) {
    const what = kind(m);
    if (what === undefined) {
      warn("dropped a client message that is not a JSON-RPC request, notification or response");
      return undefined;
    }
    if (m.method === "tools/call") {
      const params = m.params ?? {};
      const decision = guard.check(params.name, params.arguments ?? {});
      if (!decision.allow) {
        const why = `blocked by policy${decision.gene ? ` (${decision.gene})` : ""}: ${decision.reason}`;
        warn(`${typeof params.name === "string" ? params.name : "a tool"}: ${why}`);
        if (what === "request") {
          const result = { content: [{ type: "text", text: why }], isError: true };
          send(process.stdout, { jsonrpc: "2.0", id: m.id, result });
        }
        return undefined;
      }
    }
    const leak = guard.leaks(m);
    if (leak) {
      const why = `blocked by policy (${leak}): the message carries a watched secret`;
      warn(`${m.method ?? "a response"}: ${why}`);
      if (what === "request" && m.method === "tools/call") {
        send(process.stdout, {
          jsonrpc: "2.0",
          id: m.id,
          result: { content: [{ type: "text", text: why }], isError: true },
        });
      } else if (what === "request") {
        send(process.stdout, { jsonrpc: "2.0", id: m.id, error: { code: -32001, message: why } });
      } else if (what === "response") {
        send(child.stdin, { jsonrpc: "2.0", id: m.id, error: { code: -32001, message: why } });
      }
      return undefined;
    }
    if (what === "request") pending.set(idKey(m.id), m.method);
    return m;
  }

  /** One server message as the client may see it, or undefined when it must be dropped. */
  function fromServer(m) {
    const what = kind(m);
    if (what === undefined) {
      warn("dropped a server message that is not a JSON-RPC request, notification or response");
      return undefined;
    }
    if (what !== "response") return cleanMessage(guard, m);
    const method = pending.get(idKey(m.id));
    if (method === undefined) {
      warn(`dropped a response to id ${JSON.stringify(m.id)}, which no request is waiting for`);
      return undefined;
    }
    pending.delete(idKey(m.id));
    const result = m.result;
    if (!result || typeof result !== "object" || Array.isArray(result) || JSON.isRawJSON(result)) {
      return cleanMessage(guard, m);
    }
    const listKey = LISTS[method];
    const own = method === "initialize" ? "instructions" : listKey;
    const kept = own !== undefined && own in result ? result[own] : undefined;
    if (own !== undefined) delete result[own];
    cleanMessage(guard, m);
    if (method === "initialize" && typeof kept === "string") {
      const { text, why } = guard.instructions(kept);
      m.result.instructions = text;
      if (why) warn(`server instructions withheld (${why})`);
    } else if (listKey !== undefined && Array.isArray(kept)) {
      const tools = listKey === "tools";
      const out = tools ? guard.tools(kept) : guard.entries(kept);
      m.result[listKey] = tools ? out.tools : out.entries;
      for (const n of out.notes) warn(`${listKey} ${n.tool ?? n.entry}: description withheld (${n.why})`);
      if (tools) {
        for (const t of out.tools) listed.add(String(t.name).trim().toLowerCase());
        if (!m.result.nextCursor && !warned && !guard.acting.some((n) => listed.has(n))) {
          warned = true;
          warn(
            `WARNING: none of the policy's pay or send tools (${guard.acting.join(", ")}) is in this server's tool ` +
              "list, so no call is checked. Name the server's own tools under pay and send in the policy.",
          );
        }
      }
    } else if (kept !== undefined) {
      m.result[own] = cleanMessage(guard, { value: kept }).value;
    }
    return m;
  }

  /** Each message of a line through `each`, and what is left forwarded as the far side's one canonical line. */
  function relay(stream, message, each, failed) {
    const batch = Array.isArray(message) ? message : [message];
    const out = [];
    for (const m of batch) {
      try {
        const seen = each(m);
        if (seen !== undefined) out.push(seen);
      } catch (e) {
        failed(m, e);
      }
    }
    if (out.length > 0) send(stream, Array.isArray(message) ? out : out[0]);
  }

  lines(process.stdin, (message) =>
    relay(child.stdin, message, fromClient, (m, e) => {
      warn(`dropped a client message the guard could not read: ${e.message}`);
      if (kind(m) === "request") {
        send(process.stdout, { jsonrpc: "2.0", id: m.id, error: { code: -32603, message: `genes: ${e.message}` } });
      }
    }),
  );

  lines(child.stdout, (message) =>
    relay(process.stdout, message, fromServer, (m, e) => {
      warn(`withheld a server message the guard could not read: ${e.message}`);
      if (kind(m) === "response") {
        send(process.stdout, { jsonrpc: "2.0", id: m.id, error: { code: -32603, message: `genes: ${e.message}` } });
      }
    }),
  );
}

const [command, ...rest] = process.argv.slice(2);
if (command === "proxy") proxy(rest);
else if (command === "rules") process.stdout.write(`${rules}\n`);
else fail(`unknown command\n${USAGE}`);
