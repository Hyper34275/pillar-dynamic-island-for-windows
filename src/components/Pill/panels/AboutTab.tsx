import { useEffect, useState } from "react";
import { useSystemInfo } from "../../../hooks/useSystemInfo";
import { fullDate, timeParts } from "../../../lib/dateFormat";
import { t } from "../../../lib/i18n";
import { ipc } from "../../../lib/ipc";
import { SYSTEM_COLORS } from "../ui/primitives";

const NONE = "—";

/** Re-renders on every new wall-clock second while mounted (About is open); nothing runs otherwise. */
function useSecond(): Date {
  const [now, setNow] = useState(() => new Date());
  useEffect(() => {
    let handle: ReturnType<typeof setTimeout>;
    const tick = () => {
      const at = new Date();
      setNow(at);
      // Re-aligned every time, so the hand never drifts off the real second.
      handle = setTimeout(tick, 1000 - at.getMilliseconds());
    };
    handle = setTimeout(tick, 1000 - new Date().getMilliseconds());
    return () => clearTimeout(handle);
  }, []);
  return now;
}

const TICKS = Array.from({ length: 60 }, (_, i) => i);

function Hand({ angle, length, tail = 0, width, color }: { angle: number; length: number; tail?: number; width: number; color: string }) {
  return (
    <line
      x1={50}
      y1={50 + tail}
      x2={50}
      y2={50 - length}
      stroke={color}
      strokeWidth={width}
      strokeLinecap="round"
      transform={`rotate(${angle} 50 50)`}
    />
  );
}

function AnalogClock({ now, size }: { now: Date; size: number }) {
  const s = now.getSeconds();
  const m = now.getMinutes();
  const h = now.getHours() % 12;
  return (
    <svg width={size} height={size} viewBox="0 0 100 100" aria-hidden="true" focusable="false" className="flex-shrink-0">
      <circle cx={50} cy={50} r={48.5} fill="rgba(255,255,255,0.06)" stroke="rgba(255,255,255,0.12)" strokeWidth={0.6} />
      {TICKS.map((i) => {
        const hour = i % 5 === 0;
        return (
          <line
            key={i}
            x1={50}
            y1={hour ? 6.5 : 6}
            x2={50}
            y2={hour ? 13 : 8.8}
            stroke={hour ? "rgba(255,255,255,0.85)" : "rgba(255,255,255,0.28)"}
            strokeWidth={hour ? 1.8 : 0.7}
            strokeLinecap="round"
            transform={`rotate(${i * 6} 50 50)`}
          />
        );
      })}
      <Hand angle={h * 30 + m * 0.5} length={23} width={3.4} color="#f5f5f7" />
      <Hand angle={m * 6 + s * 0.1} length={34} width={2.3} color="#f5f5f7" />
      <Hand angle={s * 6} length={38} tail={9} width={0.9} color={SYSTEM_COLORS.orange} />
      <circle cx={50} cy={50} r={2.4} fill={SYSTEM_COLORS.orange} />
      <circle cx={50} cy={50} r={0.9} fill="#000" />
    </svg>
  );
}

function Clock({ now }: { now: Date }) {
  const { digits, period } = timeParts(now);
  const seconds = String(now.getSeconds()).padStart(2, "0");
  return (
    <div dir="ltr" className="flex items-center justify-center gap-5 py-1" role="timer" aria-label={t("about.time")}>
      <AnalogClock now={now} size={118} />
      <div className="flex flex-col min-w-0" style={{ fontVariantNumeric: "tabular-nums" }}>
        <div className="flex items-baseline gap-1 leading-none text-white">
          <span className="text-[40px] font-semibold tracking-tight" style={{ direction: "ltr", unicodeBidi: "isolate" }}>
            {digits}
          </span>
          <span className="text-[17px] font-semibold text-white/40">{seconds}</span>
          {period && <span className="ml-1 text-[15px] font-semibold text-white/55">{period}</span>}
        </div>
        <p className="mt-2 text-[13px] font-medium text-white/55 leading-snug" style={{ unicodeBidi: "isolate" }}>
          <span dir="auto">{fullDate(now)}</span>
        </p>
      </div>
    </div>
  );
}

const COPIED_MS = 1500;

/** A big value IT reads out over the phone; a click copies it (the label says "Copied" for a moment). */
function BigValue({ label, value, copyable, onCopy }: { label: string; value: string; copyable: boolean; onCopy: (value: string) => Promise<boolean> }) {
  const [copied, setCopied] = useState(false);
  useEffect(() => {
    if (!copied) return;
    const handle = setTimeout(() => setCopied(false), COPIED_MS);
    return () => clearTimeout(handle);
  }, [copied]);
  const copy = () => {
    void onCopy(value).then((ok) => {
      if (ok) setCopied(true);
    });
  };
  return (
    <button
      type="button"
      dir="ltr"
      disabled={!copyable}
      className="group flex flex-col items-center gap-1 px-3 py-1 mx-auto max-w-full text-center min-w-0 rounded-[14px] hover:bg-white/[0.06] disabled:hover:bg-transparent transition-colors"
      title={copyable ? t("about.copyHint") : undefined}
      aria-label={`${label}: ${value}. ${t("about.copyHint")}`}
      onClick={copy}
    >
      <span
        className="text-[10.5px] font-semibold uppercase tracking-[0.08em] transition-colors"
        style={{ color: copied ? SYSTEM_COLORS.green : "rgba(255,255,255,0.4)" }}
        dir="auto"
        aria-live="polite"
      >
        {copied ? t("about.copied") : label}
      </span>
      <span
        className="max-w-full text-[28px] font-semibold leading-tight tracking-tight text-white truncate tabular-nums"
        dir="auto"
        style={{ unicodeBidi: "plaintext" }}
      >
        {value}
      </span>
    </button>
  );
}

export interface AboutViewProps {
  computerName: string | null;
  localIpv4: string | null;
  /** The time the clock shows; the caller decides how often it changes. */
  now: Date;
  /** Copies a value; resolves true when it did (the label then says "Copied"). */
  onCopy: (value: string) => Promise<boolean>;
}

/** Pure rendering of About (the tour renders it with mock data, no IPC). */
export function AboutView({ computerName, localIpv4, now, onCopy }: AboutViewProps) {
  return (
    <div dir="ltr" className="flex-1 flex flex-col justify-center gap-4">
      <BigValue label={t("about.computer")} value={computerName ?? NONE} copyable={!!computerName} onCopy={onCopy} />
      <Clock now={now} />
      <BigValue label={t("about.ip")} value={localIpv4 ?? NONE} copyable={!!localIpv4} onCopy={onCopy} />
    </div>
  );
}

/** What IT asks for first: this computer's name and IP, with the time between them. */
export function AboutTab() {
  const { info } = useSystemInfo();
  const now = useSecond();
  return <AboutView computerName={info?.computerName ?? null} localIpv4={info?.localIpv4 ?? null} now={now} onCopy={ipc.copyTextToClipboard} />;
}
