//! Minimal 3-vector and ODE state types.
//!
//! Coordinates are East-North-Up (ENU): `x` east, `y` north, `z` up, metres.

use serde::{Deserialize, Serialize};
use std::ops::{Add, AddAssign, Div, Mul, Neg, Sub};

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Vec3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Vec3 {
    pub const ZERO: Vec3 = Vec3::new(0.0, 0.0, 0.0);

    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }

    pub fn dot(self, o: Vec3) -> f64 {
        self.x * o.x + self.y * o.y + self.z * o.z
    }

    pub fn norm(self) -> f64 {
        self.dot(self).sqrt()
    }

    /// Horizontal (east, north) part.
    pub fn xy(self) -> [f64; 2] {
        [self.x, self.y]
    }
}

impl Add for Vec3 {
    type Output = Vec3;
    fn add(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x + o.x, self.y + o.y, self.z + o.z)
    }
}

impl AddAssign for Vec3 {
    fn add_assign(&mut self, o: Vec3) {
        *self = *self + o;
    }
}

impl Sub for Vec3 {
    type Output = Vec3;
    fn sub(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x - o.x, self.y - o.y, self.z - o.z)
    }
}

impl Mul<f64> for Vec3 {
    type Output = Vec3;
    fn mul(self, s: f64) -> Vec3 {
        Vec3::new(self.x * s, self.y * s, self.z * s)
    }
}

impl Div<f64> for Vec3 {
    type Output = Vec3;
    fn div(self, s: f64) -> Vec3 {
        Vec3::new(self.x / s, self.y / s, self.z / s)
    }
}

impl Neg for Vec3 {
    type Output = Vec3;
    fn neg(self) -> Vec3 {
        Vec3::new(-self.x, -self.y, -self.z)
    }
}

/// ODE state `y = [position, velocity]`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct State {
    pub pos: Vec3,
    pub vel: Vec3,
}

impl State {
    pub const fn new(pos: Vec3, vel: Vec3) -> Self {
        Self { pos, vel }
    }

    /// Euclidean norm over all six components (used for error measurement).
    pub fn norm(self) -> f64 {
        (self.pos.dot(self.pos) + self.vel.dot(self.vel)).sqrt()
    }
}

impl Add for State {
    type Output = State;
    fn add(self, o: State) -> State {
        State::new(self.pos + o.pos, self.vel + o.vel)
    }
}

impl Sub for State {
    type Output = State;
    fn sub(self, o: State) -> State {
        State::new(self.pos - o.pos, self.vel - o.vel)
    }
}

impl Mul<f64> for State {
    type Output = State;
    fn mul(self, s: f64) -> State {
        State::new(self.pos * s, self.vel * s)
    }
}
