//! Numbers constraint equations are written over: plain `f64`s for their
//! values, [`Dual`]s for their exact derivatives.

use std::ops::{Add, Div, Mul, Neg, Sub};

/// How many variables one constraint may depend on.
pub const MAX_LOCALS: usize = 14;

/// A number an equation can be evaluated with.
pub trait Scalar:
    Copy
    + Add<Output = Self>
    + Sub<Output = Self>
    + Mul<Output = Self>
    + Div<Output = Self>
    + Neg<Output = Self>
{
    /// The constant `v`.
    fn constant(v: f64) -> Self;
    /// The value, without derivatives.
    fn value(self) -> f64;
    /// The square root.
    fn sqrt(self) -> Self;
    /// The absolute value.
    fn abs(self) -> Self;
    /// The angle of the vector (`other`, `self`): `atan2(self, other)`.
    fn atan2(self, other: Self) -> Self;
}

impl Scalar for f64 {
    fn constant(v: f64) -> Self {
        v
    }
    fn value(self) -> f64 {
        self
    }
    fn sqrt(self) -> Self {
        f64::sqrt(self)
    }
    fn abs(self) -> Self {
        f64::abs(self)
    }
    fn atan2(self, other: Self) -> Self {
        libm::atan2(self, other)
    }
}

/// A value with its partial derivatives by up to [`MAX_LOCALS`] variables.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Dual {
    /// The value.
    pub v: f64,
    /// The partial derivatives.
    pub d: [f64; MAX_LOCALS],
}

impl Dual {
    /// The variable with index `i` (below [`MAX_LOCALS`]) and value `v`.
    pub fn variable(v: f64, i: usize) -> Self {
        let mut d = [0.0; MAX_LOCALS];
        d[i] = 1.0;
        Self { v, d }
    }

    /// `f(self)` with derivative `df` there.
    fn chain(self, v: f64, df: f64) -> Self {
        Self {
            v,
            d: self.d.map(|x| x * df),
        }
    }

    fn zip(self, o: Self, v: f64, da: f64, db: f64) -> Self {
        let mut d = [0.0; MAX_LOCALS];
        for (i, x) in d.iter_mut().enumerate() {
            *x = self.d[i] * da + o.d[i] * db;
        }
        Self { v, d }
    }
}

impl Add for Dual {
    type Output = Self;
    fn add(self, o: Self) -> Self {
        self.zip(o, self.v + o.v, 1.0, 1.0)
    }
}

impl Sub for Dual {
    type Output = Self;
    fn sub(self, o: Self) -> Self {
        self.zip(o, self.v - o.v, 1.0, -1.0)
    }
}

impl Mul for Dual {
    type Output = Self;
    fn mul(self, o: Self) -> Self {
        self.zip(o, self.v * o.v, o.v, self.v)
    }
}

impl Div for Dual {
    type Output = Self;
    fn div(self, o: Self) -> Self {
        self.zip(o, self.v / o.v, 1.0 / o.v, -self.v / (o.v * o.v))
    }
}

impl Neg for Dual {
    type Output = Self;
    fn neg(self) -> Self {
        self.chain(-self.v, -1.0)
    }
}

impl Scalar for Dual {
    fn constant(v: f64) -> Self {
        Self {
            v,
            d: [0.0; MAX_LOCALS],
        }
    }
    fn value(self) -> f64 {
        self.v
    }
    fn sqrt(self) -> Self {
        let root = self.v.sqrt();
        // At zero the slope is infinite; none is the useful answer.
        self.chain(root, if root > 0.0 { 0.5 / root } else { 0.0 })
    }
    fn abs(self) -> Self {
        self.chain(self.v.abs(), if self.v < 0.0 { -1.0 } else { 1.0 })
    }
    fn atan2(self, x: Self) -> Self {
        let r2 = self.v * self.v + x.v * x.v;
        if r2 == 0.0 {
            return Self::constant(0.0);
        }
        self.zip(x, libm::atan2(self.v, x.v), x.v / r2, -self.v / r2)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duals_carry_exact_derivatives() {
        // f(x, y) = sqrt(x² + y²) · atan2(y, x) at (3, 4).
        let f = |x: Dual, y: Dual| (x * x + y * y).sqrt() * y.atan2(x);
        let (x, y) = (Dual::variable(3.0, 0), Dual::variable(4.0, 1));
        let r = f(x, y);
        let angle = libm::atan2(4f64, 3.0);
        assert!((r.v - 5.0 * angle).abs() < 1e-12);
        // ∂/∂x = x/r·θ − y/r, ∂/∂y = y/r·θ + x/r.
        assert!((r.d[0] - (0.6 * angle - 0.8)).abs() < 1e-12);
        assert!((r.d[1] - (0.8 * angle + 0.6)).abs() < 1e-12);
        assert_eq!((-x / y).abs().d[0], 0.25);
    }
}
