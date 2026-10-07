import type { ReactNode } from "react";

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
