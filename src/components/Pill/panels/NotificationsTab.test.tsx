// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { relativeMinutes } from "../../../lib/dateFormat";
import { setFixedLocale, t } from "../../../lib/i18n";
import type { IslandNotification } from "../../../lib/ipc";
import { createNotificationHistory } from "../../../lib/notifications/history";
import { NotificationsClearAction, NotificationsView } from "./NotificationsTab";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let container: HTMLDivElement;
let root: Root;
beforeEach(() => {
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});
afterEach(() => {
  setFixedLocale(null);
  act(() => root.unmount());
  container.remove();
});

const NOW = Date.UTC(2026, 9, 6, 12, 0);
const teams: IslandNotification = { id: 1, appName: "Microsoft Teams", title: "Dana", body: "See the deck", timestamp: NOW, aumid: null };
const hebrew: IslandNotification = { id: 2, appName: "Outlook", title: "תזכורת", body: "הגשת דוח", timestamp: NOW, aumid: null };
const entry = (notification: IslandNotification, minutesAgo: number, silenced = false) => ({ notification, receivedAt: NOW - minutesAgo * 60_000, silenced });

function render(entries: ReturnType<typeof entry>[], extra: Partial<Parameters<typeof NotificationsView>[0]> = {}) {
  const onActivate = vi.fn();
  const onRemove = vi.fn();
  act(() => {
    root.render(<NotificationsView entries={entries} nowMs={NOW} notificationsEnabled onActivate={onActivate} onRemove={onRemove} {...extra} />);
  });
  return { onActivate, onRemove };
}

const cards = () => [...container.querySelectorAll<HTMLElement>("li > [data-notification-card]")];
const primary = (card: Element) => card.querySelector<HTMLButtonElement>("button[data-notification-primary]")!;
const dismiss = (card: Element) => card.querySelector<HTMLButtonElement>(`button[aria-label="${t("notifs.remove")}"]`)!;

/** What assistive tech exposes under a node: aria-label wins, aria-hidden subtrees are skipped, else the text. */
function exposedText(node: Node): string {
  if (node.nodeType === Node.TEXT_NODE) return node.textContent ?? "";
  if (!(node instanceof HTMLElement) || node.getAttribute("aria-hidden") === "true") return "";
  const label = node.getAttribute("aria-label");
  if (label) return ` ${label} `;
  return [...node.childNodes].map(exposedText).join("");
}
const key = (target: Element, name: string) => act(() => void target.dispatchEvent(new KeyboardEvent("keydown", { key: name, bubbles: true })));

