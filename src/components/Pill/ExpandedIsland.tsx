import { motion } from "motion/react";
import type { Ref } from "react";
import { APP_NAME } from "../../lib/appInfo";
import { t } from "../../lib/i18n";
import type { NotificationStatus } from "../../lib/ipc";
import { PILL_DURATION_FAST } from "./animations";
import { AboutTab } from "./panels/AboutTab";
import { CalendarTab } from "./panels/CalendarTab";
import { DatetimeTab } from "./panels/DatetimeTab";
import { TabBoundary } from "./TabBoundary";
import { TabDock } from "./TabDock";
import { TABS, type TabId } from "./tabs";

interface ExpandedIslandProps {
  containerRef: Ref<HTMLDivElement>;
  activeTab: TabId;
  /** +1 when the last tab change moved right, -1 when left; panels slide the same way. */
  direction: 1 | -1;
  reducedMotion: boolean;
  notificationStatus: NotificationStatus | null;
  onRequestNotificationAccess: () => void;
  onSelectTab: (id: TabId) => void;
}

function renderPanel(tab: TabId, props: Pick<ExpandedIslandProps, "notificationStatus" | "onRequestNotificationAccess">) {
  switch (tab) {
    case "datetime":
      return <DatetimeTab />;
    case "calendar":
      return <CalendarTab />;
    case "about":
      return <AboutTab notificationStatus={props.notificationStatus} onRequestNotificationAccess={props.onRequestNotificationAccess} />;
  }
}

export function ExpandedIsland({
  containerRef,
  activeTab,
  direction,
  reducedMotion,
  notificationStatus,
  onRequestNotificationAccess,
  onSelectTab,
}: ExpandedIslandProps) {
  const config = TABS.find((tab) => tab.id === activeTab) ?? TABS[0];
  // Datetime centres its own content; the other tabs scroll inside the panel.
  const scrolls = activeTab !== "datetime";

  return (
    // No AnimatePresence on purpose: an exit animation that never settled once left this
    // layer mounted (invisible) over the collapsed pill and swallowed every click.
    <motion.div
      ref={containerRef}
      dir="ltr"
      className="island-expanded absolute inset-0 flex flex-col pt-4 pb-2 px-4 cursor-default text-white"
      style={{ borderRadius: "inherit" }}
      role="region"
      aria-label={t("island.expandedLabel", { app: APP_NAME })}
      onClick={(e) => e.stopPropagation()}
      onPointerDown={(e) => e.stopPropagation()}
      initial={reducedMotion ? { opacity: 0 } : { opacity: 0, scale: 0.96 }}
      animate={{ opacity: 1, scale: 1 }}
      transition={reducedMotion ? { duration: 0.08 } : { duration: 0.24, delay: 0.05, ease: [0.2, 0.8, 0.2, 1] }}
    >
      <div className="flex items-end justify-between mb-3 px-1 flex-shrink-0">
        <motion.h2
          key={config.id}
          className="text-white text-[20px] font-bold tracking-tight leading-none"
          dir="auto"
          initial={{ opacity: 0, y: 6 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ type: "spring", stiffness: 500, damping: 36 }}
        >
          {t(config.labelKey)}
        </motion.h2>
      </div>

      <div className="flex-1 flex flex-col min-h-0 overflow-hidden w-full relative">
        {/* Enter-only transition: gating the new panel on the old one's exit once left tab clicks doing nothing. */}
        <motion.div
          key={activeTab}
          id={`panel-${activeTab}`}
          role="tabpanel"
          aria-labelledby={`tab-${activeTab}`}
          className={`flex-1 min-h-0 w-full flex flex-col px-1 ${scrolls ? "overflow-y-auto island-scroll pb-3" : "overflow-hidden"}`}
          initial={reducedMotion ? { opacity: 0 } : { opacity: 0, x: direction * 18 }}
          animate={{ opacity: 1, x: 0 }}
          transition={reducedMotion ? { duration: 0.08 } : { x: { type: "spring", stiffness: 520, damping: 40 }, opacity: { duration: PILL_DURATION_FAST } }}
        >
          <TabBoundary tab={activeTab}>{renderPanel(activeTab, { notificationStatus, onRequestNotificationAccess })}</TabBoundary>
        </motion.div>
      </div>

      <TabDock active={activeTab} onSelect={onSelectTab} />
    </motion.div>
  );
}
