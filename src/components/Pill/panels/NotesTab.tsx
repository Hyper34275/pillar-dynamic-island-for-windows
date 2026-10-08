import { useCallback, useEffect, useMemo, useRef, useState, type KeyboardEvent, type ReactNode, type Ref } from "react";
import { useMinute } from "../../../hooks/useClock";
import { useNotes } from "../../../hooks/useNotes";
import { relativePast } from "../../../lib/dateFormat";
import { t } from "../../../lib/i18n";
import { ipc, onEvent, type Note } from "../../../lib/ipc";
import { color, control, icon } from "../../../design/tokens";
import { textDirection } from "../../../design/direction";
import { stickyHasSection, type StickyNote, type StickySnapshot } from "../../../lib/notes/sticky";
import { stickyMarkColor } from "../../../lib/notes/stickyColour";
import { islandTyping, noteDraft } from "../../../lib/notes/typing";
import { useStickyNotes } from "../../../lib/notes/useSticky";
import { ActionButton, RoundButton } from "../ui/controls";
import { CopyIcon, NoteIcon, PencilIcon, PinIcon, TrashIcon } from "../ui/icons";
import { EmptyState, ErrorState, STATE_ICON } from "../ui/states";
import { HeaderActionButton } from "../HeaderAction";

const FLASH_MS = 1800;
/** The same limit Rust applies (notes.rs MAX_TEXT_CHARS). */
const MAX_NOTE_CHARS = 10_000;
/** How long the trash button waits for its confirming second click before going back. */
const CONFIRM_DELETE_MS = 3000;
const OPEN_LABEL_CHARS = 60;

/**
 * What the list shows of a note: leading blank lines dropped and runs of blank lines collapsed, so
 * the three clamped lines are real text. The stored (and copied) text is never changed.
 */
export function notePreview(text: string): string {
  return text.replace(/^\s+/, "").replace(/(?:[ \t]*\r?\n){2,}/g, "\n");
}

interface NoteRowProps {
  note: Note;
  nowMs: number;
  copied: boolean;
  onOpen: (id: string) => void;
  onTogglePin: (id: string) => void;
  onCopy: (id: string) => void;
  onRemove: (id: string) => void;
}

