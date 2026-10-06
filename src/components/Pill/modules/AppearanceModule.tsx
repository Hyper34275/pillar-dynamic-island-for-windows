import { motion } from "motion/react";
import type { AppearanceSettings, PillMode } from "../../../hooks/useAppearance";
import { ACCENT_PRESETS } from "../../../hooks/useAppearance";
import { FillSlider, Group, PillButton, SectionLabel, Segmented, Switch } from "../ui/primitives";
import { CheckIcon, ChevronLeftIcon, EyeIcon } from "../ui/icons";

const SPEED_PRESETS = [
  { id: "0.5", label: "0.5×" },
  { id: "0.75", label: "0.75×" },
  { id: "1", label: "1×" },
  { id: "1.5", label: "1.5×" },
  { id: "2", label: "2×" },
] as const;

interface AppearanceModuleProps {
  settings: AppearanceSettings;
  animationSpeed?: number;
  onAnimationSpeedChange?: (speed: number) => void;
  onUpdate: (changes: Partial<AppearanceSettings>) => void;
  onSave: () => void;
  onReset: () => void;
  onBack: () => void;
}

function ModePreview({ mode, selected, accent, onSelect }: { mode: PillMode; selected: boolean; accent: string; onSelect: () => void }) {
  return (
    <motion.button
      type="button"
      className={`flex-1 flex flex-col items-center gap-2 pt-2 pb-2 rounded-[16px] transition-colors ${
        selected ? "bg-white/[0.12]" : "bg-white/[0.05] hover:bg-white/[0.08]"
      }`}
      style={{ boxShadow: selected ? `inset 0 0 0 1.5px ${accent}` : undefined }}
      onClick={onSelect}
      aria-label={`Switch pill style to ${mode}`}
      aria-pressed={selected}
      whileTap={{ scale: 0.96 }}
    >
      {/* Mini screen */}
      <div className="relative w-[96px] h-[40px] rounded-[8px] bg-gradient-to-b from-white/[0.14] to-white/[0.04] overflow-hidden">
        <motion.div
          className="absolute left-1/2 -translate-x-1/2 bg-black flex items-center justify-between px-1.5"
          style={{
            top: mode === "notch" ? 0 : 5,
            borderRadius: mode === "notch" ? "0 0 8px 8px" : 8,
          }}
          animate={{ width: selected ? 48 : 40, height: mode === "notch" ? 13 : 12 }}
          transition={{ type: "spring", stiffness: 420, damping: 26 }}
        >
          <span className="w-[5px] h-[5px] rounded-full" style={{ background: accent }} />
          <span className="w-[10px] h-[3px] rounded-full bg-white/50" />
        </motion.div>
      </div>
      <span className={`text-[11.5px] font-semibold ${selected ? "text-white" : "text-white/50"}`}>
        {mode === "island" ? "Island" : "Notch"}
      </span>
    </motion.button>
  );
}

export function AppearanceModule({ settings, animationSpeed = 1.0, onAnimationSpeedChange, onUpdate, onSave, onReset, onBack }: AppearanceModuleProps) {
  return (
    <div className="flex flex-col gap-3 pb-1">
      {/* Header */}
      <div className="grid grid-cols-[1fr_auto_1fr] items-center">
        <button
          type="button"
          className="justify-self-start flex items-center gap-0.5 h-7 pl-1 pr-2.5 rounded-full text-[12.5px] font-semibold hover:bg-white/[0.08] transition-colors"
          style={{ color: "var(--pillar-accent)" }}
          onClick={onBack}
          aria-label="Back to settings"
        >
          <ChevronLeftIcon size={15} strokeWidth={2.6} />
          Controls
        </button>
        <span className="text-white text-[13px] font-semibold">Appearance</span>
        <span />
      </div>

      <div className="flex gap-2">
        {(["island", "notch"] as const).map((m) => (
          <ModePreview key={m} mode={m} selected={settings.mode === m} accent={settings.accentColor} onSelect={() => onUpdate({ mode: m })} />
        ))}
      </div>

      <div>
        <SectionLabel>Accent</SectionLabel>
        <div className="flex items-center justify-between px-1">
          {ACCENT_PRESETS.map((preset) => {
            const isActive = settings.accentColor === preset.value;
            return (
              <motion.button
                key={preset.value}
                type="button"
                className="relative w-[30px] h-[30px] rounded-full flex items-center justify-center"
                style={{
                  background: preset.value,
                  boxShadow: isActive ? `0 0 0 2.5px #000, 0 0 0 4.5px ${preset.value}` : "inset 0 0 0 1px rgba(255,255,255,0.12)",
                }}
                onClick={() => onUpdate({ accentColor: preset.value })}
                whileHover={{ scale: 1.1 }}
                whileTap={{ scale: 0.88 }}
                aria-label={`${preset.name} accent color`}
                aria-pressed={isActive}
                title={preset.name}
              >
                {isActive && (
                  <span style={{ color: preset.value === "#FFFFFF" ? "#000" : "#fff" }}>
                    <CheckIcon size={13} strokeWidth={3} />
                  </span>
                )}
              </motion.button>
            );
          })}
        </div>
      </div>

      <div>
        <SectionLabel>Opacity</SectionLabel>
        <FillSlider
          percent={((settings.opacity - 30) / 70) * 100}
          icon={<EyeIcon size={16} />}
          label="Island opacity"
          valueText={`${settings.opacity}%`}
          height={40}
        >
          <input
            type="range"
            min="30"
            max="100"
            value={settings.opacity}
            onChange={(e) => onUpdate({ opacity: parseInt(e.target.value, 10) })}
            className="absolute inset-0 w-full h-full opacity-0 cursor-pointer"
            aria-label="Pill transparency"
            aria-valuemin={30}
            aria-valuemax={100}
            aria-valuenow={settings.opacity}
          />
        </FillSlider>
      </div>

      {onAnimationSpeedChange && (
        <div>
          <SectionLabel>Animation speed</SectionLabel>
          <Segmented
            options={SPEED_PRESETS.map((p) => ({ id: p.id, label: p.label, ariaLabel: `Set animation speed to ${p.label}` }))}
            value={String(animationSpeed) as (typeof SPEED_PRESETS)[number]["id"]}
            onChange={(id) => onAnimationSpeedChange(parseFloat(id))}
            size="md"
            className="w-full"
          />
        </div>
      )}

      <Group>
        <div className="flex items-center gap-2.5 px-3 h-[48px]">
          <span className="flex flex-col min-w-0 flex-1">
            <span className="text-white text-[12.5px] font-medium leading-tight">Match album art</span>
            <span className="text-white/40 text-[10.5px] leading-tight">Tint the island with the cover's colors</span>
          </span>
          <Switch
            checked={settings.useAlbumAccent}
            onChange={() => onUpdate({ useAlbumAccent: !settings.useAlbumAccent })}
            label={settings.useAlbumAccent ? "Disable album art accent" : "Enable album art accent"}
          />
        </div>
      </Group>

      <div className="flex gap-2 pt-0.5">
        <PillButton className="flex-1 h-9 text-[12.5px]" onClick={onReset} ariaLabel="Reset appearance settings to default">
          Reset
        </PillButton>
        <PillButton variant="filled" className="flex-1 h-9 text-[12.5px]" onClick={onSave} ariaLabel="Save appearance settings">
          Save
        </PillButton>
      </div>
    </div>
  );
}
