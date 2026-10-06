/**
 * Persistent debug log. Entries are queued in memory and flushed in small batches to the
 * Rust `write_logs` command, which appends them to the per-user log file.
 *
 * Privacy: callers pass coordinates, element roles, counts, timings and error codes —
 * never user content (meeting subjects, notification text, window titles). Error text is
 * reduced to name + code + stack frames here (see errors.ts), and DOM descriptions never
 * include text or labels.
 *
 * IMPORTANT: this module talks to Tauri through the RAW `window.__TAURI__.core.invoke` —
 * never through `tauriInvoke` — because `tauriInvoke` itself logs through here (that
 * would recurse).
 */

import { describeError, stackFrames } from "./errors";

export type DebugLevel = "debug" | "info" | "warn" | "error";

interface DebugEntry {
  ts: number;
  level: DebugLevel;
  scope: string;
  message: string;
}

const FLUSH_INTERVAL_MS = 750;
const MAX_QUEUE = 1000;
const DEDUPE_WINDOW_MS = 5000;
let lastKey: string | null = null;
const MAX_MESSAGE_LEN = 2000;

let queue: DebugEntry[] = [];
let droppedCount = 0;
let flushTimer: ReturnType<typeof setTimeout> | null = null;
let flushing = false;

interface DedupeState {
  firstTs: number;
  level: DebugLevel;
  scope: string;
  message: string;
  repeats: number;
}
const recent = new Map<string, DedupeState>();

type RawInvoke = (cmd: string, args?: Record<string, unknown>) => Promise<unknown>;

function getRawInvoke(): RawInvoke | null {
  if (typeof window === "undefined") return null;
  const win = window as unknown as { __TAURI__?: { core?: { invoke?: RawInvoke } } };
  return win.__TAURI__?.core?.invoke ?? null;
}

function push(entry: DebugEntry): void {
  if (queue.length >= MAX_QUEUE) {
    queue.shift();
    droppedCount++;
  }
  queue.push(entry);
}

/** Emit pending "(repeated N×)" summaries for dedupe windows that have expired. */
function sweepRepeats(now: number, force = false): void {
  for (const [key, state] of recent) {
    if (force || now - state.firstTs >= DEDUPE_WINDOW_MS) {
      if (state.repeats > 0) {
        push({
          ts: now,
          level: state.level,
          scope: state.scope,
          message: `${state.message} (repeated ${state.repeats}×)`,
        });
      }
      recent.delete(key);
    }
  }
}

export function dlog(level: DebugLevel, scope: string, message: string): void {
  try {
    const now = Date.now();
    const msg = message.length > MAX_MESSAGE_LEN ? `${message.slice(0, MAX_MESSAGE_LEN)}…` : message;
    const key = `${scope}\u0000${msg}`;
    const existing = recent.get(key);
    // Only collapse back-to-back duplicates: folding non-adjacent repeats hid real
    // state changes (e.g. "activeTab -> media" after a detour through another tab).
    if (existing && lastKey === key && now - existing.firstTs < DEDUPE_WINDOW_MS) {
      existing.repeats++;
      return;
    }
    sweepRepeats(now, true);
    lastKey = key;
    recent.set(key, { firstTs: now, level, scope, message: msg, repeats: 0 });
    push({ ts: now, level, scope, message: msg });
    ensureFlushTimer();
  } catch {
    // Logging must never throw.
  }
}

/** One-shot timer, armed only while entries are waiting: an idle app has no log timer at all. */
function ensureFlushTimer(): void {
  if (flushTimer !== null || typeof window === "undefined") return;
  flushTimer = setTimeout(() => {
    flushTimer = null;
    void flushDebugLog();
  }, FLUSH_INTERVAL_MS);
}

export async function flushDebugLog(force = false): Promise<void> {
  if (flushTimer !== null) {
    clearTimeout(flushTimer);
    flushTimer = null;
  }
  sweepRepeats(Date.now(), force);
  const invoke = getRawInvoke();
  if (!invoke) {
    // No Tauri (plain browser dev) — drop, so the queue doesn't grow forever.
    queue = [];
    droppedCount = 0;
    return;
  }
  if (flushing && !force) return;
  if (queue.length === 0 && droppedCount === 0) return;

  const entries = queue;
  queue = [];
  if (droppedCount > 0) {
    entries.unshift({
      ts: Date.now(),
      level: "warn",
      scope: "debuglog",
      message: `queue overflow: dropped ${droppedCount} entries`,
    });
    droppedCount = 0;
  }

  flushing = true;
  try {
    await invoke("write_logs", { entries });
  } catch {
    // Backend unavailable — nothing sensible to do; never recurse into logging.
  } finally {
    flushing = false;
    if (queue.length > 0) ensureFlushTimer();
  }
}

