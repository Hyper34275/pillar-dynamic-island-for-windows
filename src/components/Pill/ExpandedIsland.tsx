import { motion } from "motion/react";
import { type ReactNode } from "react";
import { APP_NAME } from "../../lib/appInfo";
import { t } from "../../lib/i18n";
import type { NotificationStatus } from "../../lib/ipc";
import { DoNotDisturbButton } from "./HeaderAction";
import { PANEL_TITLE_CLASS, PanelFrame, panelBodyClass } from "./PanelFrame";
import { useSpringValue } from "./useIslandMotion";
import { AboutTab } from "./panels/AboutTab";
import { CalendarTab } from "./panels/CalendarTab";
import { NotesTab } from "./panels/NotesTab";
import { NotificationsTab } from "./panels/NotificationsTab";
import { SettingsTab } from "./panels/SettingsTab";
import { TabBoundary } from "./TabBoundary";
import { TabDock } from "./TabDock";
import { tabCapsuleSpring, tabContentSprings, useTabLayers, useTabSteps, type TabLayer } from "./tabLayers";
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
    case "notes":
      return <NotesTab />;
    case "about":
      return <AboutTab />;
    case "settings":
      return <SettingsTab notificationStatus={props.notificationStatus} onRequestNotificationAccess={props.onRequestNotificationAccess} />;
  }
}

/**
 * A tab's title. The outgoing and incoming titles share one grid cell while the hand-off runs
 * (tabLayers.ts: at most two, and one of them owns the screen at a time).
 */
function TabTitle({ layer, children }: { layer: TabLayer; children: ReactNode }) {
  return (
    <motion.h2 className={PANEL_TITLE_CLASS} data-tab-title={layer.tab} aria-hidden={layer.leaving ? true : undefined} style={{ gridArea: "1 / 1", opacity: layer.opacity }}>
      {children}
    </motion.h2>
  );
}

/** A tab's header action; it hands over with the title in the same row. The outgoing one can't be clicked. */
function TabAction({ layer, children }: { layer: TabLayer; children: ReactNode }) {
  return (
    <motion.div data-tab-action={layer.tab} aria-hidden={layer.leaving ? true : undefined} style={{ gridArea: "1 / 1", opacity: layer.opacity, pointerEvents: layer.leaving ? "none" : undefined }}>
      {children}
    </motion.div>
  );
}

/**
 * One tab's content. The outgoing and incoming panels are stacked in the same box while they
 * hand over, so the area is never empty and never jumps; only the incoming one is the
 * tabpanel, and the outgoing one can't be clicked.
 */
function TabPanel({ layer, children }: { layer: TabLayer; children: ReactNode }) {
  const { tab, leaving } = layer;
  return (
    <motion.div
      data-panel={tab}
      id={leaving ? undefined : `panel-${tab}`}
      role={leaving ? undefined : "tabpanel"}
      aria-labelledby={leaving ? undefined : `tab-${tab}`}
      aria-hidden={leaving ? true : undefined}
      className={`absolute inset-0 ${panelBodyClass(tab)}`}
      style={{ opacity: layer.opacity, x: layer.offset, pointerEvents: leaving ? "none" : undefined }}
    >
      <TabBoundary tab={tab}>{children}</TabBoundary>
    </motion.div>
  );
}

export function ExpandedIsland({ activeTab, reducedMotion, notificationStatus, onRequestNotificationAccess, onSelectTab }: ExpandedIslandProps) {
  const config = TABS.find((tab) => tab.id === activeTab) ?? TABS[0];
  const activeIndex = TABS.indexOf(config);

  // A tab change is one event with two springs started in the same frame: the capsule (one
  // shared highlight, which moves from the old tab to the new one and is the immediate
  // acknowledgement) and the content's progress, which hands the screen from the outgoing to the
  // incoming title and panel (tabLayers.ts: never more than two pages mounted, the latest target
  // wins). The capsule is the faster of the two, so the new content is never ahead of the
  // selection, and the hand-off takes as long whether the capsule travels one tab or four. Both
  // retarget from where they are, so rapid clicks bend the motion instead of queueing it.
  // Reduced motion keeps the same rules with a tighter spring and no sideways shift.
  const capsule = useSpringValue(activeIndex, tabCapsuleSpring, { noOvershoot: true });
  const steps = useTabSteps(activeTab, activeIndex);
  const progress = useSpringValue(steps.target, reducedMotion ? tabContentSprings.reduced : tabContentSprings.normal);
  // The content shifts towards where its tab is: the dock runs left to right, so later tabs are further right.
  const layers = useTabLayers(activeTab, progress, steps, reducedMotion);

  // The parts travel with the island's edges as it opens and closes (IslandPart, in PanelFrame):
  // the header with the top-start corner, the body in the middle, the dock with the bottom edge.
  return (
    <PanelFrame
      parts
      reducedMotion={reducedMotion}
      role="region"
      aria-label={t("island.expandedLabel", { app: APP_NAME })}
      onClick={(e) => e.stopPropagation()}
      onPointerDown={(e) => e.stopPropagation()}
      title={layers.map((layer) => (
        <TabTitle key={layer.id} layer={layer}>
          {t((TABS.find((tab) => tab.id === layer.tab) ?? TABS[0]).labelKey)}
        </TabTitle>
      ))}
      action={
        // The tab's own action hands over with the title; the Do not disturb bell stays put at the
        // trailing corner on every tab.
        <div className="flex items-center gap-3">
          <div className="grid justify-items-end">
            {layers.map((layer) => {
              const HeaderAction = TABS.find((tab) => tab.id === layer.tab)?.HeaderAction;
              return (
                HeaderAction && (
                  <TabAction key={layer.id} layer={layer}>
                    <HeaderAction />
                  </TabAction>
                )
              );
            })}
          </div>
          <DoNotDisturbButton />
        </div>
      }
      dock={<TabDock active={activeTab} indicator={capsule} onSelect={onSelectTab} />}
    >
      {layers.map((layer) => (
        <TabPanel key={layer.id} layer={layer}>
          {renderPanel(layer.tab, { notificationStatus, onRequestNotificationAccess })}
        </TabPanel>
      ))}
    </PanelFrame>
  );
}
