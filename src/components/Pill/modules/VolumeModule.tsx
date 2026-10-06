import { motion, AnimatePresence } from "motion/react";
import { useState, useCallback, useEffect, useRef } from "react";
import type { VolumeInfo } from "../../../hooks/useVolume";
import type { BrightnessInfo } from "../../../hooks/useBrightness";
import type { AudioDevice } from "../../../hooks/useAudioDevices";
import type { AudioSession } from "../../../hooks/usePerAppMixer";
import type { AppearanceSettings } from "../../../hooks/useAppearance";
import { ACCENT_PRESETS } from "../../../hooks/useAppearance";
import type { LayoutSettingsData } from "../../../hooks/useSettings";
import { PerAppMixer } from "./PerAppMixer";
import { AppearanceModule } from "./AppearanceModule";
import { fireAndForget } from "../../../lib/fireAndForget";
import { tauriInvoke } from "../../../lib/tauri";
import { useThrottledCommit } from "../../../hooks/useThrottledCommit";
import { FillSlider, Group, IconBadge, SectionLabel, Switch, SYSTEM_COLORS } from "../ui/primitives";
import {
  ArchiveIcon,
  CheckIcon,
  ChevronDownIcon,
  ChevronRightIcon,
  HeadphonesIcon,
  PaletteIcon,
  PowerIcon,
  SpeakerIcon,
  SunIcon,
  SlidersIcon,
} from "../ui/icons";

const RANGE_INPUT_CLASS = "absolute inset-0 w-full h-full opacity-0 cursor-pointer";

function speakerLevel(level: number, muted: boolean): 0 | 1 | 2 | -1 {
  if (muted || level === 0) return -1;
  return level < 50 ? 1 : 2;
}

// =============================================================================
// Volume Slider
// =============================================================================

interface VolumeSliderProps {
  volume: VolumeInfo;
  onVolumeChange: (level: number) => void;
  onMuteToggle: () => void;
  /** "tile" = Control Center fill slider, "thin" = Music-app style hairline slider */
  variant?: "tile" | "thin";
}

export function VolumeSlider({ volume, onVolumeChange, variant = "tile" }: VolumeSliderProps) {
  const [isDragging, setIsDragging] = useState(false);
  const [localLevel, setLocalLevel] = useState(volume.level);
  // Pointer-state ref mirrors isDragging so event handlers (which React
  // recreates per render) always see the live drag state.
  const isDraggingRef = useRef(false);

  // Sync local level when volume prop changes (from polling) and not dragging
  useEffect(() => {
    if (!isDragging) {
      setLocalLevel(volume.level);
    }
  }, [volume.level, isDragging]);

  const { send: sendLevel, sendNow: sendLevelNow } = useThrottledCommit(onVolumeChange);

  const handleChange = useCallback((e: React.ChangeEvent<HTMLInputElement>) => {
    const newLevel = parseInt(e.target.value, 10);
    setLocalLevel(newLevel);
    // Keyboard interaction (arrow keys) fires onChange without any pointer
    // event: commit as we go so keyboard users can actually change volume.
    // Throttled, because a held arrow key auto-repeats far faster than the
    // backend can service it.
    if (!isDraggingRef.current) {
      sendLevel(newLevel);
    }
  }, [sendLevel]);

  const endDrag = useCallback(() => {
    if (!isDraggingRef.current) return;
    isDraggingRef.current = false;
    setIsDragging(false);
    sendLevelNow(localLevel);
  }, [localLevel, sendLevelNow]);

  const handleMouseDown = useCallback(() => {
    isDraggingRef.current = true;
    setIsDragging(true);
  }, []);

  // A range input keeps pointer capture while dragging, so the release can land
  // outside the element. Listen on the window rather than using onMouseLeave,
  // which fires whenever the cursor crosses the thin track mid-drag and would
  // commit early — then every later move would fire its own backend call.
  useEffect(() => {
    if (!isDragging) return;
    window.addEventListener("mouseup", endDrag);
    window.addEventListener("touchend", endDrag);
    return () => {
      window.removeEventListener("mouseup", endDrag);
      window.removeEventListener("touchend", endDrag);
    };
  }, [isDragging, endDrag]);

  const displayLevel = isDragging ? localLevel : volume.level;

  const input = (
    <input
      type="range"
      min="0"
      max="100"
      value={displayLevel}
      onChange={handleChange}
      onMouseDown={handleMouseDown}
      onMouseUp={endDrag}
      onTouchEnd={endDrag}
      onBlur={endDrag}
      aria-label="Volume level"
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={displayLevel}
      className={RANGE_INPUT_CLASS}
    />
  );

  if (variant === "thin") {
    const pct = volume.isMuted ? 0 : displayLevel;
    return (
      <div className="group flex items-center gap-2.5 w-full text-white/45">
        <SpeakerIcon size={13} level={speakerLevel(pct, volume.isMuted) === -1 ? -1 : 0} />
        <div className="relative flex-1 h-5 flex items-center">
          <div className="w-full h-[5px] group-hover:h-[8px] transition-[height] duration-150 rounded-full bg-white/[0.18] overflow-hidden">
            <div className="h-full rounded-full bg-white/75 group-hover:bg-white transition-colors" style={{ width: `${pct}%` }} />
          </div>
          {input}
        </div>
        <SpeakerIcon size={13} level={2} />
      </div>
    );
  }

  return (
    <FillSlider
      percent={volume.isMuted ? 0 : displayLevel}
      icon={null}
      label="Volume"
      valueText={volume.isMuted ? "Muted" : `${displayLevel}%`}
      height={44}
    >
      {input}
    </FillSlider>
  );
}

