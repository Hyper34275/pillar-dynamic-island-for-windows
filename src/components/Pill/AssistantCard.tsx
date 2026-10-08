import { animate, motion, useMotionValue, useTransform } from "motion/react";
import { Component, useEffect, useId, useMemo, useRef, useState, type CSSProperties, type ReactNode } from "react";
import { textDirection, uiDirection } from "../../design/direction";
import { alert as alertTokens, color, radius, smallExpanded } from "../../design/tokens";
import { timeParts } from "../../lib/dateFormat";
import { ERROR_CODES } from "../../lib/appInfo";
import { FUNNY_STATUS_MS, funnyStatusOrder } from "../../lib/assistant/funnyStatus";
import type { AssistantCard as Card, AssistantItem, Choice } from "../../lib/assistant/types";
import { dlog } from "../../lib/debugLog";
import { describeError } from "../../lib/errors";
import { t } from "../../lib/i18n";
import { ipc } from "../../lib/ipc";
import { layerFade } from "./animations";
import {
  assistantLayout,
  BLOCK_GAP,
  cardHeadline,
  cardSummary,
  choiceLabel,
  ITEM_GAP,
  ITEM_PAD_Y,
  ITEM_TIME_WIDTH,
  itemMeta,
  PROCESSING_PAD,
  PROCESSING_SPARKLE,
  REMEMBER_HEIGHT,
  SPARKLE_GAP,
  SPARKLE_SIZE,
  TEXT_GAP,
  type AssistantLayout,
} from "./assistantLayout";
import { IslandLayer } from "./IslandLayer";
import { BidiText } from "./ui/BidiText";
import { ActionButton, ActionRow, RoundButton } from "./ui/controls";
import { CheckIcon, XIcon } from "./ui/icons";

// =============================================================================
// The island's smart-search card (docs/AI_SEARCH.md 4.5): a temporary island view that shows the
// phases of one question: "working", an answer with a few items, a question with buttons, an error.
// It never takes the keyboard or the focus: everything on it is clicked. Its size is decided by
// assistantLayout.ts, which counts exactly what is rendered here. The card carries mail subjects
// and names: it is rendered, never logged or stored.
// =============================================================================

/** The Apple-Intelligence-like sparkle colours (cyan, violet, soft pink) of the search bar's glow. */
const SPARKLE_STOPS = ["#40C8E0", "#BF5AF2", "#FFA3C7"] as const;
const SPARKLE_PATH = "M12 2c.6 5.5 4.5 9.4 10 10-5.5.6-9.4 4.5-10 10-.6-5.5-4.5-9.4-10-10 5.5-.6 9.4-4.5 10-10Z";

/**
 * The identity sparkle. `animated`: a slow turn and swell driven by a motion value (not a CSS
 * animation: the global reduced-motion rule freezes those), so it also runs where the machine
 * reports reduced motion, only gentler there (no turn).
 */
function Sparkle({ size, animated = false, reducedMotion = false }: { size: number; animated?: boolean; reducedMotion?: boolean }) {
  const id = useId();
  const progress = useMotionValue(0);
  useEffect(() => {
    if (!animated) return;
    const controls = animate(progress, 1, { duration: reducedMotion ? 2.4 : 3.2, ease: "linear", repeat: Infinity });
    return () => controls.stop();
  }, [animated, reducedMotion, progress]);
  const rotate = useTransform(progress, [0, 1], [0, reducedMotion ? 0 : 360]);
  const scale = useTransform(progress, [0, 0.5, 1], [1, 1.16, 1]);
  const opacity = useTransform(progress, [0, 0.5, 1], reducedMotion ? [0.7, 1, 0.7] : [1, 1, 1]);
  return (
    <motion.svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      aria-hidden="true"
      focusable="false"
      className="flex-shrink-0"
      data-sparkle={animated ? "moving" : "still"}
      style={animated ? { rotate, scale, opacity } : undefined}
    >
      <defs>
        <linearGradient id={id} x1="2" y1="2" x2="22" y2="22" gradientUnits="userSpaceOnUse">
          {SPARKLE_STOPS.map((stop, i) => (
            <stop key={stop} offset={i / (SPARKLE_STOPS.length - 1)} stopColor={stop} />
          ))}
        </linearGradient>
      </defs>
      <path d={SPARKLE_PATH} fill={`url(#${id})`} />
    </motion.svg>
  );
}

