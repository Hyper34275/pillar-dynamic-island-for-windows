import { t } from "../../lib/i18n";
import { TABS, type TabId } from "./tabs";

interface TabDockProps {
  active: TabId;
  onSelect: (id: TabId) => void;
}

/** Bottom tab strip. Roving tabindex per the WAI-ARIA tabs pattern; arrows/Home/End live in PillShell. */
export function TabDock({ active, onSelect }: TabDockProps) {
  return (
    <div
      dir="ltr"
      className="relative flex items-stretch mt-2 rounded-[22px] bg-white/[0.06] flex-shrink-0 overflow-hidden"
      role="tablist"
      aria-label={t("island.tabs")}
    >
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
            className={`relative flex-1 h-[50px] m-1 rounded-[18px] flex flex-col items-center justify-center gap-[3px] transition-colors ${
              selected ? "bg-white/[0.12] text-white" : "text-white/40 hover:text-white/75"
            }`}
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
