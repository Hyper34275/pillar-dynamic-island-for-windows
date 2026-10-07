// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Note } from "../../../lib/ipc";
import { notePreview, NotesTab, NotesView, type NotesViewProps } from "./NotesTab";

const mocks = vi.hoisted(() => ({
  openCenter: vi.fn(async (_page: string) => true),
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
  return { ...original, ipc: { ...original.ipc, openCenter: mocks.openCenter, copyTextToClipboard: mocks.copyTextToClipboard } };
});

vi.mock("../../../hooks/useNotes", () => ({
  useNotes: () => ({
    notes: mocks.notes,
    loaded: mocks.loaded,
    loadFailed: mocks.loadFailed,
    retry: mocks.retry,
    add: vi.fn(async () => true),
    update: vi.fn(async () => true),
    remove: mocks.remove,
    togglePin: mocks.togglePin,
  }),
}));

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
  mocks.loaded = true;
  mocks.loadFailed = false;
  for (const fn of [mocks.openCenter, mocks.copyTextToClipboard, mocks.remove, mocks.togglePin, mocks.retry]) fn.mockClear();
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
    expect(text.getAttribute("dir")).toBe("auto");
    expect(text.className).toContain("line-clamp-3");
    expect(text.style.unicodeBidi).toBe("plaintext");
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

  it("opens the Island Center on a new note, or on the clicked note", async () => {
    mocks.notes = [note({ id: "abc123" })];
    await mount();
    const newButton = [...container.querySelectorAll("button")].find((b) => b.textContent === "New note")!;
    await act(async () => newButton.click());
    expect(mocks.openCenter).toHaveBeenLastCalledWith("notes-new");
    await act(async () => rows()[0].click());
    expect(mocks.openCenter).toHaveBeenLastCalledWith("note:abc123");
  });

  it("opens a new note from the empty state too", async () => {
    await mount();
    const newButton = [...container.querySelectorAll("button")].find((b) => b.textContent === "New note")!;
    await act(async () => newButton.click());
    expect(mocks.openCenter).toHaveBeenCalledWith("notes-new");
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
