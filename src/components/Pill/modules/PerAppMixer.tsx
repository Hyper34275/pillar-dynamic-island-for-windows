import { motion } from "motion/react";
import { useState, useCallback, useRef, useEffect } from "react";
import type { AudioSession } from "../../../hooks/usePerAppMixer";
import { AppAvatar, cleanAppName, FillSlider } from "../ui/primitives";
import { SpeakerIcon } from "../ui/icons";
import { useThrottledCommit } from "../../../hooks/useThrottledCommit";

// =============================================================================
// Per-App Volume Slider
// =============================================================================

interface AppVolumeSliderProps {
  session: AudioSession;
  onVolumeChange: (processId: number, volume: number) => void;
  onMuteToggle: (processId: number, muted: boolean) => void;
}

function AppVolumeSlider({ session, onVolumeChange, onMuteToggle }: AppVolumeSliderProps) {
  const [isDragging, setIsDragging] = useState(false);
  const [localVolume, setLocalVolume] = useState(session.volume);
  // Pointer-state ref mirrors isDragging so handlers always see live state.
  const isDraggingRef = useRef(false);

  const commitVolume = useCallback(
    (value: number) => onVolumeChange(session.processId, value),
    [onVolumeChange, session.processId]
  );
  const { send: sendVolume, sendNow: sendVolumeNow } = useThrottledCommit(commitVolume);

  const handleChange = useCallback((e: React.ChangeEvent<HTMLInputElement>) => {
    const newVolume = parseInt(e.target.value, 10) / 100;
    setLocalVolume(newVolume);
    // Keyboard interaction fires onChange without pointer events: commit as we
    // go so keyboard users can adjust per-app volume. Throttled, because a held
    // arrow key auto-repeats far faster than the backend can service it.
    if (!isDraggingRef.current) {
      sendVolume(newVolume);
    }
  }, [sendVolume]);

  const endDrag = useCallback(() => {
    if (!isDraggingRef.current) return;
    isDraggingRef.current = false;
    setIsDragging(false);
    sendVolumeNow(localVolume);
  }, [localVolume, sendVolumeNow]);

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

  const displayVolume = isDragging ? localVolume : session.volume;
  const displayPercent = Math.round(displayVolume * 100);
  const label = cleanAppName(session.appName) || session.appName;

  return (
    <div className="flex items-center gap-1.5">
      <motion.button
        type="button"
        className="relative w-[34px] h-[34px] rounded-[11px] flex items-center justify-center flex-shrink-0 overflow-hidden"
        aria-label={`${session.isMuted ? "Unmute" : "Mute"} ${session.appName}`}
        aria-pressed={session.isMuted}
        onClick={() => onMuteToggle(session.processId, !session.isMuted)}
        whileTap={{ scale: 0.9 }}
        title={session.isMuted ? "Unmute" : "Mute"}
      >
        <span className={session.isMuted ? "opacity-35 grayscale" : ""}>
          <AppAvatar name={session.appName} size={34} radius={11} />
        </span>
        {session.isMuted && (
          <span className="absolute inset-0 flex items-center justify-center text-white">
            <SpeakerIcon size={15} level={-1} />
          </span>
        )}
      </motion.button>

      <div className="flex-1 min-w-0">
        <FillSlider
          percent={session.isMuted ? 0 : displayPercent}
          icon={null}
          label={label}
          valueText={session.isMuted ? "Muted" : `${displayPercent}%`}
          fill={session.isActive ? "#ffffff" : "rgba(255,255,255,0.55)"}
          height={34}
        >
          <input
            type="range"
            min="0"
            max="100"
            value={displayPercent}
            onChange={handleChange}
            onMouseDown={handleMouseDown}
            onMouseUp={endDrag}
            onTouchEnd={endDrag}
            onBlur={endDrag}
            aria-label={`${session.appName} volume`}
            aria-valuemin={0}
            aria-valuemax={100}
            aria-valuenow={displayPercent}
            className="absolute inset-0 w-full h-full opacity-0 cursor-pointer"
          />
        </FillSlider>
      </div>
    </div>
  );
}

// =============================================================================
// Per-App Mixer Panel
// =============================================================================

interface PerAppMixerProps {
  sessions: AudioSession[];
  onVolumeChange: (processId: number, volume: number) => void;
  onMuteToggle: (processId: number, muted: boolean) => void;
}

export function PerAppMixer({ sessions, onVolumeChange, onMuteToggle }: PerAppMixerProps) {
  if (sessions.length === 0) {
    return (
      <div className="flex items-center justify-center gap-2 py-2 text-white/45 text-[12px]">
        <SpeakerIcon size={14} level={-1} />
        No apps playing audio
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-1.5 max-h-[150px] overflow-y-auto">
      {sessions.map((session) => (
        <AppVolumeSlider
          key={session.processId}
          session={session}
          onVolumeChange={onVolumeChange}
          onMuteToggle={onMuteToggle}
        />
      ))}
    </div>
  );
}
