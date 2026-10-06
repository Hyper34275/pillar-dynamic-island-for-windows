import { describe, expect, it } from "vitest";
import { describeError, errorCode, stackFrames } from "./errors";

describe("errorCode", () => {
  it("extracts the stable code from a Rust command error", () => {
    expect(errorCode("OUTLOOK-102: COM attach failed")).toBe("OUTLOOK-102");
    expect(errorCode(new Error("APP-002: settings write failed"))).toBe("APP-002");
  });
  it("returns null when there is none", () => {
    expect(errorCode("something broke")).toBeNull();
    expect(errorCode(42)).toBeNull();
  });
});

describe("describeError", () => {
  it("never includes the message text", () => {
    const secret = new TypeError("Quarterly review with Alice <alice@corp.example>");
    expect(describeError(secret)).toBe("TypeError");
    expect(describeError(secret)).not.toMatch(/Alice|corp/);
  });
  it("appends a code when the message carries one", () => {
    expect(describeError(new Error("NET-301: no usable LAN IPv4"))).toBe("Error [NET-301]");
    expect(describeError("WIN-501: geometry failed")).toBe("string [WIN-501]");
  });
});

describe("stackFrames", () => {
  it("keeps frames and drops the header line that carries the message", () => {
    const error = new Error("meeting subject: secret");
    error.stack = "Error: meeting subject: secret\n    at fn (app.js:1:1)\n    at other (app.js:2:2)";
    const frames = stackFrames(error);
    expect(frames).toContain("at fn (app.js:1:1)");
    expect(frames).not.toContain("secret");
  });
  it("limits the number of frames and tolerates non-errors", () => {
    const error = new Error("x");
    error.stack = ["Error: x", ...Array.from({ length: 30 }, (_, i) => `    at f${i} (a.js:${i}:1)`)].join("\n");
    expect(stackFrames(error, 5).split("\n")).toHaveLength(5);
    expect(stackFrames("nope")).toBe("");
  });
});
