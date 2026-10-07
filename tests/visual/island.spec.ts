import { expect, test, type Locator, type Page } from "@playwright/test";
import { card, compact, dock, panel, ringer, smallExpanded, toast } from "../../src/design/tokens";

// Frozen "now": the gallery's TOUR_NOW (src/tour/mockData.ts) = 2026-10-06 10:25 local (Asia/Jerusalem).
const FROZEN_NOW = new Date(2026, 9, 6, 10, 25).getTime();

const STILL_CSS = `*,*::before,*::after{transition:none!important;animation:none!important;caret-color:transparent!important}`;

async function open(page: Page, exhibit: string): Promise<Locator> {
  // Date frozen from the first script onwards; timers stay real (React and motion need them).
  await page.clock.setFixedTime(FROZEN_NOW);
  await page.goto(`/gallery.html?exhibit=${exhibit}`);
  await page.addStyleTag({ content: STILL_CSS });
  const island = page.getByTestId("island");
  await island.waitFor();
  await page.evaluate(async () => {
    await document.fonts.ready;
    // two frames: layout effects and canvas text measuring settle
    await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
  });
  return island;
}

interface Box {
  x: number;
  y: number;
  width: number;
  height: number;
}
const rel = async (island: Locator, target: Locator): Promise<Box[]> => {
  const root = (await island.boundingBox())!;
  const boxes = await target.evaluateAll((els) => els.map((e) => e.getBoundingClientRect().toJSON()));
  return boxes.map((b: Box) => ({ x: b.x - root.x, y: b.y - root.y, width: b.width, height: b.height }));
};
const radiusOf = (island: Locator) => island.evaluate((el) => parseFloat(getComputedStyle(el).borderTopLeftRadius));
const close = (actual: number, expected: number, tol = 0.5) => expect(Math.abs(actual - expected), `${actual} vs ${expected}`).toBeLessThanOrEqual(tol);

type Kind = "compact" | "ringer" | "alert" | "toast" | "panel";
interface Spec {
  id: string;
  kind: Kind;
  /** Also rendered at deviceScaleFactor 2. */
  hi?: boolean;
}

export const SPECS: Spec[] = [
  // Compact
  { id: "compact-date", kind: "compact", hi: true }, // normal: date / time / day + count
  { id: "compact-dot", kind: "compact" },
  { id: "compact-plain", kind: "compact" }, // quiet, no notifications
  { id: "compact-soon", kind: "compact" }, // meeting countdown
  { id: "compact-now", kind: "compact", hi: true }, // meeting in progress + progress (+ silent)
  { id: "compact-now-badge", kind: "compact" },
  { id: "compact-muted", kind: "compact" },
  { id: "ringer", kind: "ringer" },
  // Meeting alerts
  { id: "alert", kind: "alert", hi: true }, // reminder
  { id: "alert-now", kind: "alert" }, // starting now
  { id: "alert-long", kind: "alert" },
  // Toasts
  { id: "invite", kind: "toast", hi: true }, // meeting invitation, three actions
  { id: "teams", kind: "toast", hi: true }, // Teams, Hebrew
  { id: "teams-long", kind: "toast" },
  { id: "mixed", kind: "toast" }, // Teams mixed Hebrew / English
  { id: "snipping", kind: "toast" },
  { id: "snipping-short", kind: "toast" }, // title only
  { id: "outlook-en", kind: "toast" },
  { id: "long-app", kind: "toast" },
  { id: "long-hebrew", kind: "toast" },
  // Panels
  { id: "panel-notifications", kind: "panel", hi: true },
  { id: "panel-notifications-empty", kind: "panel" },
  { id: "panel-notifications-1", kind: "panel" },
  { id: "panel-notifications-30", kind: "panel" }, // scrolling
  { id: "panel-notifications-mixed", kind: "panel" },
  { id: "panel-notifications-bidi", kind: "panel", hi: true },
  { id: "panel-calendar", kind: "panel", hi: true },
  { id: "panel-notes", kind: "panel" },
  { id: "panel-notes-empty", kind: "panel" },
  { id: "panel-settings", kind: "panel", hi: true },
  { id: "panel-about", kind: "panel" },
];