function NoteRow({ note, nowMs, copied, onOpen, onTogglePin, onCopy, onRemove }: NoteRowProps) {
  // Deleting is permanent (no recycle bin), so the trash button asks for a second click first.
  const [confirming, setConfirming] = useState(false);
  const confirmTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const cancelConfirm = useCallback(() => {
    if (confirmTimer.current !== null) {
      clearTimeout(confirmTimer.current);
      confirmTimer.current = null;
    }
    setConfirming(false);
  }, []);
  useEffect(
    () => () => {
      if (confirmTimer.current !== null) clearTimeout(confirmTimer.current);
    },
    []
  );
  const onDelete = () => {
    if (confirming) {
      cancelConfirm();
      onRemove(note.id);
      return;
    }
    setConfirming(true);
    confirmTimer.current = setTimeout(() => {
      confirmTimer.current = null;
      setConfirming(false);
    }, CONFIRM_DELETE_MS);
  };

  const preview = notePreview(note.text);
  // The li is a plain list item. The preview is its one primary button (keyboard and screen readers);
  // a click anywhere else on the row reaches the li's handler for the mouse. The action buttons are
  // siblings of the preview button, never inside it.
  return (
    <li
      data-note-id={note.id}
      className="group flex flex-col gap-1 ci-surface hover:bg-surface-hover rounded-surface p-card-pad cursor-pointer transition-colors"
      onClick={() => onOpen(note.id)}
    >
      <button
        type="button"
        data-note-open
        aria-label={`${t("notes.open")}: ${preview.slice(0, OPEN_LABEL_CHARS)}`}
        className="ci-bare block w-full text-start rounded-control"
      >
        <span className="bidi block text-body text-fg line-clamp-3 whitespace-pre-wrap break-words" dir={textDirection(preview)} data-note-text>
          {preview}
        </span>
      </button>
      {/* The row is the 28px RoundButton height; the card's 12 padding sits below it, so the time baseline is pulled up 4 (-mb-1) to balance the 12 above the text. */}
      <div className="flex items-center gap-2 -mb-1" style={{ minHeight: control.round }}>
        {note.pinned && (
          <span className="flex-shrink-0 flex" style={{ color: color.warning }} title={t("notes.pinned")} role="img" aria-label={t("notes.pinned")}>
            <PinIcon size={icon.small} fill="currentColor" />
          </span>
        )}
        <span className="text-meta text-fg-tertiary tabular-nums truncate">{relativePast(note.updatedAt, nowMs)}</span>
        <span className="ms-auto flex items-center gap-2">
          {copied && (
            <span className="text-meta" style={{ color: color.positive }} role="status">
              {t("notes.copied")}
            </span>
          )}
          {/* Hidden until the row is hovered or focused, but always there for keyboard and screen readers. */}
          <span className={`flex items-center gap-2 transition-opacity ${copied || confirming ? "" : "opacity-0 group-hover:opacity-100 group-focus-within:opacity-100"}`}>
            <RoundButton
              fill="none"
              tint={note.pinned ? color.warning : color.fgSecondary}
              ariaLabel={t(note.pinned ? "notes.unpin" : "notes.pin")}
              title={t(note.pinned ? "notes.unpin" : "notes.pin")}
              onPress={() => onTogglePin(note.id)}
            >
              <PinIcon size={icon.small} fill={note.pinned ? "currentColor" : "none"} />
            </RoundButton>
            <RoundButton fill="none" ariaLabel={t("notes.copy")} title={t("notes.copy")} onPress={() => onCopy(note.id)}>
              <CopyIcon size={icon.small} />
            </RoundButton>
            {/* One button element in both states so the focus stays on it. */}
            <RoundButton
              grow={confirming}
              fill={confirming ? "tint" : "none"}
              tint={confirming ? color.destructiveText : color.fgSecondary}
              ariaLabel={confirming ? t("notes.confirmDeleteLabel") : t("notes.delete")}
              title={confirming ? t("notes.confirmDeleteLabel") : t("notes.delete")}
              onPress={onDelete}
              buttonProps={{
                "data-delete-confirming": confirming ? "true" : undefined,
                onBlur: cancelConfirm,
                onKeyDown: (e) => {
                  if (e.key === "Escape" && confirming) {
                    e.stopPropagation();
                    cancelConfirm();
                  }
                },
              }}
            >
              {confirming ? <span>{t("notes.confirmDelete")}</span> : <TrashIcon size={icon.small} />}
            </RoundButton>
          </span>
        </span>
      </div>
    </li>
  );
}

export interface NoteComposerProps {
  value: string;
  onChange: (text: string) => void;
  /** Ctrl+Enter or the Save button, with text that is not blank. */
  onSave: () => void;
  onFocus?: () => void;
  onBlur?: () => void;
  saving?: boolean;
  /** The last note was just saved: the hint says so for a moment. */
  justSaved?: boolean;
  textareaRef?: Ref<HTMLTextAreaElement>;
}

/**
 * Writing a note inside the island. Every key stays in the text box (the island's own shortcuts,
 * Escape and the arrows, must not act while typing): Ctrl+Enter saves, Escape leaves the box.
 */
