// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { IslandNotification } from "../../lib/ipc";
import { toastLayout, toastMaxSize } from "./toastLayout";
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

const primaryOf = (layer: HTMLElement) => layer.querySelector<HTMLButtonElement>("button[data-notification-primary]")!;
const dismissOf = (layer: HTMLElement) => layer.querySelector<HTMLButtonElement>('button[aria-label="Dismiss notification"]')!;
const pointer = (el: Element, type: string, clientX: number, clientY = 20) => el.dispatchEvent(new MouseEvent(type, { bubbles: true, clientX, clientY }));

describe("NotificationToast layout", () => {
  it("has a 40px tile with a 12px corner, a headline title, a body, 16px padding and no 'now' label", () => {
    const layer = show();
    const tile = layer.querySelector<HTMLElement>("div[aria-hidden=true]")!;
    expect(tile.style.width).toBe("40px");
    expect(tile.style.height).toBe("40px");
    expect(tile.style.borderRadius).toBe("12px");
    expect(layer.textContent).toContain("New message");
    expect(layer.querySelector(".text-headline")!.textContent).toBe("New message");
    expect(layer.querySelector(".text-body")!.textContent).toBe("See you at five");
    expect(layer.textContent).not.toMatch(/now/i);
    expect(layer.style.padding).toBe("16px");
  });

  it("gives an invitation the same tile corner and three equal actions", () => {
    const layer = show(note({ invite: { id: "inv", startUtc: null } }));
    expect(layer.querySelector<HTMLElement>("div[aria-hidden=true]")!.style.borderRadius).toBe("12px");
    expect(layer.querySelectorAll(".auto-cols-fr button")).toHaveLength(3);
  });

  it("has ONE primary button named by the whole notification, and hides the duplicate visible text", () => {
    const layer = show();
    expect(primaryOf(layer).getAttribute("aria-label")).toBe("Spotify notification. New message. See you at five.");
    // The text spans are aria-hidden: a screen reader hears the notification once, from the button.
    const hiddenText = [...layer.querySelectorAll<HTMLElement>(".bidi")].map((el) => el.getAttribute("aria-hidden"));
    expect(hiddenText.length).toBeGreaterThanOrEqual(3);
    expect(hiddenText.every((value) => value === "true")).toBe(true);
    expect(layer.getAttribute("role")).toBeNull();
    expect(layer.tabIndex).toBe(-1);
  });

  it("never nests something interactive inside something interactive", () => {
    const layer = show(note({ invite: { id: "inv", startUtc: null } }));
    for (const el of layer.querySelectorAll<HTMLElement>("button, [role=button], [tabindex]")) {
      expect(el.querySelector("button, [role=button], [tabindex], a[href], input")).toBeNull();
    }
    expect(primaryOf(layer).children).toHaveLength(0);
  });

  it("is not an alert, has no live region of its own, and never takes focus", () => {
    const outside = document.createElement("button");
    document.body.appendChild(outside);
    outside.focus();
    const focus = vi.spyOn(HTMLElement.prototype, "focus");
    const layer = show();
    expect(layer.querySelector("[role=alert], [role=alertdialog], [aria-live], [autofocus]")).toBeNull();
    expect(layer.getAttribute("role")).not.toBe("alert");
    expect(focus).not.toHaveBeenCalled();
    expect(document.activeElement).toBe(outside);
    focus.mockRestore();
    outside.remove();
  });

  it("lays an English toast out left to right and a Hebrew one right to left", () => {
    expect(show().querySelector("[dir]")!.getAttribute("dir")).toBe("ltr");
    expect(show(note({ title: "הודעה חדשה", body: "נתראה בחמש" })).querySelector("[dir]")!.getAttribute("dir")).toBe("rtl");
  });

  it("has a 28px dismiss button that appears on hover or keyboard focus, reserves its slot and stays in the tab order", () => {
    const layer = show();
    const x = dismissOf(layer);
    expect(x.getAttribute("aria-label")).toBe("Dismiss notification");
    expect(x.style.width).toBe("28px");
    expect(x.style.height).toBe("28px");
    expect(x.className).toContain("opacity-0");
    expect(x.className).toContain("group-hover:opacity-100");
    expect(x.className).toContain("focus-visible:opacity-100");
    expect(x.className).not.toContain("hidden");
    expect(x.tabIndex).toBe(0);
    expect(primaryOf(layer).tabIndex).toBe(0);
  });

  it("is compact for short content (never under the 200 floor), grows to 368 for long, and an invitation always takes 368", () => {
    const short = toastLayout(note({ title: "Hi", body: "OK" })).size;
    expect(short.width).toBeGreaterThanOrEqual(200);
    expect(short.width).toBeLessThan(260);
    const long = toastLayout(note({ body: "x ".repeat(200) }));
    expect(long.size.width).toBe(368);
    expect(long.bodyLines).toBe(2);
    expect(toastLayout(note({ invite: { id: "i", startUtc: null } })).size.width).toBe(368);
    expect(toastLayout(note({ body: "" })).size.height).toBeLessThan(long.size.height);
    // A title-only toast is as narrow as its title, never the full width.
    expect(toastLayout(note({ title: "Snip saved", body: "" })).size.width).toBeLessThan(260);
  });

  it("is tall enough for its lines inside the 16px padding, an invitation adds the 12px gap and the 40px actions", () => {
    const withBody = toastLayout(note({ body: "x ".repeat(200) })).size;
    expect(withBody.height).toBe(16 * 2 + (16 + 2 + 20 + 2 + 2 * 18));
    const invite = toastLayout(note({ body: "x ".repeat(200), invite: { id: "i", startUtc: null } })).size;
    expect(invite.height).toBe(withBody.height + 12 + 40);
    expect(toastMaxSize().height).toBe(invite.height);
  });
});

