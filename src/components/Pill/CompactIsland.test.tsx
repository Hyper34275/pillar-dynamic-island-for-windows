// @vitest-environment jsdom
import { renderToStaticMarkup } from "react-dom/server";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { CalendarEventDto } from "../../lib/calendar/types";
import type { MeetingStatus } from "../../lib/calendar/meetingStatus";
import type { IslandDisplay } from "../../lib/ipc";
import { badge, compact, progress } from "../../design/tokens";
import { uiDirection } from "../../design/direction";
import { pillDimensions } from "./animations";
import { CompactIsland } from "./CompactIsland";
import { countBadgeWidth } from "./ui/identity";
import { compactContentSize, type CompactContent } from "./useCompactLayout";

// useReducedMotion is the OS preference: the tests flip it.
const motionPrefs = vi.hoisted(() => ({ reduced: false }));
vi.mock("motion/react", async (importOriginal) => ({
  ...(await importOriginal<typeof import("motion/react")>()),
  useReducedMotion: () => motionPrefs.reduced,
}));

beforeEach(() => {
  motionPrefs.reduced = false;
});

const timeWidth = 37;
const time24 = { digits: "14:35", period: null };
const time12 = { digits: "2:35", period: "PM" };

function labelsFor(display: IslandDisplay, weekday = "Tuesday") {
  return {
    date: "6.10",
    weekday: display === "clock" ? "" : weekday,
    contentWidth: display === "clock" ? timeWidth : 90,
    display,
    timeWidth: display === "date" ? 0 : timeWidth,
  };
}

function content(extra: Partial<CompactContent> & { display?: IslandDisplay; weekday?: string } = {}): CompactContent {
  const { display = "date", weekday, ...rest } = extra;
  const merged = { labels: labelsFor(display, weekday), status: null, statusText: null, unseen: 0, silent: false, time: time24, ariaLabel: "", ...rest };
  return { ...merged, size: compactContentSize(merged.labels, merged.statusText ? 150 : null, merged.unseen, merged.silent) };
}

const html = (extra: Parameters<typeof content>[0] = {}) => renderToStaticMarkup(<CompactIsland content={content(extra)} />);
const dom = (extra: Parameters<typeof content>[0] = {}) => {
  const host = document.createElement("div");
  host.innerHTML = html(extra);
  return host.querySelector<HTMLElement>('[data-layer="compact"]')!;
};

const meeting: CalendarEventDto = {
  id: "m",
  calendarId: "c",
  subject: "Standup",
  startUtc: "2026-10-06T10:00:00Z",
  endUtc: "2026-10-06T10:30:00Z",
  allDay: false,
  location: null,
  organizer: null,
  isRecurring: false,
  meetingUrl: null,
  busyStatus: "busy",
  responseStatus: "accepted",
  color: "#A34E78",
};
const nowStatus: MeetingStatus = { kind: "now", event: meeting, endMs: Date.parse(meeting.endUtc), progress: 0.5 };
const soonStatus = { kind: "soon", event: meeting, startMs: Date.parse(meeting.startUtc), minutes: 5 } as unknown as MeetingStatus;

/** The unseen indicator (CountBadge) inside a rendered layer: its inner element carries the accent colours. */
const indicatorOf = (layer: HTMLElement) => [...layer.querySelectorAll<HTMLElement>("span")].find((s) => /--ci-accent/.test(s.getAttribute("style") ?? ""));
/** What the badge adds to the island's width (gap + badge + endInset, minus the padding it replaces). */
const extraFor = (unseen: number) => compact.gap + countBadgeWidth(unseen) + badge.endInset - compact.paddingX;

describe("CompactIsland, direction", () => {
  it("lays the layer out in the UI language's direction", () => {
    for (const display of ["full", "clock", "date"] as const) expect(dom({ display }).getAttribute("dir")).toBe(uiDirection());
    expect(dom({ status: nowStatus, statusText: "Standup" }).getAttribute("dir")).toBe(uiDirection());
  });
});

