import { afterEach, describe, expect, it } from "vitest";
import { setFixedLocale } from "../lib/i18n";
import { contentDirection, paragraphDirection, sourceDirection, splitBidi, textDirection, uiDirection } from "./direction";

/** The isolated LTR tokens of a text, in order. */
const tokens = (text: string) => splitBidi(text).filter((s) => s.ltr).map((s) => s.text);

describe("direction", () => {
  afterEach(() => setFixedLocale(null));

  it("lays the island out right to left for the pinned Hebrew UI, whatever the browser says", () => {
    setFixedLocale("he");
    expect(uiDirection()).toBe("rtl");
    setFixedLocale("en");
    expect(uiDirection()).toBe("ltr");
  });

  it("reads any text with Hebrew as a right-to-left paragraph, even when it starts in English", () => {
    expect(textDirection("Project Alpha_v2.pptx מוכן לבדיקה")).toBe("rtl");
    expect(textDirection("Teams — דניאל כהן")).toBe("rtl");
    expect(textDirection("Meeting at 11:00 בחדר 3")).toBe("rtl");
    expect(textDirection("אפשר לעבור על המצגת לפני הישיבה?")).toBe("rtl");
    expect(paragraphDirection).toBe(textDirection);
  });

  it("keeps English-only text left to right and lets neutral text inherit", () => {
    expect(textDirection("Select here to mark up and share the image.")).toBe("ltr");
    expect(textDirection("10.20.30.41")).toBeUndefined();
    expect(textDirection("")).toBeUndefined();
    expect(textDirection(null)).toBeUndefined();
  });

  it("gives an app name its OWN direction: an English name stays LTR in a Hebrew card, a Hebrew name RTL", () => {
    expect(sourceDirection("Microsoft Teams")).toBe("ltr");
    expect(sourceDirection("Snipping Tool")).toBe("ltr");
    expect(sourceDirection("אאוטלוק")).toBe("rtl");
    expect(sourceDirection("7-Zip")).toBe("ltr"); // first strong letter, not first character
    expect(sourceDirection("365")).toBeUndefined();
    expect(sourceDirection("")).toBeUndefined();
  });

  it("lays out a standalone toast by its content: Hebrew anywhere → rtl, English only → ltr, neutral → the UI", () => {
    setFixedLocale("he");
    expect(contentDirection("Snip saved", "Select here to mark up and share the image.")).toBe("ltr");
    expect(contentDirection("Teams — דניאל כהן", "")).toBe("rtl");
    expect(contentDirection("Quarterly review", "נא לאשר עד מחר")).toBe("rtl");
    expect(contentDirection("12:30", "")).toBe("rtl");
  });
});

