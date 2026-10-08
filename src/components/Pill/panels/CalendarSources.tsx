import { useEffect, useState } from "react";
import { sourceDirection, textDirection } from "../../../design/direction";
import { color, compact } from "../../../design/tokens";
import { t } from "../../../lib/i18n";
import { ipc } from "../../../lib/ipc";
import { canSwitchSource, openRequests, sourceCounts, sourceStatusKeys, withRequest, type SwitchRequest } from "../../../lib/calendar/sources";
import type { CalendarSourceDto, CalendarSourcesReport } from "../../../lib/calendar/types";
import { GROUP_CLASS, Switch } from "../ui/primitives";

type SetSelected = (id: string, selected: boolean) => Promise<boolean>;

/** One calendar: a mark in its Outlook colour (filled = contributes events), its name, what it is / why it is quiet, and its switch. */
function SourceRow({ source, onSwitch }: { source: CalendarSourceDto; onSwitch: (source: CalendarSourceDto) => void }) {
  const name = source.name || t(sourceStatusKeys(source)[0]);
  const detail = sourceStatusKeys(source).map((key) => t(key));
  if (source.state === "unavailable" && source.errorCode) detail.push(source.errorCode);
  return (
    <li className="flex items-center gap-3 px-3 py-2" style={{ minHeight: 44 }}>
      <span
        className="ci-mark flex-shrink-0 rounded-full"
        style={{
          width: compact.statusDot,
          height: compact.statusDot,
          // Its colour in Outlook, the one its meetings carry; amber while it cannot be read.
          background: source.active ? (source.state === "unavailable" ? color.warning : (source.color ?? color.positive)) : "transparent",
          boxShadow: source.active ? undefined : `inset 0 0 0 1.5px ${source.color ?? color.fgTertiary}`,
        }}
        aria-hidden="true"
      />
      <div className="min-w-0 flex-1 flex flex-col">
        {/* Long corporate names end in an ellipsis; the full name is the tooltip. */}
        <span dir={sourceDirection(name)} className="bidi truncate text-body text-fg" title={name}>
          {name}
        </span>
        <span dir={textDirection(detail.join(" · "))} className="bidi truncate text-micro text-fg-tertiary">
          {detail.join(" · ")}
        </span>
      </div>
      {canSwitchSource(source) && <Switch checked={source.active} onChange={() => onSwitch(source)} label={name} />}
    </li>
  );
}

/**
 * Which calendars the island found in Outlook and which it follows. Each one but the default
 * calendar has a switch: the island follows at once, and Outlook's own checkbox follows when
 * Outlook shows its calendar. Collapsed to one summary line by default.
 */
export function CalendarSources({
  report,
  defaultOpen = false,
  setSelected = ipc.outlookSetCalendarSelected,
}: {
  report: CalendarSourcesReport;
  defaultOpen?: boolean;
  setSelected?: SetSelected;
}) {
  const [open, setOpen] = useState(defaultOpen);
  const [requests, setRequests] = useState<Record<string, SwitchRequest>>({});
  const [failed, setFailed] = useState<string | null>(null);
  // A new report settles every request it reflects.
  useEffect(() => setRequests((r) => openRequests(report, r, Date.now())), [report]);

  const sources = report.sources.map((s) => withRequest(s, requests[s.id]));
  const { total, active } = sourceCounts({ ...report, sources });
  const onSwitch = (source: CalendarSourceDto) => {
    const on = !source.active;
    setFailed(null);
    setRequests((r) => ({ ...r, [source.id]: { on, atMs: Date.now() } }));
    void setSelected(source.id, on).then((ok) => {
      if (ok) return;
      setRequests(({ [source.id]: _, ...rest }) => rest);
      setFailed(t("calendar.sourceSwitchFailed", { name: source.name }));
    });
  };
  const note =
    failed ??
    (report.selection === "remembered" ? t("calendar.sourcesRemembered") : report.selection === "primaryOnly" ? t("calendar.sourcesPrimaryOnly") : t("calendar.sourcesHint"));
  return (
    <section aria-label={t("calendar.sources")} className="flex flex-col gap-1">
      <button
        type="button"
        className="flex items-baseline justify-between gap-2 px-3 text-start rounded-control focus-visible:outline-none focus-visible:ring-2"
        style={{ minHeight: 32 }}
        aria-expanded={open}
        onClick={() => setOpen((v) => !v)}
      >
        <span className="text-micro text-fg-tertiary truncate">{t("calendar.sources")}</span>
        <span className="flex-shrink-0 text-micro text-fg-tertiary tabular-nums">{t("calendar.sourcesSummary", { active, total })}</span>
      </button>
      {open && (
        <>
          <ul className={GROUP_CLASS}>
            {sources.map((source) => (
              <SourceRow key={source.id} source={source} onSwitch={onSwitch} />
            ))}
          </ul>
          <p className="bidi px-3 text-micro text-fg-tertiary" role={failed ? "status" : undefined}>
            {note}
          </p>
        </>
      )}
    </section>
  );
}
