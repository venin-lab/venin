import { readFileSync } from "node:fs";
import { describe, expect, it, vi } from "vitest";
import { initSync, scan } from "../wasm/venin_core.js";
import { lesions, MAX_BYTES, type Picked, picked, readFolder, scannedBody, skip, triage } from "./checkup-read";
import { answer, type Finding, type Scan } from "./verify-wasm";

const root = new URL("../../../", import.meta.url);
initSync({ module: readFileSync(new URL("web/src/wasm/venin_core_bg.wasm", root)) });
const sample = readFileSync(new URL("library/examples/sample-agent.json", root), "utf8");

describe("files the check-up never opens", () => {
  const names = [
    ".env",
    "app/.env.local",
    "certs/server.pem",
    "apikey.txt",
    "secret-keys.json",
    "wallet/UTC--2024-x",
    "keystore/a.json",
    ".ssh/id_rsa",
    "AGENTS.md",
    "notes/env.md",
    "skills/x/SKILL.md",
  ];

  it("match the core's own rule", () => {
    const core = answer<Scan>(scan(JSON.stringify({ files: names.map((path) => ({ path, content: "" })) })));
    expect(names.filter(skip)).toEqual(core.skipped);
    expect(core.skipped).toHaveLength(8);
  });

  it("are left unread, along with installed, large and binary files", async () => {
    expect(triage("node_modules/x/README.md", 10)).toBe("pass");
    expect(triage("big.md", MAX_BYTES + 1)).toBe("pass");
    expect(triage("logo.png", 10)).toBe("pass");
    expect(triage(".env", 10)).toBe("skip");
    expect(triage("CLAUDE.md", 10)).toBe("read");
    const text = vi.fn(async () => "never");
    const files: Picked[] = [
      { path: ".env", size: 9, text },
      { path: "AGENTS.md", size: 9, text: async () => "# rules" },
    ];
    expect(await readFolder(files)).toEqual({ files: [{ path: "AGENTS.md", content: "# rules" }], skipped: [".env"] });
    expect(text).not.toHaveBeenCalled();
  });

  it("are named relative to the picked folder", () => {
    const f = new File(["x"], "SKILL.md");
    Object.defineProperty(f, "webkitRelativePath", { value: "agent/skills/a/SKILL.md" });
    expect(picked([f]).map((p) => p.path)).toEqual(["skills/a/SKILL.md"]);
  });
});

describe("the sample agent", () => {
  const report = answer<Scan>(scan(sample));

  it("shows lesions on the hands and the heart, with genes for each", () => {
    const per = lesions(report.findings);
    expect(report.read).toBe(5);
    expect(per.hands.length).toBeGreaterThan(5);
    expect(report.findings.filter((f) => f.organ === "heart").map((f) => f.kind)).toEqual(["allowlist", "cap"]);
    expect(per.heart).toHaveLength(2);
    expect(per.eyes).toEqual([]);
    expect(report.findings.every((f) => f.genes.length > 0)).toBe(true);
    expect(JSON.stringify(report)).not.toContain("REDACTED");
  });

  it("makes one card of findings that say the same thing at one line, listing each cause", () => {
    const acts = lesions(report.findings).hands.filter((l) => l.check === "actor");
    expect(acts).toHaveLength(1);
    expect(acts[0]).toMatchObject({ file: ".mcp.json", line: 15 });
    expect(acts[0].says).toBe("quickpay_send can send messages or files and move money with no one asking:");
    expect(acts[0].causes).toEqual([
      "mcp__quickpay__quickpay_send lets it",
      "the server setting QUICKPAY_AUTO_APPROVE lets it",
    ]);
    const at = { organ: "hands", check: "wide", kind: "x", file: "a", line: 1 } as const;
    const per = lesions([
      { ...at, says: "One thing", genes: ["S01"] },
      { ...at, says: "Another thing", genes: ["S02"] },
      { ...at, says: "One thing", genes: ["S01", "S03"] },
    ]).hands;
    expect(per).toEqual([
      { check: "wide", file: "a", line: 1, says: "One thing", causes: [], genes: ["S01", "S03"] },
      { check: "wide", file: "a", line: 1, says: "Another thing", causes: [], genes: ["S02"] },
    ]);
  });

  it("draws a body whose ink runs from the hands to the heart", () => {
    expect(scannedBody(report.findings)).toEqual({
      body: "checkup",
      pill: "breached",
      gene: null,
      reach: 7,
      route: "SKL",
    });
    const eye = { organ: "eyes" } as Finding;
    const brain = { organ: "brain" } as Finding;
    expect(scannedBody([brain, eye])).toMatchObject({ route: "EYE", reach: 3 });
    expect(scannedBody([])).toMatchObject({ pill: "resisted", reach: 0, route: null });
  });
});