export function NoteComposer({ value, onChange, onSave, onFocus, onBlur, saving = false, justSaved = false, textareaRef }: NoteComposerProps) {
  const canSave = value.trim().length > 0 && !saving;
  const onKeyDown = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    e.stopPropagation();
    if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
      e.preventDefault();
      if (canSave) onSave();
    } else if (e.key === "Escape") {
      e.preventDefault();
      e.currentTarget.blur();
    }
  };
  return (
    <div className="ci-surface ci-field rounded-surface p-card-pad flex flex-col gap-2 flex-shrink-0" data-note-composer>
      {/* A pen at the leading edge and a placeholder at secondary contrast: the box reads as the place to write. */}
      <label className="flex items-start gap-2 cursor-text">
        <PencilIcon size={icon.small} className="flex-shrink-0 text-fg-secondary" style={{ marginTop: 2 }} aria-hidden="true" />
        <textarea
        ref={textareaRef}
        value={value}
        rows={value ? 4 : 2}
        maxLength={MAX_NOTE_CHARS}
        placeholder={t("notes.placeholder")}
        aria-label={t("notes.composerLabel")}
        spellCheck={false}
        dir={textDirection(value)}
        className="bidi block w-full min-w-0 flex-1 resize-none bg-transparent text-body text-fg placeholder:text-fg-secondary outline-none"
        onChange={(e) => onChange(e.target.value)}
        // A click in a window that is not active may not move the page's focus; the press itself asks too.
        onPointerDown={onFocus}
        onFocus={onFocus}
        onBlur={onBlur}
        onKeyDown={onKeyDown}
        />
      </label>
      {/* An empty box is just the field, so the list keeps the room; the row comes with the first character. */}
      {(value || justSaved) && (
        <div className="flex items-center gap-2">
          <span className="text-meta text-fg-tertiary me-auto truncate" role="status">
            {justSaved && !value ? t("notes.saved") : t("notes.saveHint")}
          </span>
          {value && (
            <ActionButton variant="primary" onPress={onSave} disabled={!canSave}>
              {t("notes.save")}
            </ActionButton>
          )}
        </div>
      )}
    </div>
  );
}

export interface NotesViewProps {
  /** Canonical order: pinned first, then newest update. */
  notes: readonly Note[];
  nowMs: number;
  onNew: () => void;
  onOpen: (id: string) => void;
  onTogglePin: (id: string) => void;
  onCopy: (id: string) => void;
  onRemove: (id: string) => void;
  /** The note whose "Copied" confirmation is showing right now. */
  copiedId?: string | null;
  /** A change could not be saved and has been reverted. */
  saveFailed?: boolean;
  /** The list has not arrived yet: nothing is shown (an empty list would claim there are no notes). */
  loading?: boolean;
  /** The list could not be read: says so, with a retry, instead of "no notes". */
  loadFailed?: boolean;
  onRetry?: () => void;
  /** Writing inside the island (NoteComposer), shown above the list once the list is known. */
  composer?: ReactNode;
  /** Opens the Island Center's Notes page; with it, the header and the empty state offer that instead of "New note". */
  onOpenApp?: () => void;
  /**
   * The user's Windows Sticky Notes, a read-only section under their own notes. Absent (the tour, the
   * gallery's older exhibits): no section. `snapshot` null = not known yet, or no backend: no section.
   */
  sticky?: StickyView;
}

export interface StickyView {
  snapshot: StickySnapshot | null;
  /** Starts the Sticky Notes app (the only way to edit them). */
  onOpen: () => void;
}

/** Pure rendering of the notes list (the tour renders it with mock data, no IPC). */
export function NotesView(props: NotesViewProps) {
  const { notes, loading = false, loadFailed = false, composer, sticky } = props;
  // While the list is unknown the composer waits too: the panel shows one thing at a time.
  const unknown = notes.length === 0 && (loading || loadFailed);
  const stickySnapshot = !unknown && sticky && stickyHasSection(sticky.snapshot) ? sticky.snapshot : null;
  const list = <NotesList {...props} stacked={stickySnapshot !== null} />;
  if (unknown || (!composer && !stickySnapshot)) return list;
  return (
    <div className="flex flex-col gap-2 flex-1">
      {composer}
      {list}
      {stickySnapshot && sticky && <StickySection snapshot={stickySnapshot} nowMs={props.nowMs} onOpen={sticky.onOpen} />}
    </div>
  );
}

