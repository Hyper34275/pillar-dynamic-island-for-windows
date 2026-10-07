import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { textDirection } from "../../../design/direction";
import { useCalendar } from "../../../hooks/useCalendar";
import { useSettings } from "../../../hooks/useSettings";
import { useSystemInfo } from "../../../hooks/useSystemInfo";
import { APP_VERSION } from "../../../lib/appInfo";
import { formatDateTime } from "../../../lib/dateFormat";
import {
  buildDiagnosticsText,
  formatOs,
  internalErrorOf,
  notificationCodeOf,
  outlookModeOf,
  outlookRunningOf,
  type NotificationDiagnostic,
} from "../../../lib/diagnostics";
import {
  ipc,
  onEvent,
  REMINDER_MINUTE_OPTIONS,
  type Diagnostics,
  type IslandDisplay,
  type MonitorInfo,
  type NotificationStatus,
  type Settings,
  type SettingsPatch,
  type SystemInfo,
} from "../../../lib/ipc";
import { t, type MessageKey } from "../../../lib/i18n";
import type { CalendarSnapshot, CalendarStatus } from "../../../lib/calendar/types";
import { ActionButton, ActionRow } from "../ui/controls";
import { Group, SectionLabel, Segmented, Switch } from "../ui/primitives";

const NONE = "—";
const FLASH_MS = 1800;

const STATUS_LABEL: Record<CalendarStatus, MessageKey> = {
  waiting: "status.waiting",
  connecting: "status.connecting",
  connected: "status.connected",
  newOutlookOnly: "status.newOutlookOnly",
  elevationMismatch: "status.elevationMismatch",
  unresponsive: "status.unresponsive",
  failed: "status.failed",
};

const DELIVERY_LABEL = {
  events: "about.deliveryEvents",
  polling: "about.deliveryPolling",
  none: "about.deliveryNone",
} as const satisfies Record<string, MessageKey>;

/** What the collapsed island shows (physical order: date, clock, weekday). */
const DISPLAY_OPTIONS: ReadonlyArray<{ id: IslandDisplay; labelKey: MessageKey }> = [
  { id: "full", labelKey: "settings.display.full" },
  { id: "clock", labelKey: "settings.display.clock" },
  { id: "date", labelKey: "settings.display.date" },
];

const MODE_LABEL = { classic: "about.modeClassic", new: "about.modeNew", none: "about.modeNone" } as const satisfies Record<string, MessageKey>;

/** Every settings row: at least 44 high (the hit minimum), 12 inline padding (the group's text inset), 12 between its parts. */
const ROW = "flex items-center justify-between gap-3 px-3 min-h-hit";

/** A read-only fact: the label at the leading edge, the value (app / system text, bidi) at the trailing edge. */
function Row({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className={ROW}>
      <span className="text-body text-fg flex-shrink-0">{label}</span>
      <span dir={typeof children === "string" ? textDirection(children) : undefined} className="bidi min-w-0 text-body text-fg-secondary truncate tabular-nums">{children}</span>
    </div>
  );
}

function SwitchRow({ label, checked, onChange }: { label: string; checked: boolean; onChange: () => void }) {
  return (
    <div className={ROW}>
      <span className="text-body text-fg">{label}</span>
      <Switch checked={checked} onChange={onChange} label={label} />
    </div>
  );
}

function monitorLabel(monitor: MonitorInfo, index: number): string {
  return monitor.name || (monitor.isPrimary ? t("settings.monitorPrimary") : t("settings.monitorN", { n: index + 1 }));
}

/** What the diagnostics report for notifications: "off" when the user turned them off, else Windows' own status. */
function notificationDiagnosticOf(settings: Settings, notificationStatus: NotificationStatus | null): NotificationDiagnostic | null {
  return settings.notificationsEnabled ? notificationStatus : "off";
}

export type SettingsFlash = "copied" | "failed" | "saveFailed";

export interface SettingsViewProps {
  settings: Settings;
  monitors: readonly MonitorInfo[];
  info: SystemInfo | null;
  diagnostics: Diagnostics | null;
  snapshot: CalendarSnapshot;
  notificationStatus: NotificationStatus | null;
  /** Transient feedback on the diagnostics button / the settings header; null when there is none. */
  flash: SettingsFlash | null;
  onChange: (patch: SettingsPatch) => void;
  onRequestNotificationAccess: () => void;
  onCopyDiagnostics: () => void;
  onOpenLogs: () => void;
  onOpenCenter: () => void;
  onOpenTour: () => void;
}

