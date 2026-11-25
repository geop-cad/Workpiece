//! Operations that create basic solid shapes, one per file: a box, a
//! sphere, a cylinder, a cone and a torus, each placed by a datum frame. Every face, edge
//! and vertex is named after the operation (see each operation), so later
//! steps can refer to them.

mod add_box;
mod add_cone;
mod add_cylinder;
mod add_sphere;
mod add_torus;
mod brep;

pub use add_box::{AddBox, AddBoxArgs};
pub use add_cone::{AddCone, AddConeArgs};
pub use add_cylinder::{AddCylinder, AddCylinderArgs};
pub use add_sphere::{AddSphere, AddSphereArgs};
pub use add_torus::{AddTorus, AddTorusArgs};

use std::collections::BTreeMap;

use workpiece_core::handles::{Handle, LinearHandle};
use workpiece_core::math::{Axis, Point3};
use workpiece_core::world::DatumFrame;
use workpiece_core::{Reference, World};

/// Arrow handles from the origin of `frame` along each of its `axes`,
/// named after the axis (`"x"`, `"y"`, `"z"`); none while there is no
/// frame or it does not resolve. `make` gets the axis, the frame's origin and the axis's
/// direction.
fn frame_handles<M: 'static>(
    world: &World,
    frame: &Option<Reference<DatumFrame>>,
    axes: &[Axis],
    make: impl Fn(Axis, Point3, Point3) -> Option<LinearHandle<M>>,
) -> BTreeMap<String, Handle<M>> {
    let Some(Ok(frame)) = frame.as_ref().map(|f| world.resolve(f)) else {
        return BTreeMap::new();
    };
    axes.iter()
        .filter_map(|&axis| {
            let handle = make(axis, frame.origin(), frame.direction(axis))?;
            Some((axis.name().to_owned(), handle.into()))
        })
        .collect()
}

/// Tells `r` of this crate's operations: their kinds, the tools that
/// add them, and their icons.
pub fn register(r: &mut workpiece_core::Registry) {
    r.kind::<AddBox>("box");
    r.kind::<AddSphere>("sphere");
    r.kind::<AddCylinder>("cylinder");
    r.kind::<AddCone>("cone");
    r.kind::<AddTorus>("torus");
    r.icon("box", include_str!("../icons/box.svg"));
    r.icon("cone", include_str!("../icons/cone.svg"));
    r.icon("cylinder", include_str!("../icons/cylinder.svg"));
    r.icon("sphere", include_str!("../icons/sphere.svg"));
    r.icon("torus", include_str!("../icons/torus.svg"));
}
