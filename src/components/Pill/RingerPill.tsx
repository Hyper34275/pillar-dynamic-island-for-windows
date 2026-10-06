import { t } from "../../lib/i18n";
import type { Ringer } from "../../lib/island/state";
import { layerFade, ringerSize } from "./animations";
import { IslandLayer } from "./IslandLayer";
import { BellFilledIcon, BellSlashIcon } from "./ui/icons";
import { SYSTEM_COLORS } from "./ui/primitives";

/** "Ring" / "Silent" as a screen reader hears it. */
export function ringerLabel(ringer: Ringer): string {
  return t(ringer.silent ? "ringer.silent" : "ringer.ring");
}

/**
 * The ring / silent pill (as on a phone's side switch): a white bell and "Ring", or a red slashed
 * bell and "Silent". The island itself takes the click (PillShell toggles); this only draws.
 */
export function RingerPill({ ringer }: { ringer: Ringer }) {
  const color = ringer.silent ? SYSTEM_COLORS.red : "#f5f5f7";
  return (
    <IslandLayer
      name="ringer"
      fade={layerFade.temporary}
      size={ringerSize()}
      dir="ltr"
      className="flex items-center justify-between select-none pointer-events-none"
      style={{ paddingInline: 18, color }}
      title={ringer.phase === "start" ? t("ringer.hint") : undefined}
    >
      {ringer.silent ? <BellSlashIcon size={22} /> : <BellFilledIcon size={21} />}
      <span className="text-[17px] font-semibold tracking-tight" dir="auto">
        {ringerLabel(ringer)}
      </span>
    </IslandLayer>
  );
}