/** Pure rendering of the settings and diagnostics (the tour renders it with mock data, no IPC). */
export function SettingsView({
  settings,
  monitors,
  info,
  diagnostics,
  snapshot,
  notificationStatus,
  flash,
  onChange: change,
  onRequestNotificationAccess,
  onCopyDiagnostics,
  onOpenLogs,
  onOpenCenter,
  onOpenTour,
}: SettingsViewProps) {
  const notificationDiagnostic = notificationDiagnosticOf(settings, notificationStatus);

  const mode = outlookModeOf(snapshot, diagnostics);
  const running = outlookRunningOf(snapshot, diagnostics);
  const internalError = internalErrorOf(snapshot);
  const notificationCode = notificationCodeOf(notificationDiagnostic);
  const recentCodes = diagnostics?.recentErrorCodes ?? [];
  const reminderOptions = [...new Set<number>([...REMINDER_MINUTE_OPTIONS, settings.reminderMinutes])]
    .sort((a, b) => a - b)
    .map((n) => ({ id: String(n), label: t("settings.minutes", { n }) }));

  const displayOptions = DISPLAY_OPTIONS.map(({ id, labelKey }) => ({ id, label: t(labelKey) }));

  const copyLabel = flash === "copied" ? t("about.copied") : flash === "failed" ? t("about.copyFailed") : t("about.copy");

  return (
    <div className="flex flex-col gap-4">
      <section data-section="settings">
        <SectionLabel
          trailing={
            flash === "saveFailed" ? (
              <span className="text-micro text-warning" role="alert">
                {t("settings.saveFailed")}
              </span>
            ) : undefined
          }
        >
          {t("about.settings")}
        </SectionLabel>
        <Group>
          <SwitchRow
            label={t("settings.launchWithWindows")}
            checked={settings.launchWithWindows}
            onChange={() => change({ launchWithWindows: !settings.launchWithWindows })}
          />
          <SwitchRow
            label={t("settings.hideInFullscreen")}
            checked={settings.hideInFullscreen}
            onChange={() => change({ hideInFullscreen: !settings.hideInFullscreen })}
          />
          <SwitchRow
            label={t("settings.meetingReminders")}
            checked={settings.meetingReminderEnabled}
            onChange={() => change({ meetingReminderEnabled: !settings.meetingReminderEnabled })}
          />
          <div className={`${ROW} transition-opacity ${settings.meetingReminderEnabled ? "" : "opacity-40 pointer-events-none"}`} aria-disabled={!settings.meetingReminderEnabled}>
            <span className="text-body text-fg flex-shrink-0">{t("settings.reminderMinutes")}</span>
            <Segmented
              options={reminderOptions}
              value={String(settings.reminderMinutes)}
              onChange={(id) => change({ reminderMinutes: Number(id) })}
              ariaLabel={t("settings.reminderMinutes")}
            />
          </div>
          <SwitchRow
            label={t("settings.meetingInvites")}
            checked={settings.meetingInvitesEnabled}
            onChange={() => change({ meetingInvitesEnabled: !settings.meetingInvitesEnabled })}
          />
          <SwitchRow
            label={t("settings.sharedCalendarReminders")}
            checked={settings.sharedCalendarReminders}
            onChange={() => change({ sharedCalendarReminders: !settings.sharedCalendarReminders })}
          />
          <SwitchRow
            label={t("settings.meetingSilence")}
            checked={settings.meetingSilencePrompt}
            onChange={() => change({ meetingSilencePrompt: !settings.meetingSilencePrompt })}
          />
          <SwitchRow
            label={t("settings.notifications")}
            checked={settings.notificationsEnabled}
            onChange={() => change({ notificationsEnabled: !settings.notificationsEnabled })}
          />
          {settings.notificationsEnabled && notificationStatus === "unspecified" && (
            <div className="flex flex-col gap-2 p-3">
              <span className="text-meta text-fg-secondary">{t("notif.status.unspecified")}</span>
              <ActionRow>
                <ActionButton onPress={onRequestNotificationAccess}>{t("notif.allow")}</ActionButton>
              </ActionRow>
            </div>
          )}
          <div className="flex flex-col gap-2 p-3">
            <span className="text-body text-fg">{t("settings.islandDisplay")}</span>
            <Segmented
              size="md"
              className="w-full"
              options={displayOptions}
              value={settings.islandDisplay}
              onChange={(id) => change({ islandDisplay: id })}
              ariaLabel={t("settings.islandDisplay")}
            />
          </div>
          {monitors.length > 1 && (
            <div className={ROW}>
              <span className="text-body text-fg flex-shrink-0">{t("settings.monitor")}</span>
              <Segmented
                options={monitors.map((monitor, index) => ({ id: monitor.id, label: monitorLabel(monitor, index) }))}
                value={settings.monitorId ?? monitors.find((m) => m.isPrimary)?.id ?? monitors[0].id}
                onChange={(id) => change({ monitorId: id })}
                ariaLabel={t("settings.monitor")}
              />
            </div>
          )}
        </Group>
      </section>

      <section data-section="center">
        <SectionLabel>{t("settings.center")}</SectionLabel>
        <ActionRow>
          <ActionButton onPress={onOpenCenter}>{t("settings.openCenter")}</ActionButton>
          <ActionButton onPress={onOpenTour}>{t("settings.tour")}</ActionButton>
        </ActionRow>
      </section>

      <section data-section="diagnostics">
        <SectionLabel>{t("about.diagnostics")}</SectionLabel>
        <Group>
          <Row label={t("about.windowsUser")}>{info?.windowsUser ?? NONE}</Row>
          <Row label={t("about.computer")}>{info?.computerName ?? NONE}</Row>
          <Row label={t("about.ip")}>{info?.localIpv4 ?? NONE}</Row>
          <Row label={t("about.os")}>{formatOs(info) ?? NONE}</Row>
          <Row label={t("about.version")}>{info?.appVersion ?? APP_VERSION}</Row>
          <Row label={t("about.outlook")}>{t(running ? "about.outlookRunning" : "about.outlookNotRunning")}</Row>
          <Row label={t("about.outlookMode")}>{t(MODE_LABEL[mode])}</Row>
          <Row label={t("about.calendar")}>{t(STATUS_LABEL[snapshot.status])}</Row>
          {internalError && <Row label={t("about.internalError")}>{internalError}</Row>}
          <Row label={t("about.cachedEvents")}>{snapshot.cachedCount}</Row>
          <Row label={t("about.lastSync")}>{snapshot.lastSyncUnixMs === null ? NONE : formatDateTime(new Date(snapshot.lastSyncUnixMs))}</Row>
          <Row label={t("about.notifications")}>
            {notificationDiagnostic === null ? NONE : t(`notif.status.${notificationDiagnostic}` as MessageKey)}
            {notificationCode ? ` · ${notificationCode}` : ""}
          </Row>
          {diagnostics?.notificationMode && <Row label={t("about.notificationDelivery")}>{t(DELIVERY_LABEL[diagnostics.notificationMode])}</Row>}
          <Row label={t("about.recentErrors")}>{recentCodes.length > 0 ? recentCodes.join(", ") : NONE}</Row>
        </Group>
        <ActionRow className="mt-2">
          <ActionButton variant={flash === "copied" ? "primary" : flash === "failed" ? "destructive" : "neutral"} onPress={onCopyDiagnostics}>
            <span aria-live="polite">{copyLabel}</span>
          </ActionButton>
          <ActionButton onPress={onOpenLogs}>{t("about.openLogs")}</ActionButton>
        </ActionRow>
      </section>

      <p className="bidi !text-center text-micro text-fg-tertiary">{t("about.credit")}</p>
    </div>
  );
}

