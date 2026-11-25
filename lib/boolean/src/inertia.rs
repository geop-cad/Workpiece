//! How a solid's volume is spread: where its centre is, and its second
//! moments about that centre — what its mass, its centre of gravity and
//! its moments of inertia follow from, given what it is made of.

use workpiece_core::math::Point3;

use crate::model::Solid;

/// How the volume of a solid is spread.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spread {
    /// Its volume.
    pub volume: f64,
    /// Its centre: its centre of gravity, if it is of one material.
    pub centre: Point3,
    /// Its moments of inertia about its centre for a density of one,
    /// along the axes of space: `[[Ixx, Ixy, Ixz], [Ixy, Iyy, Iyz],
    /// [Ixz, Iyz, Izz]]` with the products of inertia taken negative,
    /// as the tensor has them.
    pub inertia: [[f64; 3]; 3],
}

/// How the volume of `solid` is spread: summed over its faces'
/// surfaces themselves, as its volume is — no mesh stands in for them.
pub fn spread(solid: &Solid) -> Spread {
    let (volume, centre, [xx, yy, zz, xy, xz, yz]) = crate::measure::second_moments(solid);
    Spread {
        volume,
        centre,
        inertia: [
            [yy + zz, -xy, -xz],
            [-xy, xx + zz, -yz],
            [-xz, -yz, xx + yy],
        ],
    }
}
