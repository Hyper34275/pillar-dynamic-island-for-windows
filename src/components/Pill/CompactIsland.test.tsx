// @vitest-environment jsdom
import { renderToStaticMarkup } from "react-dom/server";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { CalendarEventDto } from "../../lib/calendar/types";
import type { MeetingStatus } from "../../lib/calendar/meetingStatus";
import type { IslandDisplay } from "../../lib/ipc";
import { badgeWidth, pillDimensions } from "./animations";
import { CompactIsland } from "./CompactIsland";
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

/** The unseen indicator inside a rendered layer: the only element with the blue dot or capsule fill. */
const indicatorOf = (layer: HTMLElement) =>
  [...layer.querySelectorAll<HTMLElement>("span")].find((s) => /rgb\(10, 132, 255\)|rgba\(10, 132, 255, 0\.24\)/.test(s.style.background));

describe("CompactIsland, display date (the look before the clock)", () => {
  it("puts the date on the left and the weekday on the right, whatever the language", () => {
    for (const weekday of ["Tuesday", "יום שלישי", "الثلاثاء"]) {
      const markup = html({ weekday });
      expect(markup.indexOf("6.10")).toBeLessThan(markup.indexOf(weekday));
      expect(markup).toContain('dir="ltr"');
    }
  });

  it("isolates each label as LTR and lets the text inside choose its own direction", () => {
    const markup = html({ weekday: "יום שלישי" });
    expect(markup.match(/direction:ltr;unicode-bidi:isolate/g)).toHaveLength(2);
    expect(markup.match(/<span dir="auto">/g)).toHaveLength(2);
  });

  it("shows no clock", () => {
    const markup = html();
    expect(markup).not.toContain("14:35");
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

  it("sets the clock small (12px, 600, secondary) on the date's baseline", () => {
    const layer = dom({ display: "full" });
    const clock = [...layer.querySelectorAll<HTMLElement>("span")].find((s) => s.textContent === "14:35" && s.style.fontSize === "")!.parentElement!;
    expect(clock.style.fontSize).toBe("12px");
    expect(clock.style.fontVariantNumeric).toBe("tabular-nums");
    expect(clock.style.direction).toBe("ltr");
    expect(clock.style.unicodeBidi).toBe("isolate");
    expect(clock.style.minWidth).toBe(`${timeWidth}px`);
    expect(clock.getAttribute("aria-hidden")).toBe("true");
    expect(clock.className).toContain("font-semibold");
    expect(clock.parentElement!.className).toContain("items-baseline");
  });

  it("has no period in a 24-hour region and a smaller one after the digits in a 12-hour one", () => {
    expect(dom({ display: "full" }).querySelectorAll("span[dir=auto]")).toHaveLength(2); // date + weekday only
    const layer = dom({ display: "full", time: time12 });
    const period = [...layer.querySelectorAll<HTMLElement>("span")].find((s) => s.textContent === "PM")!;
    expect(period.style.fontSize).toBe("11px");
    expect(period.previousElementSibling!.textContent).toBe("2:35");
  });
});

describe("CompactIsland, display clock", () => {
  it("shows only the time, promoted to 15px", () => {
    const layer = dom({ display: "clock" });
    expect(layer.textContent).toBe("14:35");
    const clock = layer.querySelector<HTMLElement>("span")!;
    expect(clock.style.fontSize).toBe("15px");
    expect(layer.className).toContain("justify-center");
  });

  it("keeps a 12-hour period at 11px", () => {
    const layer = dom({ display: "clock", time: time12 });
    expect(layer.textContent).toBe("2:35PM");
    expect([...layer.querySelectorAll<HTMLElement>("span")].find((s) => s.textContent === "PM")!.style.fontSize).toBe("11px");
  });
});

describe("CompactIsland, meeting status", () => {
  it("shows the status and not the clock in every display, with the colour, progress bar and bell", () => {
    for (const display of ["full", "clock", "date"] as const) {
      const markup = html({ display, status: nowStatus, statusText: "In a meeting until 13:30", silent: true });
      expect(markup).toContain("In a meeting until 13:30");
      expect(markup).toContain("background:#A34E78");
      expect(markup).toContain("width:50%");
      expect(markup).not.toContain("14:35");
      expect(markup).not.toContain("6.10");
    }
  });

  it("is wider for a meeting status, up to its own limit", () => {
    expect(compactContentSize(labelsFor("date"), 2000, 0, false).width).toBe(pillDimensions.compactMeeting.maxWidth);
  });
});

describe("CompactIsland, unseen indicator", () => {
  const b = pillDimensions.badge;

  it("is absent for 0", () => {
    for (const display of ["full", "clock", "date"] as const) expect(indicatorOf(dom({ display }))).toBeUndefined();
  });

  it("is an 8x8 blue dot for one, with no number", () => {
    const dot = indicatorOf(dom({ unseen: 1 }))!;
    expect(dot.style.width).toBe("8px");
    expect(dot.style.height).toBe("8px");
    expect(dot.textContent).toBe("");
    expect(dot.getAttribute("aria-hidden")).toBe("true");
  });

  it("becomes a 16px capsule with the count from two, and 9+ in a 24px one from ten", () => {
    const small = indicatorOf(dom({ unseen: 3 }))!;
    expect(small.textContent).toBe("3");
    expect(small.style.width).toBe("16px");
    expect(small.style.height).toBe("16px");
    expect(small.style.fontSize).toBe("11px");
    const wide = indicatorOf(dom({ unseen: 12 }))!;
    expect(wide.textContent).toBe("9+");
    expect(wide.style.width).toBe("24px");
    expect(indicatorOf(dom({ unseen: 9 }))!.textContent).toBe("9");
  });

  it("is never red, hung over an edge or positioned absolutely", () => {
    for (const unseen of [1, 3, 12]) {
      const markup = html({ unseen, display: "full" });
      expect(markup.toLowerCase()).not.toContain("#ff453a");
      expect(markup.toLowerCase()).not.toContain("255, 69, 58");
      const el = indicatorOf(dom({ unseen }))!;
      expect(el.className).not.toContain("absolute");
      expect(el.style.position).toBe("");
    }
  });

  it("is the last item of the row in every layout, after the weekday, the time or the bell", () => {
    const lastOf = (layer: HTMLElement) => {
      const row = indicatorOf(layer)!.parentElement!;
      return row;
    };
    // date and full: the weekday and the indicator share the trailing group, the indicator last.
    for (const display of ["date", "full"] as const) {
      const layer = dom({ display, unseen: 3 });
      const group = lastOf(layer);
      expect(group.lastElementChild).toBe(indicatorOf(layer));
      expect(group.textContent).toBe("Tuesday3");
    }
    const clock = dom({ display: "clock", unseen: 3 });
    expect(lastOf(clock)).toBe(clock);
    expect(clock.lastElementChild).toBe(indicatorOf(clock));
    const status = dom({ status: nowStatus, statusText: "Standup in 5 min", silent: true, unseen: 3 });
    expect(lastOf(status)).toBe(status);
    expect(status.lastElementChild).toBe(indicatorOf(status));
  });

  it("sits exactly badge.gap after the time in the clock display (the layer adds no gap of its own)", () => {
    for (const unseen of [1, 3, 12]) {
      const layer = dom({ display: "clock", unseen });
      const indicator = indicatorOf(layer)!;
      expect(indicator.parentElement).toBe(layer);
      const layerGap = parseFloat(layer.style.gap || "0");
      expect(layerGap + parseFloat(indicator.style.marginInlineStart)).toBe(b.gap);
    }
  });

  it("makes room for the indicator instead of squeezing the weekday", () => {
    const labels = labelsFor("date");
    const without = compactContentSize(labels, null, 0, false).width;
    expect(badgeWidth(0)).toBe(0);
    expect(badgeWidth(1)).toBe(b.gap + b.dot);
    expect(badgeWidth(1)).toBe(16);
    expect(badgeWidth(3)).toBe(24);
    expect(badgeWidth(12)).toBe(32);
    expect(compactContentSize(labels, null, 1, false).width - without).toBe(16);
    expect(compactContentSize(labels, null, 3, false).width - without).toBe(24);
    expect(compactContentSize(labels, null, 12, false).width - without).toBe(32);
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
  it("lets the clock island be narrower than the others, down to its own minimum", () => {
    const clock = compactContentSize(labelsFor("clock"), null, 0, false).width;
    expect(clock).toBe(pillDimensions.compact.clockMinWidth);
    expect(clock).toBe(88);
    expect(compactContentSize(labelsFor("date"), null, 0, false).width).toBeGreaterThanOrEqual(pillDimensions.compact.minWidth);
  });

  it("does not widen a clock island for a dot while its content is below the minimum, and grows only past it", () => {
    const clock = labelsFor("clock");
    const base = compactContentSize(clock, null, 0, false).width;
    expect(compactContentSize(clock, null, 1, false).width).toBe(base);
    expect(compactContentSize(clock, null, 3, false).width).toBe(timeWidth + 24 + 30); // a capsule is past the minimum
    const wide = { ...clock, contentWidth: 73 }; // a 12-hour clock with its period
    expect(compactContentSize(wide, null, 0, false).width).toBe(73 + 30);
    expect(compactContentSize(wide, null, 3, false).width).toBe(73 + 24 + 30);
  });
});
