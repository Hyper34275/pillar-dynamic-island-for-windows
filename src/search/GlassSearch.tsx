import { useEffect, useLayoutEffect, useMemo, useRef, useState, type CSSProperties, type ReactNode } from "react";
import { BidiText } from "../components/Pill/ui/BidiText";
import { textDirection } from "../design/direction";
import { spotlight } from "../design/tokens";
import { dlog } from "../lib/debugLog";
import { FUNNY_STATUS_MS, funnyStatusOrder } from "../lib/assistant/funnyStatus";
import type { AssistantCard, AssistantItem, Choice, SearchBarState } from "../lib/assistant/types";
import type { Locale } from "../lib/i18n";
import {
  BACKDROP_FADE_MS,
  HEIGHT_MS,
  answerRows,
  choiceText,
  estimateSheetHeight,
  glassHeadline,
  glassOptionId,
  glassSummary,
  GLASS_LIST_ID,
  GLASS_METRICS,
  heightPlan,
  maxSheetHeight,
  orderedChoices,
  rowMeta,
  rowTime,
  showsAllResults,
  showsRemember,
  tintAlpha,
  type GlassBackdrop,
  type GlassTheme,
  type GlassView,
} from "./glassModel";
import { ss } from "./strings";
import "./glass.css";

export type GlassSearchProps = {
  bar: SearchBarState;
  view: GlassView;
  /** The text input (owned by SearchBar, which keeps all the logic). */
  input: ReactNode;
  hasText: boolean;
  locale: Locale;
  backdrop: GlassBackdrop | null;
  /** The system colour scheme, used until (or without) the Windows theme of the backdrop. */
  systemDark: boolean;
  reducedMotion: boolean;
  /** Forced colours / high contrast: system colours, a 2 px border, no picture. */
  plain: boolean;
  /** document.hidden: nothing moves. */
  hidden: boolean;
  /** Keeps the playful line on one order for the whole search (a new card must not reshuffle it). */
  seed: string;
  /** The selected row / button (index), or -1. */
  selected: number;
  /** The button whose answer is on its way (all buttons wait meanwhile). */
  pendingChoice: string | null;
  /** The "remember my choice" switch of a mailbox question. */
  remember: boolean;
  onToggleRemember: () => void;
  onSelect: (index: number) => void;
  onOpenItem: (card: AssistantCard, item: AssistantItem) => void;
  /** Resolves true when the answer came back (false: the buttons are live again). */
  onChoose: (card: AssistantCard, choice: Choice, remember: boolean) => Promise<boolean>;
  onExtend: (card: AssistantCard) => void;
  onOpenCenter: (card: AssistantCard) => void;
  /** A click on the sheet outside the input hands the focus back to it. */
  onBackground: () => void;
  /** The sheet's height (DIP). Resolves once the window's click-through region follows. */
  onRegion: (height: number) => unknown;
  /** The clock of the rows' time column (tests). */
  now?: () => Date;
};

/** The class flags of the root: what the fallbacks switch on (tested as flags, painted by glass.css). */
export function glassClasses(opts: { theme: GlassTheme; plain: boolean; reducedMotion: boolean; opaque: boolean }): string {
  return ["gl-root", `gl-${opts.theme}`, opts.plain ? "gl-plain" : "", opts.reducedMotion ? "gl-reduced" : "", opts.opaque ? "gl-opaque" : ""].filter(Boolean).join(" ");
}

// =============================================================================
// Icons (the mock's, drawn inline: nothing to fetch)
// =============================================================================

const SPARK_PATH = "M12 2c.6 5.5 4.5 9.4 10 10-5.5.6-9.4 4.5-10 10-.6-5.5-4.5-9.4-10-10 5.5-.6 9.4-4.5 10-10Z";

function Spark({ size }: { size: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" aria-hidden="true" focusable="false">
      <path d={SPARK_PATH} fill="currentColor" />
    </svg>
  );
}

