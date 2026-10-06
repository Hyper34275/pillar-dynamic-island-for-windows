import { motion } from "motion/react";
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
        dir="ltr"
        role="menu"
        aria-label={t("island.tabs")}
        className="absolute min-w-[168px] rounded-[14px] bg-black p-1"
        style={{ left: x, top: y, boxShadow: "0 0 0 0.5px rgba(255,255,255,0.12), 0 12px 32px rgba(0,0,0,0.5)", originX: 0, originY: 0 }}
        initial={{ opacity: 0, scale: 0.92 }}
        animate={{ opacity: 1, scale: 1 }}
        transition={{ type: "spring", stiffness: 600, damping: 34 }}
      >
        {items.map((item) => (
          <button
            key={item.label}
            type="button"
            role="menuitem"
            className="w-full text-left text-[12px] font-medium text-white/85 px-2.5 h-8 rounded-[10px] hover:bg-white/[0.1] transition-colors"
            dir="auto"
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
