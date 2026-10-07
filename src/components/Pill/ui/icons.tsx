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

export const ClockIcon = (p: IconProps) => (
  <Svg {...p}>
    <circle cx="12" cy="12" r="9" />
    <path d="M12 7v5.2l3.3 2" />
  </Svg>
);

export const CalendarIcon = (p: IconProps) => (
  <Svg {...p}>
    <rect x="3" y="4.5" width="18" height="16.5" rx="3" />
    <path d="M3 9.5h18M8 2.5v4M16 2.5v4" />
  </Svg>
);

export const InfoIcon = (p: IconProps) => (
  <Svg {...p}>
    <circle cx="12" cy="12" r="9" />
    <path d="M12 11v5.5" />
    <circle cx="12" cy="7.8" r="0.6" fill="currentColor" />
  </Svg>
);

export const GearIcon = (p: IconProps) => (
  <Svg {...p}>
    <circle cx="12" cy="12" r="3" />
    <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06A1.65 1.65 0 0 0 4.68 15a1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06A1.65 1.65 0 0 0 9 4.68a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06A1.65 1.65 0 0 0 19.4 9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1Z" />
  </Svg>
);

export const BellIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M18 9.5a6 6 0 1 0-12 0c0 6-2.5 7.5-2.5 7.5h17S18 15.5 18 9.5Z" />
    <path d="M10.3 20.5a2 2 0 0 0 3.4 0" />
  </Svg>
);

export const XIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M6 6l12 12M18 6 6 18" />
  </Svg>
);

/** Filled bell, for the ring / silent pill. */
export const BellFilledIcon = (p: IconProps) => (
  <Svg {...p} fill="currentColor" stroke="none">
    <path d="M12 2.5a6.5 6.5 0 0 0-6.5 6.5c0 5.2-2.2 6.9-2.6 7.2A1 1 0 0 0 3.5 18h17a1 1 0 0 0 .6-1.8c-.4-.3-2.6-2-2.6-7.2A6.5 6.5 0 0 0 12 2.5Z" />
    <path d="M9.5 19.5h5a2.5 2.5 0 0 1-5 0Z" />
  </Svg>
);

/** Filled bell with a slash, for silent. */
export const BellSlashIcon = (p: IconProps) => (
  <Svg {...p}>
    <path
      d="M12 2.5a6.5 6.5 0 0 0-6.5 6.5c0 5.2-2.2 6.9-2.6 7.2A1 1 0 0 0 3.5 18h17a1 1 0 0 0 .6-1.8c-.4-.3-2.6-2-2.6-7.2A6.5 6.5 0 0 0 12 2.5Z"
      fill="currentColor"
      stroke="none"
    />
    <path d="M9.5 19.5h5a2.5 2.5 0 0 1-5 0Z" fill="currentColor" stroke="none" />
    <path d="M3 3l18 18" stroke="#000" strokeWidth={4.5} />
    <path d="M3 3l18 18" strokeWidth={2.2} />
  </Svg>
);

export const VideoIcon = (p: IconProps) => (
  <Svg {...p}>
    <rect x="2.5" y="6" width="13" height="12" rx="3" />
    <path d="M15.5 10.5 21 7.5v9l-5.5-3" />
  </Svg>
);

export const ChevronLeftIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M14.5 6 8.5 12l6 6" />
  </Svg>
);

export const ChevronRightIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="m9.5 6 6 6-6 6" />
  </Svg>
);

export const CheckIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="m5 12.5 4.5 4.5L19 7.5" />
  </Svg>
);

/** A sticky note with a folded corner and two lines of text. */
export const NoteIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M5 3.5h14a2 2 0 0 1 2 2V14l-6.5 6.5H5a2 2 0 0 1-2-2V5.5a2 2 0 0 1 2-2Z" />
    <path d="M21 14h-4.5a2 2 0 0 0-2 2v4.5" />
    <path d="M7 8.5h10M7 12.5h5" />
  </Svg>
);

/** A push-pin; pass `fill="currentColor"` for the pinned state. */
export const PinIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M9 3.5h6l-1 5.5 3 3.2V14H7v-1.8L10 9 9 3.5Z" />
    <path d="M12 14v6.5" />
  </Svg>
);

export const CopyIcon = (p: IconProps) => (
  <Svg {...p}>
    <rect x="9" y="9" width="12" height="12" rx="2.5" />
    <path d="M5.5 15H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h8a2 2 0 0 1 2 2v.5" />
  </Svg>
);

export const TrashIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M3.5 6.5h17" />
    <path d="M9 6.5V4.5a1 1 0 0 1 1-1h4a1 1 0 0 1 1 1v2" />
    <path d="m18.5 6.5-.8 12.4a2 2 0 0 1-2 1.6H8.3a2 2 0 0 1-2-1.6L5.5 6.5" />
    <path d="M10 11v5.5M14 11v5.5" />
  </Svg>
);
