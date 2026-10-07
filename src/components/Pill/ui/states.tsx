import type { ReactNode } from "react";
import { uiDirection } from "../../../design/direction";
import { compact, icon, toast } from "../../../design/tokens";
import { ActionButton } from "./controls";
import { InfoIcon } from "./icons";

// =============================================================================
// Empty and error states: ONE grammar for "there is nothing to show" (a list without items, a
// calendar that is off) and "this could not be shown" (a failed load, a crashed tab).
//
//     [glyph 24, tertiary]      calm, compact: never a big dark box, never a card
//     Title (headline, primary)
//     Hint  (body, secondary)    one short sentence, optional
//     [ action ]                 only when the person can do something about it
//
// Placement rule: the block is centred in the tab's content area (both axes, via `flex-1` of the
// tab root) and nudged a quarter-row up by the bottom padding, which is where the eye expects
// "nothing here" in a panel whose dock sits below. It has no size of its own, so it can never
// change the island's geometry; it only fills the box the panel already has.
// An error is the same shape with a neutral glyph: no red, no alarm, only what happened and what to do.
// =============================================================================

/** Glyph size of a state (tokens.icon.state). */
export const STATE_ICON = icon.state;

interface StateAction {
  label: string;
  onPress: () => void;
}

interface StateProps {
  /** A 24px glyph from ui/icons (it inherits the tertiary colour). */
  icon: ReactNode;
  title: string;
  /** One short sentence. */
  hint?: string;
  /** Only when the person can act on it (retry, open the app). */
  action?: StateAction;
  /** A small error code under the text (support). */
  code?: string;
  role?: "status" | "alert";
}

function StateBlock({ icon, title, hint, action, code, role }: StateProps) {
  // No entrance animation: it appears with its tab, whose fade is part of the island's transition.
  // `!text-center`: the bidi class aligns by paragraph direction, a centred block must win.
  return (
    <div role={role} data-state className="flex-1 min-h-0 flex flex-col items-center justify-center text-center gap-1 px-panel-inset pb-6">
      <span className="flex text-fg-tertiary mb-1" aria-hidden="true">
        {icon}
      </span>
      <span className="bidi !text-center text-headline text-fg">{title}</span>
      {hint && <span className="bidi !text-center text-body text-fg-secondary">{hint}</span>}
      {code && <span className="text-micro text-fg-tertiary tabular-nums">{code}</span>}
      {action && (
        <div className="mt-3 w-full" style={{ maxWidth: toast.minWidth }}>
          <ActionButton variant="neutral" onPress={action.onPress} className="w-full">
            {action.label}
          </ActionButton>
        </div>
      )}
    </div>
  );
}

/** Nothing to show (yet). `action` only when there is something to do about it. */
export function EmptyState(props: Omit<StateProps, "role" | "code">) {
  return <StateBlock {...props} role="status" />;
}

/** Could not be shown. Same grammar, neutral tone; `action` is normally a retry. */
export function ErrorState({ icon = <InfoIcon size={STATE_ICON} />, ...rest }: Omit<StateProps, "icon" | "role"> & { icon?: ReactNode }) {
  return <StateBlock icon={icon} {...rest} role="alert" />;
}

/**
 * The error state of the COLLAPSED island (CrashBoundary): the window is only as big as the compact
 * island, so the same grammar (glyph, title, code, retry) is one line in a compact-height capsule.
 * The whole capsule is the retry button.
 */
export function ErrorPill({ title, code, tooltip, onPress }: { title: string; code?: string; tooltip: string; onPress: () => void }) {
  return (
    <button
      type="button"
      onClick={onPress}
      className="rounded-full bg-island text-fg-secondary text-label flex items-center"
      style={{ height: compact.height, paddingInline: compact.paddingX, gap: compact.gap }}
      title={tooltip}
      dir={uiDirection()}
    >
      <span className="flex text-fg-tertiary" aria-hidden="true">
        <InfoIcon size={icon.medium} />
      </span>
      <span className="bidi">{title}</span>
      {code && <span className="text-fg-tertiary text-meta tabular-nums">{code}</span>}
    </button>
  );
}