describe("CompactIsland, display date (the look before the clock)", () => {
  it("puts the date on the left and the weekday on the right, whatever the language", () => {
    for (const weekday of ["Tuesday", "יום שלישי", "الثلاثاء"]) {
      const markup = html({ weekday });
      expect(markup.indexOf("6.10")).toBeLessThan(markup.indexOf(weekday));
    }
  });

  it("keeps the labels in one isolated LTR row and lets each text choose its own direction (bidi)", () => {
    const layer = dom({ weekday: "יום שלישי" });
    const row = layer.querySelector<HTMLElement>("span")!;
    expect(row.style.direction).toBe("ltr");
    expect(row.style.unicodeBidi).toBe("isolate");
    expect(layer.querySelectorAll("span.bidi")).toHaveLength(2);
    expect(html()).not.toContain('dir="auto"');
  });

  it("sets the date in the primary colour and the weekday in the secondary one, both label role", () => {
    const [date, weekday] = [...dom().querySelectorAll<HTMLElement>("span.bidi")];
    expect(date.className).toContain("text-label");
    expect(date.getAttribute("style")).toContain("--ci-fg)");
    expect(weekday.className).toContain("text-label");
    expect(weekday.getAttribute("style")).toContain("--ci-fg-secondary");
  });

  it("shows no clock", () => {
    expect(html()).not.toContain("14:35");
  });
});

describe("CompactIsland, display full", () => {
  it("shows date, clock, weekday in that physical order", () => {
    const markup = html({ display: "full" });
    const at = (text: string) => markup.indexOf(text);
    expect(at("6.10")).toBeGreaterThan(-1);
    expect(at("6.10")).toBeLessThan(at("14:35"));
    expect(at("14:35")).toBeLessThan(at("Tuesday"));
  });

  it("makes the clock the primary label (headline, primary colour) and the date and weekday secondary", () => {
    const layer = dom({ display: "full" });
    const clock = [...layer.querySelectorAll<HTMLElement>("span")].find((s) => s.textContent === "14:35" && s.className.includes("text-headline"))!;
    expect(clock.className).toContain("text-headline");
    expect(clock.className).toContain("tabular-nums");
    expect(clock.getAttribute("style")).toContain("--ci-fg)");
    expect(clock.style.direction).toBe("ltr");
    expect(clock.style.unicodeBidi).toBe("isolate");
    expect(clock.style.minWidth).toBe(`${timeWidth}px`);
    expect(clock.getAttribute("aria-hidden")).toBe("true");
    expect(clock.parentElement!.className).toContain("items-baseline");
    const [date, weekday] = [...layer.querySelectorAll<HTMLElement>("span.bidi")];
    expect(date.getAttribute("style")).toContain("--ci-fg-secondary");
    expect(weekday.getAttribute("style")).toContain("--ci-fg-secondary");
  });

  it("has no period in a 24-hour region and a 4px-apart meta-size one after the digits in a 12-hour one", () => {
    expect(dom({ display: "full" }).querySelectorAll("span.bidi")).toHaveLength(2); // date + weekday only
    const layer = dom({ display: "full", time: time12 });
    const period = [...layer.querySelectorAll<HTMLElement>("span")].find((s) => s.textContent === "PM")!;
    expect(period.className).toContain("text-meta");
    expect(period.style.marginInlineStart).toBe(`${compact.gapTight}px`);
    expect(period.previousElementSibling!.textContent).toBe("2:35");
  });
});

describe("CompactIsland, display clock", () => {
  it("shows only the time, as the headline label, centred", () => {
    const layer = dom({ display: "clock" });
    expect(layer.textContent).toBe("14:35");
    expect(layer.querySelector("span.text-headline")).not.toBeNull();
    expect(layer.querySelector(".justify-center")).not.toBeNull();
  });

  it("keeps a 12-hour period after the digits", () => {
    expect(dom({ display: "clock", time: time12 }).textContent).toBe("2:35PM");
  });
});