function NotesList({
  notes,
  nowMs,
  onNew,
  onOpen,
  onTogglePin,
  onCopy,
  onRemove,
  copiedId = null,
  saveFailed = false,
  loading = false,
  loadFailed = false,
  onRetry,
  onOpenApp,
  composer,
  stacked = false,
}: NotesViewProps & { stacked?: boolean }) {
  if (loading && notes.length === 0) return <div className="flex-1" aria-busy="true" />;
  if (loadFailed && notes.length === 0) {
    return <ErrorState icon={<NoteIcon size={STATE_ICON} />} title={t("notes.loadFailed")} action={onRetry ? { label: t("notes.retry"), onPress: onRetry } : undefined} />;
  }
  if (notes.length === 0) {
    const empty = (
      <EmptyState
        icon={<NoteIcon size={STATE_ICON} />}
        title={t("notes.empty")}
        hint={t(composer ? "notes.emptyHintComposer" : "notes.emptyHint")}
        action={{ label: t(onOpenApp ? "notes.openApp" : "notes.new"), onPress: onOpenApp ?? onNew }}
      />
    );
    // With the Sticky Notes section below, the empty state takes its own height instead of all the room.
    return stacked ? <div className="flex flex-col flex-shrink-0">{empty}</div> : empty;
  }
  return (
    <div className="flex flex-col gap-2">
      {/* The section row: label at the card text inset (12), the action at the trailing edge. */}
      <div className="flex items-center justify-between gap-2 px-3" style={{ minHeight: control.round }}>
        {saveFailed ? (
          <span className="text-micro" style={{ color: color.warning }} role="alert">
            {t("notes.saveFailed")}
          </span>
        ) : (
          <span className="text-micro text-fg-tertiary">{t("notes.list")}</span>
        )}
        <HeaderActionButton onPress={onOpenApp ?? onNew}>{t(onOpenApp ? "notes.openApp" : "notes.new")}</HeaderActionButton>
      </div>
      <ul className="flex flex-col gap-2" aria-label={t("notes.list")}>
        {notes.map((note) => (
          <NoteRow
            key={note.id}
            note={note}
            nowMs={nowMs}
            copied={copiedId === note.id}
            onOpen={onOpen}
            onTogglePin={onTogglePin}
            onCopy={onCopy}
            onRemove={onRemove}
          />
        ))}
      </ul>
    </div>
  );
}

/** This many Sticky Notes show before "Show N more". */
const STICKY_COLLAPSED = 3;
/** The note's colour bar, as wide as the calendar rows' colour bars. */
const STICKY_BAR_WIDTH = 4;

/**
 * One Windows Sticky Note: read only. Same card as an own note (surface, 12 padding, three clamped
 * lines, relative time), with the note's colour as a rounded bar at the leading edge (the calendar
 * rows' grammar for an identity colour); a click, or the action that shows on hover and focus, opens
 * the Sticky Notes app (editing happens there).
 */
function StickyRow({ note, nowMs, onOpen }: { note: StickyNote; nowMs: number; onOpen: () => void }) {
  const preview = notePreview(note.text);
  return (
    <li
      data-sticky-id={note.id}
      className="group flex gap-3 ci-surface hover:bg-surface-hover rounded-surface p-card-pad cursor-pointer transition-colors"
      onClick={onOpen}
    >
      <span
        aria-hidden="true"
        data-sticky-mark={note.colour}
        className="ci-mark flex-shrink-0 self-stretch rounded-full"
        style={{ width: STICKY_BAR_WIDTH, background: stickyMarkColor(note.colour) }}
      />
      <div className="flex min-w-0 flex-1 flex-col gap-1">
        <button
          type="button"
          data-sticky-open
          aria-label={`${t("sticky.open")}: ${preview.slice(0, OPEN_LABEL_CHARS)}`}
          className="ci-bare block w-full text-start rounded-control"
        >
          <span className="bidi block text-body text-fg line-clamp-3 whitespace-pre-wrap break-words" dir={textDirection(preview)} data-sticky-text>
            {preview}
          </span>
        </button>
        {/* As in an own note: the 28px action row sits in the card's bottom padding (-mb-1 balances the 12 above the text). */}
        <div className="flex items-center gap-2 -mb-1" style={{ minHeight: control.round }}>
          {note.updatedAt > 0 && <span className="text-meta text-fg-tertiary tabular-nums truncate">{relativePast(note.updatedAt, nowMs)}</span>}
          {/* Hidden until the row is hovered or focused, but always there for keyboard and screen readers. */}
          <span className="ms-auto transition-opacity opacity-0 group-hover:opacity-100 group-focus-within:opacity-100">
            <HeaderActionButton onPress={onOpen}>{t("sticky.open")}</HeaderActionButton>
          </span>
        </div>
      </div>
    </li>
  );
}

