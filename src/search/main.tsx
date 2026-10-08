// Entry of the smart search bar window (label "search"): the input and its glow.
import React, { useEffect, useState } from "react";
import ReactDOM from "react-dom/client";
import type { SearchBarState } from "../lib/assistant/types";
import { applyDocumentLocale } from "../lib/i18n";
import { ipc, normalizeSettings, onEvent } from "../lib/ipc";
import { DEFAULT_BAR, sanitizeBar } from "./bar";
import { SearchBar } from "./SearchBar";
import type { GlowState } from "./searchState";

type Preview = { glow: GlowState; bar: SearchBarState; text: string };

const GLOW_STATES: readonly GlowState[] = ["idle", "activated", "typing", "submitting", "processing", "completed", "error", "disabled"];

/** Dev only: ?preview=1&state=processing&w=360&h=52&r=4&hc=1&text=... renders without Tauri. */
function readPreview(): Preview | null {
  const q = new URLSearchParams(window.location.search);
  if (q.get("preview") !== "1") return null;
  const state = q.get("state") as GlowState;
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

  return <SearchBar bar={bar} disabled={disabled} previewGlow={preview?.glow} previewText={preview?.text} />;
}

applyDocumentLocale();
const preview = import.meta.env.DEV ? readPreview() : null;

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App preview={preview} />
  </React.StrictMode>,
);
