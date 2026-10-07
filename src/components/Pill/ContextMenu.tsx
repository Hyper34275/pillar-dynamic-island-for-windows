import { motion } from "motion/react";
import { color, radius } from "../../design/tokens";
import { uiDirection } from "../../design/direction";
import { t } from "../../lib/i18n";

export interface ContextMenuItem {
  label: string;
  run: () => void;
}

interface ContextMenuProps {
  x: number;
  y: number;
  items: ContextMenuItem[];
  onClose: () => void;
}

/** Menu padding; rows are concentric with the menu: item radius = menu radius − padding. */
const MENU_PAD = 4;
const MENU_RADIUS = radius.selection;
const ITEM_RADIUS = MENU_RADIUS - MENU_PAD;

export function ContextMenu({ x, y, items, onClose }: ContextMenuProps) {
  return (
    <div
      className="fixed inset-0 z-[120]"
      // Don't let menu clicks bubble to the island's onClick, which would expand() right
      // after a "Collapse" item ran.
      onClick={(e) => {
        e.stopPropagation();
        onClose();
      }}
      onContextMenu={(e) => {
        e.preventDefault();
        onClose();
      }}
    >
      <motion.div
        dir={uiDirection()}
        role="menu"
        aria-label={t("island.tabs")}
        // Menu floor (42 × the 4-pt unit): wide enough for its longest Hebrew item at the label role.
        className="absolute min-w-[168px]"
        // left/top are the physical pointer coordinates, not layout: the one place physical is right.
        style={{
          left: x,
          top: y,
          padding: MENU_PAD,
          borderRadius: MENU_RADIUS,
          background: color.island,
          boxShadow: `0 0 0 1px ${color.islandEdge}, 0 12px 32px rgba(0,0,0,0.5)`,
          originX: 0,
          originY: 0,
        }}
        initial={{ opacity: 0, scale: 0.96 }}
        animate={{ opacity: 1, scale: 1 }}
        transition={{ duration: 0.12, ease: "easeOut" }}
      >
        {items.map((item) => (
          <button
            key={item.label}
            type="button"
            role="menuitem"
            className="bidi w-full text-start text-label text-fg px-3 h-9 hover:bg-surface-hover active:bg-surface-pressed transition-colors"
            style={{ borderRadius: ITEM_RADIUS }}
            onClick={() => {
              item.run();
              onClose();
            }}
          >
            {item.label}
          </button>
        ))}
      </motion.div>
    </div>
  );
}
