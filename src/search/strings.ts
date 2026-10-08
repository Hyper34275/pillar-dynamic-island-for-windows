// Strings of the smart search bar page (he/en). Not in lib/i18n.ts: this page is its own entry and
// the contract gives it its own table. Parity of the two tables is tested.

import { getLocale, type Locale } from "../lib/i18n";

const en = {
  placeholder: "Ask Yuval…",
  aiToggle: "Leave AI mode",
  choicesHint: "Pick one in the island or type an answer",
  glassChoicesHint: "Pick one below or type an answer",
  inputLabel: "Ask Yuval",
  failed: "Something went wrong. Try again.",
  spotlightPlaceholder: "Ask me anything…",
  spotlightKeys: "Shortcut: Alt and backtick",
  /** What a screen reader hears while the playful line shows. */
  processing: "Searching…",
  showAll: "Show all results",
  extend: "Search 10 s more",
  remember: "Remember my choice",
  openItem: "Open: {title}",
  results: "Results",
  choices: "Choices",
  escHint: "Esc closes",
  enterHint: "Enter",
  unavailable: "The answer could not be shown.",
} as const;

export type SearchStringKey = keyof typeof en;

const he: Record<SearchStringKey, string> = {
  placeholder: "שאל את יובל…",
  aiToggle: "יציאה ממצב AI",
  choicesHint: "אפשר לבחור באי או להקליד תשובה",
  glassChoicesHint: "אפשר לבחור למטה או להקליד תשובה",
  inputLabel: "שאל את יובל",
  failed: "משהו השתבש. אפשר לנסות שוב.",
  spotlightPlaceholder: "שאל אותי כל דבר…",
  spotlightKeys: "קיצור: Alt ותו הגרש ההפוך",
  processing: "מחפש…",
  showAll: "הצג את כל התוצאות",
  extend: "חפש עוד 10 שניות",
  remember: "זכור את הבחירה",
  openItem: "פתח: {title}",
  results: "תוצאות",
  choices: "אפשרויות",
  escHint: "Esc סוגר",
  enterHint: "Enter",
  unavailable: "אי אפשר היה להציג את התשובה.",
};

export const SEARCH_STRINGS: Record<Locale, Record<SearchStringKey, string>> = { en, he };

export function ss(key: SearchStringKey, locale: Locale = getLocale(), params?: Record<string, string>): string {
  const text = SEARCH_STRINGS[locale][key];
  return params ? text.replace(/\{(\w+)\}/g, (whole, name: string) => params[name] ?? whole) : text;
}
