//! What the tests of the operations on bodies share: sketches and
//! boxes to start from, and what is checked of what is made.

pub use shapecraft_cad_rasterize::tessellate::volume;
pub use shapecraft_core::math::{Axis, Point2};
pub use shapecraft_core::world::ORIGIN;
pub use shapecraft_core::world::topo::{BodyId, BodyKind};
pub use shapecraft_core::{Measure, Operation, Reference, Source, Standalone};
pub use shapecraft_ops_basic_shapes::{AddBox, AddBoxArgs};
pub use shapecraft_ops_sketch::{Shape, Sketch};

/// π.
pub const PI: f64 = std::f64::consts::PI;

/// A sketch `id` on `plane`: a rectangle from `lo` to `hi`, and circles
/// (center, radius).
pub fn sketch(
    b: &mut Standalone,
    id: &str,
    plane: Reference<shapecraft_core::world::DatumPlane>,
    rectangle: Option<([f64; 2], [f64; 2])>,
    circles: &[([f64; 2], f64)],
) -> anyhow::Result<()> {
    let mut sketch = Sketch::new(id);
    sketch.args.plane = Some(plane);
    let args = &mut sketch.args;
    if let Some((lo, hi)) = rectangle {
        let corners = [
            [lo[0], lo[1]],
            [hi[0], lo[1]],
            [hi[0], hi[1]],
            [lo[0], hi[1]],
        ]
        .map(|p| args.add_point(Point2::from_array(p)));
        for k in 0..4 {
            args.add_line(corners[k], corners[(k + 1) % 4]);
        }
    }
    for &(center, radius) in circles {
        let center = args.add_point(Point2::from_array(center));
        args.add(Shape::Circle { center, radius });
    }
    sketch.apply(b)
}

/// The plane of x and y through the origin.
pub fn xy() -> Reference<shapecraft_core::world::DatumPlane> {
    Reference::frame_axis(ORIGIN, Axis::Z)
}

/// A box `base`, 4 × 4 × 2 around the origin.
pub fn base(b: &mut Standalone) -> anyhow::Result<()> {
    AddBox {
        id: "base".into(),
        args: AddBoxArgs {
            frame: Some(Reference::datum(ORIGIN)),
            size: [4.0, 4.0, 2.0].map(Measure::new),
        },
    }
    .apply(b)
}

/// The body called `name`.
pub fn body(b: &Standalone, name: &str) -> BodyId {
    b.world.topo.resolve(name).expect("the body")
}

/// Fails unless `got` is within `within` of `wanted`.
pub fn close(got: f64, wanted: f64, within: f64) {
    assert!((got - wanted).abs() <= within, "{got} is not {wanted}");
}

/// Fails unless what was built is sound.
pub fn check(b: &Standalone) {
    assert_eq!(b.world.topo.validate(), Ok(()));
}

/// The names of the edges of `body` that `pick` takes by their ends.
pub fn edges_where(
    b: &Standalone,
    name: &str,
    pick: impl Fn(shapecraft_core::math::Point3, shapecraft_core::math::Point3) -> bool,
) -> Vec<Source> {
    let topo = &b.world.topo;
    let solid = shapecraft_boolean::read(topo, body(b, name)).unwrap();
    solid
        .edges
        .iter()
        .filter(|e| {
            pick(
                solid.vertices[e.ends[0]].point,
                solid.vertices[e.ends[1]].point,
            )
        })
        .map(|e| Source::Topology(e.name.clone().unwrap()))
        .collect()
}

/// The volume and area of `solid`, which has to be a valid closed one.
pub fn sound(solid: &shapecraft_boolean::Solid) -> anyhow::Result<(f64, f64)> {
    let mut topo = shapecraft_core::world::topo::Topo::new();
    shapecraft_boolean::write(&mut topo, solid, "it")?;
    assert_eq!(topo.validate(), Ok(()));
    let m = shapecraft_boolean::measure(solid);
    Ok((m.volume, m.area))
}
