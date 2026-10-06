// Error text can carry user data (paths, subjects, mail addresses), so nothing here ever
// returns a message: logs and crash reports get the error name, a stable code and stack
// frames only.

const CODE_PATTERN = /^([A-Z]{2,8}-\d{3})\b/;

/** Stable code ("OUTLOOK-102") from a Rust command error ("CODE: short message"), if any. */
export function errorCode(error: unknown): string | null {
  const text = error instanceof Error ? error.message : typeof error === "string" ? error : "";
  return CODE_PATTERN.exec(text)?.[1] ?? null;
}

/** Message-free description: `Name` or `Name [CODE]`. */
export function describeError(error: unknown): string {
  const name = error instanceof Error ? error.name : typeof error;
  const code = errorCode(error);
  return code ? `${name} [${code}]` : name;
}

/** Stack frames only — the leading "Name: message" lines are dropped. */
export function stackFrames(error: unknown, max = 12): string {
  if (!(error instanceof Error) || !error.stack) return "";
  return error.stack
    .split("\n")
    .filter((line) => /^\s+at\s/.test(line))
    .slice(0, max)
    .join("\n");
}
