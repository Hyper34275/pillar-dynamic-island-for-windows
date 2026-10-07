import { useEffect, type ReactNode } from "react";
import { color, control } from "../../design/tokens";
import { t } from "../../lib/i18n";
import { doNotDisturb, useDoNotDisturb, type DoNotDisturb } from "../../lib/island/dnd";
import { RoundButton } from "./ui/controls";
import { BellIcon, BellSlashIcon } from "./ui/icons";

/**
 * A tab's header action ("נקה הכל"): a plain text button at the trailing edge of the header row,
 * label role in the accent colour. Its visible box is the text; `hit-area` extends the pointer
 * target to 44px around it without changing the row. States: hover and pressed dim the label,
 * keyboard focus draws the global ring around its own radius, disabled is 40%.
 */
export function HeaderActionButton({ children, onPress, disabled }: { children: ReactNode; onPress: () => void; disabled?: boolean }) {
  return (
    <button
      type="button"
      className="ci-link hit-area rounded-control text-label text-accent whitespace-nowrap select-none transition-opacity duration-150 hover:opacity-80 active:opacity-60 disabled:opacity-40 disabled:cursor-default"
      disabled={disabled}
      onClick={(e) => {
        e.stopPropagation();
        onPress();
      }}
    >
      {children}
    </button>
  );
}

/**
 * The Windows "Do not disturb" bell, at the very trailing corner of the header row on every tab
 * (a tab's own action sits before it). A round toggle: an outline bell while Windows notifications
 * ring, a slashed bell on a tinted fill while they are muted. It re-reads Windows each time the
 * island opens (quick settings may have changed it); no bell when the state can't be read.
 */
export function DoNotDisturbButton({ store = doNotDisturb }: { store?: DoNotDisturb }) {
  const on = useDoNotDisturb(store);
  useEffect(() => {
    void store.refresh();
  }, [store]);
  if (on === null) return null;
  const label = t(on ? "dnd.turnOff" : "dnd.turnOn");
  return (
    <RoundButton
      onPress={() => void store.toggle()}
      ariaLabel={label}
      title={label}
      pressed={on}
      fill={on ? "tint" : "none"}
      tint={on ? color.destructiveText : color.fgSecondary}
      buttonProps={{ "data-dnd": on ? "on" : "off" }}
    >
      {on ? <BellSlashIcon size={control.iconSize} /> : <BellIcon size={control.iconSize} />}
    </RoundButton>
  );
}
