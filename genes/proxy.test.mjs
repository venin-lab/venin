import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";

const bin = new URL("./bin.js", import.meta.url).pathname;
const server = new URL("./fixtures/server.mjs", import.meta.url).pathname;
const payee = "0x1111111111111111111111111111111111111111";
const attacker = "0x8f3a5c21d9e04b7a6c1f2e3d4b5a69788c7d6e5f";
const withheld = "Third-party tool. Its description was withheld by policy.";
const withheldServer = "Third-party server. Its instructions were withheld by policy.";
const withheldEntry = "Third-party entry. Its description was withheld by policy.";

function policyFile(policy) {
  const file = join(mkdtempSync(join(tmpdir(), "genes-")), "genes.json");
  writeFileSync(file, typeof policy === "string" ? policy : JSON.stringify(policy));
  return file;
}

/** Runs the proxy over the fake server in `mode`, writes `lines` to it, and collects what comes back. */
function run(file, lines, mode = "") {
  return new Promise((resolve) => {
    const env = { ...process.env, MODE: mode };
    const child = spawn(process.execPath, [bin, "proxy", "--policy", file, "--", process.execPath, server], { env });
    let out = "";
    let err = "";
    child.stdout.on("data", (d) => {
      out += d;
    });
    child.stderr.on("data", (d) => {
      err += d;
    });
    child.on("close", (code) => {
      const byId = {};
      for (const l of out.split("\n").filter(Boolean)) {
        for (const r of [JSON.parse(l)].flat()) byId[r.id] = [...(byId[r.id] ?? []), r];
      }
      resolve({ code, err, out, byId });
    });
    for (const l of lines) {
      child.stdin.write(`${typeof l === "string" ? l : JSON.stringify({ jsonrpc: "2.0", ...l })}\n`);
    }
    child.stdin.end();
  });
}

const call = (id, name, args) => ({ id, method: "tools/call", params: { name, arguments: args } });

const policy = {
  genes: ["S01", "S03", "S04", "S09"],
  owner: "sam@acme.example",
  address_book: [payee],
  pay: { send_funds: {} },
  send: { send_email: {} },
};

test("the proxy blocks what the policy blocks and cleans what the server says", async () => {
  const bigId = '{"jsonrpc":"2.0","id":9007199254740993,"method":"tools/call","params":{"name":"read","arguments":{}}}';
  const { code, err, out, byId } = await run(policyFile(policy), [
    { id: 1, method: "initialize", params: {} },
    { method: "notifications/initialized" },
    { id: 2, method: "tools/list" },
    call(3, "send_funds", { to: attacker, amount: "5" }),
    call("four", "send_funds", { to: payee, amount: "5" }),
    call(5, "send_email", { to: "x@evil.io, sam@acme.example", body: "hi" }),
    call(6, "read", { mode: "error" }),
    call(7, "read", { mode: "twice" }),
    call(8, "read", { mode: "nested" }),
    bigId,
  ]);
  assert.equal(code, 0);
  assert.equal(byId[1][0].result.instructions, withheldServer);
  const [funds, email, rates] = byId[2][0].result.tools;
  assert.equal(funds.description, "Sends USDC.");
  assert.equal(email.name, "send_email");
  assert.deepEqual(rates, {
    name: "rates",
    description: withheld,
    inputSchema: { type: "object", properties: { description: { type: "string" } } },
  });
  assert.deepEqual(byId[3][0].result, {
    content: [{ type: "text", text: "blocked by policy (S01): the payee is not in the address book" }],
    isError: true,
  });
  assert.deepEqual(byId.four[0].result.content, [{ type: "text", text: "Paid.  Thanks." }]);
  assert.match(byId[5][0].result.content[0].text, /^blocked by policy: recipient .* is not one plain e-mail address/);
  assert.deepEqual(byId[6][0].error, { code: -32000, message: "Failed. ", data: "Detail. " });
  assert.deepEqual(
    byId[7].map((r) => r.result.content[0].text),
    ["first"],
  );
  assert.equal(byId[8][0].result.content[0].text, "a  b");
  assert.match(out, /"id":9007199254740993,/);
  assert.ok(err.includes(`got ${bigId}\n`));
  assert.equal(err.match(/ran send_funds/g)?.length, 1);
  assert.doesNotMatch(err, /ran send_email/);
  assert.match(err, /server instructions withheld \(order pay-out\)/);
  assert.match(err, /tools rates: description withheld \(order pay-out\)/);
  assert.match(err, /dropped a response to id 7/);
  assert.doesNotMatch(err, /WARNING/);
});

