//! # lifeline-core
//!
//! The world model every planning module shares: ENU vectors, the standard
//! atmosphere, wind fields, terrain, line of sight, root finders and
//! reproducible statistics.

pub mod atmosphere;
pub mod raster;
pub mod risk;
pub mod roots;
pub mod search;
pub mod stats;
pub mod terrain;
pub mod vec3;
pub mod wind;

pub use vec3::{State, Vec3};
