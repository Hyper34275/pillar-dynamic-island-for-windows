// Entry of the smart search bar window (label "search"): the input and its glow.
import React, { useEffect, useState } from "react";
import ReactDOM from "react-dom/client";
import type { AssistantCard, SearchBarState } from "../lib/assistant/types";
import { applyDocumentLocale } from "../lib/i18n";
import { ipc, normalizeSettings, onEvent } from "../lib/ipc";
import { DEFAULT_BAR, SPOTLIGHT_BAR, sanitizeBar } from "./bar";
import { previewBackdrop, previewCard, type GlassPreviewState } from "./glassPreview";
import type { GlassBackdrop } from "./glassModel";
import { SearchBar, type SearchApi } from "./SearchBar";
import type { GlowState } from "./searchState";

type Preview = { glow: GlowState; bar: SearchBarState; text: string; card?: AssistantCard; backdrop?: GlassBackdrop };

const GLOW_STATES: readonly GlowState[] = ["idle", "activated", "typing", "submitting", "processing", "completed", "error", "disabled"];

/** The glass sheet's preview states and the glow state each stands for. */
const GLASS_STATES: Record<GlassPreviewState, GlowState> = {
  ready: "activated",
  typing: "typing",
  processing: "processing",
  answer: "completed",
  choices: "completed",
  error: "error",
};

/** A backend that does nothing: the preview sheet reports its height and clicks go nowhere. */
const PREVIEW_API: SearchApi = {
  submit: () => Promise.resolve(null),
  close: () => {},
  openItem: () => {},
  openCenter: () => {},
  choose: () => Promise.resolve(null),
  extend: () => Promise.resolve(null),
  region: () => {},
  backdrop: () => Promise.resolve(null),
};

/**
 * Dev only: ?preview=1&state=processing&w=360&h=52&r=4&hc=1&text=... renders without Tauri.
 * The glass sheet: ?preview=1&variant=spotlight&state=ready|typing|processing|answer|choices|error
 * &theme=dark|light &bg=<image url> (the screen behind it, as the backend would capture it)
 * &rows=3 (answer rows) &hc=1 &opaque=1 &text=...
 */
function readPreview(): Preview | null {
  const q = new URLSearchParams(window.location.search);
  if (q.get("preview") !== "1") return null;
  const state = q.get("state") as GlowState;
  if (q.get("variant") === "spotlight") {
    const gs = q.get("state") as GlassPreviewState;
    const sheet = gs in GLASS_STATES ? gs : null;
    const text = q.get("text") ?? (sheet === "typing" || sheet === "processing" || sheet === "answer" || sheet === "choices" || sheet === "error" ? "מה יש לי מחר?" : "");
    return {
      glow: sheet ? GLASS_STATES[sheet] : GLOW_STATES.includes(state) ? state : "activated",
      bar: sanitizeBar({ ...SPOTLIGHT_BAR, highContrast: q.get("hc") === "1" }),
      text,
      card: sheet ? previewCard(sheet, { rows: Number(q.get("rows")) || 3 }) : undefined,
      backdrop: previewBackdrop({ dark: q.get("theme") !== "light", image: q.get("bg"), opaque: q.get("opaque") === "1" }),
    };
  }
  const w = Number(q.get("w")) || 360;
  const h = Number(q.get("h")) || 52;
  return {
    glow: GLOW_STATES.includes(state) ? state : "activated",
    bar: sanitizeBar({ width: w, height: h, radius: q.has("r") ? Number(q.get("r")) : 4, highContrast: q.get("hc") === "1", anchored: q.get("a") === "1" }),
    text: q.get("text") ?? "",
  };
}

function App({ preview }: { preview: Preview | null }) {
  const [bar, setBar] = useState<SearchBarState>(preview?.bar ?? DEFAULT_BAR);
  const [disabled, setDisabled] = useState(false);

  useEffect(() => {
    if (preview) return;
    let alive = true;
    ipc
      .searchBarState()
      .then((s) => alive && setBar(sanitizeBar(s)))
      .catch(() => {});
    ipc
      .getSettings()
      .then((s) => alive && s && setDisabled(!s.aiSearchEnabled))
      .catch(() => {});
    const offBar = onEvent<Partial<SearchBarState>>("search-bar-state", (s) => setBar(sanitizeBar(s)));
    const offSettings = onEvent<unknown>("settings-changed", (raw) => setDisabled(!normalizeSettings(raw).aiSearchEnabled));
    return () => {
      alive = false;
      offBar();
      offSettings();
    };
  }, [preview]);

  return (
    <SearchBar
      bar={bar}
      disabled={disabled}
      api={preview ? PREVIEW_API : undefined}
      previewGlow={preview?.glow}
      previewText={preview?.text}
      previewCard={preview?.card}
      previewBackdrop={preview?.backdrop}
    />
  );
}

applyDocumentLocale();
const preview = import.meta.env.DEV ? readPreview() : null;

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App preview={preview} />
  </React.StrictMode>,
);