/** What a screen reader says when the card appears or changes (the shell's live region). */
export function assistantAnnouncement(card: Card): string {
  switch (card.phase) {
    case "processing":
      return t("ai.processing");
    case "choices":
      return cardHeadline(card);
    case "error":
      return [cardHeadline(card), cardSummary(card)].filter(Boolean).join(". ");
    case "answer": {
      const items = card.items.slice(0, 3).map((item) => [item.time !== null ? timeParts(new Date(item.time)).digits : "", item.title].filter(Boolean).join(" "));
      return [cardHeadline(card), cardSummary(card), ...items].filter(Boolean).join(". ");
    }
  }
}

/** The island's accessible name while the card shows. */
export const assistantLabel = (): string => t("ai.label");

/** A render error in the card stays in the card: the island (and CrashBoundary) never see it. Modelled on TabBoundary. */
class AssistantBoundary extends Component<{ resetKey: string; fallback: ReactNode; children: ReactNode }, { failed: boolean }> {
  state = { failed: false };

  static getDerivedStateFromError() {
    return { failed: true };
  }

  componentDidCatch(error: Error) {
    // The error text only: nothing of the card (subjects, names) is ever logged.
    dlog("error", "react", `assistant card render failed [${ERROR_CODES.uiRender}] ${describeError(error)}`);
  }

  componentDidUpdate(prev: { resetKey: string }) {
    // The next state of the card gets a fresh try.
    if (this.state.failed && prev.resetKey !== this.props.resetKey) this.setState({ failed: false });
  }

  render() {
    return this.state.failed ? this.props.fallback : this.props.children;
  }
}

function CloseButton({ onClose }: { onClose?: () => void }) {
  return (
    <RoundButton fill="none" ariaLabel={t("ai.close")} className="pointer-events-auto" onPress={() => onClose?.()}>
      <XIcon size={14} />
    </RoundButton>
  );
}

/** The headline row: sparkle, headline (two lines at most), close. */
function Headline({ text, onClose }: { text: string; onClose?: () => void }) {
  return (
    <div className="flex items-center" style={{ gap: SPARKLE_GAP }}>
      <Sparkle size={SPARKLE_SIZE} />
      <h3 dir={textDirection(text)} className="bidi min-w-0 flex-1 line-clamp-2 text-title" style={{ overflowWrap: "anywhere" }}>
        <BidiText text={text} />
      </h3>
      <CloseButton onClose={onClose} />
    </div>
  );
}

function ItemRow({ card, item, showTime }: { card: Card; item: AssistantItem; showTime: boolean }) {
  const meta = itemMeta(item);
  const accent = item.accent ?? (item.kind === "event" || item.kind === "action" ? color.accent : color.fgQuaternary);
  const body = (
    <>
      <span aria-hidden="true" className="flex-none" style={{ width: 3, alignSelf: "stretch", borderRadius: 2, background: accent }} />
      {showTime && (
        <span dir="ltr" className="flex-none text-meta tabular-nums whitespace-nowrap" style={{ width: ITEM_TIME_WIDTH, color: color.fgSecondary, textAlign: "start" }}>
          {item.time !== null ? timeParts(new Date(item.time)).digits : ""}
        </span>
      )}
      <span className="min-w-0 flex-1 flex flex-col text-start">
        {/* Every row starts at the layer's own edge, whatever language its text is in (a list that wobbles reads badly): the line takes the layer's direction, the text inside shapes itself. */}
        <span className="bidi truncate text-body text-start" style={{ color: color.fg, fontWeight: item.unread ? 600 : undefined }}>
          <bdi dir={textDirection(item.title)}>
            <BidiText text={item.title} />
          </bdi>
        </span>
        {meta && (
          <span className="bidi truncate text-meta text-start" style={{ color: color.fgTertiary }}>
            <bdi dir={textDirection(meta)}>
              <BidiText text={meta} />
            </bdi>
          </span>
        )}
      </span>
    </>
  );
  const rowStyle = { gap: SPARKLE_GAP, paddingBlock: ITEM_PAD_Y, paddingInline: 8, borderRadius: radius.control };
  if (!item.openable) {
    return (
      <li className="flex items-center" style={{ ...rowStyle, background: color.surface }}>
        {body}
      </li>
    );
  }
  return (
    <li>
      <button
        type="button"
        className="ci-action hit-area w-full flex items-center pointer-events-auto"
        style={{ ...rowStyle, "--btn-bg": color.surface, "--btn-bg-hover": color.surfaceHover, "--btn-bg-pressed": color.surfacePressed } as CSSProperties}
        // A command item ("חפש בגוגל") is named together with what it will do (the card's headline).
        aria-label={item.kind === "action" ? [item.title, card.title].filter(Boolean).join(". ") : t("ai.openItem", { title: item.title })}
        onPointerDown={(e) => e.stopPropagation()}
        onClick={(e) => {
          e.stopPropagation();
          void ipc.assistantOpenItem(card.queryId, item.id);
        }}
      >
        {body}
      </button>
    </li>
  );
}

