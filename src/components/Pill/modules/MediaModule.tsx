import { motion, AnimatePresence } from "motion/react";
import { useState, useCallback, useRef, useMemo } from "react";
import type { MediaInfo, MediaTimeline, MediaPlaybackInfo } from "../../../hooks/useMediaSession";
import { gpuLayerHints } from "../animations";
import { cleanAppName, EmptyState, AppAvatar } from "../ui/primitives";
import { VolumeSlider } from "./VolumeModule";
import type { VolumeInfo } from "../../../hooks/useVolume";
import {
  BackwardIcon,
  ForwardIcon,
  MusicIcon,
  PauseIcon,
  PlayIcon,
  RepeatIcon,
  ShuffleIcon,
} from "../ui/icons";

// =============================================================================
// Waveform (idle pill + now playing header)
// =============================================================================

interface MediaIndicatorProps {
  isPlaying: boolean;
  color?: string;
  bars?: number;
  height?: number;
}

const BAR_PATTERNS = [
  [0.35, 1, 0.5, 0.85, 0.35],
  [0.6, 0.3, 1, 0.45, 0.6],
  [0.4, 0.8, 0.35, 1, 0.4],
  [0.7, 0.45, 0.9, 0.3, 0.7],
  [0.3, 0.9, 0.6, 0.75, 0.3],
];

export function MediaIndicator({ isPlaying, color = "var(--pillar-accent)", bars = 4, height = 14 }: MediaIndicatorProps) {
  return (
    <div className="flex items-center gap-[2.5px]" style={{ height, ...gpuLayerHints.transform }} aria-hidden="true">
      {Array.from({ length: bars }, (_, i) => (
        <motion.span
          key={i}
          className="w-[3px] rounded-full"
          style={{ background: color, height, originY: 0.5 }}
          animate={{ scaleY: isPlaying ? BAR_PATTERNS[i % BAR_PATTERNS.length] : 0.22 }}
          transition={
            isPlaying
              ? { duration: 0.9 + i * 0.07, repeat: Infinity, ease: "easeInOut", delay: i * 0.08 }
              : { duration: 0.25 }
          }
        />
      ))}
    </div>
  );
}

// =============================================================================
// Artwork
// =============================================================================

export function AlbumArt({
  url,
  size,
  radius,
  className = "",
}: {
  url?: string | null;
  size: number;
  radius: number;
  className?: string;
}) {
  return (
    <div
      className={`relative flex-shrink-0 overflow-hidden ${className}`}
      style={{
        width: size,
        height: size,
        borderRadius: radius,
        background: "linear-gradient(145deg, #3a3a3e, #1c1c1f)",
      }}
    >
      <AnimatePresence initial={false}>
        {url ? (
          <motion.img
            key={url.slice(-48)}
            src={url}
            alt=""
            draggable={false}
            className="absolute inset-0 w-full h-full object-cover"
            initial={{ opacity: 0, scale: 1.08 }}
            animate={{ opacity: 1, scale: 1 }}
            exit={{ opacity: 0 }}
            transition={{ duration: 0.35 }}
          />
        ) : (
          <motion.span
            key="placeholder"
            className="absolute inset-0 flex items-center justify-center text-white/40"
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
          >
            <MusicIcon size={Math.round(size * 0.42)} />
          </motion.span>
        )}
      </AnimatePresence>
    </div>
  );
}

// =============================================================================
// Seek bar
// =============================================================================

function formatTime(ms: number): string {
  const totalSeconds = Math.max(0, Math.floor(ms / 1000));
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = totalSeconds % 60;
  return `${minutes}:${seconds.toString().padStart(2, "0")}`;
}

interface SeekBarProps {
  timeline: MediaTimeline;
  onSeek: (positionMs: number) => void;
}

