import { motion, useReducedMotion } from "motion/react";
import type { CSSProperties } from "react";
import { layerFade, pillDimensions, springConfig } from "./animations";
import { colorOf } from "./ui/eventColor";
import { IslandLayer } from "./IslandLayer";
import { BellSlashIcon } from "./ui/icons";
import { SYSTEM_COLORS, UNSEEN_COLORS } from "./ui/primitives";
import {
  CLOCK_FONT_SIZE,
  COMPACT_FONT_SIZE,
  PERIOD_FONT_SIZE,
  PERIOD_GAP,
  STATUS_FONT_SIZE,
  type CompactContent,
} from "./useCompactLayout";

// Direction is forced physically: date on the left, weekday on the right (the clock between
// them), whatever the UI language. Each label is its own isolated LTR box; the text inside
// picks its own direction (dir="auto") so Hebrew/Arabic weekdays still shape correctly.
const LABEL_STYLE: CSSProperties = {
  direction: "ltr",
  unicodeBidi: "isolate",
  fontSize: COMPACT_FONT_SIZE,
  letterSpacing: "-0.005em",
  fontVariantNumeric: "tabular-nums",
};

const b = pillDimensions.badge;

/**
 * The unseen notifications, inside the island at its end: a calm dot for one, a tinted count
 * capsule from two (9+ past nine). Apple's island has no counter, and a red number would read as
 * a badge look-alike, so this stays quiet. (Hanging it over the island's corner got it clipped by
 * the island and the native window, which are exactly the island's shape.) `gap` is the space
 * before it; the status layout already has its own flex gap and passes 0. It pops in once on a
 * spring that settles (opacity only under reduced motion); nothing loops. The element is keyed by
 * its shape, so the dot turning into a capsule replays the entrance and a count changing
 * from 2 to 3 swaps the digit with no animation.
 */
function UnseenIndicator({ unseen, gap = b.gap }: { unseen: number; gap?: number }) {
  const reducedMotion = useReducedMotion() ?? false;
  const isDot = unseen === 1;
  const shape: CSSProperties = isDot
    ? { width: b.dot, height: b.dot, background: UNSEEN_COLORS.dot }
    : {
        width: unseen > 9 ? b.countWide : b.count,
        height: b.height,
        background: UNSEEN_COLORS.fill,
        color: UNSEEN_COLORS.text,
        fontSize: 11,
        lineHeight: `${b.height}px`,
      };
  return (
    <motion.span
      key={isDot ? "dot" : "count"}
      className="flex-shrink-0 rounded-full flex items-center justify-center font-semibold tabular-nums"
      style={{ ...shape, marginInlineStart: gap }}
      initial={reducedMotion ? { opacity: 0 } : { scale: 0.6, opacity: 0 }}
      animate={reducedMotion ? { opacity: 1 } : { scale: 1, opacity: 1 }}
      transition={reducedMotion ? { duration: 0.12, ease: "easeOut" } : springConfig.island}
      aria-hidden="true"
    >
      {isDot ? null : unseen > 9 ? "9+" : unseen}
    </motion.span>
  );
}

/**
 * The time in its fixed slot (the island never resizes with the minute): the digits, then in a
 * 12-hour region the smaller period. It reads as secondary next to the date in "full" and is
 * the one label in "clock". Changes every minute without animation; the island stays calm.
 */
function TimeLabel({ time, display, width }: { time: CompactContent["time"]; display: "full" | "clock"; width: number }) {
  const solo = display === "clock";
  return (
    <span
      className="flex-shrink-0 whitespace-nowrap font-semibold text-center"
      style={{ ...LABEL_STYLE, fontSize: CLOCK_FONT_SIZE[display], letterSpacing: solo ? "-0.005em" : 0, minWidth: width }}
      aria-hidden="true"
    >
      <span style={{ opacity: solo ? 1 : 0.72 }}>{time.digits}</span>
      {time.period && (
        <span
          dir="auto"
          style={{ fontSize: PERIOD_FONT_SIZE, letterSpacing: 0, marginLeft: PERIOD_GAP, opacity: solo ? 0.6 : 0.5, unicodeBidi: "isolate" }}
        >
          {time.period}
        </span>
      )}
    </span>
  );
}