/**
 * The user's Windows Sticky Notes under their own notes: a section row (label with the count, the open
 * action), then up to three cards with "Show N more" for the rest, or one calm state (empty, or not
 * available on this computer: never an error). Not installed is handled by the caller (no section).
 */
function StickySection({ snapshot, nowMs, onOpen }: { snapshot: StickySnapshot; nowMs: number; onOpen: () => void }) {
  const [expanded, setExpanded] = useState(false);
  const { availability, notes } = snapshot;
  const hasNotes = availability === "ok" && notes.length > 0;
  const shown = expanded ? notes : notes.slice(0, STICKY_COLLAPSED);
  const rest = notes.length - shown.length;
  return (
    <section className="flex flex-col gap-2" data-sticky-section aria-label={t("sticky.title")}>
      <div className="flex items-center justify-between gap-2 px-3" style={{ minHeight: control.round }}>
        {/* The count is its own element, so it sits after the label in the layout's direction whatever the label's bidi runs do. */}
        <span className="flex min-w-0 items-baseline gap-2 text-micro text-fg-tertiary">
          <span className="truncate">{t("sticky.title")}</span>
          {hasNotes && (
            <span className="flex-shrink-0 tabular-nums" data-sticky-count>
              {notes.length}
            </span>
          )}
        </span>
        {hasNotes && <HeaderActionButton onPress={onOpen}>{t("sticky.open")}</HeaderActionButton>}
      </div>
      {hasNotes ? (
        <>
          <ul className="flex flex-col gap-2" aria-label={t("sticky.title")}>
            {shown.map((note) => (
              <StickyRow key={note.id} note={note} nowMs={nowMs} onOpen={onOpen} />
            ))}
          </ul>
          {notes.length > STICKY_COLLAPSED && (
            <div className="flex justify-center">
              <button
                type="button"
                data-sticky-toggle
                aria-expanded={expanded}
                className="ci-link hit-area rounded-control text-label text-accent whitespace-nowrap select-none transition-opacity duration-150 hover:opacity-80 active:opacity-60"
                onClick={() => setExpanded((e) => !e)}
              >
                {expanded ? t("sticky.less") : t("sticky.more", { n: rest })}
              </button>
            </div>
          )}
        </>
      ) : availability === "ok" || availability === "noData" ? (
        <EmptyState icon={<NoteIcon size={STATE_ICON} />} title={t("sticky.empty")} hint={t("sticky.emptyHint")} action={{ label: t("sticky.open"), onPress: onOpen }} />
      ) : (
        <EmptyState icon={<NoteIcon size={STATE_ICON} />} title={t("sticky.unavailable")} hint={t("sticky.unavailableHint")} />
      )}
    </section>
  );
}

interface Flash {
  copiedId: string | null;
  saveFailed: boolean;
  justSaved: boolean;
}

const NO_FLASH: Flash = { copiedId: null, saveFailed: false, justSaved: false };

/**
 * The island takes the keyboard for the composer only while its text box has focus. The window is
 * non-activating otherwise, so the box asks the backend first (a click in it is the user's input
 * that lets the island come forward) and gives the keyboard back when it loses focus, the tab
 * closes or another window takes the keyboard (`island-keyboard-ended`).
 */
