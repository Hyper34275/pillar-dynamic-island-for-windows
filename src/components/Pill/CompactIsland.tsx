import type { CSSProperties } from "react";
import { layerFade, pillDimensions } from "./animations";
import { colorOf } from "./ui/eventColor";
import { IslandLayer } from "./IslandLayer";
import { BellSlashIcon } from "./ui/icons";
import { SYSTEM_COLORS } from "./ui/primitives";
import { COMPACT_FONT_SIZE, STATUS_FONT_SIZE, type CompactContent } from "./useCompactLayout";

// Direction is forced physically: date on the left, weekday on the right, whatever the
// UI language. Each label is its own isolated LTR box; the text inside picks its own
// direction (dir="auto") so Hebrew/Arabic weekdays still shape correctly.
const LABEL_STYLE: CSSProperties = {
  direction: "ltr",
  unicodeBidi: "isolate",
  fontSize: COMPACT_FONT_SIZE,
  letterSpacing: "-0.005em",
  fontVariantNumeric: "tabular-nums",
};

const b = pillDimensions.badge;

/**
 * The unseen count, inside the island at its end. (Hanging it over the island's corner got it
 * clipped by the island and the native window, which are exactly the island's shape.)
 */
function UnseenBadge({ unseen }: { unseen: number }) {
  return (
    <span
      className="flex-shrink-0 rounded-full flex items-center justify-center text-[10px] font-bold text-white tabular-nums"
      style={{ background: SYSTEM_COLORS.red, height: b.size, minWidth: unseen > 9 ? b.wide : b.size, marginInlineStart: b.gap - 4 }}
      aria-hidden="true"
    >
      {unseen > 9 ? "9+" : unseen}
    </span>
  );
}

interface CompactIslandProps {
  content: CompactContent;
}

export function CompactIsland({ content }: CompactIslandProps) {
  const { labels, status, statusText, unseen, silent, size } = content;
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
        {unseen > 0 && <UnseenBadge unseen={unseen} />}
        {status.kind === "now" && (
          <span className="absolute left-[18px] right-[18px] bottom-[3px] h-[2px] rounded-full bg-white/[0.12] overflow-hidden" aria-hidden="true">
            <span className="block h-full rounded-full" style={{ width: `${status.progress * 100}%`, background: color }} />
          </span>
        )}
      </IslandLayer>
    );
  }
  return (
    <IslandLayer
      name="compact"
      fade={layerFade.compact}
      size={size}
      dir="ltr"
      className="flex items-center justify-between select-none pointer-events-none"
      style={{
        paddingInline: pillDimensions.compact.paddingX,
        gap: pillDimensions.compact.gap,
        color: "#f5f5f7",
      }}
    >
      <span className="flex-shrink-0 whitespace-nowrap font-semibold" style={LABEL_STYLE}>
        <span dir="auto">{labels.date}</span>
      </span>
      <span className="min-w-0 flex items-center">
        <span className="min-w-0 overflow-hidden text-ellipsis whitespace-nowrap font-semibold opacity-80" style={LABEL_STYLE}>
          <span dir="auto">{labels.weekday}</span>
        </span>
        {unseen > 0 && <UnseenBadge unseen={unseen} />}
      </span>
    </IslandLayer>
  );
}
