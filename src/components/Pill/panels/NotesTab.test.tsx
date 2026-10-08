// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Note } from "../../../lib/ipc";
import type { StickyNote, StickySnapshot } from "../../../lib/notes/sticky";
import { islandTyping, noteDraft } from "../../../lib/notes/typing";
import { notePreview, NotesTab, NotesView, type NotesViewProps } from "./NotesTab";

const mocks = vi.hoisted(() => ({
  stickyOpen: vi.fn(async () => true),
  sticky: null as StickySnapshot | null,
  openCenter: vi.fn(async (_page: string) => true),
  islandKeyboard: vi.fn(async (_on: boolean) => true),
  add: vi.fn(async (_text: string) => true),
  events: new Map<string, (payload: unknown) => void>(),
  copyTextToClipboard: vi.fn(async (_text: string) => true),
  remove: vi.fn(async (_id: string) => true),
  togglePin: vi.fn(async (_id: string) => true),
  retry: vi.fn(),
  notes: [] as Note[],
  loaded: true,
  loadFailed: false,
}));

vi.mock("../../../lib/ipc", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../lib/ipc")>();
  return {
    ...original,
    ipc: { ...original.ipc, openCenter: mocks.openCenter, copyTextToClipboard: mocks.copyTextToClipboard, islandKeyboard: mocks.islandKeyboard, stickyNotesOpen: mocks.stickyOpen },
    onEvent: (name: string, handler: (payload: unknown) => void) => {
      mocks.events.set(name, handler);
      return () => mocks.events.delete(name);
    },
  };
});

vi.mock("../../../hooks/useNotes", () => ({
  useNotes: () => ({
    notes: mocks.notes,
    loaded: mocks.loaded,
    loadFailed: mocks.loadFailed,
    retry: mocks.retry,
    add: mocks.add,
    update: vi.fn(async () => true),
    remove: mocks.remove,
    togglePin: mocks.togglePin,
  }),
}));

vi.mock("../../../lib/notes/useSticky", () => ({ useStickyNotes: () => mocks.sticky }));

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const NOW = new Date(2026, 9, 6, 15, 30, 0).getTime();
const MIN = 60_000;

const note = (over: Partial<Note> = {}): Note => ({ id: "n1", text: "Buy milk", createdAt: NOW - 10 * MIN, updatedAt: NOW - 5 * MIN, pinned: false, ...over });

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout", "Date"] });
  vi.setSystemTime(NOW);
  mocks.notes = [];
  mocks.sticky = null;
  mocks.loaded = true;
  mocks.loadFailed = false;
  mocks.stickyOpen.mockClear();
  for (const fn of [mocks.openCenter, mocks.copyTextToClipboard, mocks.remove, mocks.togglePin, mocks.retry, mocks.islandKeyboard, mocks.add]) fn.mockClear();
  mocks.add.mockImplementation(async () => true);
  mocks.events.clear();
  islandTyping.set(false);
  noteDraft.set("");
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});
afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.useRealTimers();
});

function viewProps(over: Partial<NotesViewProps> = {}): NotesViewProps {
  return { notes: [], nowMs: NOW, onNew: vi.fn(), onOpen: vi.fn(), onTogglePin: vi.fn(), onCopy: vi.fn(), onRemove: vi.fn(), ...over };
}

function renderView(props: NotesViewProps) {
  act(() => root.render(<NotesView {...props} />));
}

const rows = () => [...container.querySelectorAll<HTMLElement>("li[data-note-id]")];
const button = (scope: ParentNode, label: string) => scope.querySelector<HTMLElement>(`button[aria-label="${label}"]`)!;
/** The trash button, whichever state it is in (the first click asks, the second deletes). */
const trash = (row: HTMLElement) => row.querySelector<HTMLElement>('button[aria-label="Delete note"], button[data-delete-confirming]')!;

