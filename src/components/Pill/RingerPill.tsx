import { color, compact, ringer } from "../../design/tokens";
import { uiDirection } from "../../design/direction";
import { t } from "../../lib/i18n";
import type { Ringer } from "../../lib/island/state";
import { layerFade, ringerSize } from "./animations";
import { IslandLayer } from "./IslandLayer";
import { BellFilledIcon, BellSlashIcon } from "./ui/icons";

/** "Ring" / "Silent" as a screen reader hears it. */
export function ringerLabel(ringer: Ringer): string {
  return t(ringer.silent ? "ringer.silent" : "ringer.ring");
}

/**
 * The ring / silent pill (as on a phone's side switch): a white bell and "Ring", or a grey slashed
 * bell and "Silent". The island itself takes the click (PillShell toggles); this only draws.
 *
 * It is a compact-family capsule (176 x 36, text inset 12). The bell sits at the leading edge and the
 * word at the trailing edge (the layer's `dir` is the UI language's). The word is the headline role
 * (15/600): the same weight as the compact clock, so the two pills are one family, and one step
 * above the label role because it is the pill's only text and is read at a glance.
 */
export function RingerPill({ ringer: state }: { ringer: Ringer }) {
  // Silent is a state the person chose, not a danger: a calm grey (never the destructive red).
  const tint = state.silent ? color.fgSecondary : color.fg;
  return (
    <IslandLayer
      name="ringer"
      fade={layerFade.temporary}
      size={ringerSize()}
      dir={uiDirection()}
      className="flex items-center justify-between select-none pointer-events-none"
      style={{ paddingInline: compact.paddingX, color: tint }}
      title={state.phase === "start" ? t("ringer.hint") : undefined}
    >
      {state.silent ? <BellSlashIcon size={ringer.icon} /> : <BellFilledIcon size={ringer.icon} />}
      <span className="bidi text-headline whitespace-nowrap">{ringerLabel(state)}</span>
    </IslandLayer>
  );
}