describe("CompactIsland, meeting status", () => {
  it("shows the status and not the clock in every display, with the colour, progress ring and a muted bell", () => {
    for (const display of ["full", "clock", "date"] as const) {
      const markup = html({ display, status: nowStatus, statusText: "In a meeting until 13:30", silent: true });
      expect(markup).toContain("In a meeting until 13:30");
      expect(markup).toContain(`stroke="#A34E78"`);
      expect(markup).toContain("stroke-dasharray");
      expect(markup).not.toContain("destructive");
      expect(markup).not.toContain("14:35");
      expect(markup).not.toContain("6.10");
    }
  });

  it("lays out dot, status text, bell, badge in that logical order, text on one line with an ellipsis", () => {
    const layer = dom({ status: nowStatus, statusText: "Standup in 5 min", silent: true, unseen: 3 });
    const group = layer.querySelector<HTMLElement>(".flex-1")!;
    const [dot, text, bell] = [...group.children] as HTMLElement[];
    // A meeting in progress: the dot is the progress ring (tokens.progress.ring), same slot.
    expect(dot.getAttribute("width")).toBe(String(progress.ring));
    expect(bell.style.color).toBe("var(--ci-muted)");
    expect(text.textContent).toBe("Standup in 5 min");
    expect(text.className).toContain("bidi");
    expect(text.className).toContain("text-ellipsis");
    expect(text.className).toContain("whitespace-nowrap");
    expect(bell.querySelector("svg")).not.toBeNull();
    expect(group.nextElementSibling).toBe(indicatorOf(layer)!.parentElement);
  });

  it("is wider for a meeting status, up to its own limit (badge included)", () => {
    expect(compactContentSize(labelsFor("date"), 2000, 0, false).width).toBe(pillDimensions.compactMeeting.maxWidth);
    expect(compactContentSize(labelsFor("date"), 2000, 12, false).width).toBe(pillDimensions.compactMeeting.maxWidth);
  });

  it("never lifts the text row: progress is a ring in the dot's slot, the dot stays a dot before the meeting starts", () => {
    for (const status of [nowStatus, soonStatus]) {
      const layer = dom({ status, statusText: "x", unseen: 3 });
      expect(parseFloat(layer.style.paddingBottom || "0")).toBe(0);
      expect(layer.querySelector("[style*='bottom']")).toBeNull(); // no bar anywhere
    }
    const ring = dom({ status: nowStatus, statusText: "x" }).querySelector("svg")!;
    expect(ring.getAttribute("width")).toBe("16");
    expect(dom({ status: soonStatus, statusText: "x" }).querySelector("svg")).toBeNull();
  });

  it("keeps the badge concentric with the end cap while a meeting is in progress (vertically and at the end)", () => {
    const layer = dom({ status: nowStatus, statusText: "x", unseen: 3 });
    const badgeSlot = indicatorOf(layer)!.parentElement!;
    expect(layer.style.paddingInlineEnd).toBe(`${badge.endInset}px`);
    expect(layer.classList.contains("items-center")).toBe(true); // centred on the 36px row, nothing shifts it
    expect(badgeSlot.style.height).toBe("20px");
  });

  it("sizes the island for the ring: 8 wider than for the soon-dot, same 36 height", () => {
    const labels = labelsFor("full");
    const dot = compactContentSize(labels, 100, 0, false, false);
    const ring = compactContentSize(labels, 100, 0, false, true);
    expect(ring.width - dot.width).toBe(progress.ring - compact.statusDot);
    expect(ring.height).toBe(36);
  });
});

