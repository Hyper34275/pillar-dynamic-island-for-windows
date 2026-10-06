import { AnimatePresence, motion, useSpring } from "motion/react";
import { useEffect, useMemo, type ReactNode } from "react";
import { APP_NAME } from "../../lib/appInfo";
import { t } from "../../lib/i18n";
import type { NotificationStatus } from "../../lib/ipc";
import { expandedSize, layerFade, springConfig, TAB_SHIFT_PX } from "./animations";
import { TransitionContext, useDrivenTransition, useTransitionLayer } from "./drivenTransition";
import { IslandLayer } from "./IslandLayer";
import { AboutTab } from "./panels/AboutTab";
import { CalendarTab } from "./panels/CalendarTab";
import { NotificationsTab } from "./panels/NotificationsTab";
import { SettingsTab } from "./panels/SettingsTab";
import { TabBoundary } from "./TabBoundary";
import { TabDock } from "./TabDock";
import { TABS, type TabId } from "./tabs";

interface ExpandedIslandProps {
  activeTab: TabId;
  reducedMotion: boolean;
  notificationStatus: NotificationStatus | null;
  onRequestNotificationAccess: () => void;
  onSelectTab: (id: TabId) => void;
}

function renderPanel(tab: TabId, props: Pick<ExpandedIslandProps, "notificationStatus" | "onRequestNotificationAccess">) {
  switch (tab) {
    case "calendar":
      return <CalendarTab />;
    case "notifications":
      return <NotificationsTab />;
    case "about":
      return <AboutTab />;
    case "settings":
      return <SettingsTab notificationStatus={props.notificationStatus} onRequestNotificationAccess={props.onRequestNotificationAccess} />;
  }
}

/** A tab's title. The outgoing and incoming titles share one grid cell while they cross-fade. */
function TabTitle({ children }: { children: ReactNode }) {
  const { opacity, isPresent } = useTransitionLayer({ fade: layerFade.tab });
  return (
    <motion.h2
      className="text-white text-[20px] font-bold tracking-tight leading-none"
      dir="auto"
      aria-hidden={isPresent ? undefined : true}
      style={{ gridArea: "1 / 1", opacity }}
    >
      {children}
    </motion.h2>
  );
}

/**
 * One tab's content. The outgoing and incoming panels are stacked in the same box while they
 * cross-fade, so the area is never empty and never jumps; only the incoming one is the
 * tabpanel, and the outgoing one can't be clicked.
 */
function TabPanel({ tab, slide, children }: { tab: TabId; slide: boolean; children: ReactNode }) {
  const { opacity, offset, isPresent } = useTransitionLayer({ fade: layerFade.tab, offset: slide ? TAB_SHIFT_PX : 0 });
  // About centres its own content; the other tabs scroll inside the panel.
  const scrolls = tab !== "about";
  return (
    <motion.div
      data-panel={tab}
      id={isPresent ? `panel-${tab}` : undefined}
      role={isPresent ? "tabpanel" : undefined}
      aria-labelledby={isPresent ? `tab-${tab}` : undefined}
      aria-hidden={isPresent ? undefined : true}
      className={`absolute inset-0 flex flex-col px-1 ${scrolls ? "overflow-y-auto island-scroll pb-3" : "overflow-hidden"}`}
      style={{ opacity, x: offset, pointerEvents: isPresent ? undefined : "none" }}
    >
      <TabBoundary tab={tab}>{children}</TabBoundary>
    </motion.div>
  );
}

export function ExpandedIsland({ activeTab, reducedMotion, notificationStatus, onRequestNotificationAccess, onSelectTab }: ExpandedIslandProps) {
  const config = TABS.find((tab) => tab.id === activeTab) ?? TABS[0];
  const activeIndex = TABS.indexOf(config);

  // A tab change is one transition driven by the dock indicator: it starts moving in the frame
  // the tab is chosen, and the outgoing/incoming title and panel fade by how far it has
  // travelled. Indicator and content can't disagree about which tab is shown.
  const indicator = useSpring(activeIndex, springConfig.tab);
  useEffect(() => {
    indicator.set(activeIndex);
  }, [activeIndex, indicator]);
  const drivers = useMemo(() => [indicator], [indicator]);
  const tabTransition = useDrivenTransition(drivers, [activeIndex], activeTab);

  return (
    <IslandLayer
      name="expanded"
      fade={layerFade.expanded}
      size={expandedSize()}
      dir="ltr"
      className="island-expanded flex flex-col pt-4 pb-2 px-4 cursor-default text-white"
      role="region"
      aria-label={t("island.expandedLabel", { app: APP_NAME })}
      onClick={(e) => e.stopPropagation()}
      onPointerDown={(e) => e.stopPropagation()}
    >
      <TransitionContext.Provider value={tabTransition}>
        <div className="flex items-end justify-between mb-3 px-1 flex-shrink-0">
          <div className="grid justify-items-start">
            <AnimatePresence>
              <TabTitle key={config.id}>{t(config.labelKey)}</TabTitle>
            </AnimatePresence>
          </div>
        </div>

        <div className="flex-1 min-h-0 overflow-hidden w-full relative">
          <AnimatePresence>
            <TabPanel key={activeTab} tab={activeTab} slide={!reducedMotion}>
              {renderPanel(activeTab, { notificationStatus, onRequestNotificationAccess })}
            </TabPanel>
          </AnimatePresence>
        </div>

        <TabDock active={activeTab} indicator={indicator} onSelect={onSelectTab} />
      </TransitionContext.Provider>
    </IslandLayer>
  );
}
