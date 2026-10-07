// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { textDirection } from "../../../design/direction";
import { BidiText } from "./BidiText";

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

/** The way the notification renders a field: a paragraph with its own dir, tokens inside. */
function paragraph(text: string, className = "bidi line-clamp-2") {
  act(() => root.render(<span dir={textDirection(text)} className={className}><BidiText text={text} /></span>));
  return container.firstElementChild as HTMLElement;
}
const isolated = (el: HTMLElement) => [...el.querySelectorAll("bdi")].map((bdi) => [bdi.textContent, bdi.getAttribute("dir")]);

describe("BidiText", () => {
  it("wraps the file name in a Hebrew sentence in <bdi dir=ltr> and keeps the sentence RTL", () => {
    const el = paragraph("Project Alpha_v2.pptx מוכן לבדיקה");
    expect(el.getAttribute("dir")).toBe("rtl");
    expect(isolated(el)).toEqual([["Project Alpha_v2.pptx", "ltr"]]);
    expect(el.textContent).toBe("Project Alpha_v2.pptx מוכן לבדיקה");
  });

  it("renders no wrapper when there is no token (a plain text node)", () => {
    const el = paragraph("Teams — דניאל כהן");
    expect(el.getAttribute("dir")).toBe("rtl");
    expect(el.querySelector("bdi")).toBeNull();
    expect(el.childNodes).toHaveLength(1);
  });

  it("isolates 'Meeting at 11:00' as one phrase in 'Meeting at 11:00 בחדר 3' and leaves the 3 to the algorithm", () => {
    const el = paragraph("Meeting at 11:00 בחדר 3");
    expect(el.getAttribute("dir")).toBe("rtl");
    expect(isolated(el)).toEqual([["Meeting at 11:00", "ltr"]]);
  });

  it("isolates a bare IP, a path, an address and a URL as the whole text", () => {
    expect(isolated(paragraph("10.20.30.41"))).toEqual([["10.20.30.41", "ltr"]]);
    expect(isolated(paragraph("C:\\Users\\Daniel\\Report.pdf"))).toEqual([["C:\\Users\\Daniel\\Report.pdf", "ltr"]]);
    expect(isolated(paragraph("user@example.com"))).toEqual([["user@example.com", "ltr"]]);
    expect(isolated(paragraph("https://example.com/path"))).toEqual([["https://example.com/path", "ltr"]]);
    expect(paragraph("10.20.30.41").hasAttribute("dir")).toBe(false); // neutral paragraph inherits the shell
  });

  it("keeps the sentence punctuation outside the token and the text lossless", () => {
    const text = "ראה https://example.com/path. (טיוטה) 🎉";
    const el = paragraph(text);
    expect(isolated(el)).toEqual([["https://example.com/path", "ltr"]]);
    expect(el.textContent).toBe(text);
  });

  it("works inside a truncating or clamping element: inline children, the clamp classes stay on the paragraph", () => {
    const one = paragraph("Report C:\\Users\\Daniel\\Report.pdf is ready", "bidi truncate");
    expect(one.className).toContain("truncate");
    expect(one.children[0].tagName).toBe("BDI");
    expect(getComputedStyle(one.children[0]).display).not.toBe("block"); // inline, so ellipsis and line-clamp see one inline flow
    const two = paragraph("a 11:00–12:00 b", "bidi line-clamp-2");
    expect(two.className).toContain("line-clamp-2");
  });
});