interface SettingsTabProps {
  notificationStatus: NotificationStatus | null;
  onRequestNotificationAccess: () => void;
}

/** The app's settings, then the diagnostics IT asks for. (Both used to sit under About.) */
export function SettingsTab({ notificationStatus, onRequestNotificationAccess }: SettingsTabProps) {
  const { info, diagnostics } = useSystemInfo();
  const snapshot = useCalendar();
  const { settings, update } = useSettings();

  // Transient button feedback; one timer, always cleared.
  const [flash, setFlash] = useState<"copied" | "failed" | "saveFailed" | null>(null);
  const flashTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const showFlash = useCallback((kind: "copied" | "failed" | "saveFailed") => {
    if (flashTimer.current !== null) clearTimeout(flashTimer.current);
    setFlash(kind);
    flashTimer.current = setTimeout(() => {
      flashTimer.current = null;
      setFlash(null);
    }, FLASH_MS);
  }, []);
  useEffect(
    () => () => {
      if (flashTimer.current !== null) clearTimeout(flashTimer.current);
    },
    []
  );

  const [monitors, setMonitors] = useState<MonitorInfo[]>([]);
  useEffect(() => {
    let disposed = false;
    const load = () =>
      void ipc.getMonitors().then((list) => {
        if (!disposed && list) setMonitors(list);
      });
    load();
    // A display plugged in or removed while Settings is open.
    const off = onEvent("display-changed", load);
    return () => {
      disposed = true;
      off();
    };
  }, []);

  const copyDiagnostics = () => {
    const text = buildDiagnosticsText({
      info,
      diagnostics,
      snapshot,
      notifications: notificationDiagnosticOf(settings, notificationStatus),
      generatedAt: new Date(),
    });
    void ipc.copyTextToClipboard(text).then((ok) => showFlash(ok ? "copied" : "failed"));
  };

  const change = (patch: SettingsPatch) => {
    void update(patch).then((ok) => {
      if (!ok) showFlash("saveFailed");
    });
  };

  return (
    <SettingsView
      settings={settings}
      monitors={monitors}
      info={info}
      diagnostics={diagnostics}
      snapshot={snapshot}
      notificationStatus={notificationStatus}
      flash={flash}
      onChange={change}
      onRequestNotificationAccess={onRequestNotificationAccess}
      onCopyDiagnostics={copyDiagnostics}
      onOpenLogs={() => void ipc.openLogDir()}
      onOpenCenter={() => void ipc.openCenter("settings")}
      onOpenTour={() => void ipc.openCenter("tour")}
    />
  );
}