interface CardCallbacks {
  /** The person closed the card (X, Escape): the shell also tells the backend. */
  onClose?: () => void;
  /** The card did its job (the results opened in the Center): it goes, the backend keeps the results. */
  onHide?: () => void;
  /** A card that came back from choose / extend (it also arrives as an event; the shell drops duplicates). */
  onCard?: (card: Card) => void;
}

function AnswerBody({ card, layout, onClose, onHide, onCard }: { card: Card; layout: AssistantLayout } & CardCallbacks) {
  const [pending, setPending] = useState(false);
  const summary = cardSummary(card);
  return (
    <>
      <Headline text={cardHeadline(card)} onClose={onClose} />
      {layout.summaryLines > 0 && (
        <p dir={textDirection(summary)} className="bidi line-clamp-2 text-body" style={{ marginTop: TEXT_GAP, color: color.fgSecondary, overflowWrap: "anywhere" }}>
          <BidiText text={summary} />
        </p>
      )}
      {layout.items.length > 0 && (
        <ul className="flex flex-col" style={{ marginTop: BLOCK_GAP, gap: ITEM_GAP }}>
          {layout.items.map((item) => (
            <ItemRow key={item.id} card={card} item={item} showTime={layout.showTime} />
          ))}
        </ul>
      )}
      {(layout.showAll || layout.showExtend) && (
        <ActionRow className="pointer-events-auto" style={{ marginTop: alertTokens.actionsGap }}>
          {layout.showAll && (
            <ActionButton
              variant="neutral"
              onPress={() => {
                void ipc.assistantOpenCenter(card.queryId);
                onHide?.();
              }}
            >
              {t("ai.showAll")}
            </ActionButton>
          )}
          {layout.showExtend && (
            <ActionButton
              variant="neutral"
              disabled={pending}
              onPress={() => {
                setPending(true);
                void ipc.assistantExtend(card.queryId).then((next) => {
                  if (next) onCard?.(next);
                  else setPending(false);
                });
              }}
            >
              {t("ai.extend")}
            </ActionButton>
          )}
        </ActionRow>
      )}
    </>
  );
}

function ChoicesBody({ card, layout, onClose, onCard }: { card: Card; layout: AssistantLayout } & CardCallbacks) {
  const [pending, setPending] = useState<string | null>(null);
  const [remember, setRemember] = useState(false);
  const choose = (choice: Choice) => {
    setPending(choice.id);
    // "Remember" is about mailboxes; a plain option is a one-off answer.
    void ipc.assistantChoose(card.queryId, choice.id, remember && choice.kind !== "option").then((next) => {
      if (next) onCard?.(next);
      else setPending(null);
    });
  };
  return (
    <>
      <Headline text={cardHeadline(card)} onClose={onClose} />
      <div className="flex flex-col pointer-events-auto" style={{ marginTop: BLOCK_GAP, gap: 8 }}>
        {layout.choices.map((choice) => (
          <ActionButton key={choice.id} variant={choice.preferred ? "primary" : "neutral"} ariaLabel={choice.label} title={choice.label} disabled={pending !== null} pressed={pending === choice.id ? true : undefined} onPress={() => choose(choice)}>
            <span dir={textDirection(choice.label)} className="bidi">
              <BidiText text={choiceLabel(choice)} />
            </span>
          </ActionButton>
        ))}
      </div>
      {layout.showRemember && (
        <div className="flex pointer-events-auto" style={{ marginTop: 8, height: REMEMBER_HEIGHT }}>
          <RoundButton
            grow
            fill={remember ? "tint" : "none"}
            tint={remember ? color.accentOnSoft : color.fgSecondary}
            pressed={remember}
            ariaLabel={t("ai.remember")}
            disabled={pending !== null}
            onPress={() => setRemember((on) => !on)}
          >
            <span className="flex items-center" style={{ gap: 6 }}>
              {remember ? <CheckIcon size={14} /> : <span aria-hidden="true" style={{ width: 14, height: 14, borderRadius: 7, boxShadow: "inset 0 0 0 1.5px currentColor" }} />}
              {t("ai.remember")}
            </span>
          </RoundButton>
        </div>
      )}
    </>
  );
}

