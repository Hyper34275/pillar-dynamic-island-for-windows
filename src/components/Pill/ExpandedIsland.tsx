import { AnimatePresence, motion } from "motion/react";
import { useMemo, useState, type ReactNode } from "react";
import { APP_NAME } from "../../lib/appInfo";
import { t } from "../../lib/i18n";
import type { NotificationStatus } from "../../lib/ipc";
import { layerFade, TAB_SHIFT_PX, tabSprings } from "./animations";
import { TransitionContext, useDrivenTransition, useTransitionLayer } from "./drivenTransition";
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

/** A tab's title. The outgoing and incoming titles share one grid cell while they cross-fade. */
function TabTitle({ children }: { children: ReactNode }) {
  const { opacity, isPresent } = useTransitionLayer({ fade: layerFade.tab });
  return (
    <motion.h2 className={PANEL_TITLE_CLASS} aria-hidden={isPresent ? undefined : true} style={{ gridArea: "1 / 1", opacity }}>
      {children}
    </motion.h2>
  );
}

/** A tab's header action; it cross-fades with the title in the same row (a tab without one has an empty cell). */
function TabAction({ children }: { children: ReactNode }) {
  const { opacity, isPresent } = useTransitionLayer({ fade: layerFade.tab });
  return (
    <motion.div aria-hidden={isPresent ? undefined : true} style={{ gridArea: "1 / 1", opacity, pointerEvents: isPresent ? undefined : "none" }}>
      {children}
    </motion.div>
  );
}

/**
 * One tab's content. The outgoing and incoming panels are stacked in the same box while they
 * cross-fade, so the area is never empty and never jumps; only the incoming one is the
 * tabpanel, and the outgoing one can't be clicked.
 */
function TabPanel({ tab, slide, children }: { tab: TabId; slide: boolean; children: ReactNode }) {
  const { opacity, offset, isPresent } = useTransitionLayer({ fade: layerFade.tab, offset: slide ? TAB_SHIFT_PX : 0 });
  return (
    <motion.div
      data-panel={tab}
      id={isPresent ? `panel-${tab}` : undefined}
      role={isPresent ? "tabpanel" : undefined}
      aria-labelledby={isPresent ? `tab-${tab}` : undefined}
      aria-hidden={isPresent ? undefined : true}
      className={`absolute inset-0 ${panelBodyClass(tab)}`}
      style={{ opacity, x: offset, pointerEvents: isPresent ? undefined : "none" }}
    >
      <TabBoundary tab={tab}>{children}</TabBoundary>
    </motion.div>
  );
}

/**
 * Where the tab content's progress spring is heading: one step further every time the tab
 * changes, and which way the change went (the content shifts towards where its tab is). The
 * progress only ever counts forwards: it is the cross-fade's clock, not a physical object, so a
 * change caught mid-way never has to undo momentum first (that held a half-faded pair of
 * panels on screen when clicks changed direction); what is on screen continues from where it
 * is regardless. The capsule, which is a physical object, keeps its own momentum.
 */
function useTabSteps(activeTab: TabId, activeIndex: number): { target: number; direction: -1 | 1 } {
  const [steps, setSteps] = useState({ tab: activeTab, index: activeIndex, target: 0, direction: 1 as -1 | 1 });
  if (steps.tab === activeTab) return steps;
  const next = { tab: activeTab, index: activeIndex, target: steps.target + 1, direction: (activeIndex < steps.index ? -1 : 1) as -1 | 1 };
  setSteps(next);
  return next;
}

export function ExpandedIsland({ activeTab, reducedMotion, notificationStatus, onRequestNotificationAccess, onSelectTab }: ExpandedIslandProps) {
  const config = TABS.find((tab) => tab.id === activeTab) ?? TABS[0];
  const activeIndex = TABS.indexOf(config);

  // A tab change is one event with two springs started in the same frame: the capsule (one
  // shared highlight, which moves from the old tab to the new one and is the immediate
  // acknowledgement) and the content's progress, which the outgoing and incoming title and
  // panel fade by. The capsule is the faster of the two, so the new content is never ahead of
  // the selection, and the cross-fade takes as long whether the capsule travels one tab or four.
  // Both retarget from where they are, so rapid clicks bend the motion instead of queueing it.
  const capsule = useSpringValue(activeIndex, tabSprings.capsule, { noOvershoot: true });
  const steps = useTabSteps(activeTab, activeIndex);
  const progress = useSpringValue(steps.target, tabSprings.content);
  const drivers = useMemo(() => [progress], [progress]);
  // The content shifts towards where its tab is: the dock runs left to right, so later tabs are further right.
  const shiftDirection = steps.direction;
  const tabTransition = useDrivenTransition(drivers, [steps.target], activeTab, shiftDirection);
  const inTab = (node: ReactNode) => <TransitionContext.Provider value={tabTransition}>{node}</TransitionContext.Provider>;

  const HeaderAction = config.HeaderAction;

  // The parts travel with the island's edges as it opens and closes (IslandPart, in PanelFrame):
  // the header with the top-start corner, the body in the middle, the dock with the bottom edge.
  return (
    <PanelFrame
      parts
      role="region"
      aria-label={t("island.expandedLabel", { app: APP_NAME })}
      onClick={(e) => e.stopPropagation()}
      onPointerDown={(e) => e.stopPropagation()}
      title={inTab(
        <AnimatePresence>
          <TabTitle key={config.id}>{t(config.labelKey)}</TabTitle>
        </AnimatePresence>
      )}
      action={
        // The tab's own action cross-fades with the title; the Do not disturb bell stays put at the
        // trailing corner on every tab.
        <div className="flex items-center gap-3">
          <div className="grid justify-items-end">
            {inTab(
              <AnimatePresence>
                {HeaderAction && (
                  <TabAction key={config.id}>
                    <HeaderAction />
                  </TabAction>
                )}
              </AnimatePresence>
            )}
          </div>
          <DoNotDisturbButton />
        </div>
      }
      dock={<TabDock active={activeTab} indicator={capsule} onSelect={onSelectTab} />}
    >
      {inTab(
        <AnimatePresence>
          <TabPanel key={activeTab} tab={activeTab} slide={!reducedMotion}>
            {renderPanel(activeTab, { notificationStatus, onRequestNotificationAccess })}
          </TabPanel>
        </AnimatePresence>
      )}
    </PanelFrame>
  );
}