// =============================================================================
// Brightness Slider
// =============================================================================

interface BrightnessSliderProps {
  brightness: BrightnessInfo;
  onBrightnessChange: (level: number) => void;
}

export function BrightnessSlider({ brightness, onBrightnessChange }: BrightnessSliderProps) {
  const [isDragging, setIsDragging] = useState(false);
  const [localLevel, setLocalLevel] = useState(brightness.level);

  // Sync local level when brightness prop changes (from polling) and not dragging
  useEffect(() => {
    if (!isDragging) {
      setLocalLevel(brightness.level);
    }
  }, [brightness.level, isDragging]);

  const { send: sendBrightness, sendNow: sendBrightnessNow } = useThrottledCommit(onBrightnessChange);

  const handleChange = useCallback((e: React.ChangeEvent<HTMLInputElement>) => {
    const newLevel = parseInt(e.target.value, 10);
    setLocalLevel(newLevel);
    // Commit as the slider moves for responsive feedback, but throttled: a DDC/CI
    // write takes far longer than the gap between pointer samples, so one call
    // per change event would queue writes faster than the monitor drains them.
    sendBrightness(newLevel);
  }, [sendBrightness]);

  const handleMouseUp = useCallback(() => {
    if (isDragging) {
      sendBrightnessNow(localLevel);
      setIsDragging(false);
    }
  }, [isDragging, localLevel, sendBrightnessNow]);

  const handleMouseDown = useCallback(() => {
    setIsDragging(true);
  }, []);

  const displayLevel = isDragging ? localLevel : brightness.level;

  return (
    <FillSlider
      percent={displayLevel}
      icon={<SunIcon size={17} />}
      label="Display"
      valueText={brightness.isSupported ? `${displayLevel}%` : "Not supported"}
      disabled={!brightness.isSupported}
      height={44}
    >
      <input
        type="range"
        min="0"
        max="100"
        step="1"
        value={displayLevel}
        onChange={handleChange}
        onInput={handleChange}
        onMouseDown={handleMouseDown}
        onMouseUp={handleMouseUp}
        onMouseLeave={() => {
          if (isDragging) {
            handleMouseUp();
          }
        }}
        onTouchStart={handleMouseDown}
        onTouchEnd={handleMouseUp}
        aria-label="Brightness level"
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={displayLevel}
        aria-disabled={!brightness.isSupported}
        className={RANGE_INPUT_CLASS}
        style={{ cursor: brightness.isSupported ? "pointer" : "not-allowed" }}
      />
    </FillSlider>
  );
}

