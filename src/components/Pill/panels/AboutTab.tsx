import { useEffect, useState } from "react";
import { useSystemInfo } from "../../../hooks/useSystemInfo";
import { fullDate, timeParts } from "../../../lib/dateFormat";
import { t } from "../../../lib/i18n";
import { ipc } from "../../../lib/ipc";
import { color, space, type as typeRoles } from "../../../design/tokens";

// The About tab is the one hero screen: the clock and the two values IT reads out over the phone.
// Their sizes are not roles but multiples of the 4-pt unit (7 x 4 = 28, 10 x 4 = 40, 30 x 4 = 120).
const HERO_VALUE_SIZE = space[1] * 7;
const HERO_TIME_SIZE = space[1] * 10;
const CLOCK_SIZE = space[1] * 30;

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
      <circle cx={50} cy={50} r={48.5} fill={color.surface} stroke={color.fillHover} strokeWidth={0.6} />
      {TICKS.map((i) => {
        const hour = i % 5 === 0;
        return (
          <line
            key={i}
            x1={50}
            y1={hour ? 6.5 : 6}
            x2={50}
            y2={hour ? 13 : 8.8}
            stroke={hour ? color.fg : color.fgQuaternary}
            strokeWidth={hour ? 1.8 : 0.7}
            strokeLinecap="round"
            transform={`rotate(${i * 6} 50 50)`}
          />
        );
      })}
      <Hand angle={h * 30 + m * 0.5} length={23} width={3.4} color={color.fg} />
      <Hand angle={m * 6 + s * 0.1} length={34} width={2.3} color={color.fg} />
      <Hand angle={s * 6} length={38} tail={9} width={0.9} color={color.warning} />
      <circle cx={50} cy={50} r={2.4} fill={color.warning} />
      <circle cx={50} cy={50} r={0.9} fill={color.island} />
    </svg>
  );
}

function Clock({ now }: { now: Date }) {
  const { digits, period } = timeParts(now);
  const seconds = String(now.getSeconds()).padStart(2, "0");
  return (
    <div className="flex items-center justify-center gap-5 py-1" role="timer" aria-label={t("about.time")}>
      <AnalogClock now={now} size={CLOCK_SIZE} />
      <div className="flex flex-col min-w-0" style={{ fontVariantNumeric: "tabular-nums" }}>
        <div className="flex items-baseline gap-1 leading-none text-fg" style={{ fontWeight: typeRoles.title.weight }}>
          {/* Digits are an LTR run in either layout direction. */}
          <span className="tracking-tight" style={{ direction: "ltr", unicodeBidi: "isolate", fontSize: HERO_TIME_SIZE }}>
            {digits}
          </span>
          <span className="text-title text-fg-tertiary">{seconds}</span>
          {period && <span className="text-headline text-fg-secondary">{period}</span>}
        </div>
        <p className="bidi mt-2 text-body text-fg-secondary">{fullDate(now)}</p>
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
      disabled={!copyable}
      className="hit-area group flex flex-col items-center gap-1 px-3 py-2 mx-auto max-w-full text-center min-w-0 rounded-surface hover:bg-surface-hover disabled:hover:bg-transparent transition-colors"
      title={copyable ? t("about.copyHint") : undefined}
      aria-label={`${label}: ${value}. ${t("about.copyHint")}`}
      onClick={copy}
    >
      <span className="text-micro transition-colors" style={{ color: copied ? color.positive : color.fgTertiary }} aria-live="polite">
        {copied ? t("about.copied") : label}
      </span>
      {/* An IP or a computer name is an LTR run whatever the layout. */}
      <span
        dir="ltr"
        className="bidi !text-center max-w-full leading-tight tracking-tight text-fg truncate tabular-nums"
        style={{ fontSize: HERO_VALUE_SIZE, fontWeight: typeRoles.title.weight }}
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
    <div className="flex-1 flex flex-col justify-center gap-4">
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
