import { motion } from "motion/react";
import type { CSSProperties } from "react";
import { pillDimensions } from "./animations";
import { COMPACT_FONT_SIZE, type CompactLabels } from "./useCompactLayout";

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

interface CompactIslandProps {
  labels: CompactLabels;
  /** Notifications received since the island was last opened. */
  unseen: number;
  reducedMotion: boolean;
}

export function CompactIsland({ labels, unseen, reducedMotion }: CompactIslandProps) {
  return (
    <motion.div
      dir="ltr"
      className="absolute inset-0 flex items-center justify-between select-none pointer-events-none"
      style={{
        paddingInline: pillDimensions.compact.paddingX,
        gap: pillDimensions.compact.gap,
        color: "#f5f5f7",
      }}
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      exit={{ opacity: 0, transition: { duration: 0.06 } }}
      transition={{ duration: reducedMotion ? 0.05 : 0.2, delay: reducedMotion ? 0 : 0.05 }}
    >
      <span className="flex-shrink-0 whitespace-nowrap font-semibold" style={LABEL_STYLE}>
        <span dir="auto">{labels.date}</span>
      </span>
      <span className="min-w-0 overflow-hidden text-ellipsis whitespace-nowrap font-semibold opacity-80" style={LABEL_STYLE}>
        <span dir="auto">{labels.weekday}</span>
      </span>
      {unseen > 0 && (
        <span
          className="absolute top-0 right-[-4px] min-w-[15px] h-[15px] px-1 rounded-full flex items-center justify-center text-[9.5px] font-bold text-white tabular-nums"
          style={{ background: "#FF453A", boxShadow: "0 0 0 2px #000" }}
          aria-hidden="true"
        >
          {unseen > 9 ? "9+" : unseen}
        </span>
      )}
    </motion.div>
  );
}
