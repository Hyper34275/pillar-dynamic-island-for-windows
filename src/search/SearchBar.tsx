import { useCallback, useEffect, useReducer, useRef, useState, type KeyboardEvent } from "react";
import { glow as glowTokens } from "../design/tokens";
import { normalizeAssistantCard, ASSISTANT_UPDATE_EVENT, type AssistantCard, type SearchBarState } from "../lib/assistant/types";
import { getLocale } from "../lib/i18n";
import { ipc, onEvent } from "../lib/ipc";
import { AiSearchGlow } from "./AiSearchGlow";
import {
  INITIAL_STATE,
  MAX_QUERY_CHARS,
  glowMode,
  keyAction,
  recall,
  searchReducer,
  type GlowState,
} from "./searchState";
import { ss } from "./strings";
import "./search.css";

export type SearchApi = {
  submit: (text: string) => Promise<AssistantCard | null>;
  close: () => void;
};

const defaultApi: SearchApi = {
  submit: (text) => ipc.assistantSubmit(text, "searchBar"),
  close: () => void ipc.searchBarClose(),
};

export type SearchBarProps = {
  bar: SearchBarState;
  /** Smart search switched off in the settings. */
  disabled?: boolean;
  api?: SearchApi;
  subscribe?: (name: string, handler: (payload: unknown) => void) => () => void;
  /** Dev preview only: show this glow state without any backend. */
  previewGlow?: GlowState;
  /** Dev preview only: initial text. */
  previewText?: string;
};

/** Tracks a media query; false where matchMedia does not exist. */
export function useMediaFlag(query: string): boolean {
  const get = () => (typeof window !== "undefined" && typeof window.matchMedia === "function" ? window.matchMedia(query).matches : false);
  const [flag, setFlag] = useState(get);
  useEffect(() => {
    if (typeof window.matchMedia !== "function") return;
    const mq = window.matchMedia(query);
    const on = () => setFlag(mq.matches);
    on();
    mq.addEventListener?.("change", on);
    return () => mq.removeEventListener?.("change", on);
  }, [query]);
  return flag;
}

function useDocumentHidden(): boolean {
  const [hidden, setHidden] = useState(() => typeof document !== "undefined" && document.hidden);
  useEffect(() => {
    const on = () => setHidden(document.hidden);
    document.addEventListener("visibilitychange", on);
    return () => document.removeEventListener("visibilitychange", on);
  }, []);
  return hidden;
}

