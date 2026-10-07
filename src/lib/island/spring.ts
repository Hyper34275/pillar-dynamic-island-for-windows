// A damped spring, solved in closed form.
//
// Every value the island animates (its width and height, the tab capsule, the tab content's
// progress) is one of these. The state is a position and a velocity; `stepSpring` moves it
// towards a target by any time step exactly (no numerical integration), so a frame that comes
// late (a 30 Hz remote session, a busy frame) lands exactly where a 120 Hz display would have
// been at that instant: nothing is skipped, nothing overshoots because the step was coarse.
//
// Parameters are Apple's (SwiftUI `.spring(response:dampingFraction:)`): `response` is the
// period of the undamped oscillation in seconds (how quickly it gets there), `dampingFraction`
// 1 is critically damped (the fastest approach that never overshoots from rest).
//
// Retargeting keeps position and velocity: a spring sent somewhere else mid-flight bends
// towards the new target instead of restarting, which is what makes an interrupted motion look
// like one physical object changing its mind.

export interface SpringParams {
  /** Seconds: period of the undamped oscillation. */
  readonly response: number;
  /** 1 = critically damped; below 1 overshoots, above 1 is slower and never overshoots. */
  readonly dampingFraction: number;
}

export interface SpringState {
  value: number;
  /** Units per second. */
  velocity: number;
}

/** The state after `dt` seconds of moving towards `target` (dt <= 0 returns it unchanged). */
export function stepSpring(state: SpringState, target: number, params: SpringParams, dt: number): SpringState {
  if (!(dt > 0)) return { value: state.value, velocity: state.velocity };
  const omega = (2 * Math.PI) / Math.max(params.response, 1e-3);
  const zeta = Math.max(params.dampingFraction, 0);
  const x0 = state.value - target;
  const v0 = state.velocity;
  let x: number;
  let v: number;
  if (zeta < 1) {
    const wd = omega * Math.sqrt(1 - zeta * zeta);
    const decay = Math.exp(-zeta * omega * dt);
    const b = (v0 + zeta * omega * x0) / wd;
    const cos = Math.cos(wd * dt);
    const sin = Math.sin(wd * dt);
    x = decay * (x0 * cos + b * sin);
    v = decay * ((v0 * cos) - (x0 * wd + zeta * omega * b) * sin);
  } else if (zeta === 1) {
    const decay = Math.exp(-omega * dt);
    const b = v0 + omega * x0;
    x = (x0 + b * dt) * decay;
    v = (v0 - omega * b * dt) * decay;
  } else {
    const root = Math.sqrt(zeta * zeta - 1);
    const r1 = -omega * (zeta - root);
    const r2 = -omega * (zeta + root);
    const c2 = (v0 - r1 * x0) / (r2 - r1);
    const c1 = x0 - c2;
    const e1 = Math.exp(r1 * dt);
    const e2 = Math.exp(r2 * dt);
    x = c1 * e1 + c2 * e2;
    v = c1 * r1 * e1 + c2 * r2 * e2;
  }
  return { value: target + x, velocity: v };
}

/**
 * At rest for the eye: within `epsilon` of the target and slower than `epsilon` per frame at
 * 60 Hz. Snapping to the target from here moves it by less than `epsilon` (a fraction of a
 * pixel for the island), so the final snap is invisible.
 */
export function springAtRest(state: SpringState, target: number, epsilon: number): boolean {
  return Math.abs(state.value - target) <= epsilon && Math.abs(state.velocity) <= epsilon * 60;
}