describe("NotesView", () => {
  it("explains that notes stay on this computer and offers a new note when empty", () => {
    const props = viewProps();
    renderView(props);
    expect(container.textContent).toContain("No notes yet");
    expect(container.textContent).toContain("Notes are saved only on this computer.");
    expect(rows()).toHaveLength(0);
    const newButton = [...container.querySelectorAll("button")].find((b) => b.textContent === "New note")!;
    act(() => newButton.click());
    expect(props.onNew).toHaveBeenCalledTimes(1);
  });

  it("lists the notes in the order given, clamped to three lines, each text picking its own direction", () => {
    renderView(viewProps({ notes: [note({ id: "a", text: "first" }), note({ id: "b", text: "שלום עולם" })] }));
    expect(rows().map((r) => r.dataset.noteId)).toEqual(["a", "b"]);
    const text = rows()[1].querySelector<HTMLElement>("[data-note-text]")!;
    expect(text.textContent).toBe("שלום עולם");
    expect(text.getAttribute("dir")).toBe("rtl");
    expect(rows()[0].querySelector<HTMLElement>("[data-note-text]")!.getAttribute("dir")).toBe("ltr");
    expect(text.className).toContain("line-clamp-3");
    expect(text.className).toContain("bidi");
  });

  it("shows when each note was last changed, relative to now", () => {
    renderView(viewProps({ notes: [note({ updatedAt: NOW - 5 * MIN })] }));
    expect(rows()[0].textContent).toMatch(/5 min/);
  });

  it("marks only pinned notes and labels the pin action by state", () => {
    renderView(viewProps({ notes: [note({ id: "a", pinned: true }), note({ id: "b", pinned: false })] }));
    const [pinned, plain] = rows();
    expect(pinned.querySelector('[role="img"][aria-label="Pinned"]')).not.toBeNull();
    expect(plain.querySelector('[role="img"][aria-label="Pinned"]')).toBeNull();
    expect(button(pinned, "Unpin note")).not.toBeNull();
    expect(button(plain, "Pin note")).not.toBeNull();
  });

  it("opens a note on a click on the row or on its preview button, and the action buttons never open it", () => {
    const props = viewProps({ notes: [note({ id: "a" })] });
    renderView(props);
    const row = rows()[0];
    act(() => row.click());
    expect(props.onOpen).toHaveBeenLastCalledWith("a");
    // The preview button is the keyboard / screen reader way in: its click (Enter and Space produce one) reaches the row once.
    act(() => row.querySelector<HTMLElement>("button[data-note-open]")!.click());
    expect(props.onOpen).toHaveBeenCalledTimes(2);

    act(() => button(row, "Pin note").click());
    act(() => button(row, "Copy note").click());
    act(() => trash(row).click());
    act(() => trash(row).click());
    expect(props.onTogglePin).toHaveBeenCalledWith("a");
    expect(props.onCopy).toHaveBeenCalledWith("a");
    expect(props.onRemove).toHaveBeenCalledWith("a");
    expect(props.onOpen).toHaveBeenCalledTimes(2);
  });

  it("is a real list: plain list items, one primary button each, the action buttons beside it and never inside", () => {
    renderView(viewProps({ notes: [note({ id: "a", text: "Buy milk" }), note({ id: "b", text: "Call Dana" })] }));
    const list = container.querySelector("ul")!;
    expect(list.getAttribute("aria-label")).toBe("Notes");
    expect(list.querySelectorAll(":scope > li")).toHaveLength(2);
    for (const row of rows()) {
      expect(row.getAttribute("role")).toBeNull();
      expect(row.getAttribute("tabindex")).toBeNull();
      const buttons = [...row.querySelectorAll("button")];
      for (const inner of buttons) expect(inner.querySelector("button, [role=button]")).toBeNull(); // nothing interactive inside a button
      const open = row.querySelector<HTMLElement>("button[data-note-open]")!;
      expect(open.getAttribute("aria-label")).toBe(`Open note: ${row.dataset.noteId === "a" ? "Buy milk" : "Call Dana"}`);
      expect(buttons.filter((x) => open.contains(x))).toEqual([open]);
    }
  });

  it("keeps the Copied status outside every button", () => {
    renderView(viewProps({ notes: [note({ id: "a" })], copiedId: "a" }));
    const status = container.querySelector('[role="status"]')!;
    expect(status.closest("button")).toBeNull();
  });

  it("asks before deleting: the first click turns the trash into a confirmation, the second deletes, and it times out", () => {
    const props = viewProps({ notes: [note({ id: "a" })] });
    renderView(props);
    const row = rows()[0];
    expect(vi.getTimerCount()).toBe(0);
    act(() => trash(row).click());
    expect(props.onRemove).not.toHaveBeenCalled();
    expect(button(row, "Confirm deleting the note").textContent).toBe("Delete?");
    expect(vi.getTimerCount()).toBe(1);

    act(() => {
      vi.advanceTimersByTime(3100);
    });
    expect(props.onRemove).not.toHaveBeenCalled();
    expect(row.querySelector("[data-delete-confirming]")).toBeNull();
    expect(button(row, "Delete note")).not.toBeNull();
    expect(vi.getTimerCount()).toBe(0);

    act(() => trash(row).click());
    act(() => button(row, "Confirm deleting the note").click());
    expect(props.onRemove).toHaveBeenCalledTimes(1);
    expect(props.onRemove).toHaveBeenCalledWith("a");
    expect(vi.getTimerCount()).toBe(0);
    expect(button(row, "Delete note")).not.toBeNull();
  });

  it("cancels the confirmation on Escape, on losing focus, and when the row goes away", () => {
    const props = viewProps({ notes: [note({ id: "a" })] });
    renderView(props);
    const row = rows()[0];
    act(() => trash(row).click());
    act(() => {
      button(row, "Confirm deleting the note").dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    });
    expect(row.querySelector("[data-delete-confirming]")).toBeNull();

    act(() => trash(row).click());
    act(() => {
      button(row, "Confirm deleting the note").dispatchEvent(new FocusEvent("focusout", { bubbles: true }));
    });
    expect(row.querySelector("[data-delete-confirming]")).toBeNull();
    expect(vi.getTimerCount()).toBe(0);

    act(() => trash(row).click());
    expect(vi.getTimerCount()).toBe(1);
    act(() => root.unmount());
    expect(vi.getTimerCount()).toBe(0);
    root = createRoot(container);
    expect(props.onRemove).not.toHaveBeenCalled();
  });

  it("shows the first lines of text for a note that starts with blank lines, leaving the stored text alone", () => {
    expect(notePreview("\n\n\nBuy milk")).toBe("Buy milk");
    expect(notePreview("  \n \nBuy milk")).toBe("Buy milk");
    expect(notePreview("one\n\n\n  \ntwo\nthree")).toBe("one\ntwo\nthree");
    expect(notePreview("Buy milk")).toBe("Buy milk");
    const raw = note({ text: "\n\n\nBuy milk" });
    renderView(viewProps({ notes: [raw] }));
    expect(rows()[0].querySelector("[data-note-text]")!.textContent).toBe("Buy milk");
    expect(raw.text).toBe("\n\n\nBuy milk");
  });

  it("shows nothing (not the empty state) while the list is loading, and says so when it could not be read", () => {
    renderView(viewProps({ loading: true }));
    expect(container.textContent).toBe("");
    expect(container.querySelector("[aria-busy=true]")).not.toBeNull();

    const onRetry = vi.fn();
    renderView(viewProps({ loadFailed: true, onRetry }));
    expect(container.textContent).toContain("Couldn't load the notes");
    expect(container.textContent).not.toContain("No notes yet");
    const retry = [...container.querySelectorAll("button")].find((x) => x.textContent === "Try again")!;
    act(() => retry.click());
    expect(onRetry).toHaveBeenCalledTimes(1);

    // Notes already on screen are never replaced by the message.
    renderView(viewProps({ notes: [note()], loading: true, loadFailed: true }));
    expect(rows()).toHaveLength(1);
  });

  it("offers a new note above the list", () => {
    const props = viewProps({ notes: [note()] });
    renderView(props);
    const newButton = [...container.querySelectorAll("button")].find((b) => b.textContent === "New note")!;
    act(() => newButton.click());
    expect(props.onNew).toHaveBeenCalledTimes(1);
  });

  it("confirms a copy on that note only, and reports a failed save", () => {
    renderView(viewProps({ notes: [note({ id: "a" }), note({ id: "b" })], copiedId: "b" }));
    expect(rows()[0].textContent).not.toContain("Copied");
    expect(rows()[1].textContent).toContain("Copied");
    expect(container.querySelector('[role="alert"]')).toBeNull();

    renderView(viewProps({ notes: [note()], saveFailed: true }));
    expect(container.querySelector('[role="alert"]')!.textContent).toBe("Couldn't save the note");
  });

  it("its empty and failed states use the shared state blocks (status / alert) and fill the tab", () => {
    renderView(viewProps());
    expect(container.querySelector('[data-state][role="status"]')!.className).toContain("flex-1");
    renderView(viewProps({ loadFailed: true, onRetry: vi.fn() }));
    expect(container.querySelector('[data-state][role="alert"]')).not.toBeNull();
  });

  it("creates no timers of its own", () => {
    renderView(viewProps({ notes: [note({ id: "a" }), note({ id: "b" })], copiedId: "a" }));
    expect(vi.getTimerCount()).toBe(0);
  });
});