export function SearchBar({ bar, disabled = false, api = defaultApi, subscribe = onEvent, previewGlow, previewText }: SearchBarProps) {
  const [state, dispatch] = useReducer(searchReducer, INITIAL_STATE);
  const [text, setText] = useState(previewText ?? "");
  const inputRef = useRef<HTMLInputElement | null>(null);
  const history = useRef<string[]>([]);
  const recallIndex = useRef(-1);
  const dirty = useRef(false); // the user typed since the last submit
  const request = useRef(0); // id of the newest submit (older answers are ignored)
  const locale = getLocale();

  const reducedMotion = useMediaFlag("(prefers-reduced-motion: reduce)");
  const forcedColors = useMediaFlag("(forced-colors: active)");
  const hidden = useDocumentHidden();
  const mode = glowMode({ reducedMotion, forcedColors, highContrast: bar.highContrast });

  const focusInput = useCallback(() => {
    inputRef.current?.focus({ preventScroll: true });
  }, []);

  // The window is shown on mount; it is also re-shown (focus / visible) without a remount.
  useEffect(() => {
    dispatch({ type: "SHOW" });
    focusInput();
    const onFocus = () => {
      dispatch({ type: "SHOW" });
      focusInput();
    };
    const onVisibility = () => {
      if (document.hidden) {
        dispatch({ type: "HIDE" });
        setText("");
        recallIndex.current = -1;
      } else {
        dispatch({ type: "SHOW" });
        focusInput();
      }
    };
    window.addEventListener("focus", onFocus);
    document.addEventListener("visibilitychange", onVisibility);
    return () => {
      window.removeEventListener("focus", onFocus);
      document.removeEventListener("visibilitychange", onVisibility);
    };
  }, [focusInput]);

  useEffect(() => {
    dispatch({ type: "DISABLED", on: disabled });
  }, [disabled]);

  // Cards from the assistant (also those of the island): the reducer ignores stale ones.
  useEffect(
    () =>
      subscribe(ASSISTANT_UPDATE_EVENT, (payload) => {
        const card = normalizeAssistantCard(payload);
        if (card) dispatch({ type: "CARD", card });
      }),
    [subscribe],
  );

  // Completed is a visual hold of 600 ms, then the bar settles back to Activated.
  useEffect(() => {
    if (state.glow !== "completed") return;
    const id = window.setTimeout(() => dispatch({ type: "HOLD_DONE" }), glowTokens.completedHoldMs);
    return () => window.clearTimeout(id);
  }, [state.glow]);

  // After an answer the text stays selected (a follow-up replaces it); after a question the field
  // empties so the hint shows and the reply can be typed. Never touches text typed meanwhile.
  useEffect(() => {
    if (state.glow !== "completed" || dirty.current) return;
    if (state.choices) setText("");
    else inputRef.current?.select();
  }, [state.glow, state.choices, state.queryId]);

  const submit = useCallback(
    (value: string) => {
      const q = value.trim().slice(0, MAX_QUERY_CHARS);
      if (!q) return;
      if (history.current[history.current.length - 1] !== q) history.current = [...history.current.slice(-19), q];
      recallIndex.current = -1;
      dirty.current = false;
      dispatch({ type: "SUBMIT" });
      // Only the newest submit may settle the bar: a quick second Enter must not let the older
      // request's returned card be taken for the current one.
      const mine = ++request.current;
      api
        .submit(q)
        .then((card) => {
          if (mine !== request.current) return;
          if (card) dispatch({ type: "CARD", card });
          else dispatch({ type: "SUBMIT_FAILED" });
        })
        .catch(() => {
          if (mine === request.current) dispatch({ type: "SUBMIT_FAILED" });
        });
    },
    [api],
  );

  const onKeyDown = (e: KeyboardEvent<HTMLInputElement>) => {
    const composing = e.nativeEvent.isComposing || e.keyCode === 229;
    const action = keyAction({ key: e.key, isComposing: composing }, text);
    switch (action) {
      case "submit":
        e.preventDefault();
        submit(text);
        break;
      case "close":
        e.preventDefault();
        api.close();
        break;
      case "recall":
      case "recallDown": {
        const r = recall(history.current, recallIndex.current, action === "recall" ? "up" : "down");
        if (r.text !== null) {
          e.preventDefault();
          recallIndex.current = r.index;
          setText(r.text);
          dispatch({ type: "INPUT", empty: r.text.length === 0 });
        }
        break;
      }
      default:
        break;
    }
  };

  const glowState: GlowState = previewGlow ?? state.glow;
  const showHint = state.choices && text.length === 0;
  // Anchored on the Windows 10 search box the window is exactly the box: no outer margin, the
  // ring is drawn inside its edge so nothing shows above the taskbar.
  const margin = bar.anchored ? 0 : glowTokens.margin;
  const barRadius = Math.max(0, Math.min(bar.radius, (bar.height - 2 * margin) / 2));
  const live = glowState !== "idle" && glowState !== "disabled";

  return (
    <div className="sb-root" style={{ width: bar.width, height: bar.height }} data-glow={live ? "on" : "off"} data-mode={mode}>
      <AiSearchGlow state={glowState} mode={mode} width={bar.width} height={bar.height} radius={barRadius} margin={margin} hidden={hidden} />
      <div
        className="sb-bar"
        data-state={glowState}
        style={{ inset: margin, borderRadius: barRadius }}
        onMouseDown={(e) => {
          if (e.target !== inputRef.current) {
            e.preventDefault();
            focusInput();
          }
        }}
      >
        <button type="button" className="sb-ai" aria-label={ss("aiToggle", locale)} title={ss("aiToggle", locale)} onClick={() => api.close()}>
          <SparkleIcon />
        </button>
        <input
          ref={inputRef}
          className="sb-input"
          type="text"
          dir={text ? "auto" : locale === "he" ? "rtl" : "ltr"}
          value={text}
          maxLength={MAX_QUERY_CHARS}
          placeholder={showHint ? ss("choicesHint", locale) : ss("placeholder", locale)}
          aria-label={ss("inputLabel", locale)}
          autoComplete="off"
          autoCorrect="off"
          spellCheck={false}
          disabled={disabled}
          onChange={(e) => {
            dirty.current = true;
            recallIndex.current = -1;
            setText(e.target.value);
            dispatch({ type: "INPUT", empty: e.target.value.length === 0 });
          }}
          onKeyDown={onKeyDown}
        />
        <span className="sb-status" role="status" aria-live="polite">
          {glowState === "error" ? ss("failed", locale) : ""}
        </span>
      </div>
    </div>
  );
}

/** Four-point sparkle, filled with the glow hues. Static (no animation, no filter). */
function SparkleIcon() {
  return (
    <svg width="18" height="18" viewBox="0 0 24 24" aria-hidden="true" focusable="false">
      <defs>
        <linearGradient id="sb-sparkle" x1="3" y1="21" x2="21" y2="3" gradientUnits="userSpaceOnUse">
          <stop offset="0" stopColor={glowTokens.cyan} />
          <stop offset="0.45" stopColor={glowTokens.indigo} />
          <stop offset="0.75" stopColor={glowTokens.magenta} />
          <stop offset="1" stopColor={glowTokens.softPink} />
        </linearGradient>
      </defs>
      <path className="sb-sparkle" fill="url(#sb-sparkle)" d="M10 2l1.9 5.6a4 4 0 0 0 2.5 2.5L20 12l-5.6 1.9a4 4 0 0 0-2.5 2.5L10 22l-1.9-5.6a4 4 0 0 0-2.5-2.5L0 12l5.6-1.9a4 4 0 0 0 2.5-2.5L10 2z" transform="translate(2 0) scale(0.9) translate(1 1)" />
      <path className="sb-sparkle" fill={glowTokens.violet} d="M19.5 2l.8 2.2a1.6 1.6 0 0 0 1 1l2.2.8-2.2.8a1.6 1.6 0 0 0-1 1l-.8 2.2-.8-2.2a1.6 1.6 0 0 0-1-1l-2.2-.8 2.2-.8a1.6 1.6 0 0 0 1-1l.8-2.2z" />
    </svg>
  );
}
