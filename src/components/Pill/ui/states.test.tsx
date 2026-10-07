// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { CrashBoundary } from "../../CrashBoundary";
import { t } from "../../../lib/i18n";
import { WAITING_SNAPSHOT, type CalendarSnapshot } from "../../../lib/calendar/types";
import { CalendarView, DayView } from "../panels/CalendarTab";
import { TabBoundary } from "../TabBoundary";
import { CalendarIcon } from "./icons";
import { EmptyState, ErrorPill, ErrorState } from "./states";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let container: HTMLDivElement;
let root: Root;
beforeEach(() => {
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});
afterEach(() => {
  act(() => root.unmount());
  container.remove();
});
const NOW = Date.UTC(2026, 9, 6, 10, 0, 0);
const snap = (over: Partial<CalendarSnapshot>): CalendarSnapshot => ({ ...WAITING_SNAPSHOT, ...over });
const Boom = (): never => {
  throw new Error("boom");
};

describe("EmptyState / ErrorState", () => {
  it("an empty state is a status, an error state an alert, both centred and fill-only (no size of their own)", () => {
    act(() => root.render(<EmptyState icon={<CalendarIcon />} title="Nothing" hint="All clear" />));
    const empty = container.querySelector<HTMLElement>("[data-state]")!;
    expect(empty.getAttribute("role")).toBe("status");
    expect(empty.className).toContain("flex-1");
    expect(empty.textContent).toBe("NothingAll clear");
    expect(empty.querySelector("button")).toBeNull();

    const onPress = vi.fn();
    act(() => root.render(<ErrorState title="Broke" code="X-1" action={{ label: "Again", onPress }} />));
    const error = container.querySelector<HTMLElement>("[data-state]")!;
    expect(error.getAttribute("role")).toBe("alert");
    expect(error.textContent).toContain("X-1");
    act(() => container.querySelector("button")!.click());
    expect(onPress).toHaveBeenCalledTimes(1);
  });
});

describe("TabBoundary and CrashBoundary", () => {
  it("a crashed tab becomes an error state with its code and a retry, inside the same box", () => {
    const spy = vi.spyOn(console, "error").mockImplementation(() => {});
    act(() => root.render(<TabBoundary tab="x"><Boom /></TabBoundary>));
    const state = container.querySelector("[data-state]")!;
    expect(state.getAttribute("role")).toBe("alert");
    expect(state.textContent).toContain(t("island.unavailable"));
    expect([...container.querySelectorAll("button")].map((b) => b.textContent)).toEqual([t("island.tryAgain")]);
    spy.mockRestore();
  });

  it("the app crash fallback is the one-line error pill, and clicking it tries again", () => {
    const spy = vi.spyOn(console, "error").mockImplementation(() => {});
    act(() => root.render(<CrashBoundary><Boom /></CrashBoundary>));
    const pill = container.querySelector("button")!;
    expect(pill.textContent).toContain(t("island.unavailable"));
    expect(pill.getAttribute("title")).toBe(t("island.tryAgain"));
    expect(renderToStaticMarkup(<ErrorPill title="a" tooltip="b" onPress={() => {}} />)).toContain("rounded-full");
    spy.mockRestore();
  });
});

describe("calendar states", () => {
  const html = (node: React.ReactElement) => renderToStaticMarkup(node).replace(/&#x27;/g, "'");
  it("connected without events is an empty state", () => {
    const out = html(<CalendarView snapshot={snap({ status: "connected" })} nowMs={NOW} />);
    expect(out).toContain('role="status"');
    expect(out).toContain(t("calendar.noEvents"));
  });
  it("waiting / connecting are empty states (normal), the rest are error states with their code", () => {
    for (const status of ["waiting", "connecting"] as const) expect(html(<CalendarView snapshot={snap({ status })} nowMs={NOW} />)).toContain('role="status"');
    for (const status of ["newOutlookOnly", "elevationMismatch", "unresponsive", "failed"] as const) {
      const out = html(<CalendarView snapshot={snap({ status, errorCode: "OUTLOOK-102" })} nowMs={NOW} />);
      expect(out).toContain('role="alert"');
      expect(out).toContain("OUTLOOK-102");
    }
  });
  it("offers a retry only where a refresh can help", () => {
    const withRetry = (status: CalendarSnapshot["status"]) => html(<CalendarView snapshot={snap({ status })} nowMs={NOW} onRetry={() => {}} />).includes(t("island.tryAgain"));
    expect(withRetry("failed")).toBe(true);
    expect(withRetry("unresponsive")).toBe(true);
    expect(withRetry("newOutlookOnly")).toBe(false);
    expect(withRetry("waiting")).toBe(false);
  });
  it("a day that is loading is calm, one that failed is an error with a retry, and an Outlook outage wins over it", () => {
    expect(html(<DayView nowMs={NOW} day={{ state: "loading", events: null }} status="connected" />)).toContain('role="status"');
    const failed = html(<DayView nowMs={NOW} day={{ state: "error", events: null }} status="connected" onRetry={() => {}} />);
    expect(failed).toContain('role="alert"');
    expect(failed).toContain(t("calendar.dayFailed"));
    expect(failed).toContain(t("island.tryAgain"));
    expect(html(<DayView nowMs={NOW} day={{ state: "error", events: null }} status="waiting" />)).toContain(t("calendar.waiting"));
  });
});