// =============================================================================
// Square control tile (mute / mixer)
// =============================================================================

function Tile({
  label,
  active,
  activeColor = "#fff",
  onClick,
  children,
  expanded,
}: {
  label: string;
  active?: boolean;
  activeColor?: string;
  onClick: () => void;
  children: React.ReactNode;
  expanded?: boolean;
}) {
  return (
    <motion.button
      type="button"
      className="relative w-[44px] h-[44px] rounded-[14px] flex items-center justify-center flex-shrink-0 transition-colors"
      style={{
        background: active ? activeColor : "rgba(255,255,255,0.09)",
        color: active ? (activeColor === "#fff" ? "#000" : "#fff") : "rgba(255,255,255,0.85)",
      }}
      aria-label={label}
      aria-pressed={expanded === undefined ? active : undefined}
      aria-expanded={expanded}
      onClick={onClick}
      whileTap={{ scale: 0.9 }}
      transition={{ type: "spring", stiffness: 600, damping: 30 }}
    >
      {children}
    </motion.button>
  );
}

// =============================================================================
// Output device row
// =============================================================================

function deviceIcon(name: string) {
  const lower = name.toLowerCase();
  if (lower.includes("headphone") || lower.includes("earphone") || lower.includes("headset") || lower.includes("airpods") || lower.includes("buds")) {
    return <HeadphonesIcon size={14} />;
  }
  return <SpeakerIcon size={14} level={2} />;
}