// ---------------------------------------------------------------------------
// Global instrumentation
// ---------------------------------------------------------------------------

/** Tag, id and role only — never text content or aria-label, which can hold user data. */
function describeElement(el: Element | null): string {
  if (!el) return "null";
  try {
    const parts: string[] = [el.tagName.toLowerCase()];
    if (el.id) parts.push(`#${el.id}`);
    const role = el.getAttribute("role");
    if (role) parts.push(`[role=${role}]`);
    return parts.join("");
  } catch {
    return "?";
  }
}

function errToString(value: unknown): string {
  const frames = stackFrames(value);
  return frames ? `${describeError(value)}
${frames}` : describeError(value);
}

/** Console arguments: strings (our own messages) are kept, everything else is reduced to a type or error name. */
function consoleArgToString(arg: unknown): string {
  if (typeof arg === "string") return arg.length > 200 ? `${arg.slice(0, 200)}…` : arg;
  if (arg instanceof Error) return describeError(arg);
  return typeof arg === "object" && arg !== null ? "[object]" : String(arg);
}

function argsToString(args: unknown[]): string {
  return args.map(consoleArgToString).join(" ");
}

/** Opt-in diagnostics for support sessions: input tracing and the event-loop lag monitor. */
const TRACE_FLAG = "companyisland.trace";

function isTraceEnabled(): boolean {
  try {
    return localStorage.getItem(TRACE_FLAG) === "1";
  } catch {
    return false;
  }
}

let installed = false;

export function installDebugLogging(): void {
  if (installed || typeof window === "undefined") return;
  installed = true;

  // Errors. Message text is never logged: name, code and stack frames only.
  window.addEventListener("error", (e: ErrorEvent) => {
    const where = e.filename ? ` @ ${e.filename}:${e.lineno}:${e.colno}` : "";
    dlog("error", "window", `uncaught error: ${errToString(e.error)}${where}`);
  });
  window.addEventListener("unhandledrejection", (e: PromiseRejectionEvent) => {
    // Logged and swallowed: a rejected promise must never take the island down.
    e.preventDefault();
    dlog("error", "window", `unhandled rejection: ${errToString(e.reason)}`);
  });

  // Console mirroring (original output preserved)
  const origError = console.error.bind(console);
  const origWarn = console.warn.bind(console);
  console.error = (...args: unknown[]) => {
    origError(...args);
    dlog("error", "console", argsToString(args));
  };
  console.warn = (...args: unknown[]) => {
    origWarn(...args);
    dlog("warn", "console", argsToString(args));
  };

  if (isTraceEnabled()) {
    // Coordinates and element roles only (capture phase so nothing can swallow them first).
    const onInput = (e: MouseEvent) => {
      const x = Math.round(e.clientX);
      const y = Math.round(e.clientY);
      const target = e.target instanceof Element ? e.target : null;
      dlog("info", "input", `${e.type} at ${x},${y} win=${window.innerWidth}x${window.innerHeight} target=${describeElement(target)}`);
    };
    document.addEventListener("pointerdown", onInput, true);
    document.addEventListener("click", onInput, true);

    // Event-loop lag monitor
    const LAG_INTERVAL_MS = 500;
    const LAG_THRESHOLD_MS = 250;
    let last = performance.now();
    setInterval(() => {
      const now = performance.now();
      const drift = now - last - LAG_INTERVAL_MS;
      last = now;
      // Hidden windows get throttled timers; that's not a real block.
      if (drift > LAG_THRESHOLD_MS && !document.hidden) {
        dlog("warn", "perf", `UI thread blocked for ${Math.round(drift)}ms`);
      }
    }, LAG_INTERVAL_MS);
  }

  // Window lifecycle
  window.addEventListener("resize", () => {
    dlog("info", "window", `resize -> ${window.innerWidth}x${window.innerHeight}`);
  });
  document.addEventListener("visibilitychange", () => {
    dlog("info", "window", `visibility -> ${document.visibilityState}`);
  });

  // Final flush when the page goes away
  const finalFlush = () => {
    void flushDebugLog(true);
  };
  window.addEventListener("pagehide", finalFlush);
  window.addEventListener("beforeunload", finalFlush);
}
