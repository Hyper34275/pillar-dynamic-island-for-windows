// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { AboutTab, AboutView } from "./AboutTab";

vi.mock("../../../lib/ipc", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../lib/ipc")>();
  return {
    ...original,
    ipc: {
      ...original.ipc,
      getSystemInfo: async () => ({
        computerName: "PC-042",
        localIpv4: "10.20.30.40",
        ipAdapter: "Ethernet",
        windowsUser: "CORP\\dana",
        sessionId: 1,
        osName: "Windows 11 Enterprise",
        osDisplayVersion: "24H2",
        osBuild: 26100,
        appVersion: "1.0.1",
        webview2Version: null,
      }),
      getDiagnostics: async () => null,
    },
  };
});

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout", "Date"] });
  vi.setSystemTime(new Date(2026, 9, 6, 15, 30, 12));
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});
afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.useRealTimers();
});

async function mount() {
  await act(async () => {
    root.render(<AboutTab />);
  });
  await act(async () => {
    await vi.advanceTimersByTimeAsync(0);
  });
}

describe("AboutTab", () => {
  it("shows only the computer name and the IP, big, with the clock between them", async () => {
    await mount();
    const text = container.textContent ?? "";
    expect(text).toContain("PC-042");
    expect(text).toContain("10.20.30.40");
    const clock = container.querySelector('[role="timer"]')!;
    expect(clock).not.toBeNull();
    expect(text.indexOf("PC-042")).toBeLessThan(text.indexOf(clock.textContent!));
    expect(text.indexOf(clock.textContent!)).toBeLessThan(text.indexOf("10.20.30.40"));
    // Settings and diagnostics live in their own tab now.
    expect(text).not.toContain("Diagnostics");
    expect(text).not.toContain("CORP");
  });

  it("ticks every second while open", async () => {
    await mount();
    const clock = () => container.querySelector('[role="timer"]')!.textContent;
    expect(clock()).toContain("3:30");
    expect(clock()).toContain("12");
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1000);
    });
    expect(clock()).toContain("13");
  });

  it("stops its timer when closed", async () => {
    await mount();
    act(() => root.unmount());
    expect(vi.getTimerCount()).toBe(0);
    root = createRoot(container);
  });
});

describe("AboutView", () => {
  const NOW = new Date(2026, 9, 6, 15, 30, 12);
  const render = (onCopy: (value: string) => Promise<boolean>, over: { computerName?: string | null; localIpv4?: string | null } = {}) =>
    act(() => root.render(<AboutView computerName="PC-777" localIpv4="192.168.1.9" now={NOW} onCopy={onCopy} {...over} />));

  it("renders from props alone: the name, the clock at the given time, and the IP", () => {
    render(async () => true);
    const text = container.textContent ?? "";
    expect(text).toContain("PC-777");
    expect(text).toContain("192.168.1.9");
    expect(container.querySelector('[role="timer"]')!.textContent).toContain("3:30");
    expect(vi.getTimerCount()).toBe(0);
  });

  it("copies through the callback and says Copied for a moment, with a timer only after the click", async () => {
    const onCopy = vi.fn(async (_value: string) => true);
    render(onCopy);
    const [computer] = [...container.querySelectorAll("button")];
    await act(async () => computer.click());
    expect(onCopy).toHaveBeenCalledWith("PC-777");
    expect(container.textContent).toContain("Copied");
    expect(vi.getTimerCount()).toBe(1);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1600);
    });
    expect(container.textContent).not.toContain("Copied");
    expect(vi.getTimerCount()).toBe(0);
  });

  it("shows a dash and cannot be copied when a value is unknown", () => {
    render(async () => true, { computerName: null, localIpv4: null });
    const buttons = [...container.querySelectorAll("button")];
    expect(buttons.map((b) => b.disabled)).toEqual([true, true]);
    expect(buttons.map((b) => b.textContent)).toEqual(["Computer—", "Local IP—"]);
  });
});
