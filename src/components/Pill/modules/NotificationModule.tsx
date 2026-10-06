import { motion, AnimatePresence } from "motion/react";
import { useMemo, useCallback, useState } from "react";
import type { SystemNotification } from "../../../hooks/useNotifications";
import { notificationAnimations, gpuLayerHints, prefersReducedMotion } from "../animations";
import { AppAvatar, EmptyState, cleanAppName, SYSTEM_COLORS } from "../ui/primitives";
import { BellIcon, BroomIcon, ChevronDownIcon, XIcon } from "../ui/icons";

function relativeTime(timestamp: number): string {
  const diff = Date.now() - timestamp;
  if (diff < 60000) return "now";
  if (diff < 3600000) return `${Math.floor(diff / 60000)}m ago`;
  if (diff < 86400000) return `${Math.floor(diff / 3600000)}h ago`;
  return new Date(timestamp).toLocaleDateString(undefined, { month: "short", day: "numeric" });
}

// =============================================================================
// Notification Indicator (idle pill)
// =============================================================================

export type NotificationPhase = "idle" | "incoming" | "absorbing" | "showing";

interface NotificationIndicatorProps {
  count: number;
  appName: string;
  isNew?: boolean;
  layoutId?: string;
}

export function NotificationIndicator({
  count,
  appName,
  isNew = false,
  layoutId = "notification-badge",
}: NotificationIndicatorProps) {
  if (count === 0) return null;

  return (
    <motion.div
      {...(layoutId != null ? { layoutId } : {})}
      className="relative flex items-center mr-[6px]"
      initial={notificationAnimations.badge.initial}
      animate={{
        ...notificationAnimations.badge.animate,
        ...(isNew ? { scale: [1, 1.18, 1] } : {}),
      }}
      exit={notificationAnimations.badge.exit}
      transition={notificationAnimations.spring}
    >
      <AppAvatar name={appName} size={22} radius={7} />
      <span
        className="absolute -top-1 -right-1.5 min-w-[15px] h-[15px] px-1 rounded-full flex items-center justify-center text-[9.5px] font-bold text-white tabular-nums"
        style={{ background: SYSTEM_COLORS.red, boxShadow: "0 0 0 2px #000" }}
      >
        {count > 9 ? "9+" : count}
      </span>
    </motion.div>
  );
}

// =============================================================================
// Notification Toast (drops out of the island)
// =============================================================================

interface NotificationToastProps {
  notification: SystemNotification | null;
  onDismiss: () => void;
  onActivate?: (id: number) => void;
  phase?: NotificationPhase;
}

export function NotificationToast({ notification, onDismiss, onActivate, phase = "incoming" }: NotificationToastProps) {
  const shouldShow = notification && (phase === "incoming" || phase === "absorbing");

  return (
    <AnimatePresence mode="popLayout">
      {shouldShow && (
        <motion.div
          layoutId="notification-badge"
          className="z-50 overflow-visible"
          style={{ originY: 0 }}
          initial={prefersReducedMotion ? { opacity: 0 } : { y: -24, opacity: 0, scaleX: 0.55, scaleY: 0.3 }}
          animate={
            phase === "absorbing"
              ? { y: -40, opacity: 0, scaleX: 0.4, scaleY: 0.2 }
              : { y: 0, opacity: 1, scaleX: 1, scaleY: 1 }
          }
          exit={{ y: -36, opacity: 0, scaleX: 0.4, scaleY: 0.2 }}
          transition={
            phase === "absorbing"
              ? notificationAnimations.absorptionSpring
              : { type: "spring" as const, stiffness: 420, damping: 30, mass: 0.9 }
          }
        >
          <ToastCard notification={notification} phase={phase} onDismiss={onDismiss} onActivate={onActivate} />
        </motion.div>
      )}
    </AnimatePresence>
  );
}

