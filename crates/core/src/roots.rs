//! Root finding: Brent's method (1-D) and damped Newton (2-D).
//!
//! Brent combines bisection's guaranteed convergence with inverse quadratic
//! interpolation's speed; it is what `scipy.optimize.brentq` implements.

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Root {
    pub x: f64,
    pub fx: f64,
    pub iterations: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RootError {
    NotBracketed,
    NoConvergence,
}

impl std::fmt::Display for RootError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RootError::NotBracketed => write!(f, "root is not bracketed"),
            RootError::NoConvergence => write!(f, "root finder did not converge"),
        }
    }
}

impl std::error::Error for RootError {}

/// Find `x` in `[a, b]` with `f(x) = 0`, given `f(a)` and `f(b)` of opposite
/// sign, to absolute tolerance `tol`.
pub fn brent<F: FnMut(f64) -> f64>(
    mut f: F,
    a: f64,
    b: f64,
    tol: f64,
    max_iter: usize,
) -> Result<Root, RootError> {
    let (mut a, mut b) = (a, b);
    let (mut fa, mut fb) = (f(a), f(b));
    if fa == 0.0 {
        return Ok(Root {
            x: a,
            fx: fa,
            iterations: 0,
        });
    }
    if fb == 0.0 {
        return Ok(Root {
            x: b,
            fx: fb,
            iterations: 0,
        });
    }
    if fa.signum() == fb.signum() {
        return Err(RootError::NotBracketed);
    }
    let (mut c, mut fc) = (b, fb);
    let (mut d, mut e) = (b - a, b - a);
    for iter in 1..=max_iter {
        if fb.signum() == fc.signum() {
            // keep the root bracketed between b and c
            c = a;
            fc = fa;
            d = b - a;
            e = d;
        }
        if fc.abs() < fb.abs() {
            // make b the best estimate
            a = b;
            b = c;
            c = a;
            fa = fb;
            fb = fc;
            fc = fa;
        }
        let tol1 = 2.0 * f64::EPSILON * b.abs() + 0.5 * tol;
        let xm = 0.5 * (c - b);
        if xm.abs() <= tol1 || fb == 0.0 {
            return Ok(Root {
                x: b,
                fx: fb,
                iterations: iter,
            });
        }
        if e.abs() >= tol1 && fa.abs() > fb.abs() {
            // try interpolation (secant if only two points, else inverse quadratic)
            let s = fb / fa;
            let (mut p, mut q);
            if a == c {
                p = 2.0 * xm * s;
                q = 1.0 - s;
            } else {
                let qq = fa / fc;
                let r = fb / fc;
                p = s * (2.0 * xm * qq * (qq - r) - (b - a) * (r - 1.0));
                q = (qq - 1.0) * (r - 1.0) * (s - 1.0);
            }
            if p > 0.0 {
                q = -q;
            } else {
                p = -p;
            }
            let min1 = 3.0 * xm * q - (tol1 * q).abs();
            let min2 = (e * q).abs();
            if 2.0 * p < min1.min(min2) {
                e = d;
                d = p / q;
            } else {
                d = xm;
                e = d;
            }
        } else {
            d = xm;
            e = d;
        }
        a = b;
        fa = fb;
        b += if d.abs() > tol1 { d } else { tol1.copysign(xm) };
        fb = f(b);
    }
    Err(RootError::NoConvergence)
}

/// Grow `[a, b]` geometrically around its centre until `f` changes sign.
pub fn expand_bracket<F: FnMut(f64) -> f64>(
    mut f: F,
    a: f64,
    b: f64,
    max_expansions: usize,
) -> Result<(f64, f64), RootError> {
    let (mut a, mut b) = (a.min(b), a.max(b));
    let (mut fa, mut fb) = (f(a), f(b));
    for _ in 0..max_expansions {
        if fa.signum() != fb.signum() {
            return Ok((a, b));
        }
        let width = b - a;
        if fa.abs() < fb.abs() {
            a -= width;
            fa = f(a);
        } else {
            b += width;
            fb = f(b);
        }
    }
    if fa.signum() != fb.signum() {
        Ok((a, b))
    } else {
        Err(RootError::NotBracketed)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Root2 {
    pub x: [f64; 2],
    pub fx: [f64; 2],
    pub iterations: usize,
    pub converged: bool,
}

/// Damped Newton's method for `F: R^2 -> R^2` with a finite-difference
/// Jacobian (step `h`). Stops when `|F| < tol`.
pub fn newton_2d<F: FnMut([f64; 2]) -> [f64; 2]>(
    mut f: F,
    x0: [f64; 2],
    h: f64,
    tol: f64,
    max_iter: usize,
) -> Root2 {
    let norm = |v: [f64; 2]| v[0].hypot(v[1]);
    let mut x = x0;
    let mut fx = f(x);
    for iter in 0..max_iter {
        if norm(fx) < tol {
            return Root2 {
                x,
                fx,
                iterations: iter,
                converged: true,
            };
        }
        let fxh = f([x[0] + h, x[1]]);
        let fyh = f([x[0], x[1] + h]);
        let j = [
            [(fxh[0] - fx[0]) / h, (fyh[0] - fx[0]) / h],
            [(fxh[1] - fx[1]) / h, (fyh[1] - fx[1]) / h],
        ];
        let det = j[0][0] * j[1][1] - j[0][1] * j[1][0];
        if det.abs() < 1e-300 {
            break;
        }
        let dx = [
            (j[1][1] * fx[0] - j[0][1] * fx[1]) / det,
            (-j[1][0] * fx[0] + j[0][0] * fx[1]) / det,
        ];
        // backtracking line search keeps Newton from overshooting
        let mut lambda = 1.0;
        loop {
            let trial = [x[0] - lambda * dx[0], x[1] - lambda * dx[1]];
            let ft = f(trial);
            if !norm(ft).is_finite() && lambda < 1e-4 {
                return Root2 {
                    x,
                    fx,
                    iterations: iter + 1,
                    converged: false,
                };
            }
            if norm(ft) < norm(fx) || lambda < 1e-4 {
                x = trial;
                fx = ft;
                break;
            }
            lambda *= 0.5;
        }
    }
    let converged = norm(fx) < tol;
    Root2 {
        x,
        fx,
        iterations: max_iter,
        converged,
    }
}
