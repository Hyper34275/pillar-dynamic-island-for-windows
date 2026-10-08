// Dev preview data of the glass sheet (search.html?preview=1&variant=spotlight&state=...). Fixed demo
// cards and an optional picture URL; never reached in a build (main.tsx reads it under import.meta.env.DEV).

import type { AssistantCard, AssistantItem } from "../lib/assistant/types";
import type { GlassBackdrop } from "./glassModel";

export type GlassPreviewState = "ready" | "typing" | "processing" | "answer" | "choices" | "error";

const BASE: AssistantCard = {
  queryId: "preview",
  query: "מה יש לי מחר?",
  phase: "answer",
  lang: "he",
  title: "",
  summary: "",
  question: null,
  choices: [],
  items: [],
  total: 0,
  partial: false,
  canExtend: false,
  errorCode: null,
  sources: [],
  createdAt: 1,
  followUp: false,
};

/** Tomorrow at hh:mm local, as unix ms (the rows show times, not dates). */
function at(hour: number, minute: number): number {
  const d = new Date();
  d.setDate(d.getDate() + 1);
  d.setHours(hour, minute, 0, 0);
  return d.getTime();
}

const item = (id: string, title: string, source: string, from: [number, number], to: [number, number], accent: string): AssistantItem => ({
  id,
  kind: "event",
  title,
  subtitle: null,
  time: at(...from),
  endTime: at(...to),
  accent,
  openable: true,
  unread: false,
  source,
});

const ROWS: AssistantItem[] = [
  item("e1", "סינכרון צוות הפיתוח", "יומן עבודה", [9, 30], [10, 0], "#0A84FF"),
  item("e2", "ארוחת צהריים עם דנה", "יומן אישי", [13, 0], [14, 0], "#30D158"),
  item("e3", "סקירת תקציב רבעון 4", "יומן צוות", [16, 30], [17, 30], "#BF5AF2"),
  item("e4", "שיחה עם הספק", "יומן עבודה", [17, 45], [18, 15], "#0A84FF"),
  item("e5", "הכנה ליום ראשון", "יומן אישי", [19, 0], [19, 30], "#FF9F0A"),
];

/** The card the sheet shows in a state (null for the states that have none). */
export function previewCard(state: GlassPreviewState, opts: { rows: number }): AssistantCard | undefined {
  switch (state) {
    case "answer": {
      const rows = ROWS.slice(0, Math.max(0, Math.min(5, opts.rows)));
      return { ...BASE, phase: "answer", title: `מחר יש לך ${rows.length} פגישות`, items: rows, total: rows.length, sources: ["calendar"] };
    }
    case "choices":
      return {
        ...BASE,
        phase: "choices",
        title: "באיזו תיבת דואר לחפש?",
        question: "באיזו תיבת דואר לחפש?",
        choices: [
          { id: "m1", label: "דואר עבודה", kind: "mailbox", preferred: true },
          { id: "m2", label: "דואר אישי", kind: "mailbox", preferred: false },
          { id: "all", label: "לא יודע - חפש בכולן", kind: "allMailboxes", preferred: false },
        ],
      };
    case "error":
      return { ...BASE, phase: "error", title: "לא הצלחתי לקרוא את היומן", summary: "Outlook לא ענה בזמן. אפשר לנסות שוב.", errorCode: "OUTLOOK-101" };
    case "processing":
      return { ...BASE, phase: "processing" };
    default:
      return undefined;
  }
}

/** The preview's backdrop: a picture from `image` (any URL), or none (the near-opaque fallback tint). */
export function previewBackdrop(opts: { dark: boolean; image: string | null; opaque: boolean }): GlassBackdrop {
  return { id: 1, image: opts.opaque ? null : opts.image, luminance: opts.dark ? 0.2 : 0.8, dark: opts.dark, transparency: !opts.opaque };
}
