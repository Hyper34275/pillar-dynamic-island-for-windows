import { motion, AnimatePresence } from "motion/react";
import { useMemo, useState } from "react";
import type { NoteItem, ProductivityState } from "../../../types/productivity";
import { EmptyState, Segmented, SYSTEM_COLORS } from "../ui/primitives";
import { ArchiveIcon, CalendarIcon, CheckCircleIcon, CheckIcon, NoteIcon, PencilIcon, PlusIcon, TrashIcon } from "../ui/icons";

type ProductivityView = "tasks" | "notes" | "agenda";

const VIEW_OPTIONS = [
  { id: "tasks", label: "Tasks" },
  { id: "notes", label: "Notes" },
  { id: "agenda", label: "Agenda" },
] as const;

function statusLabel(status: ProductivityState["status"]): string | null {
  switch (status) {
    case "loading":
      return "Working…";
    case "conflict":
      return "Sync conflict";
    case "degraded":
      return "Backup unavailable";
    default:
      return null;
  }
}

const rowSpring = { type: "spring" as const, stiffness: 480, damping: 36 };
const inputClass =
  "w-full bg-white/[0.08] focus:bg-white/[0.11] text-white text-[12.5px] rounded-[12px] px-3 h-9 outline-none transition-colors";

interface ProductivityModuleProps {
  state: ProductivityState;
  onAddTask: (title: string) => void;
  onToggleTask: (taskId: string) => void;
  onRemoveTask: (taskId: string) => void;
  onClearCompleted: () => void;
  onAddNote: (title: string, content: string) => void;
  onUpdateNote: (note: NoteItem) => void;
  onRemoveNote: (noteId: string) => void;
  onAddEvent: (title: string, startsAt: number, endsAt: number) => void;
  onExportBackup: () => Promise<void>;
  onPreviewImport: () => Promise<void>;
  onApplyImport: () => Promise<void>;
}

function DeleteButton({ pending, onClick, onBlur, label }: { pending: boolean; onClick: () => void; onBlur: () => void; label: string }) {
  return (
    <motion.button
      type="button"
      layout
      className={`flex items-center justify-center gap-1 h-6 rounded-full text-[11px] font-semibold transition-colors flex-shrink-0 ${
        pending
          ? "px-2.5 text-white"
          : "w-6 text-white/35 hover:text-white hover:bg-white/10 opacity-0 group-hover:opacity-100 focus:opacity-100"
      }`}
      style={pending ? { background: SYSTEM_COLORS.red } : undefined}
      onClick={onClick}
      onBlur={onBlur}
      aria-label={pending ? `Confirm delete ${label}` : `Delete ${label}`}
    >
      {pending ? "Delete" : <TrashIcon size={13} />}
    </motion.button>
  );
}

