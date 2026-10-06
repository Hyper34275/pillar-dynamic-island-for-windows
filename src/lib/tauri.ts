import { invoke, isTauri } from "@tauri-apps/api/core";
import { createLogger } from "./logger";
import { dlog } from "./debugLog";
import { describeError } from "./errors";

// The page talks to Rust through the imported API (window.__TAURI_INTERNALS__), never through
// the window.__TAURI__ global, so `app.withGlobalTauri` can stay off.

const log = createLogger("tauri");

/** Default per-invoke timeout. Backend commands that legitimately take longer
 *  (heavy COM operations, large backups) should pass an explicit `timeoutMs`. */
const DEFAULT_INVOKE_TIMEOUT_MS = 10_000;

export interface InvokeOptions {
  /** Override the default timeout in ms. Pass `0` to disable the timeout. */
  timeoutMs?: number;
  /** Suppress error logging for expected/recoverable failures. */
  silent?: boolean;
}

/** Distinguishable error type so callers can branch on the failure mode. */
export class TauriTimeoutError extends Error {
  constructor(cmd: string, timeoutMs: number) {
    super(`Tauri command "${cmd}" timed out after ${timeoutMs}ms.`);
    this.name = "TauriTimeoutError";
  }
}

export function isTauriAvailable(): boolean {
  return isTauri();
}

/**
 * Invoke a Tauri backend command.
 *
 * Returns `null` when the Tauri runtime is not present (dev in plain browser); use
 * {@link isTauriAvailable} first if you need to distinguish this from a legitimate `null`
 * result. A command that threw is logged and re-thrown so callers can catch it (callers
 * that prefer "best effort" should wrap in try/catch and ignore).
 *
 * Throws {@link TauriTimeoutError} if the command does not resolve within the
 * configured timeout. The Rust call will keep running on the backend, but the
 * frontend stops waiting so UI doesn't hang.
 */
export async function tauriInvoke<T>(
  cmd: string,
  args?: Record<string, unknown>,
  options: InvokeOptions = {}
): Promise<T | null> {
  if (!isTauri()) return null;

  const timeoutMs = options.timeoutMs ?? DEFAULT_INVOKE_TIMEOUT_MS;
  const shouldTrace = !UNTRACED_COMMANDS.has(cmd);
  const startedAt = performance.now();

  try {
    const result =
      timeoutMs <= 0
        ? await invoke<T>(cmd, args)
        : await withTimeout(invoke<T>(cmd, args), timeoutMs, cmd);
    if (shouldTrace) {
      const elapsed = Math.round(performance.now() - startedAt);
      if (elapsed > SLOW_INVOKE_MS) dlog("warn", "tauri", `invoke ${cmd} took ${elapsed}ms`);
    }
    return result;
  } catch (error) {
    if (shouldTrace) {
      const elapsed = Math.round(performance.now() - startedAt);
      // Message-free: Rust errors are "CODE: text" and only the code is logged.
      const reason = error instanceof TauriTimeoutError ? `timed out after ${timeoutMs}ms` : describeError(error);
      dlog(options.silent ? "warn" : "error", "tauri", `invoke ${cmd} failed after ${elapsed}ms: ${reason}`);
    }
    if (!options.silent) {
      log.error(`invoke failed: ${cmd}`, error);
    }
    throw error;
  }
}

/** Invokes slower than this are written to the debug log. */
const SLOW_INVOKE_MS = 400;
/** Logging commands themselves — tracing them would be noise (or recursion). */
const UNTRACED_COMMANDS = new Set(["write_logs", "log_frontend_error"]);

function withTimeout<T>(promise: Promise<T>, ms: number, cmd: string): Promise<T> {
  return new Promise<T>((resolve, reject) => {
    const handle = setTimeout(() => {
      reject(new TauriTimeoutError(cmd, ms));
    }, ms);
    promise.then(
      (value) => {
        clearTimeout(handle);
        resolve(value);
      },
      (err) => {
        clearTimeout(handle);
        reject(err);
      }
    );
  });
}
