import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { fileFor } from "../../scripts/card.mjs";

describe("card renderer server", () => {
  it("serves data from the data dir, files from the build, and the app page for routes", () => {
    const dist = mkdtempSync(join(tmpdir(), "venin-dist-"));
    writeFileSync(join(dist, "index.html"), "");
    writeFileSync(join(dist, "app.js"), "");
    expect(fileFor("/data/hours/3.json?x=1", dist, "/d")).toBe("/d/hours/3.json");
    expect(fileFor("/app.js", dist, "/d")).toBe(join(dist, "app.js"));
    expect(fileFor("/card/3", dist, "/d")).toBe(join(dist, "index.html"));
    expect(fileFor("/data/../../etc/passwd", dist, "/d")).toBeNull();
    expect(fileFor("/data/%2e%2e/secret", dist, "/d")).toBeNull();
    expect(fileFor("/data/%E0%A4%A", dist, "/d")).toBeNull();
  });
});
