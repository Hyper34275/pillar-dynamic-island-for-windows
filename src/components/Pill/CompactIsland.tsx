import type { CSSProperties } from "react";
import { badge, color, compact, icon, progress } from "../../design/tokens";
import { uiDirection } from "../../design/direction";
import { layerFade } from "./animations";
import { colorOf } from "./ui/eventColor";
import { IslandLayer } from "./IslandLayer";
import { CountBadge } from "./ui/identity";
import { BellSlashIcon } from "./ui/icons";
import { PERIOD_GAP, type CompactContent } from "./useCompactLayout";

// =============================================================================
// The compact island: 36 high, radius 18, width follows the content. Layout (docs/DESIGN_SYSTEM.md):
// text 12 from the curved ends, separate elements 8 apart, digits + period 4, the unseen badge in
// the trailing slot 8 from the end (concentric with the cap). The layer's `dir` is the UI
// language's, so "leading" and "trailing" are logical (flex order + padding-inline), never margins.
// =============================================================================

/**
 * A meeting in progress: the status dot becomes a progress ring in the same leading slot, in the
 * meeting's colour (clockwise from 12 o'clock, like a clock). Progress gets a slot of its own
 * (tokens.progress.ring) instead of an extra bar, so the text row stays centred in the 36px pill
 * and the count badge stays concentric with the end cap. Decorative: the status text says it.
 */
function ProgressRing({ value, tint }: { value: number; tint: string }) {
  const { ring: size, ringStroke: stroke } = progress;
  const r = (size - stroke) / 2;
  const c = 2 * Math.PI * r;
  return (
    <svg width={size} height={size} viewBox={`0 0 ${size} ${size}`} aria-hidden="true" className="ci-progress flex-shrink-0" style={{ transform: "rotate(-90deg)" }}>
      <circle cx={size / 2} cy={size / 2} r={r} fill="none" stroke={color.track} strokeWidth={stroke} />
      <circle cx={size / 2} cy={size / 2} r={r} fill="none" stroke={tint} strokeWidth={stroke} strokeLinecap="round" strokeDasharray={`${c * Math.min(1, Math.max(0, value))} ${c}`} />
    </svg>
  );
}

// The date, clock and weekday keep their physical order (date left, clock middle, weekday right)
// whatever the UI language: one isolated LTR box. The text inside each label picks its own direction
// (`bidi`), so a Hebrew or Arabic weekday and the digits still shape correctly.
const ROW_STYLE: CSSProperties = { direction: "ltr", unicodeBidi: "isolate" };

/**
 * The time in its fixed slot (the island never resizes with the minute): the digits (the primary
 * label, headline role), then in a 12-hour region the period, a tight 4 after them in the smaller
 * meta size. Changes every minute without animation; the island stays calm.
 */
function TimeLabel({ time, width }: { time: CompactContent["time"]; width: number }) {
  return (
    <span className="flex-shrink-0 whitespace-nowrap text-center text-headline tabular-nums" style={{ ...ROW_STYLE, minWidth: width, color: color.fg }} aria-hidden="true">
      <span>{time.digits}</span>
      {time.period && (
        // text-meta is 500; the period renders at the label's 600 (the width is measured at 600).
        <span className="bidi text-meta font-semibold" style={{ marginInlineStart: PERIOD_GAP, color: color.fgSecondary, unicodeBidi: "isolate" }}>
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
  const meeting = status !== null && statusText !== null;
  const showsProgress = meeting && status.kind === "now";

  const layerStyle: CSSProperties = {
    gap: compact.gap,
    color: color.fg,
    // Text keeps 12 from the cap; a badge sits 8 from it (concentric), so the end padding shrinks.
    paddingInlineStart: compact.paddingX,
    paddingInlineEnd: unseen > 0 ? badge.endInset : compact.paddingX,
  };
  const layerProps = {
    name: "compact",
    fade: layerFade.compact,
    size,
    dir: uiDirection(),
    className: "flex items-center select-none pointer-events-none overflow-hidden",
    style: layerStyle,
  } as const;
  // The unseen badge: always the last child, so the trailing slot whatever the direction.
  const unseenBadge = unseen > 0 ? <CountBadge count={unseen} /> : null;

  if (meeting) {
    const eventColor = colorOf(status.event);
    return (
      <IslandLayer {...layerProps}>
        <span className="flex flex-1 min-w-0 items-center" style={{ gap: compact.gap }}>
          {showsProgress ? <ProgressRing value={status.progress} tint={eventColor} /> : <span className="ci-mark flex-shrink-0 rounded-full" style={{ width: compact.statusDot, height: compact.statusDot, background: eventColor }} aria-hidden="true" />}
          {/* One line, ellipsis: a long meeting name never widens the island past its maximum. */}
          <span className="bidi min-w-0 overflow-hidden text-ellipsis whitespace-nowrap text-label tabular-nums" style={{ color: color.fg }}>
            {statusText}
          </span>
          {silent && (
            <span className="flex-shrink-0 flex" style={{ color: color.muted }} aria-hidden="true">
              <BellSlashIcon size={icon.small} />
            </span>
          )}
        </span>
        {unseenBadge}
      </IslandLayer>
    );
  }

  if (labels.display === "clock") {
    return (
      <IslandLayer {...layerProps} className={`${layerProps.className} justify-center`}>
        <span className="flex flex-1 min-w-0 items-center justify-center">
          <TimeLabel time={time} width={labels.timeWidth} />
        </span>
        {unseenBadge}
      </IslandLayer>
    );
  }

  // The weekday ellipsizes when even the short form does not fit; the date and clock never shrink.
  const full = labels.display === "full";
  const date = (
    <span className="bidi flex-shrink-0 whitespace-nowrap text-label tabular-nums" style={{ color: full ? color.fgSecondary : color.fg }}>
      {labels.date}
    </span>
  );
  const weekday = (
    <span className="bidi min-w-0 overflow-hidden text-ellipsis whitespace-nowrap text-label tabular-nums" style={{ color: color.fgSecondary }}>
      {labels.weekday}
    </span>
  );
  return (
    <IslandLayer {...layerProps}>
      {/* justify-between: the measuring slack spreads into the gaps instead of piling up at one end. */}
      <span className="flex flex-1 min-w-0 items-baseline justify-between" style={{ ...ROW_STYLE, gap: compact.gap }}>
        {date}
        {full && <TimeLabel time={time} width={labels.timeWidth} />}
        {weekday}
      </span>
      {unseenBadge}
    </IslandLayer>
  );
}
