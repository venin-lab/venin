import { describe, expect, it } from "vitest";
import { leaguePill } from "../ui/header";

describe("league pill", () => {
  it("reads SYNTHETIC on fixtures, PRACTICE COPY on a dev league, nothing in production", () => {
    expect(leaguePill("synthetic fixture")).toBe("SYNTHETIC");
    expect(leaguePill("local league")).toBe("PRACTICE COPY");
    expect(leaguePill("venin")).toBeNull();
    expect(leaguePill(null)).toBeNull();
  });
});
