import { getLocale, type Locale } from "../i18n";

// =============================================================================
// The playful lines the island shows while a smart search is working, instead of a plain
// "מחפש…" (the user asked for jokes in the spirit of Claude's spinner words, mostly from army
// life, plus random nonsense). Only the visible label rotates: the screen reader still hears
// "ai.processing", and the pill is sized once for the widest line, so a new line never resizes it.
// Keep every line short (it must fit the processing pill without truncation) and kind.
// =============================================================================

const HE: readonly string[] = [
  "בשק״ם…",
  "סיגריה קטנה…",
  "מחפש את החוגר…",
  "מחכה לרס״ר…",
  "עומד בתור לחדר אוכל…",
  "מקפל שמיכות…",
  "מצחצח נעליים למסדר…",
  "סופר ימים לשחרור…",
  "מחכה לאפטר…",
  "פותח קופסת לוף…",
  "מבקש גימלים…",
  "מחכה לטרמפ בצומת…",
  "מכין קפה שחור על הגזייה…",
  "שותה שוקו בשקית…",
  "מסתתר מהמ״פ…",
  "רץ למסדר בוקר…",
  "בודק מי בשמירה…",
  "מחליף שמירה…",
  "מסדר את הפק״ל…",
  "מחפש את הדסקית…",
  "ממלא טופס בשלושה עותקים…",
  "מחכה לאישור מלמעלה…",
  "יורד לש״ג…",
  "מתדלק את הנגמ״ש…",
  "מנקה את הנשק…",
  "עושה תורנות מטבח…",
  "קם לשמירה של 2 בלילה…",
  "מחפש את הכומתה…",
  "מחכה לאוטובוס של יום חמישי…",
  "מוריד טירונות…",
  "סופר כבשים…",
  "מחפש את השלט של המזגן…",
  "מחמם פיתות…",
  "מאכיל את הדגים…",
  "מחפש גרב זוגית…",
  "מתייעץ עם החתול…",
  "מנגב אבק מהמסך…",
  "שובר ביצה על המחבת…",
  "מחפש חניה…",
  "מוריד את הזבל…",
  // one-word Israeli humour
  "יאללה…",
  "סבבה…",
  "תכלס…",
  "בקטנה…",
  "וואלה…",
  "אחי…",
  "חפיף…",
  "שנייה…",
  "בלאגן…",
  "חומוס…",
  "שקשוקה…",
  "פלאפל…",
  "על האש…",
  "יהיה בסדר…",
  "חכה חכה…",
  "קומבינה…",
  "פדיחה…",
  "סחתיין…",
];

const EN: readonly string[] = [
  "At the canteen…",
  "One quick smoke break…",
  "Looking for the AWOL guy…",
  "Waiting for the sergeant…",
  "In line for the mess hall…",
  "Folding blankets…",
  "Polishing boots…",
  "Counting days to discharge…",
  "Waiting for weekend leave…",
  "Hitchhiking at the junction…",
  "Hiding from the captain…",
  "Running to morning roll call…",
  "Filling a form in triplicate…",
  "Waiting for approval from above…",
  "Cleaning the rifle…",
  "On kitchen duty…",
  "Counting sheep…",
  "Looking for the AC remote…",
  "Warming up pitas…",
  "Feeding the fish…",
  "Looking for the other sock…",
  "Asking the cat…",
  "Dusting the screen…",
  "Looking for parking…",
  "Taking out the trash…",
];

/** How long one line stays (ms). Long enough to read a short line twice. */
export const FUNNY_STATUS_MS = 2200;

export function funnyStatusLines(locale: Locale = getLocale()): readonly string[] {
  return locale === "he" ? HE : EN;
}

/**
 * The order the lines appear in for one query: a shuffle seeded by the query id, so a search
 * starts on a different joke than the last one, and a re-render never jumps to another line.
 */
export function funnyStatusOrder(seed: string, locale: Locale = getLocale()): string[] {
  const lines = [...funnyStatusLines(locale)];
  let h = 2166136261;
  for (let i = 0; i < seed.length; i++) {
    h ^= seed.charCodeAt(i);
    h = Math.imul(h, 16777619);
  }
  // mulberry32 on the FNV-1a hash: deterministic, no Math.random (testable)
  let a = h >>> 0;
  const next = () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
  for (let i = lines.length - 1; i > 0; i--) {
    const j = Math.floor(next() * (i + 1));
    [lines[i], lines[j]] = [lines[j], lines[i]];
  }
  return lines;
}