function DeviceSelector({ devices, currentDevice }: { devices: AudioDevice[]; currentDevice: AudioDevice | null }) {
  const [isOpen, setIsOpen] = useState(false);
  const canOpen = devices.length > 1;

  return (
    <div>
      <button
        type="button"
        className={`w-full flex items-center gap-2.5 px-3 h-[46px] text-left ${canOpen ? "hover:bg-white/[0.04]" : "cursor-default"} transition-colors`}
        onClick={() => canOpen && setIsOpen(!isOpen)}
        aria-label={canOpen ? (isOpen ? "Collapse output devices list" : "Expand output devices list") : "Audio output device"}
        aria-expanded={canOpen ? isOpen : undefined}
      >
        <IconBadge color={SYSTEM_COLORS.blue}>{deviceIcon(currentDevice?.name || "")}</IconBadge>
        <span className="flex flex-col min-w-0 flex-1">
          <span className="text-white/45 text-[10.5px] font-semibold leading-tight">Output</span>
          <span className="text-white text-[12.5px] font-medium truncate leading-tight" title={currentDevice?.name || undefined}>
            {currentDevice?.name || "No audio device"}
          </span>
        </span>
        {canOpen && (
          <motion.span className="text-white/35 flex" animate={{ rotate: isOpen ? 180 : 0 }}>
            <ChevronDownIcon size={14} strokeWidth={2.4} />
          </motion.span>
        )}
      </button>
      <AnimatePresence initial={false}>
        {isOpen && (
          <motion.div
            initial={{ height: 0, opacity: 0 }}
            animate={{ height: "auto", opacity: 1 }}
            exit={{ height: 0, opacity: 0 }}
            transition={{ type: "spring", stiffness: 420, damping: 36 }}
            className="overflow-hidden"
          >
            <div className="pb-1.5">
              {devices.map((device) => (
                <button
                  key={device.id}
                  type="button"
                  className="w-full flex items-center gap-2.5 pl-[50px] pr-3 h-8 hover:bg-white/[0.05] transition-colors"
                  onClick={() => setIsOpen(false)}
                  aria-label={`${device.name}${device.isDefault ? " currently default output" : ""}`}
                >
                  <span className={`text-[12px] truncate flex-1 text-left ${device.isDefault ? "text-white" : "text-white/60"}`} title={device.name}>
                    {device.name}
                  </span>
                  {device.isDefault && (
                    <span style={{ color: SYSTEM_COLORS.blue }}>
                      <CheckIcon size={13} strokeWidth={2.8} />
                    </span>
                  )}
                </button>
              ))}
            </div>
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
}

// =============================================================================
// Quick Settings Panel (Expanded View)
// =============================================================================

interface AppearanceControls {
  active: AppearanceSettings;
  isEditing: boolean;
  startEditing: () => void;
  updateDraft: (changes: Partial<AppearanceSettings>) => void;
  // These persist through the Rust backend and reject if the write fails; the
  // signature used to claim `void`, which is what hid the unhandled rejection.
  save: () => Promise<void>;
  reset: () => Promise<void>;
  discard: () => void;
}

interface MotionSettings {
  animationSpeed: number;
  onAnimationSpeedChange: (speed: number) => void;
}

interface QuickSettingsProps {
  volume: VolumeInfo;
  onVolumeChange: (level: number) => void;
  onMuteToggle: () => void;
  brightness: BrightnessInfo;
  onBrightnessChange: (level: number) => void;
  audioDevices: AudioDevice[];
  defaultAudioDevice: AudioDevice | null;
  audioSessions: AudioSession[];
  onSessionVolumeChange: (processId: number, volume: number) => void;
  onSessionMuteToggle: (processId: number, muted: boolean) => void;
  autoStartEnabled: boolean;
  onAutoStartToggle: () => void;
  layoutSettings?: LayoutSettingsData;
  onLayoutChange?: (patch: Partial<LayoutSettingsData>) => void;
  appearance: AppearanceControls;
  motionSettings?: MotionSettings;
  /** Rendered above the controls (system monitor tiles). */
  header?: React.ReactNode;
}

const TAB_LABELS: Record<keyof LayoutSettingsData["visible_tabs"], string> = {
  timer: "Timer",
  media: "Media",
  notifications: "Notifications",
  settings: "Controls",
  productivity: "Focus",
  prism: "Prism",
};

const INDICATOR_LABELS: Record<keyof LayoutSettingsData["idle_indicators"], string> = {
  media: "Now playing",
  battery: "Battery",
  notifications: "Notifications",
};

function Chip({ on, label, onClick, ariaLabel }: { on: boolean; label: string; onClick: () => void; ariaLabel: string }) {
  return (
    <motion.button
      type="button"
      className={`flex items-center gap-1 h-7 px-2.5 rounded-full text-[11.5px] font-semibold transition-colors ${
        on ? "bg-white/[0.14] text-white" : "text-white/40 hover:text-white/70 hover:bg-white/[0.05]"
      }`}
      style={on ? undefined : { boxShadow: "inset 0 0 0 1px rgba(255,255,255,0.1)" }}
      aria-label={ariaLabel}
      aria-pressed={on}
      onClick={onClick}
      whileTap={{ scale: 0.92 }}
    >
      {on && (
        <span style={{ color: "var(--pillar-accent)" }}>
          <CheckIcon size={11} strokeWidth={3} />
        </span>
      )}
      {label}
    </motion.button>
  );
}

export function QuickSettings({
  volume,
  onVolumeChange,
  onMuteToggle,
  brightness,
  onBrightnessChange,
  audioDevices,
  defaultAudioDevice,
  audioSessions,
  onSessionVolumeChange,
  onSessionMuteToggle,
  autoStartEnabled,
  onAutoStartToggle,
  layoutSettings,
  onLayoutChange,
  appearance,
  motionSettings,
  header,
}: QuickSettingsProps) {
  const [showMixer, setShowMixer] = useState(false);
  const [view, setView] = useState<"main" | "appearance">("main");

  // The settings panel scrolls; switching sub-views should start at the top
  // (Appearance used to open scrolled down with its Back button out of view).
  useEffect(() => {
    document.getElementById("panel-settings")?.scrollTo({ top: 0 });
  }, [view]);
  const [logDir, setLogDir] = useState<string | null>(null);

  useEffect(() => {
    let mounted = true;
    tauriInvoke<string>("get_log_dir", undefined, { silent: true })
      .then((dir) => { if (mounted && dir) setLogDir(dir); })
      .catch(() => { /* backend unavailable — keep the generic subtitle */ });
    return () => { mounted = false; };
  }, []);

  // Discard draft on unmount (tab switch, pill collapse)
  useEffect(() => {
    return () => {
      appearance.discard();
    };
  }, [appearance.discard]);

  if (view === "appearance") {
    return (
      <AppearanceModule
        settings={appearance.active}
        animationSpeed={motionSettings?.animationSpeed}
        onAnimationSpeedChange={motionSettings?.onAnimationSpeedChange}
        onUpdate={appearance.updateDraft}
        onSave={() => { fireAndForget(appearance.save(), "appearance save"); setView("main"); }}
        onReset={() => { fireAndForget(appearance.reset(), "appearance reset"); setView("main"); }}
        onBack={() => { appearance.discard(); setView("main"); }}
      />
    );
  }

  const accentName = ACCENT_PRESETS.find((p) => p.value === appearance.active.accentColor)?.name ?? "Custom";
  const muted = volume.isMuted;

  return (
    <div className="flex flex-col gap-2.5 pb-1">
      {header}

      {/* Sound */}
      <div className="flex items-center gap-2">
        <Tile label={muted ? "Unmute volume" : "Mute volume"} active={muted} activeColor={SYSTEM_COLORS.red} onClick={onMuteToggle}>
          <SpeakerIcon size={18} level={muted || volume.level === 0 ? -1 : 2} />
        </Tile>
        <div className="flex-1 min-w-0">
          <VolumeSlider volume={volume} onVolumeChange={onVolumeChange} onMuteToggle={onMuteToggle} />
        </div>
        <Tile
          label={showMixer ? "Hide per-app mixer" : "Show per-app mixer"}
          active={showMixer}
          expanded={showMixer}
          onClick={() => setShowMixer(!showMixer)}
        >
          <SlidersIcon size={17} />
          {audioSessions.length > 0 && !showMixer && (
            <span className="absolute top-1 right-1 min-w-[14px] h-[14px] px-0.5 rounded-full bg-white/20 text-[9px] font-bold text-white flex items-center justify-center">
              {audioSessions.length}
            </span>
          )}
        </Tile>
      </div>

      <AnimatePresence initial={false}>
        {showMixer && (
          <motion.div
            initial={{ height: 0, opacity: 0 }}
            animate={{ height: "auto", opacity: 1 }}
            exit={{ height: 0, opacity: 0 }}
            transition={{ type: "spring", stiffness: 420, damping: 38 }}
            className="overflow-hidden"
          >
            <div className="rounded-[16px] bg-white/[0.06] p-2">
              <PerAppMixer sessions={audioSessions} onVolumeChange={onSessionVolumeChange} onMuteToggle={onSessionMuteToggle} />
            </div>
          </motion.div>
        )}
      </AnimatePresence>

      <BrightnessSlider brightness={brightness} onBrightnessChange={onBrightnessChange} />

      <Group>
        <DeviceSelector devices={audioDevices} currentDevice={defaultAudioDevice} />
        <button
          type="button"
          className="w-full flex items-center gap-2.5 px-3 h-[46px] hover:bg-white/[0.04] transition-colors text-left"
          onClick={() => { appearance.startEditing(); setView("appearance"); }}
          aria-label="Open appearance settings"
        >
          <IconBadge color={SYSTEM_COLORS.purple}><PaletteIcon size={14} /></IconBadge>
          <span className="flex flex-col min-w-0 flex-1">
            <span className="text-white text-[12.5px] font-medium leading-tight">Appearance</span>
            <span className="text-white/40 text-[10.5px] leading-tight">
              {appearance.active.mode === "island" ? "Island" : "Notch"} · {accentName} · {appearance.active.opacity}%
            </span>
          </span>
          <span className="w-3 h-3 rounded-full flex-shrink-0" style={{ background: appearance.active.accentColor }} />
          <span className="text-white/30"><ChevronRightIcon size={14} strokeWidth={2.4} /></span>
        </button>
        <div className="flex items-center gap-2.5 px-3 h-[46px]">
          <IconBadge color={SYSTEM_COLORS.green}><PowerIcon size={14} /></IconBadge>
          <span className="flex flex-col min-w-0 flex-1">
            <span className="text-white text-[12.5px] font-medium leading-tight">Start with Windows</span>
            <span className="text-white/40 text-[10.5px] leading-tight">Launch PILLAR when you log in</span>
          </span>
          <Switch checked={autoStartEnabled} onChange={onAutoStartToggle} label="Start with Windows" />
        </div>
        <button
          type="button"
          className="w-full flex items-center gap-2.5 px-3 h-[46px] hover:bg-white/[0.04] transition-colors text-left"
          onClick={() => fireAndForget(tauriInvoke("open_log_dir"), "open log dir")}
          aria-label="Open debug logs folder"
          title={logDir ?? undefined}
        >
          <IconBadge color={SYSTEM_COLORS.indigo}><ArchiveIcon size={14} /></IconBadge>
          <span className="flex flex-col min-w-0 flex-1">
            <span className="text-white text-[12.5px] font-medium leading-tight">Debug logs</span>
            <span className="text-white/40 text-[10.5px] leading-tight truncate">
              {logDir ?? "Open the log folder"}
            </span>
          </span>
          <span className="text-white/30"><ChevronRightIcon size={14} strokeWidth={2.4} /></span>
        </button>
      </Group>

      {layoutSettings && onLayoutChange && (
        <div>
          <SectionLabel
            trailing={
              <button
                type="button"
                className="text-[11px] font-semibold text-white/40 hover:text-white transition-colors"
                onClick={() =>
                  onLayoutChange({
                    visible_tabs: { timer: true, media: true, notifications: true, settings: true, prism: true, productivity: true },
                    idle_indicators: { media: true, battery: true, notifications: true },
                  })
                }
              >
                Reset
              </button>
            }
          >
            Tabs
          </SectionLabel>
          <div className="flex flex-wrap gap-1.5 mb-3">
            {(Object.keys(TAB_LABELS) as Array<keyof typeof TAB_LABELS>).map((tabId) => (
              <Chip
                key={tabId}
                on={layoutSettings.visible_tabs[tabId]}
                label={TAB_LABELS[tabId]}
                ariaLabel={`${layoutSettings.visible_tabs[tabId] ? "Hide" : "Show"} ${tabId} tab`}
                onClick={() =>
                  onLayoutChange({ visible_tabs: { ...layoutSettings.visible_tabs, [tabId]: !layoutSettings.visible_tabs[tabId] } })
                }
              />
            ))}
          </div>
          <SectionLabel>Island shows</SectionLabel>
          <div className="flex flex-wrap gap-1.5">
            {(Object.keys(INDICATOR_LABELS) as Array<keyof typeof INDICATOR_LABELS>).map((key) => (
              <Chip
                key={key}
                on={layoutSettings.idle_indicators[key]}
                label={INDICATOR_LABELS[key]}
                ariaLabel={`${layoutSettings.idle_indicators[key] ? "Hide" : "Show"} ${key} idle indicator`}
                onClick={() =>
                  onLayoutChange({ idle_indicators: { ...layoutSettings.idle_indicators, [key]: !layoutSettings.idle_indicators[key] } })
                }
              />
            ))}
          </div>
        </div>
      )}
    </div>
  );
}