function Ring() {
  return (
    <svg className="gl-ring" width="18" height="18" viewBox="0 0 18 18" fill="none" aria-hidden="true" focusable="false">
      <circle cx="9" cy="9" r="6.6" stroke="currentColor" strokeOpacity=".22" strokeWidth="2" />
      <path d="M9 2.4a6.6 6.6 0 0 1 6.6 6.6" stroke="currentColor" strokeWidth="2" strokeLinecap="round" />
    </svg>
  );
}

function EnterIcon() {
  return (
    <svg width="12" height="12" viewBox="0 0 12 12" fill="none" aria-hidden="true" focusable="false">
      <path d="M10 2.5v3.2a1.3 1.3 0 0 1-1.3 1.3H2.5m0 0L4.8 4.7M2.5 7l2.3 2.3" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round" strokeLinejoin="round" />
    </svg>
  );
}

function Chevron() {
  return (
    <svg className="gl-chevron" width="12" height="12" viewBox="0 0 12 12" fill="none" aria-hidden="true" focusable="false">
      <path d="M7.5 2.5L4 6l3.5 3.5" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round" strokeLinejoin="round" />
    </svg>
  );
}

function KindIcon({ kind }: { kind: AssistantItem["kind"] }) {
  const common = { width: 19, height: 19, viewBox: "0 0 18 18", fill: "none", "aria-hidden": true, focusable: false } as const;
  const stroke = { stroke: "currentColor", strokeWidth: 1.4, strokeLinecap: "round", strokeLinejoin: "round" } as const;
  switch (kind) {
    case "mail":
      return (
        <svg {...common}>
          <rect x="2.5" y="4" width="13" height="10" rx="2.5" {...stroke} />
          <path d="M3 5.5l6 4.2 6-4.2" {...stroke} />
        </svg>
      );
    case "note":
      return (
        <svg {...common}>
          <path d="M4 2.8h7.2L14.5 6v8.2a1.2 1.2 0 0 1-1.2 1.2H4.7a1.2 1.2 0 0 1-1.2-1.2V4A1.2 1.2 0 0 1 4 2.8Z" {...stroke} />
          <path d="M6 9h6M6 11.8h4" {...stroke} />
        </svg>
      );
    case "file":
      return (
        <svg {...common}>
          <path d="M4.5 2.5h5.2l3.8 3.8v8.2a1 1 0 0 1-1 1h-8a1 1 0 0 1-1-1v-11a1 1 0 0 1 1-1Z" {...stroke} />
          <path d="M9.5 2.8V6.5h3.7" {...stroke} />
        </svg>
      );
    case "app":
      return (
        <svg {...common}>
          <rect x="3" y="3" width="4.8" height="4.8" rx="1.2" {...stroke} />
          <rect x="10.2" y="3" width="4.8" height="4.8" rx="1.2" {...stroke} />
          <rect x="3" y="10.2" width="4.8" height="4.8" rx="1.2" {...stroke} />
          <rect x="10.2" y="10.2" width="4.8" height="4.8" rx="1.2" {...stroke} />
        </svg>
      );
    case "calc":
      return (
        <svg {...common}>
          <rect x="3.5" y="2.5" width="11" height="13" rx="2.2" {...stroke} />
          <path d="M6 6h6M6.5 9.2h1M10.5 9.2h1M6.5 12h1M10.5 12h1" {...stroke} />
        </svg>
      );
    case "action":
      return (
        <svg {...common}>
          <path d="M4 9h9.5M10 5.5L13.5 9 10 12.5" {...stroke} />
        </svg>
      );
    case "slot":
      return (
        <svg {...common}>
          <circle cx="9" cy="9" r="6" {...stroke} />
          <path d="M9 5.5V9l2.4 1.6" {...stroke} />
        </svg>
      );
    case "info":
      return (
        <svg {...common}>
          <circle cx="9" cy="9" r="6" {...stroke} />
          <path d="M9 8.2v4M9 5.7v.1" {...stroke} />
        </svg>
      );
    case "event":
    default:
      return (
        <svg {...common}>
          <rect x="2.5" y="3.5" width="13" height="12" rx="2.5" {...stroke} />
          <path d="M2.5 7.5h13" {...stroke} />
          <path d="M6 2v3M12 2v3" {...stroke} />
          <rect x="5.6" y="10" width="2" height="2" rx=".6" fill="currentColor" />
        </svg>
      );
  }
}