function SeekBar({ timeline, onSeek }: SeekBarProps) {
  const [isDragging, setIsDragging] = useState(false);
  const [dragPosition, setDragPosition] = useState(0);
  const [hover, setHover] = useState(false);
  const barRef = useRef<HTMLDivElement>(null);

  const progress = isDragging
    ? dragPosition
    : timeline.durationMs > 0
      ? Math.min(1, timeline.positionMs / timeline.durationMs)
      : 0;

  const positionFromEvent = (clientX: number) => {
    if (!barRef.current) return 0;
    const rect = barRef.current.getBoundingClientRect();
    return Math.max(0, Math.min(1, (clientX - rect.left) / rect.width));
  };

  const handlePointerDown = useCallback(
    (e: React.PointerEvent) => {
      if (!timeline.canSeek || !barRef.current) return;
      e.preventDefault();
      (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
      setDragPosition(positionFromEvent(e.clientX));
      setIsDragging(true);
    },
    [timeline.canSeek]
  );

  const handlePointerMove = useCallback(
    (e: React.PointerEvent) => {
      if (!isDragging) return;
      setDragPosition(positionFromEvent(e.clientX));
    },
    [isDragging]
  );

  const handlePointerUp = useCallback(() => {
    if (!isDragging) return;
    setIsDragging(false);
    onSeek(dragPosition * timeline.durationMs);
  }, [isDragging, dragPosition, timeline.durationMs, onSeek]);

  const displayPosition = isDragging ? dragPosition * timeline.durationMs : timeline.positionMs;

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if (!timeline.canSeek) return;
      let next: number | null = null;
      switch (e.key) {
        case "ArrowRight":
        case "ArrowUp":
          next = timeline.positionMs + 5000;
          break;
        case "ArrowLeft":
        case "ArrowDown":
          next = timeline.positionMs - 5000;
          break;
        case "Home":
          next = 0;
          break;
        case "End":
          next = timeline.durationMs;
          break;
        default:
          return;
      }
      e.preventDefault();
      e.stopPropagation();
      onSeek(Math.max(0, Math.min(timeline.durationMs, next)));
    },
    [timeline.canSeek, timeline.positionMs, timeline.durationMs, onSeek]
  );

  const active = hover || isDragging;

  return (
    <div className="flex flex-col gap-1.5 w-full">
      <div
        ref={barRef}
        className={`relative h-4 flex items-center ${timeline.canSeek ? "cursor-pointer" : ""}`}
        role="slider"
        aria-label="Seek"
        aria-valuemin={0}
        aria-valuemax={timeline.durationMs}
        aria-valuenow={displayPosition}
        aria-valuetext={formatTime(displayPosition)}
        tabIndex={timeline.canSeek ? 0 : -1}
        onPointerDown={handlePointerDown}
        onPointerMove={handlePointerMove}
        onPointerUp={handlePointerUp}
        onPointerEnter={() => setHover(true)}
        onPointerLeave={() => setHover(false)}
        onKeyDown={handleKeyDown}
      >
        <motion.div
          className="relative w-full rounded-full bg-white/[0.18] overflow-hidden"
          animate={{ height: active && timeline.canSeek ? 9 : 5 }}
          transition={{ type: "spring", stiffness: 500, damping: 32 }}
        >
          <div
            className={`absolute inset-y-0 left-0 rounded-full ${isDragging ? "" : "transition-[width] duration-300 ease-linear"}`}
            style={{ width: `${progress * 100}%`, background: active ? "#fff" : "rgba(255,255,255,0.75)" }}
          />
        </motion.div>
      </div>
      <div className="flex justify-between text-[11px] font-semibold text-white/40 tabular-nums" style={{ fontVariantNumeric: "tabular-nums" }}>
        <span>{formatTime(displayPosition)}</span>
        <span>-{formatTime(Math.max(0, timeline.durationMs - displayPosition))}</span>
      </div>
    </div>
  );
}

// =============================================================================
// Transport button
// =============================================================================

