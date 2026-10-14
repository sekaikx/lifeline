//! # lifeline-airdrop
//!
//! Point-mass trajectory physics for projectiles and parachute-dropped
//! supply packages. Atmosphere, wind, terrain and the numerical helpers come
//! from `lifeline-core` and are re-exported here.
//!
//! * [`dynamics`]: gravity + quadratic drag on air-relative velocity,
//!   parachute inflation
//! * [`integrate`]: hand-written Euler and RK4, Hermite impact location
//! * [`analytic`]: closed-form vacuum and vertical-drag solutions
//! * [`convergence`]: error-vs-step studies and observed order of accuracy
//! * [`release`]: computed air release point solver
//! * [`montecarlo`]: parallel landing-dispersion analysis and ellipses
//! * [`scenario`]: serializable scenario + the reports the app shows

pub use lifeline_core::{atmosphere, roots, stats, terrain, vec3, wind};

pub mod analytic;
pub mod convergence;
pub mod dynamics;
pub mod integrate;
pub mod montecarlo;
pub mod release;
pub mod scenario;

pub use scenario::Scenario;
pub use vec3::{State, Vec3};