export function ProductivityModule({
  state,
  onAddTask,
  onToggleTask,
  onRemoveTask,
  onClearCompleted,
  onAddNote,
  onUpdateNote,
  onRemoveNote,
  onAddEvent,
  onExportBackup,
  onPreviewImport,
  onApplyImport,
}: ProductivityModuleProps) {
  const [view, setView] = useState<ProductivityView>("tasks");
  const [taskInput, setTaskInput] = useState("");
  const [noteTitle, setNoteTitle] = useState("");
  const [noteContent, setNoteContent] = useState("");
  const [eventTitle, setEventTitle] = useState("");
  const [eventStart, setEventStart] = useState("");
  const [eventEnd, setEventEnd] = useState("");
  const [eventError, setEventError] = useState("");
  const [pendingRemoveTaskId, setPendingRemoveTaskId] = useState<string | null>(null);
  const [pendingRemoveNoteId, setPendingRemoveNoteId] = useState<string | null>(null);
  const [editingNoteId, setEditingNoteId] = useState<string | null>(null);
  const [showBackup, setShowBackup] = useState(false);
  const completedCount = useMemo(() => state.tasks.filter((t) => t.completed).length, [state.tasks]);
  const status = statusLabel(state.status);

  const handleRemoveTaskClick = (taskId: string) => {
    if (pendingRemoveTaskId === taskId) {
      setPendingRemoveTaskId(null);
      onRemoveTask(taskId);
      return;
    }
    setPendingRemoveTaskId(taskId);
    window.setTimeout(() => {
      setPendingRemoveTaskId((current) => (current === taskId ? null : current));
    }, 3000);
  };

  const handleRemoveNoteClick = (noteId: string) => {
    if (pendingRemoveNoteId === noteId) {
      setPendingRemoveNoteId(null);
      onRemoveNote(noteId);
      return;
    }
    setPendingRemoveNoteId(noteId);
    window.setTimeout(() => {
      setPendingRemoveNoteId((current) => (current === noteId ? null : current));
    }, 3000);
  };

  const handleEditNote = (note: NoteItem) => {
    setEditingNoteId(note.id);
    setNoteTitle(note.title);
    setNoteContent(note.content);
    setView("notes");
  };

  const handleSaveNote = () => {
    const editingNote = editingNoteId ? state.notes.find((n) => n.id === editingNoteId) : undefined;
    if (editingNote) {
      onUpdateNote({ ...editingNote, title: noteTitle, content: noteContent });
    } else {
      onAddNote(noteTitle, noteContent);
    }
    setEditingNoteId(null);
    setNoteTitle("");
    setNoteContent("");
  };

  const submitTask = () => {
    if (!taskInput.trim()) return;
    onAddTask(taskInput);
    setTaskInput("");
  };

  return (
    <div className="flex flex-col gap-2.5 h-full min-h-0">
      <div className="flex items-center gap-2">
        <Segmented options={VIEW_OPTIONS} value={view} onChange={setView} className="flex-1" ariaLabel="Productivity views" />
        {status && <span className="text-[10.5px] font-semibold text-white/45">{status}</span>}
        <button
          type="button"
          className={`w-[26px] h-[26px] rounded-full flex items-center justify-center transition-colors ${
            showBackup ? "bg-white text-black" : "bg-white/[0.08] text-white/55 hover:text-white"
          }`}
          aria-label={showBackup ? "Hide backup options" : "Show backup options"}
          aria-expanded={showBackup}
          onClick={() => setShowBackup((v) => !v)}
        >
          <ArchiveIcon size={13} />
        </button>
      </div>

      <AnimatePresence initial={false}>
        {showBackup && (
          <motion.div
            initial={{ height: 0, opacity: 0 }}
            animate={{ height: "auto", opacity: 1 }}
            exit={{ height: 0, opacity: 0 }}
            transition={rowSpring}
            className="overflow-hidden"
          >
            <div className="grid grid-cols-3 gap-1.5">
              {[
                { label: "Export", run: onExportBackup },
                { label: "Preview import", run: onPreviewImport },
                { label: "Apply import", run: onApplyImport },
              ].map((b) => (
                <button
                  key={b.label}
                  type="button"
                  className="h-8 rounded-[10px] bg-white/[0.07] hover:bg-white/[0.12] text-white/75 hover:text-white text-[11px] font-semibold transition-colors"
                  onClick={() => void b.run()}
                >
                  {b.label}
                </button>
              ))}
            </div>
          </motion.div>
        )}
      </AnimatePresence>

      {view === "tasks" && (
        <div className="flex flex-col gap-2 min-h-0 flex-1">
          <form
            className="relative"
            onSubmit={(e) => {
              e.preventDefault();
              submitTask();
            }}
          >
            <input value={taskInput} onChange={(e) => setTaskInput(e.target.value)} placeholder="New reminder" className={`${inputClass} pr-11`} dir="auto" />
            <motion.button
              type="submit"
              disabled={!taskInput.trim()}
              className="absolute right-1 top-1 w-7 h-7 rounded-full flex items-center justify-center disabled:opacity-30 transition-opacity"
              style={{ background: "var(--pillar-accent)", color: "var(--pillar-accent-contrast)" }}
              aria-label="Add task"
              whileTap={{ scale: 0.88 }}
            >
              <PlusIcon size={15} strokeWidth={2.8} />
            </motion.button>
          </form>

          {state.tasks.length > 0 && (
            <div className="flex items-center justify-between px-1 text-[11px] font-semibold">
              <span className="text-white/40">
                {state.tasks.length - completedCount} open · {completedCount} done
              </span>
              {completedCount > 0 && (
                <button type="button" onClick={onClearCompleted} className="text-white/40 hover:text-white transition-colors">
                  Clear done
                </button>
              )}
            </div>
          )}

          <div className="flex flex-col gap-1">
            <AnimatePresence initial={false} mode="popLayout">
              {state.tasks.map((task) => (
                <motion.div
                  key={task.id}
                  layout
                  initial={{ opacity: 0, y: -6 }}
                  animate={{ opacity: 1, y: 0 }}
                  exit={{ opacity: 0, x: 30 }}
                  transition={rowSpring}
                  className="group flex items-center gap-2.5 rounded-[14px] bg-white/[0.06] hover:bg-white/[0.09] pl-2.5 pr-1.5 h-10 transition-colors"
                >
                  <motion.button
                    type="button"
                    onClick={() => onToggleTask(task.id)}
                    className="w-[20px] h-[20px] rounded-full flex items-center justify-center flex-shrink-0"
                    style={{
                      background: task.completed ? "var(--pillar-accent)" : "transparent",
                      boxShadow: task.completed ? "none" : "inset 0 0 0 1.6px rgba(255,255,255,0.35)",
                      color: "var(--pillar-accent-contrast)",
                    }}
                    aria-label={task.completed ? `Mark ${task.title} as not done` : `Mark ${task.title} as done`}
                    aria-pressed={task.completed}
                    whileTap={{ scale: 0.8 }}
                  >
                    {task.completed && (
                      <motion.span initial={{ scale: 0 }} animate={{ scale: 1 }} transition={{ type: "spring", stiffness: 700, damping: 24 }}>
                        <CheckIcon size={12} strokeWidth={3.2} />
                      </motion.span>
                    )}
                  </motion.button>
                  <span
                    className={`text-[12.5px] flex-1 truncate transition-colors ${task.completed ? "line-through text-white/35" : "text-white/90"}`}
                    dir="auto"
                    title={task.title}
                  >
                    {task.title}
                  </span>
                  <DeleteButton
                    pending={pendingRemoveTaskId === task.id}
                    label={task.title}
                    onClick={() => handleRemoveTaskClick(task.id)}
                    onBlur={() => setPendingRemoveTaskId((current) => (current === task.id ? null : current))}
                  />
                </motion.div>
              ))}
            </AnimatePresence>
          </div>
          {state.tasks.length === 0 && (
            <EmptyState icon={<CheckCircleIcon size={22} />} title="No Reminders" subtitle="Add one above to get started" />
          )}
        </div>
      )}

      {view === "notes" && (
        <div className="flex flex-col gap-2 min-h-0 flex-1">
          <div className="rounded-[14px] bg-white/[0.07] overflow-hidden">
            <input
              value={noteTitle}
              onChange={(e) => setNoteTitle(e.target.value)}
              placeholder="Title"
              maxLength={120}
              className="w-full bg-transparent text-white text-[13px] font-semibold px-3 pt-2.5 pb-1 outline-none"
              dir="auto"
            />
            <textarea
              value={noteContent}
              onChange={(e) => setNoteContent(e.target.value)}
              placeholder="Write something…"
              maxLength={2000}
              rows={2}
              className="w-full bg-transparent text-white/80 text-[12.5px] px-3 pb-2 outline-none resize-none"
              dir="auto"
            />
            <div className="flex items-center justify-end gap-1.5 px-2 pb-2">
              {editingNoteId && (
                <button
                  type="button"
                  className="h-7 px-3 rounded-full text-[11.5px] font-semibold text-white/55 hover:text-white transition-colors"
                  onClick={() => {
                    setEditingNoteId(null);
                    setNoteTitle("");
                    setNoteContent("");
                  }}
                >
                  Cancel
                </button>
              )}
              <motion.button
                type="button"
                disabled={!noteTitle.trim()}
                className="h-7 px-3.5 rounded-full text-[11.5px] font-semibold disabled:opacity-30 transition-opacity"
                style={{ background: "var(--pillar-accent)", color: "var(--pillar-accent-contrast)" }}
                onClick={handleSaveNote}
                whileTap={{ scale: 0.92 }}
              >
                {editingNoteId ? "Update" : "Save"}
              </motion.button>
            </div>
          </div>

          <div className="flex flex-col gap-1.5">
            <AnimatePresence initial={false} mode="popLayout">
              {state.notes.map((note) => (
                <motion.div
                  key={note.id}
                  layout
                  initial={{ opacity: 0, y: -6 }}
                  animate={{ opacity: 1, y: 0 }}
                  exit={{ opacity: 0, x: 30 }}
                  transition={rowSpring}
                  className={`group rounded-[14px] px-3 py-2 transition-colors ${
                    editingNoteId === note.id ? "bg-white/[0.12]" : "bg-white/[0.06] hover:bg-white/[0.09]"
                  }`}
                >
                  <div className="flex items-center gap-2">
                    <span className="text-[13px] font-semibold text-white truncate flex-1" dir="auto">{note.title}</span>
                    <button
                      type="button"
                      className="w-6 h-6 rounded-full flex items-center justify-center text-white/35 hover:text-white hover:bg-white/10 opacity-0 group-hover:opacity-100 focus:opacity-100 transition-all"
                      onClick={() => handleEditNote(note)}
                      aria-label={`Edit ${note.title}`}
                    >
                      <PencilIcon size={12} />
                    </button>
                    <DeleteButton
                      pending={pendingRemoveNoteId === note.id}
                      label={note.title}
                      onClick={() => handleRemoveNoteClick(note.id)}
                      onBlur={() => setPendingRemoveNoteId((current) => (current === note.id ? null : current))}
                    />
                  </div>
                  <p className="text-[12px] text-white/50 line-clamp-2 whitespace-pre-wrap leading-snug" dir="auto">
                    {note.content || "No additional text"}
                  </p>
                </motion.div>
              ))}
            </AnimatePresence>
          </div>
          {state.notes.length === 0 && <EmptyState icon={<NoteIcon size={22} />} title="No Notes" subtitle="Jot something down above" />}
        </div>
      )}

      {view === "agenda" && (
        <div className="flex flex-col gap-2 min-h-0 flex-1">
          <div className="rounded-[14px] bg-white/[0.07] p-2 flex flex-col gap-1.5">
            <input
              value={eventTitle}
              onChange={(e) => setEventTitle(e.target.value)}
              placeholder="Event title"
              className="w-full bg-transparent text-white text-[13px] font-semibold px-1 h-7 outline-none"
              dir="auto"
            />
            <div className="grid grid-cols-2 gap-1.5">
              <label className="flex flex-col gap-0.5">
                <span className="text-[10px] font-semibold text-white/40 px-1">Starts</span>
                <input required value={eventStart} onChange={(e) => setEventStart(e.target.value)} type="datetime-local" className="bg-white/[0.08] text-white text-[11px] rounded-[10px] px-2 h-8 outline-none" style={{ colorScheme: "dark" }} />
              </label>
              <label className="flex flex-col gap-0.5">
                <span className="text-[10px] font-semibold text-white/40 px-1">Ends</span>
                <input required value={eventEnd} onChange={(e) => setEventEnd(e.target.value)} type="datetime-local" className="bg-white/[0.08] text-white text-[11px] rounded-[10px] px-2 h-8 outline-none" style={{ colorScheme: "dark" }} />
              </label>
            </div>
            <div className="flex items-center gap-2">
              {eventError && <span className="text-[10.5px] font-medium flex-1" style={{ color: SYSTEM_COLORS.red }}>{eventError}</span>}
              <motion.button
                type="button"
                disabled={!eventTitle.trim() || !eventStart || !eventEnd}
                className="ml-auto h-7 px-3.5 rounded-full text-[11.5px] font-semibold disabled:opacity-30 transition-opacity"
                style={{ background: "var(--pillar-accent)", color: "var(--pillar-accent-contrast)" }}
                whileTap={{ scale: 0.92 }}
                onClick={() => {
                  const startsAt = Date.parse(eventStart);
                  const endsAt = Date.parse(eventEnd);
                  if (!eventTitle.trim() || Number.isNaN(startsAt) || Number.isNaN(endsAt) || endsAt <= startsAt) {
                    setEventError("End must be after start.");
                    return;
                  }
                  onAddEvent(eventTitle, startsAt, endsAt);
                  setEventError("");
                  setEventTitle("");
                  setEventStart("");
                  setEventEnd("");
                }}
              >
                Add event
              </motion.button>
            </div>
          </div>

          <div className="flex flex-col gap-1.5">
            {state.calendarEvents.map((event) => {
              const start = new Date(event.startsAt);
              return (
                <motion.div
                  key={event.id}
                  layout
                  initial={{ opacity: 0, y: -6 }}
                  animate={{ opacity: 1, y: 0 }}
                  transition={rowSpring}
                  className="flex items-center gap-3 rounded-[14px] bg-white/[0.06] px-2.5 py-2"
                >
                  <div className="w-10 flex flex-col items-center flex-shrink-0">
                    <span className="text-[9.5px] font-bold uppercase" style={{ color: "var(--pillar-accent)" }}>
                      {start.toLocaleDateString(undefined, { month: "short" })}
                    </span>
                    <span className="text-white text-[18px] font-semibold leading-none tabular-nums">{start.getDate()}</span>
                  </div>
                  <span className="w-[3px] self-stretch rounded-full" style={{ background: "var(--pillar-accent)" }} />
                  <div className="flex flex-col min-w-0">
                    <span className="text-white text-[12.5px] font-semibold truncate" dir="auto">{event.title}</span>
                    <span className="text-white/45 text-[11px] tabular-nums">
                      {start.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" })} –{" "}
                      {new Date(event.endsAt).toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" })}
                    </span>
                  </div>
                </motion.div>
              );
            })}
          </div>
          {state.calendarEvents.length === 0 && (
            <EmptyState icon={<CalendarIcon size={22} />} title="No Upcoming Events" subtitle="Add an event above" />
          )}
        </div>
      )}
    </div>
  );
}
