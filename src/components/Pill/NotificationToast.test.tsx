// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { IslandNotification } from "../../lib/ipc";
import { notificationSize } from "./animations";
import { NotificationToast } from "./NotificationToast";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const note = (extra: Partial<IslandNotification> = {}): IslandNotification => ({
  id: 7,
  appName: "Spotify.exe",
  title: "New message",
  body: "See you at five",
  timestamp: 0,
  aumid: null,
  ...extra,
});

let container: HTMLDivElement;
let root: Root;
const onDismiss = vi.fn();
const onActivate = vi.fn();
const onSwipeAway = vi.fn();

beforeEach(() => {
  onDismiss.mockClear();
  onActivate.mockClear();
  onSwipeAway.mockClear();
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});
afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

function show(notification = note(), swipe = true) {
  act(() => {
    root.render(<NotificationToast notification={notification} onDismiss={onDismiss} onActivate={onActivate} onSwipeAway={swipe ? onSwipeAway : undefined} />);
  });
  return container.querySelector<HTMLElement>('[data-layer="notification"]')!;
}

const pointer = (el: Element, type: string, clientX: number, clientY = 20) => el.dispatchEvent(new MouseEvent(type, { bubbles: true, clientX, clientY }));

describe("NotificationToast layout", () => {
  it("has a 40px avatar with an 18px corner, a 15px title, a 13px body and no 'now' label", () => {
    const layer = show();
    const avatar = layer.querySelector<HTMLElement>("div[aria-hidden=true]")!;
    expect(avatar.style.width).toBe("40px");
    expect(avatar.style.height).toBe("40px");
    expect(avatar.style.borderRadius).toBe("18px");
    expect(layer.querySelector("h4")!.className).toContain("text-[15px]");
    expect(layer.querySelector("p")!.className).toContain("text-[13px]");
    expect(layer.textContent).not.toMatch(/now/i);
    expect(layer.style.padding).toBe("12px");
  });

  it("gives an invitation the same avatar corner", () => {
    const layer = show(note({ invite: { id: "inv", startUtc: null } }));
    expect(layer.querySelector<HTMLElement>("div[aria-hidden=true]")!.style.borderRadius).toBe("18px");
  });

  it("has a 24px dismiss button that appears on hover or keyboard focus and is always in the tab order", () => {
    const layer = show();
    const x = layer.querySelector<HTMLButtonElement>("button")!;
    expect(x.getAttribute("aria-label")).toBe("Dismiss notification");
    expect(x.className).toContain("w-6");
    expect(x.className).toContain("h-6");
    expect(x.className).toContain("opacity-0");
    expect(x.className).toContain("group-hover:opacity-100");
    expect(x.className).toContain("focus-visible:opacity-100");
    expect(x.className).not.toContain("hidden");
    expect(x.tabIndex).toBe(0);
  });

  it("keeps its sizes pinned to the island's toast size", () => {
    expect(notificationSize(false)).toMatchObject({ width: 372, height: 64 });
    expect(notificationSize(true)).toMatchObject({ width: 372, height: 94 });
    expect(notificationSize(true, true)).toMatchObject({ width: 372, height: 128 });
  });

  it("is tall enough for a two-line body inside its 12px margins, so the bottom margin stays 12", () => {
    const margins = 12 * 2;
    const text = 14 + 20 + 2 * 18; // app label, title, two clamped body lines (leading 14 / 20 / 18)
    expect(notificationSize(true).height - margins).toBeGreaterThanOrEqual(text);
    const invite = text + 8 + 26; // the answer buttons' gap and height
    expect(notificationSize(true, true).height - margins).toBeGreaterThanOrEqual(invite);
  });
});

describe("NotificationToast interaction", () => {
  it("opens the app on a plain click and dismisses", () => {
    const layer = show();
    pointer(layer, "pointerdown", 100);
    pointer(layer, "pointerup", 102);
    pointer(layer, "click", 102);
    expect(onActivate).toHaveBeenCalledTimes(1);
    expect(onDismiss).toHaveBeenCalledTimes(1);
    expect(onSwipeAway).not.toHaveBeenCalled();
  });

  it("does not open the app when the pointer travelled more than 12px, and swipes it away at 48px", () => {
    const layer = show();
    pointer(layer, "pointerdown", 100);
    pointer(layer, "pointerup", 160);
    pointer(layer, "click", 160);
    expect(onSwipeAway).toHaveBeenCalledTimes(1);
    expect(onActivate).not.toHaveBeenCalled();
    expect(onDismiss).not.toHaveBeenCalled();
  });

  it("ignores a short drag: neither a swipe nor a click", () => {
    const layer = show();
    pointer(layer, "pointerdown", 100);
    pointer(layer, "pointerup", 130);
    pointer(layer, "click", 130);
    expect(onSwipeAway).not.toHaveBeenCalled();
    expect(onActivate).not.toHaveBeenCalled();
  });

  it("swipes to the left as well, and a mostly vertical drag is not a swipe", () => {
    const layer = show();
    pointer(layer, "pointerdown", 200);
    pointer(layer, "pointerup", 120);
    expect(onSwipeAway).toHaveBeenCalledTimes(1);
    pointer(layer, "pointerdown", 100, 0);
    pointer(layer, "pointerup", 150, 120);
    expect(onSwipeAway).toHaveBeenCalledTimes(1);
  });

  it("falls back to a plain dismiss for a swipe when the shell gives no handler", () => {
    const layer = show(note(), false);
    pointer(layer, "pointerdown", 100);
    pointer(layer, "pointerup", 180);
    expect(onDismiss).toHaveBeenCalledTimes(1);
  });

  it("activates on Enter and Space, dismisses on Delete and Escape", () => {
    const layer = show();
    const key = (k: string) => layer.dispatchEvent(new KeyboardEvent("keydown", { key: k, bubbles: true }));
    key("Enter");
    expect(onActivate).toHaveBeenCalledTimes(1);
    key(" ");
    expect(onActivate).toHaveBeenCalledTimes(2);
    onDismiss.mockClear();
    key("Delete");
    key("Escape");
    expect(onDismiss).toHaveBeenCalledTimes(2);
    expect(onActivate).toHaveBeenCalledTimes(2);
  });

  it("only dismisses from the X, never opens the app", () => {
    const layer = show();
    act(() => layer.querySelector("button")!.click());
    expect(onDismiss).toHaveBeenCalledTimes(1);
    expect(onActivate).not.toHaveBeenCalled();
  });
});
