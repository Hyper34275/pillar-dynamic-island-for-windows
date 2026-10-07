// =============================================================================
// Direction: one semantic model, three levels.
//
//  1. SHELL. The island's layout (which side the icon, the tabs and the dismiss button are on)
//     follows the UI language: uiDirection(). Hebrew UI: right to left, always, for every card of
//     the Notification Center, so the icon never zig-zags between sides.
//     Exception: the standalone toast may follow its CONTENT language (contentDirection): an
//     English Snipping Tool toast reads fully left to right, a Hebrew Teams toast right to left.
//
//  2. FIELD. Each text field of an app notification has a rule of its own:
//       - app / source name  -> its own first-strong direction, isolated (sourceDirection):
//                               "Teams" stays LTR inside a Hebrew card.
//       - title, body, meeting subject, location (PARAGRAPHS) -> RTL when the text contains any
//                               Hebrew letter, LTR when it has Latin letters only, inherited when
//                               neutral (paragraphDirection / textDirection). Reason: first-strong
//                               gets Hebrew sentences wrong when they start with a Latin run.
//                               "Project Alpha_v2.pptx מוכן לבדיקה" and "Teams — דניאל כהן" are
//                               Hebrew sentences; first-strong would lay them out as English and put
//                               the trailing words and the dash on the wrong side. This rule is for
//                               whole paragraphs ONLY, never for a token inside one.
//
//  3. TOKEN. Technical tokens INSIDE any text are isolated LTR runs (<bdi dir="ltr">, see
//     splitBidi and BidiText): URLs, e-mail addresses, Windows paths, file names with an extension,
//     IPv4 addresses, times and time ranges, dates. They are atomic in reading order, so
//     "C:\Users\Daniel\Report.pdf" or "11:00–12:00" never get their punctuation shuffled by the
//     surrounding Hebrew. Plain numbers are NOT tokens: they stay with the bidi algorithm (digits
//     are weak, they take the direction of the paragraph's text).
//
// Isolation is real Unicode isolation (<bdi> = unicode-bidi: isolate), never margins or spaces.
// =============================================================================

import { getLocale, isRtl } from "../lib/i18n";

export type Dir = "rtl" | "ltr";

/**
 * The layout direction of the island: the UI language's (the pinned one, setFixedLocale("he"),
 * not the browser's; an English Windows still gets the Hebrew, right-to-left island).
 */
export function uiDirection(): Dir {
  return isRtl(getLocale()) ? "rtl" : "ltr";
}

const RTL_CHAR = /[\u0590-\u08FF\uFB1D-\uFDFF\uFE70-\uFEFF]/;
const LTR_CHAR = /[A-Za-z\u00C0-\u024F\u0370-\u03FF\u0400-\u04FF]/;

/** The direction of the first strongly directional character, or null for neutral text (digits, punctuation). */
export function firstStrongDirection(text: string | null | undefined): Dir | null {
  if (!text) return null;
  for (const char of text) {
    if (RTL_CHAR.test(char)) return "rtl";
    if (LTR_CHAR.test(char)) return "ltr";
  }
  return null;
}

/**
 * The direction of an app / source name ("Teams", "Outlook", "Snipping Tool"): its own first
 * strong letter, so an English name stays left to right inside a Hebrew card. Neutral names
 * (digits, symbols) inherit the shell.
 */
export function sourceDirection(name: string | null | undefined): Dir | undefined {
  return firstStrongDirection(name) ?? undefined;
}

/** Whether a text contains any right-to-left letter. */
export function hasRtl(text: string | null | undefined): boolean {
  return !!text && RTL_CHAR.test(text);
}

/**
 * The paragraph direction of one app-supplied text (title, body, subject, location), for its
 * `dir` (with the `bidi` class). Any Hebrew makes it right to left, even when it starts with an
 * English word ("Project Alpha_v2.pptx מוכן לבדיקה", "Teams — דניאל כהן" are Hebrew sentences, and
 * a first-strong guess reads them in the wrong order); text with Latin letters and no Hebrew is
 * left to right (its trailing punctuation stays at its end); neutral text (digits, an IP)
 * inherits. Technical tokens inside it are isolated separately (BidiText).
 */
export function textDirection(text: string | null | undefined): Dir | undefined {
  if (!text) return undefined;
  if (hasRtl(text)) return "rtl";
  return firstStrongDirection(text) ?? undefined;
}

/** The paragraph rule of the field model (see the top of the file). */
export const paragraphDirection = textDirection;

/**
 * The layout direction of content made of several texts (a standalone toast's title and body):
 * right to left when any of them has Hebrew, left to right when they are English only, the UI's
 * when they are neutral. NOT used by the Notification Center, whose cards follow uiDirection().
 */
export function contentDirection(...texts: Array<string | null | undefined>): Dir {
  if (texts.some(hasRtl)) return "rtl";
  for (const text of texts) {
    const dir = firstStrongDirection(text);
    if (dir) return dir;
  }
  return uiDirection();
}

// -----------------------------------------------------------------------------
// Technical tokens inside a text
// -----------------------------------------------------------------------------

export interface BidiSegment {
  text: string;
  /** True for a technical token: render it as <bdi dir="ltr">. */
  ltr: boolean;
}

