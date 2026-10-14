//! Closed-form solutions, used as ground truth for the integrators.
//!
//! 1. **Vacuum** (constant acceleration):
//!    `r(t) = r0 + v0 t + g t^2 / 2`, `v(t) = v0 + g t`.
//!
//! 2. **Vertical fall with quadratic drag** in uniform air, downward speed
//!    `v0 < v_t` at release. With `k = rho CdA / (2 m)` and terminal velocity
//!    `v_t = sqrt(g / k)`, `dv/dt = g - k v^2` integrates to
//!
//!    ```text
//!    v(t) = v_t tanh(g t / v_t + a),         a = artanh(v0 / v_t)
//!    s(t) = (v_t^2 / g) ln(cosh(g t / v_t + a) / cosh(a))
//!    ```
//!    with `v` and the distance fallen `s` positive downwards.

use crate::vec3::{State, Vec3};

pub fn vacuum_state(t: f64, y0: &State, gravity: f64) -> State {
    let g = Vec3::new(0.0, 0.0, -gravity);
    State::new(y0.pos + y0.vel * t + g * (0.5 * t * t), y0.vel + g * t)
}

/// Time for a vacuum projectile at height `h` above the ground with vertical
/// velocity `vz0` (positive up) to reach the ground.
pub fn vacuum_impact_time(h: f64, vz0: f64, gravity: f64) -> f64 {
    (vz0 + (vz0 * vz0 + 2.0 * gravity * h).sqrt()) / gravity
}

/// Downward speed and distance fallen at time `t` for quadratic drag.
pub fn vertical_drag_fall(t: f64, k: f64, v0: f64, gravity: f64) -> (f64, f64) {
    let vt = (gravity / k).sqrt();
    assert!((0.0..vt).contains(&v0), "need 0 <= v0 < terminal velocity");
    let a = (v0 / vt).atanh();
    let arg = gravity * t / vt + a;
    let v = vt * arg.tanh();
    let s = vt * vt / gravity * (logcosh(arg) - logcosh(a));
    (v, s)
}

/// `ln(cosh(x))` without overflow for large `x`.
fn logcosh(x: f64) -> f64 {
    let x = x.abs();
    x + (-2.0 * x).exp().ln_1p() - std::f64::consts::LN_2
}