for (const spec of SPECS) {
  test.describe(spec.id, () => {
    test(`screenshot${spec.hi ? " @2x" : ""}`, async ({ page }) => {
      test.skip(test.info().project.name === "2x" && !spec.hi);
      const island = await open(page, spec.id);
      await expect(island).toHaveScreenshot(`${spec.id}.png`);
    });

    test("geometry", async ({ page }) => {
      test.skip(test.info().project.name !== "1x", "geometry is scale independent");
      const island = await open(page, spec.id);
      const box = (await island.boundingBox())!;
      const r = await radiusOf(island);
      const expectedSize = await island.evaluate((el) => ({ w: parseFloat((el as HTMLElement).style.width), h: parseFloat((el as HTMLElement).style.height) }));
      close(box.width, expectedSize.w, 0.01);
      close(box.height, expectedSize.h, 0.01);

      if (spec.kind === "compact") {
        close(box.height, compact.height, 0.01);
        close(r, compact.radius, 0.01);
        expect(box.width).toBeGreaterThanOrEqual(compact.minWidth);
        expect(box.width).toBeLessThanOrEqual(compact.maxWidth);
        // The island is a capsule: radius = height / 2.
        close(r, box.height / 2, 0.01);
      }
      if (spec.kind === "ringer") {
        close(box.height, ringer.height, 0.01);
        close(box.width, ringer.width, 0.01);
        close(r, ringer.height / 2, 0.01);
      }
      if (spec.kind === "alert" || spec.kind === "toast") {
        close(r, spec.kind === "alert" ? smallExpanded.radius : toast.radius, 0.01);
        if (spec.kind === "alert") close(box.width, smallExpanded.width, 0.01);
        else {
          expect(box.width).toBeGreaterThanOrEqual(toast.minWidth);
          expect(box.width).toBeLessThanOrEqual(toast.maxWidth);
        }
        // Action row: equal columns that fill the content width, 8px apart.
        const buttons = await rel(island, island.locator("button.rounded-control"));
        if (spec.kind === "alert") expect(buttons.length).toBeGreaterThanOrEqual(1);
        if (spec.id === "invite") expect(buttons.length).toBe(3);
        if (buttons.length >= 1) {
          const contentWidth = box.width - 2 * smallExpanded.padding;
          const total = buttons.reduce((s, b) => s + b.width, 0) + (buttons.length - 1) * 8;
          close(total, contentWidth, 1);
          for (const b of buttons) {
            close(b.width, buttons[0].width, 0.5);
            close(b.y, buttons[0].y, 0.5);
            close(b.height, 40, 0.5);
          }
          const sorted = [...buttons].sort((a, b) => a.x - b.x);
          sorted.slice(1).forEach((b, i) => close(b.x - (sorted[i].x + sorted[i].width), 8, 0.5));
          close(sorted[0].x, smallExpanded.padding, 1);
        }
      }
      if (spec.kind === "panel") {
        close(box.width, panel.width, 0.01);
        close(box.height, panel.height, 0.01);
        close(r, panel.radius, 0.01);
        const [tabs] = await rel(island, island.getByRole("tablist"));
        close(tabs.height, dock.height, 0.01);
        close(tabs.x, panel.inset, 0.5);
        close(tabs.width, panel.width - 2 * panel.inset, 0.5);
        close(tabs.y + tabs.height, panel.height - panel.paddingBottom, 0.5);
        const [header] = await rel(island, island.locator(".island-expanded > *").first());
        close(header.height, panel.headerHeight, 0.01);
        close(header.y, panel.paddingTop, 0.5);

        const cardLoc = island.locator('[role="button"].ci-surface');
        const cards = await rel(island, cardLoc);
        const radii = await cardLoc.evaluateAll((els) => els.map((e) => parseFloat(getComputedStyle(e).borderTopLeftRadius)));
        for (const c of radii) close(c, card.radius, 0.01);
        // Same x and width for every card (the scrolling list continues past the dock), 8px apart.
        for (const c of cards) {
          close(c.x, panel.inset, 0.5);
          close(c.width, panel.width - 2 * panel.inset, 0.5);
        }
        cards.slice(1).forEach((c, i) => close(c.y - (cards[i].y + cards[i].height), card.listGap, 0.5));
      }
    });
  });
}

test("full gallery page still renders every exhibit", async ({ page }) => {
  test.skip(test.info().project.name !== "1x");
  await page.goto("/gallery.html");
  expect(await page.locator("[data-exhibit]").count()).toBeGreaterThanOrEqual(SPECS.length);
});