function ToastCard({
  notification,
  phase,
  onDismiss,
  onActivate,
}: {
  notification: SystemNotification;
  phase: NotificationPhase;
  onDismiss: () => void;
  onActivate?: (id: number) => void;
}) {
  const appLabel = cleanAppName(notification.appName) || notification.appName;

  return (
    <div
      className="group relative w-[352px] rounded-[28px] cursor-pointer overflow-hidden select-none"
      style={{
        background: "rgba(0,0,0,0.97)",
        boxShadow: "0 0 0 0.5px rgba(255,255,255,0.08), 0 16px 40px rgba(0,0,0,0.5)",
      }}
      onClick={(e) => {
        e.stopPropagation();
        onActivate?.(notification.id);
        onDismiss();
      }}
      role="button"
      tabIndex={0}
      onKeyDown={(e) => {
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          e.stopPropagation();
          onActivate?.(notification.id);
          onDismiss();
        } else if (e.key === "Delete" || e.key === "Escape") {
          e.preventDefault();
          e.stopPropagation();
          onDismiss();
        }
      }}
    >
      <motion.div
        className="relative flex items-center gap-3 pl-3 pr-2 py-3"
        initial={{ opacity: 0 }}
        animate={{ opacity: 1 }}
        transition={{ delay: 0.08, duration: 0.2 }}
      >
        <AppAvatar name={notification.appName} size={42} radius={12} />

        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-2">
            <span className="text-white/45 text-[11px] font-semibold truncate">{appLabel}</span>
            <span className="text-white/30 text-[11px] ml-auto flex-shrink-0">now</span>
          </div>
          <h4 className="text-white text-[14px] font-semibold truncate leading-snug" title={notification.title || undefined} dir="auto">
            {notification.title || "Notification"}
          </h4>
          {notification.body && (
            <p className="text-white/60 text-[12.5px] line-clamp-2 leading-snug" title={notification.body} dir="auto">
              {notification.body}
            </p>
          )}
        </div>

        <button
          type="button"
          className="self-start w-7 h-7 rounded-full flex items-center justify-center text-white/35 hover:text-white hover:bg-white/10 transition-colors flex-shrink-0"
          aria-label="Dismiss notification"
          onClick={(e) => {
            e.stopPropagation();
            onDismiss();
          }}
          onKeyDown={(e) => {
            if (e.key === "Enter" || e.key === " ") {
              e.preventDefault();
              e.stopPropagation();
              onDismiss();
            }
          }}
        >
          <XIcon size={13} strokeWidth={2.6} />
        </button>
      </motion.div>

      {phase === "incoming" && (
        <motion.div
          className="absolute bottom-0 left-6 right-6 h-[2px] rounded-full bg-white/25"
          style={{ transformOrigin: "left" }}
          initial={{ scaleX: 1 }}
          animate={prefersReducedMotion ? { scaleX: 1 } : { scaleX: 0 }}
          transition={prefersReducedMotion ? { duration: 0 } : { duration: 3.5, ease: "linear" }}
        />
      )}
    </div>
  );
}

// =============================================================================
// Notification Card (list)
// =============================================================================

interface NotificationCardProps {
  notification: SystemNotification;
  onDismiss: (id: number) => void | Promise<void>;
  onActivate?: (id: number) => void;
}

export function NotificationCard({ notification, onDismiss, onActivate }: NotificationCardProps) {
  const formattedTime = useMemo(() => relativeTime(notification.timestamp), [notification.timestamp]);

  const handleActivate = useCallback(() => {
    onActivate?.(notification.id);
  }, [onActivate, notification.id]);

  return (
    <motion.div
      className="group relative rounded-[18px] bg-white/[0.08] hover:bg-white/[0.11] transition-colors cursor-pointer px-3 py-2.5"
      layout
      initial={{ opacity: 0, y: 8, scale: 0.98 }}
      animate={{ opacity: 1, y: 0, scale: 1 }}
      exit={{ opacity: 0, x: 40, transition: { duration: 0.18 } }}
      transition={{ type: "spring", stiffness: 480, damping: 36 }}
      onClick={handleActivate}
      onKeyDown={(e) => {
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          handleActivate();
        } else if (e.key === "Delete" || e.key === "Escape") {
          e.preventDefault();
          e.stopPropagation();
          void onDismiss(notification.id);
        }
      }}
      role="button"
      tabIndex={0}
      style={gpuLayerHints.transformAndOpacity}
    >
      <div className="flex items-baseline gap-2">
        <h4 className="text-white text-[13px] font-semibold truncate flex-1" title={notification.title || undefined} dir="auto">
          {notification.title || "Notification"}
        </h4>
        <span className="text-white/35 text-[11px] flex-shrink-0 group-hover:opacity-0 group-focus-within:opacity-0 transition-opacity">
          {formattedTime}
        </span>
      </div>
      {notification.body && (
        <p className="text-white/55 text-[12px] line-clamp-2 leading-snug mt-0.5" title={notification.body} dir="auto">
          {notification.body}
        </p>
      )}

      <button
        type="button"
        className="absolute top-1.5 right-1.5 w-6 h-6 rounded-full flex items-center justify-center bg-white/15 text-white/80 hover:bg-white/25 hover:text-white opacity-0 group-hover:opacity-100 focus:opacity-100 transition-opacity"
        aria-label="Dismiss notification"
        onClick={(e) => {
          e.stopPropagation();
          void onDismiss(notification.id);
        }}
        onKeyDown={(e) => {
          if (e.key === "Enter" || e.key === " ") {
            e.preventDefault();
            e.stopPropagation();
            void onDismiss(notification.id);
          }
        }}
      >
        <XIcon size={11} strokeWidth={2.8} />
      </button>
    </motion.div>
  );
}

// =============================================================================
// Notifications List — grouped by app, iOS-style stacks
// =============================================================================

interface NotificationsListProps {
  notifications: SystemNotification[];
  hasAccess: boolean;
  onDismiss: (id: number) => void | Promise<void>;
  onActivate?: (id: number) => void;
}

const OLD_THRESHOLD_MS = 10 * 60 * 1000;