describe("NotesTab", () => {
  async function mount() {
    await act(async () => {
      root.render(<NotesTab />);
    });
  }

  const composer = () => container.querySelector<HTMLTextAreaElement>('textarea[aria-label="New note text"]')!;
  const buttonWithText = (text: string) => [...container.querySelectorAll("button")].find((b) => b.textContent === text)!;
  function typeInto(el: HTMLTextAreaElement, text: string) {
    Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(el, text);
    el.dispatchEvent(new Event("input", { bubbles: true }));
  }
  function key(el: HTMLElement, init: KeyboardEventInit) {
    const event = new KeyboardEvent("keydown", { bubbles: true, cancelable: true, ...init });
    el.dispatchEvent(event);
    return event;
  }

  it("opens the Island Center's Notes page from the header and the empty state, and a clicked note on that note", async () => {
    mocks.notes = [note({ id: "abc123" })];
    await mount();
    await act(async () => buttonWithText("Open in app").click());
    expect(mocks.openCenter).toHaveBeenLastCalledWith("notes");
    await act(async () => rows()[0].click());
    expect(mocks.openCenter).toHaveBeenLastCalledWith("note:abc123");

    mocks.notes = [];
    await mount();
    await act(async () => buttonWithText("Open in app").click());
    expect(mocks.openCenter).toHaveBeenLastCalledWith("notes");
    expect(mocks.openCenter).not.toHaveBeenCalledWith("notes-new");
  });

  it("writes a note inside the island: the box takes the keyboard, Ctrl+Enter saves through the store and empties it", async () => {
    await mount();
    const box = composer();
    expect(buttonWithText("Save")).toBeUndefined(); // an empty box is just the field
    await act(async () => box.focus());
    expect(mocks.islandKeyboard).toHaveBeenCalledWith(true);
    expect(islandTyping.get()).toBe(true);

    await act(async () => typeInto(box, "  "));
    await act(async () => void key(box, { key: "Enter", ctrlKey: true }));
    expect(mocks.add).not.toHaveBeenCalled();

    await act(async () => typeInto(box, "Call Dana"));
    expect(buttonWithText("Save").hasAttribute("disabled")).toBe(false);
    await act(async () => void key(box, { key: "Enter", ctrlKey: true }));
    expect(mocks.add).toHaveBeenCalledWith("Call Dana");
    expect(composer().value).toBe("");
    expect(container.textContent).toContain("Saved");
  });

  it("saves with the button too, and keeps the text when the store could not save it", async () => {
    mocks.add.mockImplementation(async () => false);
    await mount();
    await act(async () => typeInto(composer(), "Keep me"));
    await act(async () => buttonWithText("Save").click());
    expect(mocks.add).toHaveBeenCalledWith("Keep me");
    expect(composer().value).toBe("Keep me");
    expect(container.querySelector('[role="alert"]')).toBeNull(); // empty list: no header, the text stays as the signal
  });

  it("keeps every key inside the box, and Escape leaves it and gives the keyboard back", async () => {
    await mount();
    const box = composer();
    await act(async () => box.focus());
    const outside = vi.fn();
    window.addEventListener("keydown", outside);
    await act(async () => void key(box, { key: "ArrowLeft" }));
    await act(async () => void key(box, { key: "Escape" }));
    window.removeEventListener("keydown", outside);
    expect(outside).not.toHaveBeenCalled();
    // The shell's Escape-collapse ignores a key that was already handled.
    const escape = key(box, { key: "Escape" });
    expect(escape.defaultPrevented).toBe(true);
    expect(document.activeElement).not.toBe(box);
    expect(mocks.islandKeyboard).toHaveBeenLastCalledWith(false);
    expect(islandTyping.get()).toBe(false);
  });

  it("stops typing when another window took the keyboard, and keeps the draft for the next time the island opens", async () => {
    await mount();
    const box = composer();
    await act(async () => box.focus());
    await act(async () => typeInto(box, "half a thought"));
    await act(async () => mocks.events.get("island-keyboard-ended")!(null));
    expect(islandTyping.get()).toBe(false);
    expect(document.activeElement).not.toBe(box);

    act(() => root.unmount());
    root = createRoot(container);
    await mount();
    expect(composer().value).toBe("half a thought");
  });

  it("gives the keyboard back when the tab closes while typing", async () => {
    await mount();
    await act(async () => composer().focus());
    expect(islandTyping.get()).toBe(true);
    act(() => root.unmount());
    root = createRoot(container);
    expect(islandTyping.get()).toBe(false);
    expect(mocks.islandKeyboard).toHaveBeenLastCalledWith(false);
  });

  it("pins and deletes through the store", async () => {
    mocks.notes = [note({ id: "a" })];
    await mount();
    await act(async () => button(rows()[0], "Pin note").click());
    expect(mocks.togglePin).toHaveBeenCalledWith("a");
    await act(async () => trash(rows()[0]).click());
    expect(mocks.remove).not.toHaveBeenCalled();
    await act(async () => trash(rows()[0]).click());
    expect(mocks.remove).toHaveBeenCalledWith("a");
    expect(mocks.openCenter).not.toHaveBeenCalled();
  });

  it("renders nothing while the store has not answered, and retries from the failure message", async () => {
    mocks.loaded = false;
    await mount();
    expect(container.textContent).toBe("");

    mocks.loaded = true;
    mocks.loadFailed = true;
    await mount();
    expect(container.textContent).toContain("Couldn't load the notes");
    const retry = [...container.querySelectorAll("button")].find((x) => x.textContent === "Try again")!;
    await act(async () => retry.click());
    expect(mocks.retry).toHaveBeenCalledTimes(1);
  });

  it("says the change could not be saved when the store reverted it", async () => {
    mocks.notes = [note({ id: "a" })];
    mocks.remove.mockResolvedValueOnce(false);
    await mount();
    await act(async () => trash(rows()[0]).click());
    await act(async () => trash(rows()[0]).click());
    expect(container.querySelector('[role="alert"]')!.textContent).toBe("Couldn't save the note");
    await act(async () => {
      await vi.advanceTimersByTimeAsync(2000);
    });
    expect(container.querySelector('[role="alert"]')).toBeNull();
  });

  it("copies the note's text, flashes Copied for a moment with one timer, then goes back", async () => {
    mocks.notes = [note({ id: "a", text: "Line one\nLine two" })];
    await mount();
    const idleTimers = vi.getTimerCount();
    expect(container.textContent).not.toContain("Copied");

    await act(async () => button(rows()[0], "Copy note").click());
    expect(mocks.copyTextToClipboard).toHaveBeenCalledWith("Line one\nLine two");
    expect(rows()[0].textContent).toContain("Copied");
    expect(vi.getTimerCount()).toBe(idleTimers + 1);

    await act(async () => {
      await vi.advanceTimersByTimeAsync(2000);
    });
    expect(container.textContent).not.toContain("Copied");
    expect(vi.getTimerCount()).toBe(idleTimers);
  });

  it("does not flash when the copy failed, and clears its timer when closed mid-flash", async () => {
    mocks.notes = [note({ id: "a" })];
    mocks.copyTextToClipboard.mockResolvedValueOnce(false);
    await mount();
    const idleTimers = vi.getTimerCount();
    await act(async () => button(rows()[0], "Copy note").click());
    expect(container.textContent).not.toContain("Copied");
    expect(vi.getTimerCount()).toBe(idleTimers);

    await act(async () => button(rows()[0], "Copy note").click());
    expect(container.textContent).toContain("Copied");
    act(() => root.unmount());
    expect(vi.getTimerCount()).toBe(0);
    root = createRoot(container);
  });
});

