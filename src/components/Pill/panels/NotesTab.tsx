import { useCallback, useEffect, useRef, useState, type MouseEvent, type ReactNode } from "react";
import { useMinute } from "../../../hooks/useClock";
import { useNotes } from "../../../hooks/useNotes";
import { relativePast } from "../../../lib/dateFormat";
import { t } from "../../../lib/i18n";
import { ipc, type Note } from "../../../lib/ipc";
import { CopyIcon, NoteIcon, PinIcon, TrashIcon } from "../ui/icons";
import { EmptyState, PillButton, SectionLabel, SYSTEM_COLORS } from "../ui/primitives";

const FLASH_MS = 1800;
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

function ActionButton({ label, onClick, children, active = false }: { label: string; onClick: () => void; children: ReactNode; active?: boolean }) {
  return (
    <button
      type="button"
      className={`w-7 h-7 rounded-full flex items-center justify-center flex-shrink-0 hover:bg-white/10 transition-colors ${
        active ? "text-white" : "text-white/40 hover:text-white"
      }`}
      aria-label={label}
      title={label}
      onClick={(e: MouseEvent) => {
        // The row itself opens the note; an action must not.
        e.stopPropagation();
        onClick();
      }}
    >
      {children}
    </button>
  );
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
      dir="ltr"
      data-note-id={note.id}
      className="group flex flex-col gap-1 rounded-[18px] bg-white/[0.06] hover:bg-white/[0.09] pl-3.5 pr-2 pt-2.5 pb-1.5 cursor-pointer transition-colors"
      onClick={() => onOpen(note.id)}
    >
      <button
        type="button"
        data-note-open
        aria-label={`${t("notes.open")}: ${preview.slice(0, OPEN_LABEL_CHARS)}`}
        className="block w-full text-left rounded-[10px] focus-visible:outline focus-visible:outline-2 focus-visible:outline-white/50"
      >
        <span
          className="block text-[13.5px] leading-snug text-white/90 line-clamp-3 whitespace-pre-wrap break-words pr-1"
          dir="auto"
          data-note-text
          style={{ unicodeBidi: "plaintext" }}
        >
          {preview}
        </span>
      </button>
      <div className="flex items-center gap-1.5 min-h-[28px]">
        {note.pinned && (
          <span className="flex-shrink-0" style={{ color: SYSTEM_COLORS.orange }} title={t("notes.pinned")} role="img" aria-label={t("notes.pinned")}>
            <PinIcon size={12} fill="currentColor" />
          </span>
        )}
        <span className="text-[11px] text-white/35 tabular-nums truncate" dir="auto">
          {relativePast(note.updatedAt, nowMs)}
        </span>
        <span className="ml-auto flex items-center gap-0.5">
          {copied && (
            <span className="text-[11px] font-medium mr-1" style={{ color: SYSTEM_COLORS.green }} role="status" dir="auto">
              {t("notes.copied")}
            </span>
          )}
          {/* Hidden until the row is hovered or focused, but always there for keyboard and screen readers. */}
          <span className={`flex items-center gap-0.5 transition-opacity ${copied || confirming ? "" : "opacity-0 group-hover:opacity-100 group-focus-within:opacity-100"}`}>
            <ActionButton label={t(note.pinned ? "notes.unpin" : "notes.pin")} onClick={() => onTogglePin(note.id)} active={note.pinned}>
              <PinIcon size={14} fill={note.pinned ? "currentColor" : "none"} />
            </ActionButton>
            <ActionButton label={t("notes.copy")} onClick={() => onCopy(note.id)}>
              <CopyIcon size={14} />
            </ActionButton>
            {/* One button element in both states so the focus stays on it; set apart from pin and copy. */}
            <button
              type="button"
              data-delete-confirming={confirming ? "true" : undefined}
              className={`ml-2 h-7 rounded-full flex items-center justify-center flex-shrink-0 transition-colors ${
                confirming ? "px-2.5 text-[11.5px] font-semibold" : "w-7 text-white/40 hover:text-white hover:bg-white/10"
              }`}
              style={confirming ? { background: `color-mix(in srgb, ${SYSTEM_COLORS.red} 22%, transparent)`, color: SYSTEM_COLORS.red } : undefined}
              aria-label={confirming ? t("notes.confirmDeleteLabel") : t("notes.delete")}
              title={confirming ? t("notes.confirmDeleteLabel") : t("notes.delete")}
              onBlur={cancelConfirm}
              onKeyDown={(e) => {
                if (e.key === "Escape" && confirming) {
                  e.stopPropagation();
                  cancelConfirm();
                }
              }}
              onClick={(e: MouseEvent) => {
                e.stopPropagation();
                onDelete();
              }}
            >
              {confirming ? <span dir="auto">{t("notes.confirmDelete")}</span> : <TrashIcon size={14} />}
            </button>
          </span>
        </span>
      </div>
    </li>
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
}