interface CompactIslandProps {
  content: CompactContent;
}

export function CompactIsland({ content }: CompactIslandProps) {
  const { labels, status, statusText, unseen, silent, size, time } = content;
  if (status && statusText) {
    const color = colorOf(status.event);
    return (
      <IslandLayer
        name="compact"
        fade={layerFade.compact}
        size={size}
        dir="ltr"
        className="flex items-center select-none pointer-events-none overflow-hidden"
        style={{ paddingInline: pillDimensions.compact.paddingX, gap: 8, color: "#f5f5f7" }}
      >
        {status.kind === "now" && (
          <span className="absolute left-[18px] right-[18px] bottom-[3px] h-[2px] rounded-full bg-white/[0.12] overflow-hidden" aria-hidden="true">
            <span className="block h-full rounded-full" style={{ width: `${status.progress * 100}%`, background: color }} />
          </span>
        )}
        <span className="w-2 h-2 rounded-full flex-shrink-0" style={{ background: color }} aria-hidden="true" />
        <span
          className="min-w-0 flex-1 overflow-hidden text-ellipsis whitespace-nowrap font-semibold"
          style={{ fontSize: STATUS_FONT_SIZE, unicodeBidi: "plaintext", fontVariantNumeric: "tabular-nums" }}
          dir="auto"
        >
          {statusText}
        </span>
        {silent && (
          <span className="flex-shrink-0" style={{ color: SYSTEM_COLORS.red }} aria-hidden="true">
            <BellSlashIcon size={14} />
          </span>
        )}
        {unseen > 0 && <UnseenIndicator unseen={unseen} gap={0} />}
      </IslandLayer>
    );
  }

  const style: CSSProperties = { paddingInline: pillDimensions.compact.paddingX, gap: pillDimensions.compact.gap, color: "#f5f5f7" };

  if (labels.display === "clock") {
    // The time and the indicator are direct flex children: the indicator's own margin is the whole
    // gap (what badgeWidth reserves), so the layer adds none.
    return (
      <IslandLayer
        name="compact"
        fade={layerFade.compact}
        size={size}
        dir="ltr"
        className="flex items-center justify-center select-none pointer-events-none"
        style={{ ...style, gap: 0 }}
      >
        <TimeLabel time={time} display="clock" width={labels.timeWidth} />
        {unseen > 0 && <UnseenIndicator unseen={unseen} />}
      </IslandLayer>
    );
  }

  const weekday = (
    <span className="min-w-0 flex items-center">
      <span className="min-w-0 overflow-hidden text-ellipsis whitespace-nowrap font-semibold opacity-80" style={LABEL_STYLE}>
        <span dir="auto">{labels.weekday}</span>
      </span>
      {unseen > 0 && <UnseenIndicator unseen={unseen} />}
    </span>
  );
  const date = (
    <span className="flex-shrink-0 whitespace-nowrap font-semibold" style={LABEL_STYLE}>
      <span dir="auto">{labels.date}</span>
    </span>
  );

  if (labels.display === "full") {
    // Date, clock, weekday on one baseline (the clock is smaller), the row centred in the pill.
    return (
      <IslandLayer name="compact" fade={layerFade.compact} size={size} dir="ltr" className="flex items-center select-none pointer-events-none" style={style}>
        <span className="flex w-full items-baseline justify-between" style={{ gap: pillDimensions.compact.gapFull }}>
          {date}
          <TimeLabel time={time} display="full" width={labels.timeWidth} />
          {weekday}
        </span>
      </IslandLayer>
    );
  }

  return (
    <IslandLayer name="compact" fade={layerFade.compact} size={size} dir="ltr" className="flex items-center justify-between select-none pointer-events-none" style={style}>
      {date}
      {weekday}
    </IslandLayer>
  );
}
