import { motion, useTransform, type MotionValue } from "motion/react";
import { t } from "../../lib/i18n";
import { TAB_LABEL_TRANSITION } from "./animations";
import { TABS, type TabId } from "./tabs";

interface TabDockProps {
  active: TabId;
  /**
   * Position of the selection capsule, in tabs (0 = first): a spring that starts towards the
   * chosen tab on the frame after the click and stops at it without swinging past.
   */
  indicator: MotionValue<number>;
  onSelect: (id: TabId) => void;
}

/** Bottom tab strip. Roving tabindex per the WAI-ARIA tabs pattern; arrows/Home/End live in PillShell. */
export function TabDock({ active, indicator, onSelect }: TabDockProps) {
  // One capsule, one identity: the same element slides between the tabs (a transform: no
  // layout, no measuring); there is never a second highlight to cross-fade with. Each slot is
  // exactly 1/TABS.length of the strip, so a percentage of its own width is a slot.
  const x = useTransform(indicator, (position) => `${position * 100}%`);
  return (
    <div
      dir="ltr"
      className="relative flex items-stretch mt-2 rounded-[22px] bg-white/[0.06] flex-shrink-0 overflow-hidden"
      role="tablist"
      aria-label={t("island.tabs")}
    >
      <motion.div
        data-tab-indicator=""
        aria-hidden="true"
        className="absolute inset-y-0 left-0 p-1 pointer-events-none"
        style={{ width: `${100 / TABS.length}%`, x }}
      >
        <div className="h-full w-full rounded-[18px] bg-white/[0.12]" />
      </motion.div>
      {TABS.map((tab) => {
        const selected = tab.id === active;
        return (
          <button
            key={tab.id}
            id={`tab-${tab.id}`}
            role="tab"
            type="button"
            aria-selected={selected}
            aria-controls={`panel-${tab.id}`}
            tabIndex={selected ? 0 : -1}
            className={`relative flex-1 h-[50px] m-1 rounded-[18px] flex flex-col items-center justify-center gap-[3px] ${
              selected ? "text-white" : "text-white/40 hover:text-white/75"
            }`}
            style={{ transition: TAB_LABEL_TRANSITION }}
            onClick={() => onSelect(tab.id)}
            onKeyDown={(e) => {
              if (e.key === "Enter" || e.key === " ") {
                e.preventDefault();
                onSelect(tab.id);
              }
            }}
          >
            <tab.Icon size={18} strokeWidth={selected ? 2.3 : 2} />
            <span className="text-[9.5px] font-semibold leading-none tracking-tight uppercase" dir="auto">
              {t(tab.labelKey)}
            </span>
          </button>
        );
      })}
    </div>
  );
}
