# Physics and numerics

This is the derivation behind `crates/airdrop-core`. Work through it on paper
first (that is the week-1 exercise); the code follows it line by line.

## 1. Frame and state

East-North-Up (ENU), metres and seconds. The state is position and velocity:

```
y = [x, y, z, vx, vy, vz]
```

Track and wind directions use aviation conventions: degrees clockwise from
true north. A wind reported "from 270°" blows *towards* the east:
`w = (-s·sin θ, -s·cos θ, 0)`.

## 2. Equations of motion

A point mass `m` under gravity and quadratic drag. Drag acts on the velocity
*relative to the air*, `v_rel = v − w(r)`:

```
dr/dt = v
dv/dt = g − (ρ(z) · CdA(t) / 2m) · |v_rel| · v_rel          g = (0, 0, −g₀)
```

- `ρ(z)`: air density from the atmosphere model (§3)
- `CdA(t)`: drag coefficient × reference area of the package plus the
  inflated fraction of the canopy (§4)
- `w(r)`: wind (uniform, power-law shear, or a layered sounding)

Two consequences worth deriving by hand:

- **Terminal velocity.** Setting `dv/dt = 0` vertically: `v_t = √(2mg / ρ CdA)`.
  500 kg under 121 m² of canopy at sea level: `v_t ≈ 8.1 m/s`.
- **Wind drift.** Horizontally the package relaxes towards the wind velocity,
  so under canopy it drifts at roughly the wind speed for the whole descent:
  drift ≈ wind × time of fall.

## 3. International Standard Atmosphere

Temperature is linear in *geopotential* altitude `H = R·z / (R + z)` inside
each layer (lapse rate `L`). Hydrostatic balance `dp/dH = −ρ g₀` with the ideal
gas law `p = ρ R_air T` gives

```
L ≠ 0:  p = p_b · (T / T_b)^(−g₀ / (L · R_air))
L = 0:  p = p_b · exp(−g₀ (H − H_b) / (R_air · T_b))
ρ = p / (R_air · T)
```

with sea level `T₀ = 288.15 K`, `p₀ = 101 325 Pa`, `L = −6.5 K/km` up to 11 km,
isothermal to 20 km, and so on to 86 km. Tests check the published table
values (e.g. ρ = 0.73643 kg/m³ at 5 km) to 0.1 %.

## 4. Parachute inflation

A static line deploys the canopy `t_d` seconds after release; it fills over
`t_f` seconds with a power-law profile:

```
CdA(t) = CdA_pkg + CdA_canopy · clamp((t − t_d) / t_f, 0, 1)^n
```

During inflation the drag rises by two orders of magnitude in about a second.
That makes the ODE **stiff**: explicit methods need a small step. Linearising
`dv/dt = −k|v|v` gives an eigenvalue of about `−2k|v|`, and forward Euler is
stable only when `dt < 2 / (2k|v|) = 1 / (k|v|)`. Just after opening
(`k ≈ 0.15 /m`, `|v| ≈ 60 m/s`) that is about 0.1 s, and the app's Numerics tab
shows Euler blowing up at `dt = 1 s`. The integrator detects that and reports an
error instead of returning a meaningless answer.

## 5. Exact solutions used as ground truth

**Vacuum.** Constant acceleration: `r(t) = r₀ + v₀t + ½gt²`. A polynomial of
degree 2, so RK4 (exact for degree ≤ 4) reproduces it to round-off, and
the test suite checks that.

**Vertical fall with drag**, uniform air, downward speed `v` with
`dv/dt = g − kv²`, `k = ρCdA/2m`. Separate variables:

```
v(t) = v_t · tanh(g t / v_t + a)                       a = artanh(v₀ / v_t)
s(t) = (v_t² / g) · ln( cosh(g t / v_t + a) / cosh(a) )
```

This is the reference for the convergence study that proves the orders.

## 6. Integrators

```
Euler:  y_{n+1} = y_n + h f(t_n, y_n)                                  local O(h²), global O(h)

RK4:    k1 = f(t, y)
        k2 = f(t + h/2, y + h k1/2)
        k3 = f(t + h/2, y + h k2/2)
        k4 = f(t + h,   y + h k3)
        y_{n+1} = y_n + h (k1 + 2k2 + 2k3 + k4) / 6                     local O(h⁵), global O(h⁴)
```

**Observed order.** If error ≈ `C·h^p`, then `log(error) = log C + p·log h`:
the slope of a log-log plot. `convergence::observed_order` fits that slope on the
four finest steps. Measured: Euler 1.00, RK4 4.01.

**Fair comparison.** RK4 costs 4 evaluations per step, so the tests also give
Euler 4× as many steps (equal work). RK4 still wins by orders of magnitude.

## 7. Ground impact

Stepping past the ground and taking the last point would cost a whole step of
accuracy. Instead, the final step is replaced by the cubic Hermite interpolant
through `(y_n, f_n)` and `(y_{n+1}, f_{n+1})`, which is O(h⁴) accurate, and
`z − terrain(x, y) = 0` is bisected on it to machine precision. The same code
works for flat ground, a hill, or a DEM grid.

## 8. Release point (CARP)

Let `L(r)` be the landing point for a release at horizontal position `r`
(aircraft at fixed height, ground speed and track). The release point solves

```
L(r) − target = 0          (2 equations, 2 unknowns)
```

solved with damped Newton and a finite-difference Jacobian. If the wind depends
only on altitude and the ground is flat, `L(r) − r` is constant and Newton
finishes in one step; terrain or position-dependent wind make it properly
nonlinear (tested).

The **timing-only** variant keeps the aircraft on a track through the target and
solves the 1-D along-track miss with Brent's method (bracketing + inverse
quadratic interpolation, what `scipy.optimize.brentq` does). The crosswind
part of the drift then remains as a cross-track miss. That is why crews offset
the track upwind.

## 9. Monte Carlo landing ellipse

Each sample draws Gaussian errors: constant wind error per horizontal
component, release-timing error (shifts the release along track by
`V·δt`), navigation error, and a fractional drag-area error. Samples run in
parallel (rayon) with per-sample RNG streams, so a seed is reproducible
regardless of thread count.

From the impact points: mean point of impact (MPI), sample covariance `S`, and
confidence ellipses. For a bivariate normal, `d² = xᵀS⁻¹x` is χ² with 2 degrees
of freedom, whose quantile has a closed form:

```
d²(p) = −2 ln(1 − p)        p = 0.95  →  d² = 5.991
```

so the semi-axes are `√(λᵢ · d²)` with `λᵢ` the eigenvalues of `S`. The app also
reports the observed fraction inside each ellipse (a model check: 95.3 % for
the default scenario) and the CEP, the median radial miss.
