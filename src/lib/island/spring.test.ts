import { describe, expect, it } from "vitest";
import { springAtRest, stepSpring, type SpringParams, type SpringState } from "./spring";

const nth = <T,>(list: readonly T[], fromEnd: number): T => list[list.length - fromEnd];

const CRITICAL: SpringParams = { response: 0.3, dampingFraction: 1 };

function run(state: SpringState, target: number, params: SpringParams, dt: number, seconds: number): SpringState[] {
  const out: SpringState[] = [];
  let s = state;
  for (let t = 0; t < seconds; t += dt) {
    s = stepSpring(s, target, params, dt);
    out.push(s);
  }
  return out;
}

describe("stepSpring", () => {
  it("is exact at any frame rate: 30 Hz and 240 Hz land on the same curve", () => {
    for (const params of [CRITICAL, { response: 0.3, dampingFraction: 0.8 }, { response: 0.3, dampingFraction: 1.3 }]) {
      const coarse = run({ value: 0, velocity: 0 }, 100, params, 1 / 30, 0.3);
      const fine = run({ value: 0, velocity: 0 }, 100, params, 1 / 240, 0.3);
      // frame k at 30 Hz is frame 8k at 240 Hz
      coarse.forEach((s, k) => {
        expect(s.value).toBeCloseTo(fine[8 * k + 7].value, 6);
        expect(s.velocity).toBeCloseTo(fine[8 * k + 7].velocity, 4);
      });
    }
  });

  it("critically damped from rest never overshoots", () => {
    const states = run({ value: 404, velocity: 0 }, 142, CRITICAL, 1 / 60, 2);
    for (const s of states) expect(s.value).toBeGreaterThanOrEqual(142 - 1e-9);
    expect(nth(states, 1)!.value).toBeCloseTo(142, 3);
  });

  it("an underdamped spring does overshoot (which is why geometry clamps it)", () => {
    const states = run({ value: 0, velocity: 0 }, 100, { response: 0.3, dampingFraction: 0.6 }, 1 / 60, 1);
    expect(Math.max(...states.map((s) => s.value))).toBeGreaterThan(100);
  });

  it("keeps velocity: a moving spring retargeted continues smoothly", () => {
    const moving = nth(run({ value: 0, velocity: 0 }, 100, CRITICAL, 1 / 60, 0.1), 1)!;
    expect(moving.velocity).toBeGreaterThan(0);
    // retarget backwards: the next instant still moves forwards (momentum), then turns
    const next = stepSpring(moving, 0, CRITICAL, 1 / 1000);
    expect(next.value).toBeGreaterThan(moving.value);
    expect(Math.abs(next.velocity - moving.velocity)).toBeLessThan(Math.abs(moving.velocity) * 0.2);
  });

  it("settles within ~response for a critically damped spring", () => {
    const states = run({ value: 0, velocity: 0 }, 1, CRITICAL, 1 / 60, 1);
    const restAt = states.findIndex((s) => springAtRest(s, 1, 0.01));
    expect(restAt).toBeGreaterThan(0);
    expect((restAt + 1) / 60).toBeLessThan(0.45);
  });

  it("does nothing for a zero or negative step", () => {
    expect(stepSpring({ value: 3, velocity: 2 }, 10, CRITICAL, 0)).toEqual({ value: 3, velocity: 2 });
    expect(stepSpring({ value: 3, velocity: 2 }, 10, CRITICAL, -1)).toEqual({ value: 3, velocity: 2 });
  });
});