describe("splitBidi: technical tokens are isolated LTR runs", () => {
  const exact: Array<[string, string[], "rtl" | "ltr" | undefined]> = [
    ["Teams — דניאל כהן", [], "rtl"],
    ["Project Alpha_v2.pptx מוכן לבדיקה", ["Project Alpha_v2.pptx"], "rtl"],
    ["Meeting at 11:00 בחדר 3", ["Meeting at 11:00"], "rtl"],
    ["10.20.30.41", ["10.20.30.41"], undefined],
    ["C:\\Users\\Daniel\\Report.pdf", ["C:\\Users\\Daniel\\Report.pdf"], "ltr"],
    ["user@example.com", ["user@example.com"], "ltr"],
    ["https://example.com/path", ["https://example.com/path"], "ltr"],
  ];

  for (const [text, expected, paragraph] of exact) {
    it(`"${text}": tokens ${JSON.stringify(expected)}, paragraph ${paragraph ?? "inherit"}`, () => {
      expect(tokens(text)).toEqual(expected);
      expect(textDirection(text)).toBe(paragraph);
      expect(splitBidi(text).map((s) => s.text).join("")).toBe(text);
    });
  }

  it("isolates paths, UNC paths, file names, addresses and URLs inside a Hebrew sentence", () => {
    expect(tokens("הקובץ נשמר ב-C:\\Users\\Daniel\\Report.pdf בהצלחה")).toEqual(["C:\\Users\\Daniel\\Report.pdf"]);
    expect(tokens("פתח \\\\fileserver\\share\\Q3.xlsx עכשיו")).toEqual(["\\\\fileserver\\share\\Q3.xlsx"]);
    expect(tokens("השרת 10.20.30.41 זמין")).toEqual(["10.20.30.41"]);
    expect(tokens("שלח ל-user@example.com, תודה")).toEqual(["user@example.com"]);
    expect(tokens("הקובץ ב-Report.pdf מוכן")).toEqual(["Report.pdf"]);
    expect(tokens("קישור לאתר החדש: https://example.com/path")).toEqual(["https://example.com/path"]);
  });

  it("isolates a token together with the English words around it, never across a Hebrew letter", () => {
    // a token alone would turn into a neutral and be ordered right to left against its own words
    expect(tokens("Project Alpha_v2.pptx מוכן לבדיקה")).toEqual(["Project Alpha_v2.pptx"]);
    expect(tokens("Meeting at 11:00 בחדר 3")).toEqual(["Meeting at 11:00"]);
    expect(tokens("הדוח Report.pdf (final) נשלח")).toEqual(["Report.pdf (final)"]);
    expect(tokens("user@example.com שלח קובץ")).toEqual(["user@example.com"]);
    expect(tokens("פגישה Weekly sync at 11:00, חדר 3")).toEqual(["Weekly sync at 11:00"]);
  });

  it("gives a sentence's closing punctuation back from a URL or path", () => {
    expect(tokens("ראה https://example.com/path.")).toEqual(["https://example.com/path"]);
    expect(tokens("(https://example.com/a?b=1)")).toEqual(["https://example.com/a?b=1"]);
    expect(tokens("נשמר ב-C:\\Temp\\a.txt.")).toEqual(["C:\\Temp\\a.txt"]);
    expect(splitBidi("ראה https://example.com/path.").pop()).toEqual({ text: ".", ltr: false });
  });

  it("isolates times and time ranges whole, with either dash and optional AM / PM", () => {
    expect(tokens("11:00–12:00")).toEqual(["11:00–12:00"]);
    expect(tokens("11:00 – 12:00 בחדר 3")).toEqual(["11:00 – 12:00"]);
    expect(tokens("11:00-12:00")).toEqual(["11:00-12:00"]);
    expect(tokens("ב-9:30 AM או 10:15 pm")).toEqual(["9:30 AM", "10:15 pm"]);
  });

  it("isolates dates, but leaves plain numbers, versions and percentages to the bidi algorithm", () => {
    expect(tokens("מועד 2026-10-07 סופי")).toEqual(["2026-10-07"]);
    expect(tokens("מועד 7/10/2026 סופי")).toEqual(["7/10/2026"]);
    expect(tokens("הכל מוכן (גרסה 2.0) בחדר (3), 100% הצלחה")).toEqual([]);
    expect(tokens("חדר 3 קומה 12")).toEqual([]);
  });

  it("handles emoji, parentheses, hyphens, long English and long Hebrew without inventing tokens", () => {
    expect(tokens("הכל מוכן 🎉 (גרסה 2.0) בחדר (3)")).toEqual([]);
    expect(tokens("Done 🎉 (v2.0) — הכל תקין (100%)")).toEqual([]);
    expect(tokens("(טיוטה)")).toEqual([]);
    expect(textDirection("(טיוטה)")).toBe("rtl");
    expect(textDirection("Q3-Q4 review - draft (final)")).toBe("ltr");
    expect(tokens("Q3-Q4 review - draft (final)")).toEqual([]);
    const longEnglish = "The quarterly infrastructure review has been rescheduled because the facilities team needs the large conference room ".repeat(3);
    expect(tokens(longEnglish)).toEqual([]);
    expect(textDirection(longEnglish)).toBe("ltr");
    const longHebrew = "הישיבה הרבעונית של צוות התשתיות נדחתה מכיוון שצוות המתקנים זקוק לחדר הישיבות הגדול ".repeat(3);
    expect(tokens(longHebrew)).toEqual([]);
    expect(textDirection(longHebrew)).toBe("rtl");
  });

  it("always splits losslessly", () => {
    for (const text of [
      "a 11:00 b C:\\x\\y.txt c 10.0.0.1 d https://a.b/c, e user@x.org f 2026-01-02 g.",
      "",
      "...",
      "מגוון 11:00–12:00, 10.20.30.41; C:\\a.pdf!",
    ]) {
      expect(splitBidi(text).map((s) => s.text).join("")).toBe(text);
    }
    expect(splitBidi(null)).toEqual([]);
  });
});