describe("NotificationToast interaction", () => {
  it("opens the app on a plain click and dismisses", () => {
    const layer = show();
    pointer(primaryOf(layer), "pointerdown", 100);
    pointer(primaryOf(layer), "pointerup", 102);
    pointer(primaryOf(layer), "click", 102);
    expect(onActivate).toHaveBeenCalledTimes(1);
    expect(onDismiss).toHaveBeenCalledTimes(1);
    expect(onSwipeAway).not.toHaveBeenCalled();
  });

  it("does not open the app when the pointer travelled more than 12px, and swipes it away at 48px", () => {
    const layer = show();
    pointer(primaryOf(layer), "pointerdown", 100);
    pointer(primaryOf(layer), "pointerup", 160);
    pointer(primaryOf(layer), "click", 160);
    expect(onSwipeAway).toHaveBeenCalledTimes(1);
    expect(onActivate).not.toHaveBeenCalled();
    expect(onDismiss).not.toHaveBeenCalled();
  });

  it("ignores a short drag: neither a swipe nor a click", () => {
    const layer = show();
    pointer(primaryOf(layer), "pointerdown", 100);
    pointer(primaryOf(layer), "pointerup", 130);
    pointer(primaryOf(layer), "click", 130);
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

  it("activates from its primary button (a real <button>: Enter and Space press it natively)", () => {
    const layer = show();
    expect(primaryOf(layer).tagName).toBe("BUTTON");
    expect(primaryOf(layer).getAttribute("type")).toBe("button");
    act(() => primaryOf(layer).click());
    expect(onActivate).toHaveBeenCalledTimes(1);
    expect(onDismiss).toHaveBeenCalledTimes(1);
  });

  it("Delete and Escape take the toast away from the screen, and never activate the app", () => {
    const layer = show();
    const key = (k: string) => primaryOf(layer).dispatchEvent(new KeyboardEvent("keydown", { key: k, bubbles: true, cancelable: true }));
    key("Delete");
    key("Escape");
    expect(onDismiss).toHaveBeenCalledTimes(2);
    expect(onActivate).not.toHaveBeenCalled();
    // Enter and Space are left to the native button (no handler of ours that could double-activate).
    key("Enter");
    key(" ");
    expect(onActivate).not.toHaveBeenCalled();
    expect(onDismiss).toHaveBeenCalledTimes(2);
  });

  it("Delete removes the notification (onRemove); Escape only closes the toast", () => {
    const onRemove = vi.fn();
    act(() => {
      root.render(<NotificationToast notification={note()} onDismiss={onDismiss} onActivate={onActivate} onRemove={onRemove} />);
    });
    const layer = container.querySelector<HTMLElement>('[data-layer="notification"]')!;
    const key = (k: string) => primaryOf(layer).dispatchEvent(new KeyboardEvent("keydown", { key: k, bubbles: true, cancelable: true }));
    key("Escape");
    expect(onDismiss).toHaveBeenCalledTimes(1);
    expect(onRemove).not.toHaveBeenCalled();
    key("Delete");
    expect(onRemove).toHaveBeenCalledTimes(1);
    expect(onDismiss).toHaveBeenCalledTimes(1);
    expect(onActivate).not.toHaveBeenCalled();
  });

  it("Escape does not bubble to the shell (it would close the island too)", () => {
    const layer = show();
    const seen = vi.fn();
    document.body.addEventListener("keydown", seen);
    primaryOf(layer).dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
    expect(seen).not.toHaveBeenCalled();
    document.body.removeEventListener("keydown", seen);
  });

  it("only dismisses from the X, never opens the app", () => {
    const layer = show();
    act(() => dismissOf(layer).click());
    expect(onDismiss).toHaveBeenCalledTimes(1);
    expect(onActivate).not.toHaveBeenCalled();
  });
});