export function NotificationsList({ notifications, hasAccess, onDismiss, onActivate }: NotificationsListProps) {
  const [expandedApps, setExpandedApps] = useState<Record<string, boolean>>({});

  if (!hasAccess) {
    return (
      <EmptyState
        icon={<BellIcon size={22} />}
        title="Notification Access Needed"
        subtitle="Allow access in Windows Settings › Privacy › Notifications"
        tint={`color-mix(in srgb, ${SYSTEM_COLORS.orange} 22%, transparent)`}
      />
    );
  }

  if (notifications.length === 0) {
    return <EmptyState icon={<BellIcon size={22} />} title="No Notifications" subtitle="You're all caught up" />;
  }

  const grouped = notifications.reduce<Record<string, SystemNotification[]>>((acc, n) => {
    (acc[n.appName] ??= []).push(n);
    return acc;
  }, {});
  const groupedList = Object.entries(grouped).sort((a, b) => b[1][0].timestamp - a[1][0].timestamp);

  const isGroupExpanded = (appName: string, size: number) => expandedApps[appName] ?? size <= 2;
  const toggleGroup = (appName: string, size: number) =>
    setExpandedApps((prev) => ({ ...prev, [appName]: !isGroupExpanded(appName, size) }));

  const clearGroup = (appName: string) => {
    const target = grouped[appName]?.slice(0, 20) ?? [];
    void Promise.allSettled(target.map((n) => Promise.resolve(onDismiss(n.id))));
  };

  const clearOldInGroup = (appName: string) => {
    const now = Date.now();
    const target = (grouped[appName] ?? []).filter((n) => now - n.timestamp > OLD_THRESHOLD_MS).slice(0, 15);
    void Promise.allSettled(target.map((n) => Promise.resolve(onDismiss(n.id))));
  };

  return (
    <div className="flex flex-col gap-3">
      {groupedList.map(([appName, items]) => {
        const expanded = isGroupExpanded(appName, items.length);
        const label = cleanAppName(appName) || appName;
        const hasOld = items.some((n) => Date.now() - n.timestamp > OLD_THRESHOLD_MS);
        return (
          <motion.section key={appName} layout="position" className="flex flex-col gap-1.5" aria-label={`${label} notifications`}>
            <div className="flex items-center gap-2 px-1">
              <button
                type="button"
                className="flex items-center gap-2 min-w-0 flex-1 text-left"
                aria-label={`${expanded ? "Collapse" : "Expand"} ${label} notifications`}
                aria-expanded={expanded}
                onClick={() => toggleGroup(appName, items.length)}
              >
                <AppAvatar name={appName} size={20} radius={6} />
                <span className="text-white/85 text-[12px] font-semibold truncate">{label}</span>
                {items.length > 1 && (
                  <span className="flex items-center gap-0.5 text-white/40 text-[11px] font-medium">
                    {items.length}
                    <motion.span animate={{ rotate: expanded ? 180 : 0 }} className="flex">
                      <ChevronDownIcon size={11} strokeWidth={2.6} />
                    </motion.span>
                  </span>
                )}
              </button>
              {hasOld && (
                <button
                  type="button"
                  className="w-6 h-6 rounded-full flex items-center justify-center bg-white/[0.08] text-white/50 hover:text-white hover:bg-white/[0.14] transition-colors"
                  aria-label={`Clear old notifications from ${label}`}
                  title="Clear older than 10 minutes"
                  onClick={() => clearOldInGroup(appName)}
                >
                  <BroomIcon size={12} />
                </button>
              )}
              <button
                type="button"
                className="w-6 h-6 rounded-full flex items-center justify-center bg-white/[0.08] text-white/50 hover:text-white hover:bg-white/[0.14] transition-colors"
                aria-label={`Clear all notifications from ${label}`}
                title="Clear"
                onClick={() => clearGroup(appName)}
              >
                <XIcon size={11} strokeWidth={2.8} />
              </button>
            </div>

            {expanded ? (
              <div className="flex flex-col gap-1.5">
                <AnimatePresence initial={false}>
                  {items.slice(0, 10).map((n) => (
                    <NotificationCard key={n.id} notification={n} onDismiss={onDismiss} onActivate={onActivate} />
                  ))}
                </AnimatePresence>
              </div>
            ) : (
              // Collapsed: newest card on top of a stack of sheets
              <div
                className="relative pb-[10px] cursor-pointer"
                onClick={() => toggleGroup(appName, items.length)}
              >
                <div className="absolute left-4 right-4 bottom-0 h-6 rounded-b-[16px] bg-white/[0.04]" aria-hidden="true" />
                <div className="absolute left-2 right-2 bottom-[5px] h-6 rounded-b-[17px] bg-white/[0.06]" aria-hidden="true" />
                {/* Opaque backing so the sheets don't show through the translucent card */}
                <div className="relative rounded-[18px] bg-black" onClick={(e) => e.stopPropagation()}>
                  <NotificationCard notification={items[0]} onDismiss={onDismiss} onActivate={onActivate} />
                </div>
              </div>
            )}
          </motion.section>
        );
      })}
    </div>
  );
}
