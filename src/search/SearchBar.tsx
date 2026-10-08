import { useCallback, useEffect, useReducer, useRef, useState, type KeyboardEvent } from "react";
import { glow as glowTokens } from "../design/tokens";
import { normalizeAssistantCard, ASSISTANT_UPDATE_EVENT, type AssistantCard, type AssistantItem, type Choice, type SearchBarState } from "../lib/assistant/types";
import { getLocale } from "../lib/i18n";
import { ipc, onEvent } from "../lib/ipc";
import { AiSearchGlow } from "./AiSearchGlow";
import {
  BACKDROP_EVENT,
  GLASS_LIST_ID,
  answerRows,
  defaultSelection,
  enterOnSelection,
  glassEntries,
  glassOptionId,
  glassView,
  moveSelection,
  normalizeBackdrop,
  orderedChoices,
  type GlassBackdrop,
} from "./glassModel";
import { GlassSearch } from "./GlassSearch";
import { useDocumentHidden, useMediaFlag } from "./hooks";
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

export { useMediaFlag };

export type SearchApi = {
  submit: (text: string) => Promise<AssistantCard | null>;
  close: () => void;
  // The centre glass bar shows the answer itself; these are what its sheet calls (all optional: the
  // other variants never use them).
  /** An explicit click / Enter on a row. */
  openItem?: (queryId: string, itemId: string) => unknown;
  /** "הצג את כל התוצאות". */
  openCenter?: (queryId: string) => unknown;
  choose?: (queryId: string, optionId: string, remember: boolean) => Promise<AssistantCard | null>;
  extend?: (queryId: string) => Promise<AssistantCard | null>;
  /** The sheet's height (DIP) for the window's click-through region; resolves once it is applied. */
  region?: (height: number) => unknown;
  /** The screen picture behind the glass, for a page that missed the event. */
  backdrop?: () => Promise<unknown>;
};

const defaultApi: SearchApi = {
  submit: (text) => ipc.assistantSubmit(text, "searchBar"),
  close: () => void ipc.searchBarClose(),
  openItem: (queryId, itemId) => ipc.assistantOpenItem(queryId, itemId),
  openCenter: (queryId) => ipc.assistantOpenCenter(queryId),
  choose: (queryId, optionId, remember) => ipc.assistantChoose(queryId, optionId, remember),
  extend: (queryId) => ipc.assistantExtend(queryId),
  region: (height) => ipc.searchBarRegion(height),
  backdrop: () => ipc.searchBarBackdrop(),
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
  /** Dev preview only: the card the glass sheet shows (answer, question, error). */
  previewCard?: AssistantCard;
  /** Dev preview only: the picture behind the glass sheet. */
  previewBackdrop?: GlassBackdrop;
};

/** Cards kept for the sheet: the newest few, by query id. */
const MAX_CARDS = 8;