function TransportButton({
  label,
  onClick,
  children,
  size = 44,
  active,
  activeColor,
}: {
  label: string;
  onClick?: () => void;
  children: React.ReactNode;
  size?: number;
  active?: boolean;
  activeColor?: string;
}) {
  return (
    <motion.button
      type="button"
      className="relative rounded-full flex items-center justify-center hover:bg-white/[0.08] transition-colors"
      style={{
        width: size,
        height: size,
        color: active === undefined ? "#fff" : active ? activeColor ?? "#fff" : "rgba(255,255,255,0.4)",
      }}
      aria-label={label}
      aria-pressed={active}
      onClick={onClick}
      whileTap={{ scale: 0.82 }}
      transition={{ type: "spring", stiffness: 600, damping: 26 }}
    >
      {children}
      {active && (
        <motion.span
          className="absolute bottom-[5px] w-1 h-1 rounded-full"
          style={{ background: activeColor ?? "#fff" }}
          initial={{ scale: 0 }}
          animate={{ scale: 1 }}
        />
      )}
    </motion.button>
  );
}

// =============================================================================
// Media Expanded View
// =============================================================================

interface MediaExpandedProps {
  media: MediaInfo | null;
  artUrl?: string | null;
  recentSources?: string[];
  timeline?: MediaTimeline | null;
  playbackInfo?: MediaPlaybackInfo | null;
  accentColor?: string;
  onPlayPause: () => void;
  onNext: () => void;
  onPrevious: () => void;
  onToggleRepeat?: () => void;
  onToggleShuffle?: () => void;
  onPauseOthers?: () => void;
  onSeek?: (positionMs: number) => void;
  volume?: VolumeInfo;
  onVolumeChange?: (level: number) => void;
  onMuteToggle?: () => void;
}