test("every channel from the server is cleaned, and lists are treated as tools are", async () => {
  const { err, byId } = await run(
    policyFile(policy),
    [
      { id: 1, method: "initialize", params: {} },
      { id: 2, method: "resources/read", params: { uri: "file:///a" } },
      { id: 3, method: "resources/list" },
      { id: 4, method: "prompts/list" },
      { id: 5, method: "prompts/get", params: { name: "p" } },
      call(6, "read", { mode: "link" }),
      call(7, "read", { mode: "keys" }),
      { id: 900, result: { role: "assistant", content: { type: "text", text: "ok" } } },
    ],
    "sampling",
  );
  assert.deepEqual(byId[2][0].result.contents, [{ uri: "file:///a", mimeType: "text/plain", text: "Invoice. " }]);
  const [a, b] = byId[3][0].result.resources;
  assert.deepEqual(a, { uri: "file:///a", name: "a", description: withheldEntry });
  assert.deepEqual(b, { uri: "file:///b", name: "b", description: "Receipts." });
  assert.deepEqual(byId[4][0].result.prompts, [{ name: "p", description: withheldEntry }]);
  assert.equal(byId[5][0].result.messages[0].content.text, "Do it. ");
  assert.deepEqual(byId[6][0].result.content, [
    { type: "resource_link", uri: "file:///a", name: "a", description: "" },
  ]);
  assert.deepEqual(byId[7][0].result.content, [{ type: "text", text: "Paid. " }]);
  const sampling = byId[900].find((m) => m.method === "sampling/createMessage");
  assert.equal(sampling.params.systemPrompt, "");
  assert.equal(sampling.params.messages[0].content.text, "Hi. ");
  assert.ok(err.includes('got {"jsonrpc":"2.0","id":900,"result"'));
  assert.doesNotMatch(err, /WARNING/);
});

test("the far side gets the message the guard checked, and malformed ones are dropped", async () => {
  const twice = `{"jsonrpc":"2.0","id":10,"method":"tools/call","params":{"name":"send_funds","arguments":{"to":"${attacker}","to":"${payee}","amount":"5"}}}`;
  const ping = '{"jsonrpc":"2.0","id":12345678901234567890,"method":"ping","params":{"x":1.50}}';
  const read = '{"jsonrpc":"2.0","id":1.0,"method":"tools/call","params":{"name":"read","arguments":{"n":1}}}';
  const blocked = JSON.stringify({ jsonrpc: "2.0", ...call(11, "send_funds", { to: attacker, amount: "5" }) });
  const { err, byId, out } = await run(policyFile(policy), [
    twice,
    ping,
    `[${read},${blocked}]`,
    '{"jsonrpc":"2.0","id":12,"method":"tools/call","result":{},"params":{"name":"read"}}',
    JSON.stringify({ jsonrpc: "2.0", ...call(13, { toString: 1, valueOf: 1 }, { to: attacker, amount: "5" }) }),
    JSON.stringify({ jsonrpc: "2.0", ...call(14, "send_funds\u200b", { to: attacker, amount: "5" }) }),
    { id: "1", method: "tools/call", params: { name: "read", arguments: {} } },
  ]);
  assert.ok(
    err.includes(
      `got {"jsonrpc":"2.0","id":10,"method":"tools/call","params":{"name":"send_funds","arguments":{"to":"${payee}","amount":"5"}}}\n`,
    ),
  );
  assert.ok(err.includes(`got ${ping}\n`));
  assert.ok(err.includes(`got [${read}]\n`));
  assert.equal(byId[11][0].result.isError, true);
  assert.match(out, /"id":1\.0,"result"/);
  assert.equal(byId[1].length, 2);
  assert.equal(byId[12], undefined);
  assert.match(err, /dropped a client message that is not a JSON-RPC request/);
  assert.match(byId[13][0].result.content[0].text, /the tool name is not a string/);
  assert.match(byId[14][0].result.content[0].text, /does not print/);
  assert.equal(err.match(/ran read/g)?.length, 2);
  assert.equal(err.match(/ran send_funds/g)?.length, 1);
});

