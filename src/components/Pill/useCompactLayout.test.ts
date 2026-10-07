import { afterEach, describe, expect, it } from "vitest";
import { timeParts } from "../../lib/dateFormat";
import { setFixedLocale } from "../../lib/i18n";
import { compactContentSize, CLOCK_FONT_SIZE, PERIOD_FONT_SIZE, PERIOD_GAP, timeSlotWidth, widestDigit, withWidestDigits, type CompactLabels } from "./useCompactLayout";

afterEach(() => setFixedLocale(null));

// Digits of different widths, like a proportional font: "1" narrow, "8" widest.
const DIGIT_WIDTH: Record<string, number> = { "1": 4, "0": 8, "8": 9 };
const glyph = (ch: string) => (/\p{Nd}/u.test(ch) ? (DIGIT_WIDTH[ch.normalize("NFKD")] ?? 7) : ch === ":" ? 3 : 6);
// Width of a text at a size: proportional to the size so 12 / 15 / 11 px differ.
const measure = (text: string, size = 12) => [...text].reduce((sum, ch) => sum + glyph(ch) * (size / 12), 0);

const hour24 = (date: Date) => ({ digits: `${String(date.getHours()).padStart(2, "0")}:${String(date.getMinutes()).padStart(2, "0")}`, period: null });
const hour12 = (date: Date) => {
  const h = date.getHours() % 12 || 12;
  return { digits: `${h}:${String(date.getMinutes()).padStart(2, "0")}`, period: date.getHours() < 12 ? "לפנה״צ" : "אחה״צ" };
};

describe("widestDigit", () => {
  it("picks the widest of the ten digit glyphs", () => {
    expect(widestDigit((text) => measure(text), "en-US")).toBe("8");
  });

  it("uses the words locale's own digits", () => {
    const digit = widestDigit((text) => text.charCodeAt(0), "ar-EG");
    expect(digit).toMatch(/^\p{Nd}$/u);
    expect(digit).not.toMatch(/^[0-9]$/);
  });
});

describe("withWidestDigits (tabular-nums measuring)", () => {
  it("measures every digit as the widest one, so a run of narrow 1s is not under-measured", () => {
    for (const text of ["11/11", "1/1", "11:00", "In 11 min · 11:11"]) {
      const tabular = measure(withWidestDigits(text, "8"));
      expect(tabular).toBeGreaterThan(measure(text));
      // Every digit of the DOM's tabular run is at most as wide as the widest, so this never under-measures.
      expect(tabular).toBeGreaterThanOrEqual(measure(text) + [...text].filter((c) => c === "1").length * (DIGIT_WIDTH["8"] - DIGIT_WIDTH["1"]));
    }
  });

  it("leaves text without digits and non-digit characters alone", () => {
    expect(withWidestDigits("יום שלישי", "8")).toBe("יום שלישי");
    expect(withWidestDigits("11:00 PM", "8")).toBe("88:88 PM");
  });

  it("replaces digits of any script", () => {
    expect(withWidestDigits("١٢/٣", "٨")).toBe("٨٨/٨");
  });
});

describe("timeSlotWidth", () => {
  for (const display of ["full", "clock"] as const) {
    for (const [name, parts] of [["24-hour", hour24], ["12-hour", hour12]] as const) {
      it(`${display}, ${name}: fits every minute of the day, so the width never depends on the time`, () => {
        const slot = timeSlotWidth({ measure, digit: "8", parts });
        const size = CLOCK_FONT_SIZE;
        for (let minute = 0; minute < 24 * 60; minute++) {
          const shown = parts(new Date(2026, 9, 6, Math.floor(minute / 60), minute % 60));
          const width = measure(shown.digits, size) + (shown.period ? PERIOD_GAP + measure(shown.period, PERIOD_FONT_SIZE) : 0);
          expect(width).toBeLessThanOrEqual(slot);
        }
        // And so is the whole island: its content width is the slot, whatever minute is on screen.
        const labels: CompactLabels = { date: "6.10", weekday: display === "clock" ? "" : "Tuesday", contentWidth: slot, display, timeWidth: slot };
        expect(compactContentSize(labels, null, 0, false).width).toBe(Math.min(280, Math.max(display === "clock" ? 72 : 96, slot + 24)));
      });
    }
  }

  it("reserves two hour digits in a 12-hour clock, so 9:59 and 12:00 give the same slot", () => {
    const slot = timeSlotWidth({ measure, digit: "8", parts: hour12 });
    const nine = measure("8:88", CLOCK_FONT_SIZE);
    const twelve = measure("88:88", CLOCK_FONT_SIZE);
    expect(slot).toBeGreaterThanOrEqual(twelve + PERIOD_GAP);
    expect(slot).toBeGreaterThan(nine);
  });

  it("takes the longer of the AM and PM forms for the period", () => {
    const longer = (am: string, pm: string) =>
      timeSlotWidth({ measure, digit: "8", parts: (date) => ({ digits: "10:00", period: date.getHours() < 12 ? am : pm }) });
    expect(longer("AM", "PM")).toBe(longer("PM", "AM"));
    expect(longer("לפנה״צ", "PM")).toBe(longer("PM", "לפנה״צ"));
    expect(longer("AM", "לפנה״צ")).toBeGreaterThan(longer("AM", "PM"));
  });

  it("has no period part in a 24-hour clock", () => {
    const slot = timeSlotWidth({ measure, digit: "8", parts: hour24 });
    expect(slot).toBe(measure("88:88", CLOCK_FONT_SIZE));
  });

  it("is stable with non-Latin digits (Arabic-Indic)", () => {
    // Each Arabic-Indic digit gets its own width from its code point; separators and letters are fixed.
    const arabic = (text: string, size = 12) => [...text].reduce((sum, ch) => sum + (/\p{Nd}/u.test(ch) ? 5 + (ch.codePointAt(0)! % 5) : 4) * (size / 12), 0);
    const parts = (date: Date) => timeParts(date, "ar-EG");
    const digit = widestDigit((text) => arabic(text), "ar-EG");
    expect(digit).toMatch(/^\p{Nd}$/u);
    expect(digit).not.toMatch(/^[0-9]$/);
    const slot = timeSlotWidth({ measure: arabic, digit, parts });
    for (let minute = 0; minute < 24 * 60; minute++) {
      const shown = parts(new Date(2026, 9, 6, Math.floor(minute / 60), minute % 60));
      const width = arabic(shown.digits, CLOCK_FONT_SIZE) + (shown.period ? PERIOD_GAP + arabic(shown.period, PERIOD_FONT_SIZE) : 0);
      expect(width).toBeLessThanOrEqual(slot);
    }
  });
});
