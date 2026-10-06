import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { CompactIsland } from "./CompactIsland";

const render = (weekday: string, unseen = 0) =>
  renderToStaticMarkup(<CompactIsland labels={{ date: "6.10", weekday, contentWidth: 90 }} unseen={unseen} reducedMotion={false} />);

describe("CompactIsland", () => {
  it("puts the date on the left and the weekday on the right, whatever the language", () => {
    for (const weekday of ["Tuesday", "יום שלישי", "الثلاثاء"]) {
      const html = render(weekday);
      expect(html.indexOf("6.10")).toBeLessThan(html.indexOf(weekday));
      expect(html).toContain('dir="ltr"');
    }
  });

  it("isolates each label as LTR and lets the text inside choose its own direction", () => {
    const html = render("יום שלישי");
    expect(html.match(/direction:ltr;unicode-bidi:isolate/g)).toHaveLength(2);
    expect(html.match(/<span dir="auto">/g)).toHaveLength(2);
  });

  it("shows an unseen-notification badge only when there is one", () => {
    expect(render("Tuesday")).not.toContain("tabular-nums text-white");
    expect(render("Tuesday", 3)).toContain(">3<");
    expect(render("Tuesday", 12)).toContain(">9+<");
  });
});