/**
 * A real move of the pointer. A row that appears under a pointer at rest also gets a mousemove from
 * the browser (it follows the layout), with no movement: that must not count as the person choosing it.
 */
function pointerMoved(e: { movementX?: number; movementY?: number }): boolean {
  return !!e.movementX || !!e.movementY;
}

/** The tile colour of a row: the calendar's own colour, a neutral one for what has none. */
const EVENT_ACCENT = "#0A84FF";
function rowAccent(item: AssistantItem): string | null {
  if (item.accent) return item.accent;
  return item.kind === "event" || item.kind === "action" ? EVENT_ACCENT : null;
}

// =============================================================================
// The playful line of a working search (src/lib/assistant/funnyStatus.ts)
// =============================================================================

function useFunnyLine(seed: string, active: boolean): string {
  const lines = useMemo(() => funnyStatusOrder(seed), [seed]);
  const [index, setIndex] = useState(0);
  useEffect(() => {
    setIndex(0);
    if (!active) return;
    const timer = window.setInterval(() => setIndex((i) => (i + 1) % lines.length), FUNNY_STATUS_MS);
    return () => window.clearInterval(timer);
  }, [lines, active]);
  return lines[index] ?? ss("processing");
}

// =============================================================================
// Height: measured, then sequenced with the window's click-through region
// =============================================================================

/** The natural height of the content (px), measured; null where nothing can be measured (jsdom, hidden). */
function useMeasuredHeight(ref: React.RefObject<HTMLElement | null>, key: string): number | null {
  const [height, setHeight] = useState<number | null>(null);
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    // Layout sizes, not getBoundingClientRect: the sheet is mid scale-in when it first shows, and a
    // transform would shrink what is measured.
    const set = (h: number) => setHeight(h > 0 ? Math.ceil(h) : null);
    set(el.offsetHeight);
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver((entries) => {
      const box = entries[entries.length - 1]?.borderBoxSize?.[0];
      set(box ? box.blockSize : el.offsetHeight);
    });
    observer.observe(el);
    return () => observer.disconnect();
  }, [ref, key]);
  return height;
}

// =============================================================================
// The sheet
// =============================================================================

