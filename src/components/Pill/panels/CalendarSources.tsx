import { useState } from "react";
import { sourceDirection, textDirection } from "../../../design/direction";
import { color, compact } from "../../../design/tokens";
import { t } from "../../../lib/i18n";
import { sourceCounts, sourceStatusKeys } from "../../../lib/calendar/sources";
import type { CalendarSourceDto, CalendarSourcesReport } from "../../../lib/calendar/types";
import { GROUP_CLASS } from "../ui/primitives";

/** One calendar: a mark (filled = contributes events), its name, and what it is / why it is quiet. */
function SourceRow({ source }: { source: CalendarSourceDto }) {
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
          background: source.active ? (source.state === "unavailable" ? color.warning : color.positive) : "transparent",
          boxShadow: source.active ? undefined : `inset 0 0 0 1.5px ${color.fgTertiary}`,
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
    </li>
  );
}

/**
 * Which calendars the island found in Outlook and which it follows, for visibility and support only:
 * selection happens in Outlook. Collapsed to one summary line by default.
 */
export function CalendarSources({ report, defaultOpen = false }: { report: CalendarSourcesReport; defaultOpen?: boolean }) {
  const [open, setOpen] = useState(defaultOpen);
  const { total, active } = sourceCounts(report);
  const note =
    report.selection === "remembered" ? t("calendar.sourcesRemembered") : report.selection === "primaryOnly" ? t("calendar.sourcesPrimaryOnly") : t("calendar.sourcesHint");
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
            {report.sources.map((source) => (
              <SourceRow key={source.id} source={source} />
            ))}
          </ul>
          <p className="bidi px-3 text-micro text-fg-tertiary">{note}</p>
        </>
      )}
    </section>
  );
}
