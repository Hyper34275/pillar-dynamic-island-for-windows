// Smart search wire types (mirror of src-tauri/src/assistant/wire.rs). CONTRACT: shared by the
// island (AssistantCard surface) and the search bar window. Fields may be added, never renamed.
//
// Cards carry mail subjects, names and calendar details: memory only, never logged or persisted.

export type CardPhase = "processing" | "answer" | "choices" | "error";
export type ItemKind = "event" | "mail" | "note" | "file" | "app" | "calc" | "slot" | "info";
export type ChoiceKind = "mailbox" | "allMailboxes" | "option";
export type AssistantLang = "he" | "en";

export type AssistantItem = {
  id: string;
  kind: ItemKind;
  title: string;
  subtitle: string | null;
  /** Unix ms: event start, mail received, note/file modified. */
  time: number | null;
  /** Unix ms: event end. */
  endTime: number | null;
  accent: string | null;
  openable: boolean;
  unread: boolean;
  /** Mailbox / calendar / folder name. */
  source: string | null;
};

export type Choice = {
  id: string;
  label: string;
  kind: ChoiceKind;
  preferred: boolean;
};

export type AssistantCard = {
  queryId: string;
  query: string;
  phase: CardPhase;
  lang: AssistantLang;
  title: string;
  summary: string;
  question: string | null;
  choices: Choice[];
  items: AssistantItem[];
  total: number;
  partial: boolean;
  canExtend: boolean;
  errorCode: string | null;
  sources: string[];
  createdAt: number;
  followUp: boolean;
};

const PHASES: readonly CardPhase[] = ["processing", "answer", "choices", "error"];
const ITEM_KINDS: readonly ItemKind[] = ["event", "mail", "note", "file", "app", "calc", "slot", "info"];
const CHOICE_KINDS: readonly ChoiceKind[] = ["mailbox", "allMailboxes", "option"];
const QUERY_ID = /^[A-Za-z0-9_-]{1,64}$/;
const MAX_ITEMS = 50;
const MAX_CHOICES = 12;

function isRecord(value: unknown): value is Record<string, unknown> {
  return !!value && typeof value === "object" && !Array.isArray(value);
}
const str = (v: unknown, max = 2000): string => (typeof v === "string" ? v.slice(0, max) : "");
const strOrNull = (v: unknown, max = 2000): string | null => (typeof v === "string" && v.length > 0 ? v.slice(0, max) : null);
const num = (v: unknown): number | null => (typeof v === "number" && Number.isFinite(v) ? v : null);

export function normalizeItem(raw: unknown): AssistantItem | null {
  if (!isRecord(raw) || typeof raw.id !== "string" || !ITEM_KINDS.includes(raw.kind as ItemKind)) return null;
  return {
    id: raw.id.slice(0, 64),
    kind: raw.kind as ItemKind,
    title: str(raw.title, 400),
    subtitle: strOrNull(raw.subtitle, 400),
    time: num(raw.time),
    endTime: num(raw.endTime),
    accent: typeof raw.accent === "string" && /^#[0-9A-Fa-f]{6}$/.test(raw.accent) ? raw.accent : null,
    openable: raw.openable === true,
    unread: raw.unread === true,
    source: strOrNull(raw.source, 200),
  };
}

function normalizeChoice(raw: unknown): Choice | null {
  if (!isRecord(raw) || typeof raw.id !== "string" || !CHOICE_KINDS.includes(raw.kind as ChoiceKind)) return null;
  return { id: raw.id.slice(0, 64), label: str(raw.label, 200), kind: raw.kind as ChoiceKind, preferred: raw.preferred === true };
}

function list<T>(v: unknown, f: (x: unknown) => T | null, max: number): T[] {
  return Array.isArray(v) ? v.slice(0, max).map(f).filter((x): x is T => x !== null) : [];
}

/** A card from the backend, or null when it is unusable (never throws). */
export function normalizeAssistantCard(raw: unknown): AssistantCard | null {
  if (!isRecord(raw) || typeof raw.queryId !== "string" || !QUERY_ID.test(raw.queryId)) return null;
  if (!PHASES.includes(raw.phase as CardPhase)) return null;
  return {
    queryId: raw.queryId,
    query: str(raw.query, 500),
    phase: raw.phase as CardPhase,
    lang: raw.lang === "en" ? "en" : "he",
    title: str(raw.title, 400),
    summary: str(raw.summary, 1000),
    question: strOrNull(raw.question, 400),
    choices: list(raw.choices, normalizeChoice, MAX_CHOICES),
    items: list(raw.items, normalizeItem, MAX_ITEMS),
    total: Math.max(0, Math.round(num(raw.total) ?? 0)),
    partial: raw.partial === true,
    canExtend: raw.canExtend === true,
    errorCode: strOrNull(raw.errorCode, 40),
    sources: Array.isArray(raw.sources) ? raw.sources.filter((s): s is string => typeof s === "string").slice(0, 8) : [],
    createdAt: num(raw.createdAt) ?? 0,
    followUp: raw.followUp === true,
  };
}

/** Wire name of the Tauri event carrying an AssistantCard. */
export const ASSISTANT_UPDATE_EVENT = "assistant-update";

/** What the search bar window lays out to (search_bar::SearchBarState). */
export type SearchBarVariant = "taskbar" | "floating" | "spotlight";

export type SearchBarState = {
  /** taskbar = on the Win10 search box, floating = above the taskbar, spotlight = screen centre. */
  variant: SearchBarVariant;
  anchored: boolean;
  width: number;
  height: number;
  radius: number;
  scale: number;
  highContrast: boolean;
  edge: string;
};