export function SearchBar({ bar, disabled = false, api = defaultApi, subscribe = onEvent, previewGlow, previewText, previewCard, previewBackdrop }: SearchBarProps) {
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
  const systemDark = useMediaFlag("(prefers-color-scheme: dark)");
  const hidden = useDocumentHidden();
  const mode = glowMode({ reducedMotion, forcedColors, highContrast: bar.highContrast });
  const glass = bar.variant === "spotlight";

  // ---- the centre glass bar: the answer lives in its sheet ----
  const [cards, setCards] = useState<Record<string, AssistantCard>>({});
  const [stale, setStale] = useState(false); // typed after a result: the field alone again
  const [selected, setSelected] = useState(-1);
  // The person chose the selected entry (an arrow key, the pointer); false while it is only the default.
  const [picked, setPicked] = useState(false);
  const [pendingChoice, setPendingChoice] = useState<string | null>(null);
  const [remember, setRemember] = useState(false);
  const [seed, setSeed] = useState("");
  const [liveBackdrop, setLiveBackdrop] = useState<GlassBackdrop | null>(null);
  const backdrop = previewBackdrop ?? liveBackdrop;

  /** A card for this search: the reducer decides whether it is ours, the sheet keeps it by id. */
  const acceptCard = useCallback((card: AssistantCard) => {
    dispatch({ type: "CARD", card });
    setCards((prev) => {
      const next = { ...prev };
      delete next[card.queryId]; // re-insert at the end: the oldest go first
      next[card.queryId] = card;
      const ids = Object.keys(next);
      for (const id of ids.slice(0, Math.max(0, ids.length - MAX_CARDS))) delete next[id];
      return next;
    });
  }, []);

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
        // Memory only: a closed bar keeps neither the answer it showed nor the picture behind it.
        setCards({});
        setStale(false);
        setSelected(-1);
        setPicked(false);
        setPendingChoice(null);
        setRemember(false);
        setLiveBackdrop(null);
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
        if (card) acceptCard(card);
      }),
    [subscribe, acceptCard],
  );

  // The glass bar's backdrop: pushed just before the window shows, or pulled by a page that was not
  // loaded yet (or missed it). It is dropped with the window (memory only).
  useEffect(() => {
    if (!glass || previewBackdrop) return;
    return subscribe(BACKDROP_EVENT, (payload) => {
      const next = normalizeBackdrop(payload);
      if (next) setLiveBackdrop((prev) => (prev && prev.id > next.id ? prev : next));
    });
  }, [glass, previewBackdrop, subscribe]);
  useEffect(() => {
    if (!glass || previewBackdrop || hidden || !api.backdrop) return;
    let alive = true;
    void Promise.resolve(api.backdrop())
      .then((raw) => {
        const next = normalizeBackdrop(raw);
        if (alive && next) setLiveBackdrop((prev) => (prev && prev.id >= next.id ? prev : next));
      })
      .catch(() => {});
    return () => {
      alive = false;
    };
  }, [glass, previewBackdrop, hidden, api]);

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
      setStale(false);
      dispatch({ type: "SUBMIT" });
      // Only the newest submit may settle the bar: a quick second Enter must not let the older
      // request's returned card be taken for the current one.
      const mine = ++request.current;
      // The playful line keeps its order for the whole search, whatever cards arrive meanwhile.
      setSeed(`${Date.now().toString(36)}-${mine}`);
      api
        .submit(q)
        .then((card) => {
          if (mine !== request.current) return;
          if (card) acceptCard(card);
          else dispatch({ type: "SUBMIT_FAILED" });
        })
        .catch(() => {
          if (mine === request.current) dispatch({ type: "SUBMIT_FAILED" });
        });
    },
    [api, acceptCard],
  );

  // ---- what the glass sheet shows, and what its rows do ----
  const glowState: GlowState = previewGlow ?? state.glow;
  const shownQuery = previewCard?.queryId ?? state.queryId;
  const view = glassView({
    glow: glowState,
    queryId: shownQuery,
    card: previewCard ?? (shownQuery ? (cards[shownQuery] ?? null) : null),
    hasText: text.trim().length > 0,
    stale,
  });
  const entries = glassEntries(view);
  const viewKey = `${view.kind}:${"card" in view && view.card ? `${view.card.queryId}:${view.card.createdAt}:${view.card.phase}` : ""}`;
  // A new result starts with its first row (or the preferred button) selected.
  useEffect(() => {
    setSelected(defaultSelection(entries));
    setPicked(false);
    setPendingChoice(null);
    setRemember(false);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [viewKey]);

  const openItem = (card: AssistantCard, item: AssistantItem) => {
    if (!item.openable) return;
    // The explicit action (click / Enter on the row); the backend's confirm policy is unchanged.
    void Promise.resolve(api.openItem?.(card.queryId, item.id)).catch(() => {});
  };
  const chooseOption = (card: AssistantCard, choice: Choice, rememberIt: boolean): Promise<boolean> => {
    if (!api.choose) return Promise.resolve(false);
    setPendingChoice(choice.id);
    dirty.current = false;
    return api
      .choose(card.queryId, choice.id, rememberIt)
      .then((next) => {
        if (next) acceptCard(next);
        else setPendingChoice(null);
        return !!next;
      })
      .catch(() => {
        setPendingChoice(null);
        return false;
      });
  };
  const extendSearch = (card: AssistantCard) => {
    if (!api.extend) return;
    void api
      .extend(card.queryId)
      .then((next) => next && acceptCard(next))
      .catch(() => {});
  };
  const openCenter = (card: AssistantCard) => {
    void Promise.resolve(api.openCenter?.(card.queryId)).catch(() => {});
    api.close();
  };
  const activate = (index: number) => {
    if (view.kind === "answer") {
      const item = answerRows(view.card)[index];
      if (item) openItem(view.card, item);
    } else if (view.kind === "choices") {
      const choice = orderedChoices(view.card.choices)[index];
      if (choice && pendingChoice === null) void chooseOption(view.card, choice, remember && choice.kind !== "option");
    }
  };

  const onKeyDown = (e: KeyboardEvent<HTMLInputElement>) => {
    const composing = e.nativeEvent.isComposing || e.keyCode === 229;
    if (glass && !composing && entries.selectable.length > 0 && (e.key === "ArrowDown" || e.key === "ArrowUp")) {
      // Over a list the arrows walk it (the question recall is for an empty sheet).
      e.preventDefault();
      setSelected((current) => moveSelection(entries.selectable, current, e.key === "ArrowDown" ? 1 : -1));
      setPicked(true);
      return;
    }
    const onSelection = glass && !composing && e.key === "Enter" ? enterOnSelection(view, selected, text, picked) : "ask";
    if (onSelection !== "ask") {
      e.preventDefault();
      // A held key repeats Enter: only its first press may act, and a command waits to be chosen.
      if (onSelection === "act" && !e.repeat) activate(selected);
      return;
    }
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

  const showHint = state.choices && text.length === 0;
  // Anchored on the Windows 10 search box the window is exactly the box: no outer margin, the
  // ring is drawn inside its edge so nothing shows above the taskbar.
  const margin = bar.anchored ? 0 : glowTokens.margin;
  const barRadius = Math.max(0, Math.min(bar.radius, (bar.height - 2 * margin) / 2));
  const live = glowState !== "idle" && glowState !== "disabled";

  if (glass) {
    const expanded = entries.kind !== "none";
    const inputEl = (
      <input
        ref={inputRef}
        className="gl-input"
        type="text"
        dir={text ? "auto" : locale === "he" ? "rtl" : "ltr"}
        role="combobox"
        aria-autocomplete="none"
        aria-haspopup="listbox"
        aria-expanded={expanded}
        aria-controls={expanded ? GLASS_LIST_ID : undefined}
        aria-activedescendant={expanded && selected >= 0 ? glassOptionId(selected) : undefined}
        value={text}
        maxLength={MAX_QUERY_CHARS}
        placeholder={showHint ? ss("glassChoicesHint", locale) : ss("spotlightPlaceholder", locale)}
        aria-label={ss("inputLabel", locale)}
        autoComplete="off"
        autoCorrect="off"
        spellCheck={false}
        disabled={disabled}
        onChange={(e) => {
          dirty.current = true;
          recallIndex.current = -1;
          setText(e.target.value);
          // Typing a new question gives way to the field: the old answer goes. (A question's buttons
          // stay: what is typed there may be the reply.)
          if (view.kind === "answer" || view.kind === "error") setStale(true);
          dispatch({ type: "INPUT", empty: e.target.value.length === 0 });
        }}
        onKeyDown={onKeyDown}
      />
    );
    return (
      <GlassSearch
        bar={bar}
        view={view}
        input={inputEl}
        hasText={text.trim().length > 0}
        locale={locale}
        backdrop={backdrop}
        systemDark={systemDark}
        reducedMotion={reducedMotion}
        plain={mode === "plain"}
        hidden={hidden}
        seed={previewCard?.queryId ?? seed}
        selected={selected}
        pendingChoice={pendingChoice}
        remember={remember}
        onToggleRemember={() => setRemember((on) => !on)}
        onSelect={(index) => {
          setSelected(index);
          setPicked(true);
        }}
        onOpenItem={openItem}
        onChoose={chooseOption}
        onExtend={extendSearch}
        onOpenCenter={openCenter}
        onBackground={() => focusInput()}
        onOutside={() => api.close()}
        onRegion={(height) => api.region?.(height)}
      />
    );
  }

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