const FILE_EXTENSIONS =
  "pptx?|docx?|xlsx?|xlsm|csv|pdf|txt|rtf|md|json|xml|ya?ml|zip|rar|7z|png|jpe?g|gif|bmp|svg|webp|heic|mp[34]|wav|mov|avi|mkv|msg|eml|ics|exe|msi|dll|bat|ps1|cmd|lnk|log|html?|css|js|ts|tsx|jsx|py|cs|cpp|java|sql|bak|iso|vsdx?|one|pub|psd";

const TIME = String.raw`\d{1,2}:\d{2}(?::\d{2})?(?:\s?[AaPp][Mm])?`;

const TOKEN = new RegExp(
  [
    // URL, up to the next whitespace; trailing sentence punctuation is given back afterwards.
    String.raw`(?:https?:\/\/|www\.)[^\s<>"]+`,
    // e-mail address
    String.raw`(?<![\w.%+])[A-Za-z0-9][A-Za-z0-9._%+-]*@[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)+`,
    // Windows path "C:\Users\Daniel\Report.pdf" (no spaces inside: a path with spaces ends at the space)
    String.raw`[A-Za-z]:\\[^\s<>"|?*]+`,
    // UNC path "\\server\share\file"
    String.raw`\\\\[A-Za-z0-9_.$-]+(?:\\[^\s<>"|?*]+)+`,
    // IPv4 address (before dates: 10.20.30.41 must not read as a date)
    String.raw`(?<![\w.])\d{1,3}(?:\.\d{1,3}){3}(?![\w.])`,
    // time or time range: "11:00", "11:00–12:00", "11:00 - 12:00", with optional AM / PM
    String.raw`(?<![\w:])${TIME}(?:\s?[\u2013\u2014-]\s?${TIME})?(?![\w:])`,
    // dates 2026-10-07, 7/10/2026, 07.10.26
    String.raw`(?<![\w.])(?:\d{4}-\d{2}-\d{2}|\d{1,2}[./]\d{1,2}[./]\d{2,4})(?![\w.])`,
    // file name with a known extension (Latin letters, digits, _ - . only)
    // (a Hebrew prefix hyphen before it, "ב-Report.pdf", is not part of the name)
    String.raw`(?<![\w.])[A-Za-z0-9_][A-Za-z0-9_.-]*\.(?:${FILE_EXTENSIONS})(?![A-Za-z0-9_])`,
  ].join("|"),
  "gi"
);

/** Sentence punctuation that a URL or path match swallows but that belongs to the sentence. */
const TRAILING_PUNCTUATION = /[.,;:!?)\]}'"\u05BE\u05C3]+$/;
const URL_OR_PATH = /^(?:https?:|www\.|[A-Za-z]:\\|\\\\)/i;

/** A letter or digit that can start or end an LTR phrase (neutrals and punctuation cannot). */
const PHRASE_EDGE = /[A-Za-z0-9À-ɏͰ-ϿЀ-ӿ]/;

/**
 * Splits a text into plain runs and isolated LTR phrases. Each technical token (URL, e-mail, path,
 * file name, IPv4, time, date) is isolated TOGETHER WITH the left-to-right words around it, up to
 * the nearest Hebrew letter, so the isolate is the whole English phrase: "Project Alpha_v2.pptx",
 * "Meeting at 11:00". Isolating the token alone would make it a neutral between English and Hebrew,
 * and in a right-to-left paragraph it would then be ordered right to left against its own words
 * ("Alpha_v2.pptx Project", "11:00 Meeting at": measured in the real WebView2). Neutrals at the
 * phrase's edges (spaces, a Hebrew prefix hyphen "ב-", brackets, sentence punctuation) stay outside.
 * Text with no token is left to the bidi algorithm. Concatenating the segments gives the original
 * text back, always.
 */
export function splitBidi(text: string | null | undefined): BidiSegment[] {
  if (!text) return [];
  const spans: Array<[number, number]> = [];
  for (const match of text.matchAll(TOKEN)) {
    let token = match[0];
    const start = match.index ?? 0;
    if (URL_OR_PATH.test(token)) {
      const trimmed = token.replace(TRAILING_PUNCTUATION, "");
      if (trimmed) token = trimmed;
    }
    // Grow the token over the left-to-right text around it (never over a Hebrew letter), then
    // give back the neutrals at both ends.
    let from = start;
    while (from > 0 && !RTL_CHAR.test(text[from - 1])) from--;
    while (from < start && !PHRASE_EDGE.test(text[from])) from++;
    let to = start + token.length;
    let end = to;
    while (end < text.length && !RTL_CHAR.test(text[end])) end++;
    const region = end;
    while (end > to && !PHRASE_EDGE.test(text[end - 1])) end--;
    // A bracket opened inside the phrase closes inside it too (never a lone "(" in the isolate).
    for (const [open, close] of [["(", ")"], ["[", "]"]]) {
      const slice = text.slice(from, end);
      if (slice.split(open).length > slice.split(close).length) {
        const at = text.indexOf(close, end);
        if (at !== -1 && at < region) end = at + 1;
      }
    }
    to = end;
    const last = spans[spans.length - 1];
    if (last && from <= last[1]) last[1] = Math.max(last[1], to);
    else spans.push([from, to]);
  }
  const segments: BidiSegment[] = [];
  let last = 0;
  for (const [from, to] of spans) {
    if (from > last) segments.push({ text: text.slice(last, from), ltr: false });
    segments.push({ text: text.slice(from, to), ltr: true });
    last = to;
  }
  if (last < text.length) segments.push({ text: text.slice(last), ltr: false });
  return segments;
}