describe("CompactIsland, unseen indicator (CountBadge)", () => {
  it("is absent for 0", () => {
    for (const display of ["full", "clock", "date"] as const) expect(indicatorOf(dom({ display }))).toBeUndefined();
  });

  it("is an 8x8 dot for one, with no number", () => {
    const dot = indicatorOf(dom({ unseen: 1 }))!;
    expect(dot.style.width).toBe("8px");
    expect(dot.style.height).toBe("8px");
    expect(dot.textContent).toBe("");
    expect(dot.parentElement!.getAttribute("aria-hidden")).toBe("true");
  });

  it("becomes a 20px capsule with the count from two, and 9+ in a 28px one from ten", () => {
    const small = indicatorOf(dom({ unseen: 3 }))!;
    expect(small.textContent).toBe("3");
    expect(small.style.width).toBe("20px");
    expect(small.style.height).toBe("20px");
    const wide = indicatorOf(dom({ unseen: 12 }))!;
    expect(wide.textContent).toBe("9+");
    expect(wide.style.width).toBe("28px");
    expect(indicatorOf(dom({ unseen: 9 }))!.textContent).toBe("9");
  });

  it("is never positioned absolutely", () => {
    for (const unseen of [1, 3, 12]) expect(indicatorOf(dom({ unseen }))!.parentElement!.className).not.toContain("absolute");
  });

  it("is the trailing item of the layer in every layout, badge.endInset from the end", () => {
    const cases = [
      dom({ display: "date", unseen: 3 }),
      dom({ display: "full", unseen: 3 }),
      dom({ display: "clock", unseen: 3 }),
      dom({ status: nowStatus, statusText: "Standup in 5 min", silent: true, unseen: 3 }),
    ];
    for (const layer of cases) {
      expect(layer.lastElementChild).toBe(indicatorOf(layer)!.parentElement);
      expect(layer.style.paddingInlineEnd).toBe(`${badge.endInset}px`);
      expect(layer.style.paddingInlineStart).toBe(`${compact.paddingX}px`);
      expect(layer.style.gap).toBe(`${compact.gap}px`);
    }
    expect(dom({ display: "full" }).style.paddingInlineEnd).toBe(`${compact.paddingX}px`);
  });

  it("makes room for the indicator: width = paddingX + content + gap + badge + endInset", () => {
    const labels = labelsFor("date");
    const without = compactContentSize(labels, null, 0, false).width;
    expect(without).toBe(labels.contentWidth + compact.paddingX * 2);
    expect(countBadgeWidth(0)).toBe(0);
    for (const [unseen, badgeW] of [[1, 20], [3, 20], [12, 28]] as const) {
      expect(compactContentSize(labels, null, unseen, false).width).toBe(compact.paddingX + labels.contentWidth + compact.gap + badgeW + badge.endInset);
      expect(compactContentSize(labels, null, unseen, false).width - without).toBe(extraFor(unseen));
    }
  });

  it("enters with a scale and fade, or with a fade alone under reduced motion", () => {
    expect(indicatorOf(dom({ unseen: 1 }))!.style.transform).toContain("scale(0.6)");
    motionPrefs.reduced = true;
    const calm = indicatorOf(dom({ unseen: 1 }))!;
    expect(calm.style.transform).not.toContain("scale");
    expect(calm.style.opacity).toBe("0");
  });
});

describe("compact size per display", () => {
  it("is 36 high with radius 18 in every state", () => {
    for (const unseen of [0, 1, 12]) {
      for (const display of ["full", "clock", "date"] as const) {
        expect(compactContentSize(labelsFor(display), null, unseen, false)).toMatchObject({ height: 36, radius: 18 });
      }
      expect(compactContentSize(labelsFor("date"), 100, unseen, true)).toMatchObject({ height: 36, radius: 18 });
    }
  });

  it("lets the clock island be narrower than the others, down to its own minimum", () => {
    const clock = compactContentSize(labelsFor("clock"), null, 0, false).width;
    expect(clock).toBe(pillDimensions.compact.clockMinWidth);
    expect(clock).toBe(72);
    expect(compactContentSize(labelsFor("date"), null, 0, false).width).toBeGreaterThanOrEqual(pillDimensions.compact.minWidth);
  });

  it("grows a clock island for a badge exactly by what the badge adds", () => {
    const clock = labelsFor("clock");
    expect(compactContentSize(clock, null, 1, false).width).toBe(compact.paddingX + timeWidth + compact.gap + 20 + badge.endInset);
    const wide = { ...clock, contentWidth: 73 }; // a 12-hour clock with its period
    expect(compactContentSize(wide, null, 0, false).width).toBe(73 + 24);
    expect(compactContentSize(wide, null, 3, false).width).toBe(73 + 24 + 24);
  });
});