/** Pure rendering of the notes list (the tour renders it with mock data, no IPC). */
export function NotesView({
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
}: NotesViewProps) {
  if (loading && notes.length === 0) return <div dir="ltr" className="flex-1" aria-busy="true" />;
  if (loadFailed && notes.length === 0) {
    return (
      <div dir="ltr" className="flex-1 flex flex-col items-center justify-center">
        <EmptyState icon={<NoteIcon size={22} />} title={t("notes.loadFailed")}>
          {onRetry && (
            <div className="mt-3">
              <PillButton variant="tinted" className="h-[32px] px-4 text-[12.5px]" onClick={onRetry}>
                {t("notes.retry")}
              </PillButton>
            </div>
          )}
        </EmptyState>
      </div>
    );
  }
  if (notes.length === 0) {
    return (
      <div dir="ltr" className="flex-1 flex flex-col items-center justify-center">
        <EmptyState icon={<NoteIcon size={22} />} title={t("notes.empty")} subtitle={t("notes.emptyHint")}>
          <div className="mt-3">
            <PillButton variant="tinted" className="h-[32px] px-4 text-[12.5px]" onClick={onNew}>
              {t("notes.new")}
            </PillButton>
          </div>
        </EmptyState>
      </div>
    );
  }
  return (
    <div dir="ltr" className="flex flex-col gap-2">
      <SectionLabel
        trailing={
          <PillButton variant="tinted" className="h-[26px] px-3 text-[12px]" onClick={onNew}>
            {t("notes.new")}
          </PillButton>
        }
      >
        {saveFailed ? (
          <span style={{ color: SYSTEM_COLORS.orange }} role="alert">
            {t("notes.saveFailed")}
          </span>
        ) : (
          t("notes.list")
        )}
      </SectionLabel>
      <ul className="flex flex-col gap-1.5" aria-label={t("notes.list")}>
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

/** The island's notes: reading, pinning, copying and deleting here; writing happens in the Island Center. */
export function NotesTab() {
  const { notes, loaded, loadFailed, retry, remove, togglePin } = useNotes();
  const nowMs = useMinute().getTime();

  // One short-lived flag at a time ("Copied" on a note, or "couldn't save"); the timer exists only after a click.
  const [flash, setFlash] = useState<{ copiedId: string | null; saveFailed: boolean }>({ copiedId: null, saveFailed: false });
  const flashTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const showFlash = useCallback((next: { copiedId: string | null; saveFailed: boolean }) => {
    if (flashTimer.current !== null) clearTimeout(flashTimer.current);
    setFlash(next);
    flashTimer.current = setTimeout(() => {
      flashTimer.current = null;
      setFlash({ copiedId: null, saveFailed: false });
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
      if (ok) showFlash({ copiedId: id, saveFailed: false });
    });
  };
  const saved = (ok: boolean) => {
    if (!ok) showFlash({ copiedId: null, saveFailed: true });
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
      onNew={() => void ipc.openCenter("notes-new")}
      onOpen={(id) => void ipc.openCenter(`note:${id}`)}
      onTogglePin={(id) => void togglePin(id).then(saved)}
      onCopy={copy}
      onRemove={(id) => void remove(id).then(saved)}
    />
  );
}
