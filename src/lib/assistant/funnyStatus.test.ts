import { describe, expect, it } from "vitest";
import { funnyStatusLines, funnyStatusOrder } from "./funnyStatus";

describe("funny status lines", () => {
  it("has Hebrew army jokes, one-word Israeli humour and English lines", () => {
    const he = funnyStatusLines("he");
    expect(he).toContain("בשק״ם…");
    expect(he).toContain("סיגריה קטנה…");
    expect(he).toContain("מחפש את החוגר…");
    expect(he).toContain("יאללה…");
    expect(funnyStatusLines("en").length).toBeGreaterThan(10);
  });

  it("every line is short and unique", () => {
    for (const locale of ["he", "en"] as const) {
      const lines = funnyStatusLines(locale);
      expect(new Set(lines).size).toBe(lines.length);
      for (const line of lines) expect(line.length).toBeLessThanOrEqual(32);
    }
  });

  it("the order is a stable shuffle of all lines, different per query", () => {
    const a = funnyStatusOrder("q1", "he");
    expect(funnyStatusOrder("q1", "he")).toEqual(a);
    expect([...a].sort()).toEqual([...funnyStatusLines("he")].sort());
    const starts = new Set(["q1", "q2", "q3", "q4", "q5", "q6"].map((q) => funnyStatusOrder(q, "he")[0]));
    expect(starts.size).toBeGreaterThan(1);
  });
});