export function GlassSearch(props: GlassSearchProps) {
  const { bar, view, input, hasText, locale, backdrop, systemDark, reducedMotion, plain, hidden, seed, onRegion } = props;
  const rtl = locale === "he";

  // ----- the glass: picture, theme, tint -----
  const image = !plain && backdrop?.transparency !== false ? (backdrop?.image ?? null) : null;
  const opaque = !plain && image === null;
  const theme: GlassTheme = backdrop ? (backdrop.dark ? "dark" : "light") : systemDark ? "dark" : "light";
  const alpha = tintAlpha(theme, image ? (backdrop?.luminance ?? null) : null, opaque);
  const [loadedId, setLoadedId] = useState<number | null>(null);
  const imageReady = image !== null && loadedId === backdrop?.id;

  // ----- geometry: the window is fixed, the sheet sits in it and grows downward -----
  const sheetWidth = Math.max(160, Math.min(spotlight.width, bar.width - 2 * spotlight.marginX));
  const sheetLeft = (bar.width - sheetWidth) / 2;
  const maxHeight = maxSheetHeight(bar.height);
  const vars = {
    "--gl-w": `${sheetWidth}px`,
    "--gl-x": `${sheetLeft}px`,
    "--gl-top": `${spotlight.marginTop}px`,
    "--gl-rw": `${bar.width}px`,
    "--gl-rh": `${bar.height}px`,
    "--gl-max": `${maxHeight - GLASS_METRICS.frame}px`,
    "--gl-radius": `${spotlight.radius}px`,
    "--gl-a": alpha,
    "--gl-fade": `${BACKDROP_FADE_MS}ms`,
    "--gl-grow": `${HEIGHT_MS}ms`,
  } as CSSProperties;

  // ----- height -----
  const contentRef = useRef<HTMLDivElement | null>(null);
  const viewKey = `${view.kind}:${"card" in view && view.card ? `${view.card.queryId}:${view.card.phase}:${view.card.items.length}:${view.card.choices.length}` : ""}`;
  const measured = useMeasuredHeight(contentRef, viewKey);
  const target = Math.min(maxHeight, measured === null ? estimateSheetHeight(view, maxHeight) : measured + GLASS_METRICS.frame);
  const [applied, setApplied] = useState(target);
  const [armed, setArmed] = useState(false);
  const reported = useRef<number | null>(null);
  const region = useRef(onRegion);
  region.current = onRegion;

  const report = (height: number) => {
    if (reported.current === height) return Promise.resolve();
    reported.current = height;
    try {
      return Promise.resolve(region.current(height)).catch(() => undefined);
    } catch {
      return Promise.resolve();
    }
  };

  // The first measurement is the starting size, not a growth; transitions start after it.
  useEffect(() => {
    const id = window.requestAnimationFrame(() => setArmed(true));
    return () => window.cancelAnimationFrame(id);
  }, []);

  useEffect(() => {
    const plan = heightPlan(applied, target);
    if (plan === "none") return;
    if (plan === "region-then-sheet") {
      let live = true;
      void report(target).then(() => live && setApplied(target));
      return () => {
        live = false;
      };
    }
    setApplied(target);
    return;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [target]);

  // The window follows a shrink once the sheet has finished it. (The size at the start, and again at
  // every show, is reported by the effect below.) Also when a growth was cancelled before the window
  // answered: the region was already told the taller size, the sheet never grew into it, and without
  // this it would stay taller than the sheet (a dead transparent area). Not while a growth is pending
  // (`target` differs): that one is on its way.
  useEffect(() => {
    if (reported.current === null || reported.current === applied || target !== applied) return;
    const wait = reducedMotion || !armed ? 0 : HEIGHT_MS + 40;
    const id = window.setTimeout(() => void report(applied), wait);
    return () => window.clearTimeout(id);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [applied, target, reducedMotion, armed]);

  // A window shown again starts from what the backend reset it to (the field alone): say where we are.
  useEffect(() => {
    if (hidden) return;
    reported.current = null;
    void report(applied);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [hidden]);

  // A fresh show plays the entrance (a scale-in; opacity only under reduced motion).
  const [entering, setEntering] = useState(true);
  useEffect(() => {
    if (hidden) return;
    setEntering(true);
    const id = window.setTimeout(() => setEntering(false), 260);
    return () => window.clearTimeout(id);
  }, [hidden]);

  const flagsClass = glassClasses({ theme, plain, reducedMotion, opaque });
  const processing = view.kind === "processing";
  const line = useFunnyLine(seed, processing && !hidden);

  // What a screen reader hears when the sheet changes (the playful line is decoration).
  const announcement = (() => {
    switch (view.kind) {
      case "processing":
        return ss("processing", locale);
      case "answer":
        return [glassHeadline(view.card), glassSummary(view.card)].filter(Boolean).join(". ");
      case "choices":
        return glassHeadline(view.card);
      case "error":
        return view.card ? [glassHeadline(view.card), glassSummary(view.card)].filter(Boolean).join(". ") || ss("failed", locale) : ss("failed", locale);
      default:
        return "";
    }
  })();

  const hint: "keys" | "enter" | null = (() => {
    switch (view.kind) {
      case "ready":
        return "keys";
      case "typing":
      case "answer":
        return "enter";
      case "processing":
        return null;
      case "choices":
        return hasText ? "enter" : null;
      case "error":
        return hasText ? "enter" : "keys";
    }
  })();

  const body = <div className="gl-body" key={viewKey}>{renderBody()}</div>;

  function renderBody() {
    switch (view.kind) {
      case "processing":
        return (
          <>
            <div className="gl-sep" />
            <div className="gl-status" aria-hidden="true">
              <Ring />
              <span data-funny-status="">{line}</span>
            </div>
          </>
        );
      case "answer":
        return <AnswerBody {...props} card={view.card} />;
      case "choices":
        return <ChoicesBody {...props} card={view.card} />;
      case "error":
        return <ErrorBody card={view.card} locale={locale} />;
      default:
        return null;
    }
  }

  return (
    <div className={flagsClass} style={vars} dir={rtl ? "rtl" : "ltr"} data-view={view.kind} data-entering={entering ? "on" : "off"} data-armed={armed ? "on" : "off"}>
      <div
        className="gl-sheet"
        style={{ height: applied }}
        onMouseDown={(e) => {
          // Rows and buttons keep working (click still fires); the field keeps the focus.
          if ((e.target as HTMLElement).tagName !== "INPUT") {
            e.preventDefault();
            props.onBackground();
          }
        }}
      >
        {image && (
          <div className="gl-bg" data-ready={imageReady ? "on" : "off"} aria-hidden="true">
            <img
              src={image}
              alt=""
              draggable={false}
              onLoad={() => setLoadedId(backdrop?.id ?? null)}
              onError={() => dlog("warn", "search", "glass backdrop could not be decoded")}
            />
          </div>
        )}
        <div className="gl-tint" aria-hidden="true" />
        <div className="gl-content" ref={contentRef}>
          <div className="gl-field">
            <span className="gl-ico" data-lit={processing ? "on" : "off"} aria-hidden="true">
              <Spark size={26} />
            </span>
            {input}
            {hint === "keys" && (
              <span className="gl-hint" dir="ltr" title={ss("spotlightKeys", locale)} data-kind="keys">
                Alt + `
              </span>
            )}
            {hint === "enter" && (
              <span className="gl-hint" dir="ltr" data-kind="enter">
                Enter <EnterIcon />
              </span>
            )}
            {processing && <span className="gl-shimmer" aria-hidden="true" />}
          </div>
          {body}
        </div>
        <div className="gl-edge" aria-hidden="true" />
      </div>
      <span className="gl-sr" role="status" aria-live="polite">
        {announcement}
      </span>
    </div>
  );
}

// =============================================================================
// Bodies
// =============================================================================

type BodyProps = GlassSearchProps & { card: AssistantCard };

function Headline({ text }: { text: string }) {
  return (
    <div className="gl-ans">
      <Spark size={18} />
      <h3 dir={textDirection(text)}>
        <BidiText text={text} />
      </h3>
    </div>
  );
}

function AnswerBody({ card, selected, onSelect, onOpenItem, onOpenCenter, onExtend, locale, now }: BodyProps) {
  const rows = answerRows(card);
  const summary = glassSummary(card);
  const all = showsAllResults(card);
  const clock = now ?? (() => new Date());
  const current = clock();
  return (
    <>
      <div className="gl-sep" />
      <Headline text={glassHeadline(card)} />
      {summary && (
        <p className="gl-sub" dir={textDirection(summary)}>
          <BidiText text={summary} />
        </p>
      )}
      {rows.length > 0 && (
        <div className="gl-rows" role="listbox" id={GLASS_LIST_ID} aria-label={ss("results", locale)}>
          {rows.map((item, i) => {
            const accent = rowAccent(item);
            const meta = rowMeta(item);
            const time = rowTime(item, current);
            const sel = i === selected;
            const cls = "gl-row";
            const style = accent ? ({ "--c": accent } as CSSProperties) : undefined;
            const inner = (
              <>
                <span className="gl-tile" style={style} data-neutral={accent ? undefined : ""}>
                  <KindIcon kind={item.kind} />
                </span>
                <span className="gl-main">
                  <span className="gl-t" style={{ fontWeight: item.unread ? 600 : undefined }}>
                    <bdi dir={textDirection(item.title)}>
                      <BidiText text={item.title} />
                    </bdi>
                  </span>
                  {meta && (
                    <span className="gl-s">
                      <bdi dir={textDirection(meta)}>
                        <BidiText text={meta} />
                      </bdi>
                    </span>
                  )}
                </span>
                {time && (
                  <span className="gl-time" dir="ltr">
                    {time}
                  </span>
                )}
              </>
            );
            if (!item.openable) {
              return (
                <div key={item.id} className={cls} role="option" id={glassOptionId(i)} aria-selected={false} aria-disabled="true" data-openable="off">
                  {inner}
                </div>
              );
            }
            return (
              <button
                key={item.id}
                type="button"
                tabIndex={-1}
                className={cls}
                role="option"
                id={glassOptionId(i)}
                aria-selected={sel}
                data-sel={sel ? "" : undefined}
                data-openable="on"
                aria-label={item.kind === "action" ? [item.title, card.title].filter(Boolean).join(". ") : ss("openItem", locale, { title: item.title })}
                onMouseMove={(e) => pointerMoved(e) && onSelect(i)}
                onClick={() => onOpenItem(card, item)}
              >
                {inner}
              </button>
            );
          })}
        </div>
      )}
      {(all || card.canExtend) && (
        <div className="gl-foot">
          <span className="gl-foot-actions">
            {all && (
              <button type="button" className="gl-go" onClick={() => onOpenCenter(card)}>
                {ss("showAll", locale)}
                <Chevron />
              </button>
            )}
            {card.canExtend && (
              <button type="button" className="gl-go" onClick={() => onExtend(card)}>
                {ss("extend", locale)}
              </button>
            )}
          </span>
          <span className="gl-esc" dir="ltr" title={ss("escHint", locale)}>
            Esc
          </span>
        </div>
      )}
    </>
  );
}

function ChoicesBody({ card, selected, pendingChoice: pending, remember, onToggleRemember, onSelect, onChoose, locale }: BodyProps) {
  const choices = orderedChoices(card.choices);
  return (
    <>
      <div className="gl-sep" />
      <Headline text={glassHeadline(card)} />
      <div className="gl-choices" role="listbox" id={GLASS_LIST_ID} aria-label={ss("choices", locale)}>
        {choices.map((choice, i) => {
          const sel = i === selected;
          return (
            <button
              key={choice.id}
              type="button"
              tabIndex={-1}
              className="gl-choice"
              role="option"
              id={glassOptionId(i)}
              aria-selected={sel}
              aria-label={choice.label}
              title={choice.label}
              data-sel={sel ? "" : undefined}
              data-preferred={choice.preferred ? "" : undefined}
              disabled={pending !== null}
              onMouseMove={(e) => pointerMoved(e) && onSelect(i)}
              onClick={() => {
                // "Remember" is about mailboxes; a plain option is a one-off answer.
                void onChoose(card, choice, remember && choice.kind !== "option");
              }}
            >
              <bdi dir={textDirection(choice.label)}>
                <BidiText text={choiceText(choice)} />
              </bdi>
            </button>
          );
        })}
        {showsRemember(card) && (
          <button type="button" className="gl-remember" role="checkbox" aria-checked={remember} disabled={pending !== null} onClick={onToggleRemember}>
            <span className="gl-check" data-on={remember ? "on" : "off"} aria-hidden="true" />
            {ss("remember", locale)}
          </button>
        )}
      </div>
    </>
  );
}

function ErrorBody({ card, locale }: { card: AssistantCard | null; locale: Locale }) {
  const headline = card ? glassHeadline(card) || ss("failed", locale) : ss("failed", locale);
  const summary = card ? glassSummary(card) : "";
  return (
    <>
      <div className="gl-sep" />
      <div className="gl-ans" data-error="">
        <Spark size={18} />
        <h3 dir={textDirection(headline)}>
          <BidiText text={headline} />
        </h3>
      </div>
      {summary && (
        <p className="gl-sub" dir={textDirection(summary)}>
          <BidiText text={summary} />
        </p>
      )}
      {card?.errorCode && (
        <div className="gl-code">
          <bdi dir="ltr">{card.errorCode}</bdi>
        </div>
      )}
      <div className="gl-pad" />
    </>
  );
}
