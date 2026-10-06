import type { SVGProps } from "react";

// =============================================================================
// Icon set — rounded line icons in the spirit of SF Symbols. One stroke weight,
// one grid (24), currentColor everywhere so callers control tint via text color.
// =============================================================================

type IconProps = SVGProps<SVGSVGElement> & { size?: number };

function Svg({ size = 16, children, ...rest }: IconProps) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={2}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      focusable="false"
      {...rest}
    >
      {children}
    </svg>
  );
}

export const TimerIcon = (p: IconProps) => (
  <Svg {...p}>
    <circle cx="12" cy="13.5" r="7.5" />
    <path d="M12 9.5v4l2.5 1.5M9.5 2.5h5M12 2.5V6" />
  </Svg>
);

export const MusicIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M9 18V5.5l11-2V16" />
    <circle cx="6.5" cy="18" r="2.5" />
    <circle cx="17.5" cy="16" r="2.5" />
  </Svg>
);

export const BellIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M18 9.5a6 6 0 1 0-12 0c0 6-2.5 7.5-2.5 7.5h17S18 15.5 18 9.5Z" />
    <path d="M10.3 20.5a2 2 0 0 0 3.4 0" />
  </Svg>
);

export const SlidersIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M4 7h9M17 7h3M4 17h3M11 17h9" />
    <circle cx="15" cy="7" r="2" />
    <circle cx="9" cy="17" r="2" />
  </Svg>
);

export const CheckCircleIcon = (p: IconProps) => (
  <Svg {...p}>
    <circle cx="12" cy="12" r="9" />
    <path d="m8.5 12.2 2.4 2.3 4.6-4.8" />
  </Svg>
);

export const SparklesIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M11 3.5 12.6 8a3 3 0 0 0 1.9 1.9L19 11.5l-4.5 1.6a3 3 0 0 0-1.9 1.9L11 19.5l-1.6-4.5a3 3 0 0 0-1.9-1.9L3 11.5l4.5-1.6A3 3 0 0 0 9.4 8L11 3.5Z" />
    <path d="M19 3v3M20.5 4.5h-3" />
  </Svg>
);

export const PlayIcon = ({ size = 16, ...rest }: IconProps) => (
  <svg width={size} height={size} viewBox="0 0 24 24" fill="currentColor" aria-hidden="true" {...rest}>
    <path d="M7 4.9v14.2c0 .8.9 1.3 1.6.9l11.3-7.1a1.05 1.05 0 0 0 0-1.8L8.6 4c-.7-.4-1.6.1-1.6.9Z" />
  </svg>
);

export const PauseIcon = ({ size = 16, ...rest }: IconProps) => (
  <svg width={size} height={size} viewBox="0 0 24 24" fill="currentColor" aria-hidden="true" {...rest}>
    <rect x="5.5" y="4" width="4.5" height="16" rx="1.4" />
    <rect x="14" y="4" width="4.5" height="16" rx="1.4" />
  </svg>
);

export const BackwardIcon = ({ size = 16, ...rest }: IconProps) => (
  <svg width={size} height={size} viewBox="0 0 24 24" fill="currentColor" aria-hidden="true" {...rest}>
    <path d="M11 7.2v9.6c0 .7-.8 1.1-1.3.7L3.5 12.7a.9.9 0 0 1 0-1.4l6.2-4.8c.5-.4 1.3 0 1.3.7Z" />
    <path d="M21 7.2v9.6c0 .7-.8 1.1-1.3.7l-6.2-4.8a.9.9 0 0 1 0-1.4l6.2-4.8c.5-.4 1.3 0 1.3.7Z" />
  </svg>
);

export const ForwardIcon = ({ size = 16, ...rest }: IconProps) => (
  <svg width={size} height={size} viewBox="0 0 24 24" fill="currentColor" aria-hidden="true" {...rest}>
    <path d="M13 7.2v9.6c0 .7.8 1.1 1.3.7l6.2-4.8a.9.9 0 0 0 0-1.4l-6.2-4.8c-.5-.4-1.3 0-1.3.7Z" />
    <path d="M3 7.2v9.6c0 .7.8 1.1 1.3.7l6.2-4.8a.9.9 0 0 0 0-1.4L4.3 6.5C3.8 6.1 3 6.5 3 7.2Z" />
  </svg>
);

export const ShuffleIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M3 7h3.5a4 4 0 0 1 3.3 1.8l4.4 6.4a4 4 0 0 0 3.3 1.8H21M3 17h3.5a4 4 0 0 0 3.3-1.8M14.2 8.8A4 4 0 0 1 17.5 7H21" />
    <path d="m18 4 3 3-3 3M18 14l3 3-3 3" />
  </Svg>
);

export const RepeatIcon = ({ one = false, ...p }: IconProps & { one?: boolean }) => (
  <Svg {...p}>
    <path d="M17 2.5 20 5.5l-3 3" />
    <path d="M4 11.5v-1a5 5 0 0 1 5-5h11" />
    <path d="m7 21.5-3-3 3-3" />
    <path d="M20 12.5v1a5 5 0 0 1-5 5H4" />
    {one && <path d="M11.5 10.5 12.5 10v4" strokeWidth={1.8} />}
  </Svg>
);

export const SpeakerIcon = ({ level = 2, ...p }: IconProps & { level?: 0 | 1 | 2 | -1 }) => (
  <Svg {...p}>
    <path d="M4 9.5h3l4.5-4v13L7 14.5H4a1 1 0 0 1-1-1v-3a1 1 0 0 1 1-1Z" fill="currentColor" />
    {level === -1 && <path d="m16 9.5 5 5M21 9.5l-5 5" />}
    {level >= 1 && <path d="M15.5 9a4 4 0 0 1 0 6" />}
    {level >= 2 && <path d="M18.5 6.5a7.5 7.5 0 0 1 0 11" />}
  </Svg>
);

