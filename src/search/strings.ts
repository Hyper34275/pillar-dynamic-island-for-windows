// Strings of the smart search bar page (he/en). Not in lib/i18n.ts: this page is its own entry and
// the contract gives it its own table. Parity of the two tables is tested.

import { getLocale, type Locale } from "../lib/i18n";

const en = {
  placeholder: "Ask CompanyIsland…",
  aiToggle: "Leave AI mode",
  choicesHint: "Pick one in the island or type an answer",
  inputLabel: "Ask CompanyIsland",
  failed: "Something went wrong. Try again.",
} as const;

export type SearchStringKey = keyof typeof en;

const he: Record<SearchStringKey, string> = {
  placeholder: "שאל את CompanyIsland…",
  aiToggle: "יציאה ממצב AI",
  choicesHint: "אפשר לבחור באי או להקליד תשובה",
  inputLabel: "שאל את CompanyIsland",
  failed: "משהו השתבש. אפשר לנסות שוב.",
};

export const SEARCH_STRINGS: Record<Locale, Record<SearchStringKey, string>> = { en, he };

export function ss(key: SearchStringKey, locale: Locale = getLocale()): string {
  return SEARCH_STRINGS[locale][key];
}