function ErrorBody({ card, layout, onClose }: { card: Card; layout: AssistantLayout } & CardCallbacks) {
  const summary = cardSummary(card);
  return (
    <>
      <Headline text={cardHeadline(card)} onClose={onClose} />
      {layout.summaryLines > 0 && (
        <p dir={textDirection(summary)} className="bidi line-clamp-2 text-body" style={{ marginTop: TEXT_GAP, color: color.fgSecondary, overflowWrap: "anywhere" }}>
          <BidiText text={summary} />
        </p>
      )}
      {card.errorCode && (
        <span dir="ltr" className="text-meta tabular-nums text-start" style={{ marginTop: TEXT_GAP, color: color.fgTertiary }}>
          {card.errorCode}
        </span>
      )}
    </>
  );
}

/** The rotating playful line of a working search (src/lib/assistant/funnyStatus.ts), one per FUNNY_STATUS_MS. */
function useFunnyStatus(seed: string): string {
  const lines = useMemo(() => funnyStatusOrder(seed), [seed]);
  const [index, setIndex] = useState(0);
  useEffect(() => {
    setIndex(0);
    const timer = window.setInterval(() => setIndex((i) => (i + 1) % lines.length), FUNNY_STATUS_MS);
    return () => window.clearInterval(timer);
  }, [lines]);
  return lines[index] ?? t("ai.processing");
}

function ProcessingBody({ seed, reducedMotion, onClose }: { seed: string; reducedMotion: boolean } & CardCallbacks) {
  const line = useFunnyStatus(seed);
  return (
    <div className="flex items-center h-full" style={{ gap: SPARKLE_GAP }}>
      <Sparkle size={PROCESSING_SPARKLE} animated reducedMotion={reducedMotion} />
      {/* a screen reader hears "ai.processing" through the shell's live region; the joke is decoration */}
      <span className="bidi min-w-0 flex-1 truncate text-headline" style={{ color: color.fg }} aria-hidden="true" data-funny-status="">
        <motion.span
          key={line}
          className="inline-block"
          initial={reducedMotion ? false : { opacity: 0, y: 4 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.22, ease: "easeOut" }}
        >
          {line}
        </motion.span>
      </span>
      <CloseButton onClose={onClose} />
    </div>
  );
}

interface AssistantCardProps extends CardCallbacks {
  card: Card;
  reducedMotion?: boolean;
}

/**
 * Content of the island while a smart-search card shows. A phase change of the same query swaps the
 * content in place (a short fade; none under reduced motion) while the island morphs to the new
 * size; the layer itself stays mounted.
 */
export function AssistantCard({ card, reducedMotion = false, onClose, onHide, onCard }: AssistantCardProps) {
  const layout = useMemo(() => assistantLayout(card), [card]);
  const processing = card.phase === "processing";
  const firstRender = useRef(true);
  useEffect(() => {
    firstRender.current = false;
  }, []);
  // A new state of the card remounts its body (pending flags, the remember switch start over).
  const stateKey = `${card.queryId}:${card.phase}:${card.partial ? "p" : "f"}`;
  const fallback = (
    <div className="flex items-center" style={{ gap: SPARKLE_GAP }}>
      <span className="min-w-0 flex-1 truncate text-body" style={{ color: color.fgSecondary }}>
        {t("ai.unavailable")}
      </span>
      <CloseButton onClose={onClose} />
    </div>
  );
  const callbacks = { onClose, onHide, onCard };
  return (
    <IslandLayer
      name="assistant"
      fade={layerFade.temporary}
      size={layout.size}
      dir={uiDirection()}
      className="select-none pointer-events-none"
      style={{ padding: processing ? `0 ${PROCESSING_PAD}px` : smallExpanded.padding, color: color.fg }}
    >
      <AssistantBoundary resetKey={stateKey} fallback={fallback}>
        <motion.div
          key={stateKey}
          className="flex flex-col h-full"
          initial={reducedMotion || firstRender.current ? false : { opacity: 0 }}
          animate={{ opacity: 1 }}
          transition={{ duration: 0.16, ease: "easeOut" }}
        >
          {card.phase === "processing" && <ProcessingBody seed={card.queryId} reducedMotion={reducedMotion} {...callbacks} />}
          {card.phase === "answer" && <AnswerBody card={card} layout={layout} {...callbacks} />}
          {card.phase === "choices" && <ChoicesBody card={card} layout={layout} {...callbacks} />}
          {card.phase === "error" && <ErrorBody card={card} layout={layout} {...callbacks} />}
        </motion.div>
      </AssistantBoundary>
    </IslandLayer>
  );
}
