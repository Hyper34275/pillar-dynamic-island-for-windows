import { useCallback, useEffect, useRef, useState, type KeyboardEvent, type ReactNode, type Ref } from "react";
import { useMinute } from "../../../hooks/useClock";
import { useNotes } from "../../../hooks/useNotes";
import { relativePast } from "../../../lib/dateFormat";
import { t } from "../../../lib/i18n";
import { ipc, onEvent, type Note } from "../../../lib/ipc";
import { color, control, icon } from "../../../design/tokens";
import { textDirection } from "../../../design/direction";
import { islandTyping, noteDraft } from "../../../lib/notes/typing";
import { ActionButton, RoundButton } from "../ui/controls";
import { CopyIcon, NoteIcon, PinIcon, TrashIcon } from "../ui/icons";
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
      <textarea
        ref={textareaRef}
        value={value}
        rows={value ? 4 : 2}
        maxLength={MAX_NOTE_CHARS}
        placeholder={t("notes.placeholder")}
        aria-label={t("notes.composerLabel")}
        spellCheck={false}
        dir={textDirection(value)}
        className="bidi block w-full resize-none bg-transparent text-body text-fg placeholder:text-fg-tertiary outline-none"
        onChange={(e) => onChange(e.target.value)}
        // A click in a window that is not active may not move the page's focus; the press itself asks too.
        onPointerDown={onFocus}
        onFocus={onFocus}
        onBlur={onBlur}
        onKeyDown={onKeyDown}
      />
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
}

/** Pure rendering of the notes list (the tour renders it with mock data, no IPC). */
export function NotesView(props: NotesViewProps) {
  const { notes, loading = false, loadFailed = false, composer } = props;
  const list = <NotesList {...props} />;
  // While the list is unknown the composer waits too: the panel shows one thing at a time.
  if (!composer || (notes.length === 0 && (loading || loadFailed))) return list;
  return (
    <div className="flex flex-col gap-2 flex-1">
      {composer}
      {list}
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
}: NotesViewProps) {
  if (loading && notes.length === 0) return <div className="flex-1" aria-busy="true" />;
  if (loadFailed && notes.length === 0) {
    return <ErrorState icon={<NoteIcon size={STATE_ICON} />} title={t("notes.loadFailed")} action={onRetry ? { label: t("notes.retry"), onPress: onRetry } : undefined} />;
  }
  if (notes.length === 0) {
    return (
      <EmptyState
        icon={<NoteIcon size={STATE_ICON} />}
        title={t("notes.empty")}
        hint={t("notes.emptyHint")}
        action={{ label: t(onOpenApp ? "notes.openApp" : "notes.new"), onPress: onOpenApp ?? onNew }}
      />
    );
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
    />
  );
}
