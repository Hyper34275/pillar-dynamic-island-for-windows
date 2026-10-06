import { motion } from "motion/react";
import type { TimerState, TimerStats, TimerCategory } from "../../../hooks/useTimer";
import type { TimerPreset } from "../../../types/pill";
import { Segmented, PillButton, SYSTEM_COLORS } from "../ui/primitives";
import { BellIcon, PauseIcon, PlayIcon, XIcon } from "../ui/icons";

// iOS timer orange — the color the island uses for every timer surface.
export const TIMER_TINT = SYSTEM_COLORS.orange;

const spring = { type: "spring" as const, stiffness: 420, damping: 32 };

function formatFocus(totalSeconds: number): string {
  const h = Math.floor(totalSeconds / 3600);
  const m = Math.floor((totalSeconds % 3600) / 60);
  return h > 0 ? `${h}h ${m}m` : `${m}m`;
}

// =============================================================================
// Timer Alert (complete)
// =============================================================================

function TimerAlert({ label, onDismiss }: { label: string; onDismiss: () => void }) {
  return (
    <motion.div
      className="flex flex-col items-center justify-center gap-1 h-full py-2"
      initial={{ opacity: 0, scale: 0.94 }}
      animate={{ opacity: 1, scale: 1 }}
      transition={spring}
    >
      <div className="relative w-[84px] h-[84px] flex items-center justify-center mb-2">
        <motion.span
          className="absolute inset-0 rounded-full"
          style={{ background: TIMER_TINT }}
          animate={{ scale: [1, 1.35], opacity: [0.35, 0] }}
          transition={{ duration: 1.6, repeat: Infinity, ease: "easeOut" }}
        />
        <span
          className="relative w-[84px] h-[84px] rounded-full flex items-center justify-center"
          style={{ background: `color-mix(in srgb, ${TIMER_TINT} 22%, black)`, color: TIMER_TINT }}
        >
          <motion.span
            animate={{ rotate: [0, -14, 12, -8, 6, 0] }}
            transition={{ duration: 0.9, repeat: Infinity, repeatDelay: 0.8 }}
          >
            <BellIcon size={34} strokeWidth={2.2} />
          </motion.span>
        </span>
      </div>
      <span className="text-white text-[17px] font-semibold" dir="auto">{label}</span>
      <span className="text-white/50 text-[13px] mb-3">Time's up</span>
      <PillButton variant="filled" tint={TIMER_TINT} className="h-9 px-6 text-[13px]" onClick={onDismiss} ariaLabel="Dismiss timer alert">
        Dismiss
      </PillButton>
    </motion.div>
  );
}

// =============================================================================
// Running timer
// =============================================================================

function TimerRunning({
  timer,
  progress,
  formatTime,
  onPause,
  onResume,
  onStop,
}: {
  timer: TimerState;
  progress: number;
  formatTime: (s: number) => string;
  onPause: () => void;
  onResume: () => void;
  onStop: () => void;
}) {
  const size = 148;
  const stroke = 7;
  const r = (size - stroke) / 2;
  const circ = 2 * Math.PI * r;
  // useTimer's progress is the REMAINING fraction (1 → 0), so the ring depletes.
  const remaining = Math.max(0, Math.min(1, progress));
  const tint = timer.isPaused ? "rgba(255,255,255,0.45)" : TIMER_TINT;

  return (
    <div className="flex items-center justify-between h-full px-3 gap-4">
      <div className="relative flex-shrink-0" style={{ width: size, height: size }}>
        <svg width={size} height={size} style={{ transform: "rotate(-90deg)" }} aria-hidden="true">
          <circle cx={size / 2} cy={size / 2} r={r} fill="none" stroke="rgba(255,255,255,0.1)" strokeWidth={stroke} />
          <motion.circle
            cx={size / 2}
            cy={size / 2}
            r={r}
            fill="none"
            stroke={tint}
            strokeWidth={stroke}
            strokeLinecap="round"
            strokeDasharray={circ}
            animate={{ strokeDashoffset: circ * (1 - remaining), stroke: tint }}
            transition={{ duration: 0.5, ease: "easeOut" }}
          />
        </svg>
        <div className="absolute inset-0 flex flex-col items-center justify-center">
          <span
            className="text-white text-[32px] font-semibold tabular-nums leading-none tracking-tight"
            style={{ fontVariantNumeric: "tabular-nums" }}
          >
            {formatTime(timer.remainingSeconds)}
          </span>
          <span className="text-[12px] mt-1.5 font-medium truncate max-w-[110px]" style={{ color: timer.isPaused ? "rgba(255,255,255,0.45)" : TIMER_TINT }} dir="auto">
            {timer.isPaused ? "Paused" : timer.label}
          </span>
        </div>
      </div>

      <div className="flex flex-col items-center gap-3 flex-1" role="group" aria-label="Timer controls">
        <div className="flex items-center gap-3">
          <RoundButton label="Stop timer" onClick={onStop} bg="rgba(255,255,255,0.12)" fg="#fff">
            <XIcon size={20} strokeWidth={2.6} />
          </RoundButton>
          {timer.isPaused ? (
            <RoundButton label="Resume timer" onClick={onResume} bg={`color-mix(in srgb, ${SYSTEM_COLORS.green} 24%, black)`} fg={SYSTEM_COLORS.green}>
              <PlayIcon size={22} />
            </RoundButton>
          ) : (
            <RoundButton label="Pause timer" onClick={onPause} bg={`color-mix(in srgb, ${TIMER_TINT} 24%, black)`} fg={TIMER_TINT}>
              <PauseIcon size={20} />
            </RoundButton>
          )}
        </div>
        <span className="text-white/40 text-[11px] font-medium tabular-nums">
          {Math.round((1 - remaining) * 100)}% complete
        </span>
      </div>
    </div>
  );
}

