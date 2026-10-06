import { useMinute } from "../../../hooks/useClock";
import { fullDate, timeParts } from "../../../lib/dateFormat";

export function DatetimeTab() {
  const now = useMinute();
  const { digits, period } = timeParts(now);

  return (
    <div dir="ltr" className="flex-1 flex flex-col items-center justify-center gap-2 text-center">
      <div
        className="flex items-baseline justify-center gap-2 leading-none text-white"
        style={{ fontVariantNumeric: "tabular-nums" }}
      >
        <span className="text-[60px] font-semibold tracking-tight" style={{ direction: "ltr", unicodeBidi: "isolate" }}>
          {digits}
        </span>
        {period && <span className="text-[20px] font-semibold text-white/55">{period}</span>}
      </div>
      <p className="text-[15px] font-medium text-white/60" style={{ unicodeBidi: "isolate" }}>
        <span dir="auto">{fullDate(now)}</span>
      </p>
    </div>
  );
}