// -----------------------------------------------------------------------------
// The user's Windows Sticky Notes: a read-only section under their own notes.
// -----------------------------------------------------------------------------

const sticky = (over: Partial<StickyNote> = {}): StickyNote => ({
  id: "s1",
  text: "Pick up the dry cleaning",
  title: "Pick up the dry cleaning",
  colour: "yellow",
  updatedAt: NOW - 2 * 60 * MIN,
  createdAt: NOW - 3 * 60 * MIN,
  ...over,
});
const snapshot = (notes: StickyNote[], availability: StickySnapshot["availability"] = "ok"): StickySnapshot => ({ availability, notes, revision: 1 });
const stickyRows = () => [...container.querySelectorAll<HTMLElement>("li[data-sticky-id]")];
const section = () => container.querySelector<HTMLElement>("[data-sticky-section]");

describe("NotesView · Windows Sticky Notes", () => {
  const withSticky = (snap: StickySnapshot | null, over: Partial<NotesViewProps> = {}) => {
    const onOpen = vi.fn();
    const props = viewProps({ notes: [note()], sticky: { snapshot: snap, onOpen }, ...over });
    renderView(props);
    return { props, onOpen };
  };

  it("is exactly the old view without the sticky prop, with nothing known, and when Sticky Notes is not installed", () => {
    renderView(viewProps({ notes: [note()] }));
    const plain = container.innerHTML;
    expect(section()).toBeNull();
    withSticky(null);
    expect(container.innerHTML).toBe(plain);
    withSticky(snapshot([], "notInstalled"));
    expect(container.innerHTML).toBe(plain);
  });

  it("shows the Windows notes under the user's own, read only, newest first as given", () => {
    withSticky(snapshot([sticky({ id: "a", text: "first", title: "first" }), sticky({ id: "b", text: "שלום עולם", title: "שלום עולם", colour: "blue" })]));
    expect(rows().map((r) => r.dataset.noteId)).toEqual(["n1"]);
    expect(stickyRows().map((r) => r.dataset.stickyId)).toEqual(["a", "b"]);
    // the section comes after the own list
    expect(rows()[0].compareDocumentPosition(section()!) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(section()!.textContent).toContain("Windows Sticky Notes");
    expect(section()!.querySelector("[data-sticky-count]")!.textContent).toBe("2");
    const hebrew = stickyRows()[1].querySelector<HTMLElement>("[data-sticky-text]")!;
    expect(hebrew.textContent).toBe("שלום עולם");
    expect(hebrew.getAttribute("dir")).toBe("rtl");
    expect(stickyRows()[0].querySelector<HTMLElement>("[data-sticky-text]")!.getAttribute("dir")).toBe("ltr");
    expect(hebrew.className).toContain("line-clamp-3");
    expect(hebrew.className).toContain("bidi");
    // read only: no pin / copy / delete on a Sticky Note
    for (const r of stickyRows()) {
      expect(r.querySelector('button[aria-label="Pin note"], button[aria-label="Copy note"], button[aria-label="Delete note"]')).toBeNull();
      expect(r.textContent).toMatch(/2 hr|2 hours/);
    }
  });

  it("marks each note with its own colour and keeps the mark out of the accessibility tree", () => {
    withSticky(snapshot([sticky({ id: "a", colour: "yellow" }), sticky({ id: "b", colour: "green" }), sticky({ id: "c", colour: "charcoal" })]));
    const marks = stickyRows().map((r) => r.querySelector<HTMLElement>("[data-sticky-mark]")!);
    expect(marks.map((m) => m.dataset.stickyMark)).toEqual(["yellow", "green", "charcoal"]);
    const colours = marks.map((m) => m.style.background);
    expect(new Set(colours).size).toBe(3);
    expect(colours.every((c) => c !== "")).toBe(true);
    for (const m of marks) expect(m.getAttribute("aria-hidden")).toBe("true");
  });

  it("collapses to three with a count and expands and collapses again", () => {
    const many = Array.from({ length: 7 }, (_, i) => sticky({ id: `s${i}`, text: `note ${i}`, title: `note ${i}` }));
    withSticky(snapshot(many));
    expect(stickyRows()).toHaveLength(3);
    const toggle = container.querySelector<HTMLElement>("button[data-sticky-toggle]")!;
    expect(toggle.textContent).toBe("Show 4 more");
    expect(toggle.getAttribute("aria-expanded")).toBe("false");
    act(() => toggle.click());
    expect(stickyRows()).toHaveLength(7);
    expect(container.querySelector("button[data-sticky-toggle]")!.textContent).toBe("Show fewer");
    expect(container.querySelector("button[data-sticky-toggle]")!.getAttribute("aria-expanded")).toBe("true");
    act(() => container.querySelector<HTMLElement>("button[data-sticky-toggle]")!.click());
    expect(stickyRows()).toHaveLength(3);
    // three or fewer: nothing to collapse
    withSticky(snapshot(many.slice(0, 3)));
    expect(container.querySelector("button[data-sticky-toggle]")).toBeNull();
  });

  it("opens Sticky Notes from the section action, from a click on a note and from its own action, once each", () => {
    const { onOpen } = withSticky(snapshot([sticky({ id: "a" })]));
    const header = [...section()!.querySelectorAll("button")].find((b) => b.textContent === "Open in Sticky Notes" && !b.closest("li"))!;
    act(() => header.click());
    expect(onOpen).toHaveBeenCalledTimes(1);
    act(() => stickyRows()[0].click());
    expect(onOpen).toHaveBeenCalledTimes(2);
    act(() => stickyRows()[0].querySelector<HTMLElement>("button[data-sticky-open]")!.click());
    expect(onOpen).toHaveBeenCalledTimes(3);
    const inRow = [...stickyRows()[0].querySelectorAll("button")].find((b) => b.textContent === "Open in Sticky Notes")!;
    act(() => inRow.click());
    expect(onOpen).toHaveBeenCalledTimes(4);
    expect(stickyRows()[0].querySelector("button[data-sticky-open]")!.getAttribute("aria-label")).toBe("Open in Sticky Notes: Pick up the dry cleaning");
  });

  it("says calmly when there are no Sticky Notes yet, with a way to open the app", () => {
    const { onOpen } = withSticky(snapshot([], "noData"));
    expect(section()!.querySelector('[data-state][role="status"]')).not.toBeNull();
    expect(section()!.textContent).toContain("No Sticky Notes yet");
    const open = [...section()!.querySelectorAll("button")].find((b) => b.textContent === "Open in Sticky Notes")!;
    act(() => open.click());
    expect(onOpen).toHaveBeenCalledTimes(1);
    withSticky(snapshot([], "ok"));
    expect(section()!.textContent).toContain("No Sticky Notes yet");
  });

  it("says calmly that they are not available when the layout is unknown or unreadable: a status block, no error, no action", () => {
    for (const availability of ["unsupported", "unavailable"] as const) {
      withSticky(snapshot([], availability));
      expect(section()!.textContent).toContain("Sticky Notes isn't available");
      expect(section()!.querySelector('[role="alert"]')).toBeNull();
      expect(section()!.querySelector("button")).toBeNull();
      // the user's own notes are untouched
      expect(rows()).toHaveLength(1);
    }
  });

  it("stacks under an empty own list without taking over the tab, and waits while the own list is unknown", () => {
    withSticky(snapshot([sticky({ id: "a" })]), { notes: [], composer: <div data-composer /> });
    expect(container.textContent).toContain("No notes yet");
    expect(stickyRows()).toHaveLength(1);
    expect(container.querySelector("[data-composer]")).not.toBeNull();
    // the own empty state is not the tab-filling block there
    const ownEmpty = container.querySelector<HTMLElement>('[data-state][role="status"]')!;
    expect(ownEmpty.parentElement!.className).toContain("flex-shrink-0");

    withSticky(snapshot([sticky({ id: "a" })]), { notes: [], loading: true });
    expect(section()).toBeNull();
    withSticky(snapshot([sticky({ id: "a" })]), { notes: [], loadFailed: true });
    expect(section()).toBeNull();
    expect(container.textContent).toContain("Couldn't load the notes");
  });

  it("creates no timers of its own", () => {
    withSticky(snapshot(Array.from({ length: 5 }, (_, i) => sticky({ id: `s${i}` }))));
    expect(vi.getTimerCount()).toBe(0);
  });
});

describe("NotesTab · Windows Sticky Notes", () => {
  async function mount() {
    await act(async () => {
      root.render(<NotesTab />);
    });
  }

  it("shows the notes the store knows and opens Sticky Notes through the backend, never the Island Center", async () => {
    mocks.notes = [note({ id: "own1" })];
    mocks.sticky = snapshot([sticky({ id: "s1" }), sticky({ id: "s2", text: "Second", title: "Second" })]);
    await mount();
    expect(stickyRows()).toHaveLength(2);
    expect(rows()).toHaveLength(1);
    await act(async () => stickyRows()[1].click());
    expect(mocks.stickyOpen).toHaveBeenCalledTimes(1);
    expect(mocks.openCenter).not.toHaveBeenCalled();
    // an own note still opens in the Center
    await act(async () => rows()[0].click());
    expect(mocks.openCenter).toHaveBeenLastCalledWith("note:own1");
  });

  it("has no section while the store has no answer (outside the app, before the first answer)", async () => {
    mocks.notes = [note({ id: "own1" })];
    mocks.sticky = null;
    await mount();
    expect(section()).toBeNull();
    expect(rows()).toHaveLength(1);
  });
});
