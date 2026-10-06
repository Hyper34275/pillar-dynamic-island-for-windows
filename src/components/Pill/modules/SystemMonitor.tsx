import { useSystemStats } from "../../../hooks/useSystemStats";
import { SYSTEM_COLORS } from "../ui/primitives";
import { CpuIcon, MemoryIcon } from "../ui/icons";

interface SystemMonitorProps {
  /** Poll only while true (e.g. the Settings tab is open). */
  enabled: boolean;
  accentColor: string;
}

function clampPercent(value: number): number {
  if (!Number.isFinite(value)) return 0;
  return Math.max(0, Math.min(100, value));
}

function StatTile({
  label,
  icon,
  percent,
  detail,
  tint,
  ready,
}: {
  label: string;
  icon: React.ReactNode;
  percent: number;
  detail?: string;
  tint: string;
  ready: boolean;
}) {
  const pct = clampPercent(percent);
  // Shift toward red as load climbs so a glance reads "busy" — paired with the
  // always-present numeric value for non-color users.
  const color = pct >= 85 ? SYSTEM_COLORS.red : pct >= 65 ? SYSTEM_COLORS.orange : tint;

  return (
    <div className="rounded-[16px] bg-white/[0.07] px-3 pt-2.5 pb-3 flex flex-col gap-2 min-w-0">
      <div className="flex items-center justify-between">
        <span className="flex items-center gap-1.5 text-white/50 text-[11px] font-semibold">
          <span style={{ color }}>{icon}</span>
          {label}
        </span>
        {detail && <span className="text-white/35 text-[10px] tabular-nums truncate ml-1">{detail}</span>}
      </div>
      <span className="text-white text-[22px] font-semibold leading-none tabular-nums tracking-tight" style={{ fontVariantNumeric: "tabular-nums" }}>
        {ready ? Math.round(pct) : "—"}
        <span className="text-[13px] text-white/40 ml-0.5">%</span>
      </span>
      <div
        className="h-[5px] w-full rounded-full bg-white/[0.1] overflow-hidden"
        role="progressbar"
        aria-label={`${label} usage`}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={Math.round(pct)}
      >
        <div
          className="h-full rounded-full transition-[width,background-color] duration-700 ease-out"
          style={{ width: `${pct}%`, background: color }}
        />
      </div>
    </div>
  );
}

export function SystemMonitor({ enabled, accentColor }: SystemMonitorProps) {
  const { stats } = useSystemStats(enabled);
  // A white accent reads as "empty" on the bars; fall back to system green.
  const tint = accentColor.toLowerCase() === "#ffffff" ? SYSTEM_COLORS.green : accentColor;

  return (
    <section className="grid grid-cols-2 gap-2" aria-label="System monitor">
      <StatTile
        label="CPU"
        icon={<CpuIcon size={12} strokeWidth={2.4} />}
        percent={stats?.cpuPercent ?? 0}
        tint={tint}
        ready={!!stats}
      />
      <StatTile
        label="Memory"
        icon={<MemoryIcon size={12} strokeWidth={2.4} />}
        percent={stats?.memPercent ?? 0}
        detail={stats ? `${(stats.memUsedMb / 1024).toFixed(1)}/${(stats.memTotalMb / 1024).toFixed(0)} GB` : undefined}
        tint={tint}
        ready={!!stats}
      />
    </section>
  );
}
