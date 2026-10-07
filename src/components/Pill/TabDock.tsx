import { useRef, type KeyboardEvent } from "react";
import { motion, useTransform, type MotionValue } from "motion/react";
import { dock } from "../../design/tokens";
import { t } from "../../lib/i18n";
import { TAB_LABEL_TRANSITION } from "./animations";
import { DOCK_DIRECTION, TABS, type TabId } from "./tabs";

interface TabDockProps {
  active: TabId;
  /**
   * Position of the selection capsule, in tabs (0 = first): a spring that starts towards the
   * chosen tab on the frame after the click and stops at it without swinging past.
   */
  indicator: MotionValue<number>;
  onSelect: (id: TabId) => void;
}

/**
 * Bottom tab strip: 56 high, radius 18 (a surface), as wide as the panel's content (the frame
 * insets it). Each tab is the dock's full height and its whole slot is the click target. The
 * selection is one shape inset 4 from the dock with radius 14 (concentric), which slides between
 * the slots. The strip runs left to right in every language (DOCK_DIRECTION): the first tab is
 * the leftmost, even in Hebrew.
 * Keyboard per the WAI-ARIA tabs pattern, handled here on the tablist and only while a tab has focus
 * (so arrows never reach for a text box or a control inside the panel): roving tabindex, ArrowRight /
 * ArrowLeft move to the next / previous tab with wrap, Home / End to the first / last, and the tab
 * that receives focus is selected at once (automatic activation: the panels are cheap). The dock is
 * always left to right, so DOM order is visual order is screen-reader order, and ArrowRight is "next".
 */
export function TabDock({ active, indicator, onSelect }: TabDockProps) {
  const dir = DOCK_DIRECTION;
  const tabRefs = useRef<Array<HTMLButtonElement | null>>([]);
  const onTablistKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    if (e.altKey || e.ctrlKey || e.metaKey) return;
    // Only when focus is on a tab itself (a key bubbling up from anything else is not ours).
    const from = tabRefs.current.findIndex((el) => el === e.target);
    if (from < 0) return;
    const last = TABS.length - 1;
    const to = e.key === "ArrowRight" ? (from === last ? 0 : from + 1) : e.key === "ArrowLeft" ? (from === 0 ? last : from - 1) : e.key === "Home" ? 0 : e.key === "End" ? last : -1;
    if (to < 0) return;
    e.preventDefault();
    onSelect(TABS[to].id);
    tabRefs.current[to]?.focus();
  };
  // One capsule, one identity: the same element slides between the tabs (a transform: no
  // layout, no measuring); there is never a second highlight to cross-fade with. Each slot is
  // exactly 1/TABS.length of the strip, so a percentage of its own width is a slot. It starts at
  // the left edge and moves +x towards the later tabs.
  const x = useTransform(indicator, (position) => `${position * 100}%`);
  return (
    <div dir={dir} className="ci-surface relative flex items-stretch rounded-surface h-dock flex-shrink-0" role="tablist" aria-label={t("island.tabs")} onKeyDown={onTablistKeyDown}>
      <motion.div
        data-tab-indicator=""
        aria-hidden="true"
        className="absolute inset-y-0 start-0 pointer-events-none"
        style={{ width: `${100 / TABS.length}%`, padding: dock.selectionInset, x }}
      >
        <div className="h-full w-full rounded-selection bg-selection" />
      </motion.div>
      {TABS.map((tab, index) => {
        const selected = tab.id === active;
        return (
          <button
            key={tab.id}
            id={`tab-${tab.id}`}
            ref={(el) => {
              tabRefs.current[index] = el;
            }}
            role="tab"
            type="button"
            aria-selected={selected}
            aria-controls={`panel-${tab.id}`}
            tabIndex={selected ? 0 : -1}
            // The focus ring is drawn on the inner shape (same inset and radius as the selection).
            className={`group relative flex-1 min-w-0 h-dock flex items-stretch outline-none ${selected ? "text-fg" : "text-fg-tertiary hover:text-fg-secondary"}`}
            style={{ transition: TAB_LABEL_TRANSITION, padding: dock.selectionInset }}
            onClick={() => onSelect(tab.id)}
            onKeyDown={(e) => {
              if (e.key === "Enter" || e.key === " ") {
                e.preventDefault();
                onSelect(tab.id);
              }
            }}
          >
            <span
              className="flex-1 min-w-0 flex flex-col items-center justify-center rounded-selection group-focus-visible:[outline:2px_solid_var(--ci-focus)]"
              style={{ gap: dock.labelGap }}
            >
              {/* One glyph size for every tab (tokens.dock.iconSize); the stroke thickens a touch when selected. */}
              <tab.Icon size={dock.iconSize} strokeWidth={selected ? 2.3 : 2} />
              <span className="text-micro max-w-full truncate">{t(tab.labelKey)}</span>
            </span>
          </button>
        );
      })}
    </div>
  );
}