export function MediaExpanded({
  media,
  artUrl,
  recentSources = [],
  timeline,
  playbackInfo,
  accentColor,
  onPlayPause,
  onNext,
  onPrevious,
  onToggleRepeat,
  onToggleShuffle,
  onPauseOthers,
  onSeek,
  volume,
  onVolumeChange,
  onMuteToggle,
}: MediaExpandedProps) {
  const sourceLabel = useMemo(() => cleanAppName(media?.appName), [media?.appName]);
  const otherSources = useMemo(
    () => recentSources.filter((s) => s !== media?.appName).slice(0, 3),
    [recentSources, media?.appName]
  );

  if (!media) {
    return (
      <EmptyState
        icon={<MusicIcon size={22} />}
        title="Nothing Playing"
        subtitle="Play something in Spotify, YouTube or Apple Music"
      />
    );
  }

  const tint = accentColor || "#fff";
  const repeatMode = playbackInfo?.repeatMode ?? "none";

  return (
    <div className="relative flex flex-col h-full">
      {/* Now playing */}
      <div className="relative flex items-center gap-3.5">
        <motion.div
          animate={{ scale: media.isPlaying ? 1 : 0.92 }}
          transition={{ type: "spring", stiffness: 380, damping: 24 }}
          style={{ boxShadow: "0 8px 24px rgba(0,0,0,0.5)", borderRadius: 16 }}
        >
          <AlbumArt url={artUrl} size={68} radius={16} />
        </motion.div>

        <div className="flex flex-col min-w-0 flex-1">
          <span className="text-white text-[16px] font-semibold truncate leading-tight text-left" title={media.title || undefined} dir="auto">
            {media.title || "Unknown Track"}
          </span>
          <span className="text-white/55 text-[13px] truncate leading-tight mt-0.5 text-left" title={media.artist || undefined} dir="auto">
            {media.artist || "Unknown Artist"}
          </span>
          {sourceLabel && (
            <span className="flex items-center gap-1.5 mt-1.5">
              <AppAvatar name={sourceLabel} size={14} radius={4} />
              <span className="text-white/35 text-[11px] font-medium truncate">{sourceLabel}</span>
            </span>
          )}
        </div>

        <div className="self-start pt-1">
          <MediaIndicator isPlaying={media.isPlaying} color={tint} bars={4} height={16} />
        </div>
      </div>

      {/* Seek */}
      <div className="relative mt-4">
        {timeline && timeline.durationMs > 0 && onSeek ? (
          <SeekBar timeline={timeline} onSeek={onSeek} />
        ) : (
          <div className="h-[5px] rounded-full bg-white/[0.12] my-[5.5px] mb-[22px]" />
        )}
      </div>

      {/* Transport */}
      <div className="relative flex items-center justify-between px-1 mt-1" role="group" aria-label="Media playback controls">
        {onToggleShuffle ? (
          <TransportButton
            label={playbackInfo?.isShuffle ? "Disable shuffle" : "Enable shuffle"}
            onClick={onToggleShuffle}
            size={36}
            active={!!playbackInfo?.isShuffle}
            activeColor={tint}
          >
            <ShuffleIcon size={17} />
          </TransportButton>
        ) : (
          <span className="w-9" />
        )}
        <TransportButton label="Previous track" onClick={onPrevious} size={48}>
          <BackwardIcon size={28} />
        </TransportButton>
        <TransportButton label={media.isPlaying ? "Pause playback" : "Play playback"} onClick={onPlayPause} size={56}>
          <AnimatePresence mode="popLayout" initial={false}>
            <motion.span
              key={media.isPlaying ? "pause" : "play"}
              initial={{ scale: 0.4, opacity: 0 }}
              animate={{ scale: 1, opacity: 1 }}
              exit={{ scale: 0.4, opacity: 0 }}
              transition={{ type: "spring", stiffness: 600, damping: 30 }}
              className="flex"
            >
              {media.isPlaying ? <PauseIcon size={34} /> : <PlayIcon size={34} />}
            </motion.span>
          </AnimatePresence>
        </TransportButton>
        <TransportButton label="Next track" onClick={onNext} size={48}>
          <ForwardIcon size={28} />
        </TransportButton>
        {onToggleRepeat ? (
          <TransportButton
            label={`Repeat: ${repeatMode}`}
            onClick={onToggleRepeat}
            size={36}
            active={repeatMode !== "none"}
            activeColor={tint}
          >
            <RepeatIcon size={17} one={repeatMode === "track"} />
          </TransportButton>
        ) : (
          <span className="w-9" />
        )}
      </div>

      {/* Volume */}
      {volume && onVolumeChange && onMuteToggle && (
        <div className="relative mt-2 px-1">
          <VolumeSlider volume={volume} onVolumeChange={onVolumeChange} onMuteToggle={onMuteToggle} variant="thin" />
        </div>
      )}

      {/* Other sources */}
      {(otherSources.length > 0 || onPauseOthers) && (
        <div className="relative flex items-center gap-1.5 mt-auto pt-2 min-w-0">
          {otherSources.map((source) => (
            <span
              key={source}
              className="flex items-center gap-1 h-6 pl-1 pr-2 rounded-full bg-white/[0.07] text-white/55 text-[11px] font-medium min-w-0"
              aria-label={`Recent media source ${cleanAppName(source)}`}
            >
              <AppAvatar name={source} size={16} radius={8} />
              <span className="truncate max-w-[80px]">{cleanAppName(source)}</span>
            </span>
          ))}
          {onPauseOthers && otherSources.length > 0 && (
            <button
              type="button"
              className="ml-auto flex items-center gap-1 h-6 px-2.5 rounded-full bg-white/[0.07] hover:bg-white/[0.12] text-white/60 hover:text-white text-[11px] font-semibold transition-colors flex-shrink-0"
              aria-label="Pause audio in other apps"
              onClick={onPauseOthers}
            >
              <PauseIcon size={9} />
              Pause others
            </button>
          )}
        </div>
      )}
    </div>
  );
}