function useComposerKeyboard() {
  const textareaRef = useRef<HTMLTextAreaElement | null>(null);
  // Between asking and the answer the activation itself moves the focus around; that is not "done typing".
  const acquiring = useRef(false);

  const release = useCallback(() => {
    if (!islandTyping.get()) return;
    islandTyping.set(false);
    void ipc.islandKeyboard(false);
  }, []);

  const onFocus = useCallback(() => {
    if (islandTyping.get() || acquiring.current) return;
    acquiring.current = true;
    void ipc.islandKeyboard(true).then((ok) => {
      acquiring.current = false;
      if (!ok) return;
      // The tab closed while the island was taking the keyboard (its cleanup had nothing to release
      // yet): give it back now rather than leave the window active with no text box.
      if (!textareaRef.current) {
        void ipc.islandKeyboard(false);
        return;
      }
      islandTyping.set(true);
      textareaRef.current?.focus();
    });
  }, []);

  // When another window took the keyboard the backend has already ended it; telling it again is harmless.
  const onBlur = useCallback(() => {
    if (!acquiring.current) release();
  }, [release]);

  useEffect(() => {
    const unlisten = onEvent<unknown>("island-keyboard-ended", () => {
      islandTyping.set(false);
      textareaRef.current?.blur();
    });
    return () => {
      unlisten();
      release();
    };
  }, [release]);

  return { textareaRef, onFocus, onBlur };
}

/** The island's notes: writing a new one, reading, pinning, copying and deleting here; a click on a note opens it in the Island Center. */
export function NotesTab() {
  const { notes, loaded, loadFailed, retry, add, remove, togglePin } = useNotes();
  const stickySnapshot = useStickyNotes();
  const sticky = useMemo<StickyView>(() => ({ snapshot: stickySnapshot, onOpen: () => void ipc.stickyNotesOpen() }), [stickySnapshot]);
  const nowMs = useMinute().getTime();
  const keyboard = useComposerKeyboard();
  // The draft outlives the tab (the island closing unmounts it) until it is saved.
  const [draft, setDraftState] = useState(noteDraft.get);
  const setDraft = useCallback((text: string) => {
    noteDraft.set(text);
    setDraftState(text);
  }, []);
  const [saving, setSaving] = useState(false);

  // One short-lived flag at a time ("Copied" on a note, "Saved", or "couldn't save"); the timer exists only after a click.
  const [flash, setFlash] = useState<Flash>(NO_FLASH);
  const flashTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const showFlash = useCallback((next: Partial<Flash>) => {
    if (flashTimer.current !== null) clearTimeout(flashTimer.current);
    setFlash({ ...NO_FLASH, ...next });
    flashTimer.current = setTimeout(() => {
      flashTimer.current = null;
      setFlash(NO_FLASH);
    }, FLASH_MS);
  }, []);
  useEffect(
    () => () => {
      if (flashTimer.current !== null) clearTimeout(flashTimer.current);
    },
    []
  );

  const copy = (id: string) => {
    const note = notes.find((n) => n.id === id);
    if (!note) return;
    void ipc.copyTextToClipboard(note.text).then((ok) => {
      if (ok) showFlash({ copiedId: id });
    });
  };
  const saved = (ok: boolean) => {
    if (!ok) showFlash({ saveFailed: true });
  };
  const save = () => {
    const text = draft;
    if (!text.trim() || saving) return;
    setSaving(true);
    void add(text).then((ok) => {
      setSaving(false);
      if (!ok) {
        showFlash({ saveFailed: true });
        return;
      }
      // Only what was saved is cleared: text typed while the save was on its way stays.
      if (noteDraft.get() === text) setDraft("");
      showFlash({ justSaved: true });
    });
  };

  return (
    <NotesView
      notes={notes}
      nowMs={nowMs}
      copiedId={flash.copiedId}
      saveFailed={flash.saveFailed}
      loading={!loaded}
      loadFailed={loadFailed}
      onRetry={retry}
      composer={
        <NoteComposer
          value={draft}
          onChange={setDraft}
          onSave={save}
          onFocus={keyboard.onFocus}
          onBlur={keyboard.onBlur}
          saving={saving}
          justSaved={flash.justSaved}
          textareaRef={keyboard.textareaRef}
        />
      }
      onOpenApp={() => void ipc.openCenter("notes")}
      onNew={() => keyboard.textareaRef.current?.focus()}
      onOpen={(id) => void ipc.openCenter(`note:${id}`)}
      onTogglePin={(id) => void togglePin(id).then(saved)}
      onCopy={copy}
      onRemove={(id) => void remove(id).then(saved)}
      sticky={sticky}
    />
  );
}