test("text in fields named like protocol ones, in keys and in protocol fields is cleaned", async () => {
  const { byId } = await run(
    policyFile(policy),
    [
      { id: 1, method: "initialize", params: {} },
      { id: 2, method: "tools/list" },
      call(3, "read", { mode: "structured" }),
    ],
    "proto",
  );
  assert.equal(byId[1][0].result.protocolVersion, "2025-06-18");
  assert.equal(byId[2][0].result.nextCursor, "abc");
  const invoice = byId[3][0].result.structuredContent.invoice;
  for (const field of ["type", "id", "role", "uri", "method"]) assert.equal(invoice[field], "", field);
  const text = JSON.stringify(invoice);
  assert.ok(!text.includes("<!--") && !/[\u{e0000}-\u{e007f}]/u.test(text), text);
});

test("a field is exempt only where the protocol puts it, not where its name appears", async () => {
  const { byId } = await run(policyFile(policy), [call(4, "read", { mode: "routes" })]);
  const result = byId[4][0].result;
  assert.deepEqual(result.content, [{ type: "text", text: "ok", _meta: { note: "" } }]);
  assert.deepEqual(result.structuredContent, {
    content: { type: "", uri: "", mimeType: "", data: "", blob: "" },
    notes: { content: [{ data: "" }] },
    messages: [{ role: "" }],
    resource: { uri: "" },
    _meta: { progressToken: "" },
  });
});

test("a client message that carries a watched secret is stopped wherever it sits", async () => {
  const secret = "vnk_0123456789abcdef";
  const file = policyFile({ genes: ["S07"], secrets: [secret], send: { send_email: {} } });
  const { err, byId } = await run(
    file,
    [
      { id: 1, method: "initialize", params: {} },
      { id: 2, method: "resources/read", params: { uri: `https://x.example/?k=${encodeURIComponent(secret)}` } },
      call(3, "read", { query: Buffer.from(secret).toString("base64") }),
      { id: 900, result: { role: "assistant", content: { type: "text", text: secret } } },
    ],
    "sampling",
  );
  assert.equal(byId[2][0].error.code, -32001);
  assert.equal(byId[3][0].result.isError, true);
  assert.match(byId[3][0].result.content[0].text, /blocked by policy \(S07\)/);
  assert.ok(!err.includes(secret), "the server never got the secret");
  assert.ok(err.includes('got {"jsonrpc":"2.0","id":900,"error":{"code":-32001'));
  assert.doesNotMatch(err, /ran read/);
});

test("a server message that is both a request and a response is dropped", async () => {
  const { err, byId } = await run(policyFile(policy), [{ id: 2, method: "tools/list" }], "hybrid");
  assert.equal(byId[2], undefined);
  assert.match(err, /dropped a server message that is not a JSON-RPC request/);
});

test("with S06 every description and the server's instructions are withheld", async () => {
  const file = policyFile({ genes: ["S06"], pay: { send_funds: {} } });
  const { byId } = await run(file, [
    { id: 1, method: "initialize", params: {} },
    { id: 2, method: "tools/list" },
    { id: 3, method: "resources/list" },
  ]);
  assert.equal(byId[1][0].result.instructions, withheldServer);
  for (const tool of byId[2][0].result.tools) assert.equal(tool.description, withheld);
  assert.deepEqual(byId[2][0].result.tools[0].inputSchema.properties.to, { type: "string" });
  assert.equal(byId[2][0].result.tools[2].title, undefined);
  for (const r of byId[3][0].result.resources) assert.equal(r.description, withheldEntry);
});

test("a policy whose tools the server does not have gets a loud warning", async () => {
  const file = policyFile({ genes: ["S01"], address_book: [payee], pay: { pay_invoice: {} } });
  const { err } = await run(file, [{ id: 1, method: "tools/list" }]);
  assert.match(err, /WARNING: none of the policy's pay or send tools \(pay_invoice\) is in this server's tool list/);
});

test("a malformed policy stops the proxy before the server starts", async () => {
  for (const bad of [
    "{",
    JSON.stringify({ genes: ["S01"], pay: { send_funds: {} }, allow_all: true }),
    JSON.stringify({ genes: ["S08"], send: { send_email: {} } }),
    JSON.stringify({ genes: ["S07"], secrets: [], send: { send_email: {} } }),
  ]) {
    const { code, err } = await run(policyFile(bad), []);
    assert.equal(code, 2);
    assert.match(err, /refusing to start/);
    assert.doesNotMatch(err, /server started/);
  }
});