describe("NotificationsView", () => {
  it("renders one card per entry; its primary button is named by ONE sentence: source notification, title, body, time", () => {
    render([entry(teams, 5), entry(hebrew, 30)]);
    expect(cards()).toHaveLength(2);
    expect(primary(cards()[0]).getAttribute("aria-label")).toBe(`Teams notification. Dana. See the deck. ${relativeMinutes(-5)}`.replace(/\.?$/, "."));
    expect(primary(cards()[1]).getAttribute("aria-label")).toContain("Outlook notification. תזכורת. הגשת דוח.");
  });

  it("exposes each notification once: the card is not a focus stop and its visible text is hidden from assistive tech", () => {
    render([entry(teams, 5, true)]);
    const card = cards()[0];
    expect(card.getAttribute("role")).toBeNull();
    expect(card.getAttribute("tabindex")).toBeNull();
    // The focus stops of a card: the primary button, then the dismiss button. Nothing nested.
    expect([...card.querySelectorAll("button, [tabindex], a[href]")].map((el) => el.getAttribute("aria-label")?.slice(0, 5))).toEqual(["Teams", "Remov"]);
    for (const el of card.querySelectorAll("button")) expect(el.querySelector("button, [role=button], [tabindex]")).toBeNull();
    const heard = exposedText(card).replace(/\s+/g, " ").trim();
    expect(heard).toBe(`${primary(card).getAttribute("aria-label")} ${t("notifs.remove")}`);
    expect(heard).toContain(t("notifs.silenced"));
    expect(heard.match(/Dana/g)).toHaveLength(1);
  });

  it("gives an invitation its own kind as the first words (no 'notification' after it)", () => {
    render([entry({ ...teams, invite: { id: "inv", startUtc: null } }, 1)]);
    expect(primary(cards()[0]).getAttribute("aria-label")!.startsWith(`${t("invite.label")}. Dana.`)).toBe(true);
  });

  it("lays EVERY card out in the UI direction (the icon is on the same side), whatever the content language", () => {
    for (const [locale, dir] of [["he", "rtl"], ["en", "ltr"]] as const) {
      setFixedLocale(locale);
      render([entry(teams, 5), entry(hebrew, 30), entry({ ...teams, id: 3, title: "Project Alpha_v2.pptx מוכן לבדיקה", body: "10.20.30.41" }, 40)]);
      expect(cards().map((card) => card.querySelector("[dir]")!.getAttribute("dir"))).toEqual([dir, dir, dir]);
    }
  });

  it("lets only the text runs choose their own direction (title, body: Hebrew anywhere is rtl; the app name its first letter)", () => {
    setFixedLocale("he");
    render([entry(teams, 5), entry(hebrew, 30), entry({ ...teams, id: 3, title: "Project Alpha_v2.pptx מוכן לבדיקה", body: "10.20.30.41" }, 40)]);
    const dirs = cards().map((card) => [...card.querySelectorAll(".bidi")].map((el) => el.getAttribute("dir")));
    expect(dirs).toEqual([
      ["ltr", "ltr", "ltr"], // Teams, Dana, See the deck: an English card in the Hebrew shell
      ["ltr", "rtl", "rtl"], // Outlook (Latin name), then Hebrew title and body
      ["ltr", "rtl", null], // Teams; a Hebrew sentence that starts in English; a bare IP inherits the shell
    ]);
  });

  it("marks only entries received after the last close as unread", () => {
    render([entry(teams, 5), entry(hebrew, 30)], { lastViewedAt: NOW - 10 * 60_000 });
    expect(cards().map((card) => card.querySelector("[data-unread]") !== null)).toEqual([true, false]);
  });

  it("shows no unread dots by default", () => {
    render([entry(teams, 5)]);
    expect(container.querySelector("[data-unread]")).toBeNull();
  });

  it("activates from the primary button (a real <button>: click, Enter and Space), removes from the dismiss button", () => {
    const { onActivate, onRemove } = render([entry(teams, 5)]);
    const card = cards()[0];
    expect(primary(card).tagName).toBe("BUTTON");
    act(() => primary(card).click());
    expect(onActivate).toHaveBeenCalledWith(teams);
    act(() => dismiss(card).click());
    expect(onRemove).toHaveBeenCalledWith(1);
    expect(onActivate).toHaveBeenCalledTimes(1); // the dismiss button never activates the card
  });

  it("Delete removes the notification from either button; Escape never removes (or activates) anything", () => {
    const { onActivate, onRemove } = render([entry(teams, 5), entry(hebrew, 9)]);
    const [first, second] = cards();
    key(primary(first), "Escape");
    key(dismiss(first), "Escape");
    key(first, "Escape");
    expect(onRemove).not.toHaveBeenCalled();
    expect(onActivate).not.toHaveBeenCalled();
    key(primary(first), "Delete");
    expect(onRemove).toHaveBeenLastCalledWith(1);
    key(dismiss(second), "Delete");
    expect(onRemove).toHaveBeenLastCalledWith(2);
    expect(onRemove).toHaveBeenCalledTimes(2);
  });

  it("does not activate the card when Enter is pressed on its dismiss button", () => {
    const { onActivate } = render([entry(teams, 5)]);
    key(dismiss(cards()[0]), "Enter");
    expect(onActivate).not.toHaveBeenCalled();
  });

  it("marks silenced entries with a quiet grey bell (muted, never destructive red), named in the card's label", () => {
    render([entry(teams, 5, true), entry(hebrew, 9)]);
    const bells = [...container.querySelectorAll<HTMLElement>(`[title="${t("notifs.silenced")}"]`)];
    expect(bells).toHaveLength(1);
    expect(bells[0].style.color).toBe("var(--ci-muted)");
    expect(primary(cards()[0]).getAttribute("aria-label")).toContain(t("notifs.silenced"));
    expect(primary(cards()[1]).getAttribute("aria-label")).not.toContain(t("notifs.silenced"));
  });

  it("shows the calm one-line note above the list only when notifications are off", () => {
    render([entry(teams, 5)], { notificationsEnabled: false });
    expect(container.querySelector("p")!.textContent).toBe(t("notifs.off"));
    render([entry(teams, 5)]);
    expect(container.querySelector("p")).toBeNull();
  });

  it("shows the empty state, with the off message as its hint when notifications are off", () => {
    render([]);
    expect(container.textContent).toContain(t("notifs.empty"));
    expect(container.textContent).toContain(t("notifs.emptyHint"));
    render([], { notificationsEnabled: false });
    expect(container.textContent).toContain(t("notifs.off"));
    expect(container.querySelector("li")).toBeNull();
  });
});

describe("NotificationsClearAction (the header action)", () => {
  it("is absent when there is nothing to clear and clears the history otherwise", () => {
    const history = createNotificationHistory();
    act(() => root.render(<NotificationsClearAction history={history} />));
    expect(container.querySelector("button")).toBeNull();
    act(() => history.add(teams, NOW, false));
    const button = container.querySelector<HTMLButtonElement>("button")!;
    expect(button.textContent).toBe(t("notifs.clear"));
    act(() => button.click());
    expect(history.getSnapshot()).toHaveLength(0);
    expect(container.querySelector("button")).toBeNull();
  });
});