function RoundButton({
  label,
  onClick,
  bg,
  fg,
  children,
}: {
  label: string;
  onClick: () => void;
  bg: string;
  fg: string;
  children: React.ReactNode;
}) {
  return (
    <motion.button
      type="button"
      className="w-[58px] h-[58px] rounded-full flex items-center justify-center"
      style={{ background: bg, color: fg }}
      aria-label={label}
      onClick={onClick}
      whileHover={{ scale: 1.05 }}
      whileTap={{ scale: 0.9 }}
      transition={{ type: "spring", stiffness: 600, damping: 28 }}
    >
      {children}
    </motion.button>
  );
}

// =============================================================================
// Timer Expanded View
// =============================================================================

interface TimerExpandedProps {
  timer: TimerState;
  stats?: TimerStats;
  categories?: TimerCategory[];
  selectedCategory?: string;
  onSelectCategory?: (categoryId: string) => void;
  presets: TimerPreset[];
  formatTime: (seconds: number) => string;
  progress: number;
  onStart: (preset: TimerPreset) => void;
  onPause: () => void;
  onResume: () => void;
  onStop: () => void;
  onDismiss: () => void;
}

export function TimerExpanded({
  timer,
  stats,
  categories = [],
  selectedCategory,
  onSelectCategory,
  presets,
  formatTime,
  progress,
  onStart,
  onPause,
  onResume,
  onStop,
  onDismiss,
}: TimerExpandedProps) {
  if (timer.isComplete) {
    return <TimerAlert label={timer.label} onDismiss={onDismiss} />;
  }

  if (timer.isActive) {
    return (
      <TimerRunning
        timer={timer}
        progress={progress}
        formatTime={formatTime}
        onPause={onPause}
        onResume={onResume}
        onStop={onStop}
      />
    );
  }


  return (
    <div className="flex flex-col gap-3 h-full">
      {categories.length > 0 && onSelectCategory && selectedCategory && (
        <Segmented
          options={categories.map((c) => ({ id: c.id, label: c.label, ariaLabel: `Select ${c.label} category` }))}
          value={selectedCategory}
          onChange={onSelectCategory}
          className="w-full"
          ariaLabel="Timer categories"
        />
      )}

      <div className="grid grid-cols-3 gap-2" role="group" aria-label="Timer presets">
        {presets.map((preset, i) => (
          <motion.button
            key={preset.id}
            type="button"
            className="group relative flex flex-col items-start justify-between rounded-[20px] bg-white/[0.07] hover:bg-white/[0.11] transition-colors px-3 pt-2.5 pb-2.5 h-[104px] text-left overflow-hidden"
            aria-label={`Start ${preset.label} timer for ${preset.workMinutes} minutes`}
            onClick={() => onStart(preset)}
            initial={{ opacity: 0, y: 8 }}
            animate={{ opacity: 1, y: 0 }}
            transition={{ ...spring, delay: i * 0.04 }}
            whileTap={{ scale: 0.95 }}
          >
            <span
              className="w-7 h-7 rounded-full flex items-center justify-center transition-transform group-hover:scale-110"
              style={{ background: `color-mix(in srgb, ${TIMER_TINT} 22%, black)`, color: TIMER_TINT }}
              aria-hidden="true"
            >
              <PlayIcon size={12} />
            </span>
            <span className="flex flex-col">
              <span className="text-white leading-none">
                <span className="text-[28px] font-semibold tabular-nums tracking-tight">{preset.workMinutes}</span>
                <span className="text-[12px] font-semibold text-white/45 ml-0.5">min</span>
              </span>
              <span className="text-white/55 text-[11px] font-medium mt-1 truncate max-w-full">{preset.label}</span>
            </span>
          </motion.button>
        ))}
      </div>

      {stats && (
        <div className="grid grid-cols-2 gap-2 mt-auto">
          <Stat label="Sessions" value={String(stats.sessionsCompleted)} />
          <Stat label="Focus time" value={formatFocus(stats.totalFocusSeconds)} />
        </div>
      )}
    </div>
  );
}

function Stat({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-[16px] bg-white/[0.05] px-3 py-2 flex items-baseline justify-between">
      <span className="text-white/45 text-[11px] font-medium">{label}</span>
      <span className="text-white text-[15px] font-semibold tabular-nums">{value}</span>
    </div>
  );
}