export const SunIcon = (p: IconProps) => (
  <Svg {...p}>
    <circle cx="12" cy="12" r="4" fill="currentColor" />
    <path d="M12 2.5v2M12 19.5v2M2.5 12h2M19.5 12h2M5.3 5.3l1.4 1.4M17.3 17.3l1.4 1.4M5.3 18.7l1.4-1.4M17.3 6.7l1.4-1.4" />
  </Svg>
);

export const HeadphonesIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M3.5 17v-4.5a8.5 8.5 0 0 1 17 0V17" />
    <rect x="3" y="14" width="4.5" height="6.5" rx="1.6" fill="currentColor" />
    <rect x="16.5" y="14" width="4.5" height="6.5" rx="1.6" fill="currentColor" />
  </Svg>
);

export const XIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M6 6l12 12M18 6 6 18" />
  </Svg>
);

export const ChevronRightIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="m9 5.5 6.5 6.5L9 18.5" />
  </Svg>
);

export const ChevronDownIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="m5.5 9 6.5 6.5L18.5 9" />
  </Svg>
);

export const ChevronLeftIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M15 5.5 8.5 12l6.5 6.5" />
  </Svg>
);

export const PlusIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M12 5v14M5 12h14" />
  </Svg>
);

export const CheckIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="m5 12.5 4.5 4.5L19 7.5" />
  </Svg>
);

export const ArrowUpIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M12 19V5M5.5 11.5 12 5l6.5 6.5" />
  </Svg>
);

export const StopIcon = ({ size = 16, ...rest }: IconProps) => (
  <svg width={size} height={size} viewBox="0 0 24 24" fill="currentColor" aria-hidden="true" {...rest}>
    <rect x="5" y="5" width="14" height="14" rx="3" />
  </svg>
);

export const CpuIcon = (p: IconProps) => (
  <Svg {...p}>
    <rect x="6" y="6" width="12" height="12" rx="2.5" />
    <path d="M9.5 2.5v3M14.5 2.5v3M9.5 18.5v3M14.5 18.5v3M2.5 9.5h3M2.5 14.5h3M18.5 9.5h3M18.5 14.5h3" />
  </Svg>
);

export const MemoryIcon = (p: IconProps) => (
  <Svg {...p}>
    <rect x="2.5" y="7" width="19" height="10" rx="2" />
    <path d="M6.5 10.5v3M10 10.5v3M14 10.5v3M17.5 10.5v3M5 17v2.5M19 17v2.5" />
  </Svg>
);

export const PaletteIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M12 3a9 9 0 1 0 0 18c1.2 0 1.8-.8 1.8-1.7 0-.5-.2-.9-.5-1.2-.3-.3-.5-.7-.5-1.2 0-1 .8-1.7 1.7-1.7H17a4 4 0 0 0 4-4C21 6.6 17 3 12 3Z" />
    <circle cx="7.5" cy="11" r="1" fill="currentColor" />
    <circle cx="10.5" cy="7" r="1" fill="currentColor" />
    <circle cx="15" cy="7.5" r="1" fill="currentColor" />
  </Svg>
);

export const PowerIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M12 3v8" />
    <path d="M6.4 6.4a8 8 0 1 0 11.2 0" />
  </Svg>
);

export const LayoutIcon = (p: IconProps) => (
  <Svg {...p}>
    <rect x="3" y="4" width="18" height="16" rx="3" />
    <path d="M3 9h18M9 9v11" />
  </Svg>
);

export const EyeIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M2.5 12S6 5.5 12 5.5 21.5 12 21.5 12 18 18.5 12 18.5 2.5 12 2.5 12Z" />
    <circle cx="12" cy="12" r="3" />
  </Svg>
);

export const BoltIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M13 2.5 4.5 13.5H12l-1 8 8.5-11H12l1-8Z" fill="currentColor" />
  </Svg>
);

export const TrashIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M4 7h16M10 11v6M14 11v6M5.5 7l1 12a2 2 0 0 0 2 1.8h7a2 2 0 0 0 2-1.8l1-12M9 7V4.5a1 1 0 0 1 1-1h4a1 1 0 0 1 1 1V7" />
  </Svg>
);

export const PencilIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M16.5 3.5a2.1 2.1 0 0 1 3 3L8 18l-4 1 1-4L16.5 3.5Z" />
  </Svg>
);

export const CalendarIcon = (p: IconProps) => (
  <Svg {...p}>
    <rect x="3" y="4.5" width="18" height="16.5" rx="3" />
    <path d="M3 9.5h18M8 2.5v4M16 2.5v4" />
  </Svg>
);

export const NoteIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M14.5 3H6.5a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h11a2 2 0 0 0 2-2V8L14.5 3Z" />
    <path d="M14 3v5h5.5M8.5 13h7M8.5 17h4.5" />
  </Svg>
);

export const ArchiveIcon = (p: IconProps) => (
  <Svg {...p}>
    <rect x="2.5" y="3.5" width="19" height="5" rx="1.5" />
    <path d="M4.5 8.5v10a2 2 0 0 0 2 2h11a2 2 0 0 0 2-2v-10M10 12.5h4" />
  </Svg>
);

export const BroomIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M19.5 3 13 9.5" />
    <path d="M11 8.5 15.5 13 13 20.5c-2.5.5-6.5-.5-9.5-4 1.5 0 3-.5 4-2L11 8.5Z" />
  </Svg>
);
