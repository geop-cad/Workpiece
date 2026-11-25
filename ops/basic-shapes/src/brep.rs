//! Building named solids: vertices, edges, faces and the body around them,
//! each named after the operation that makes it, and the NURBS circles the
//! round shapes are made of.

use workpiece_core::Pose;
use workpiece_core::math::{Point2, Point3};
use workpiece_core::world::topo::{
    Body, BodyId, BodyKind, Coedge, CoedgeId, Curve2, Curve3, Edge, EdgeId, Face, FaceId, FaceUse,
    Loop, Lump, NurbsSurface, Orientation, Shell, Topo, Vertex, VertexId,
};

/// The tolerance of every entity of a basic shape. The shapes are exact,
/// so it only has to absorb rounding.
pub(crate) const TOLERANCE: f64 = 1e-7;

/// `√2 / 2`: the weight of a NURBS circle's corner control points.
const CORNER_WEIGHT: f64 = std::f64::consts::FRAC_1_SQRT_2;

/// The knots of a full circle of four quadratic arcs.
pub(crate) fn circle_knots() -> Vec<f64> {
    vec![
        0.0, 0.0, 0.0, 0.25, 0.25, 0.5, 0.5, 0.75, 0.75, 1.0, 1.0, 1.0,
    ]
}

/// The control points, as (x, y) in the circle's plane, and weights of the
/// unit circle around the origin, starting and ending at (1, 0) and
/// running counter-clockwise.
pub(crate) fn unit_circle() -> [(Point2, f64); 9] {
    let w = CORNER_WEIGHT;
    [
        (Point2::new(1.0, 0.0), 1.0),
        (Point2::new(1.0, 1.0), w),
        (Point2::new(0.0, 1.0), 1.0),
        (Point2::new(-1.0, 1.0), w),
        (Point2::new(-1.0, 0.0), 1.0),
        (Point2::new(-1.0, -1.0), w),
        (Point2::new(0.0, -1.0), 1.0),
        (Point2::new(1.0, -1.0), w),
        (Point2::new(1.0, 0.0), 1.0),
    ]
}

/// The circle of `radius` around `center` in the plane of `x` and `y`
/// (orthonormal), starting at `center + radius · x` towards `y`, on
/// `[0, 1]`.
pub(crate) fn circle(center: Point3, x: Point3, y: Point3, radius: f64) -> anyhow::Result<Curve3> {
    let (points, weights) = unit_circle()
        .into_iter()
        .map(|(p, w)| (center + (x * p.x + y * p.y) * radius, w))
        .unzip();
    Ok(Curve3::new(2, circle_knots(), points, Some(weights))?)
}

/// Builds one solid into a topology. Every entity it names is called
/// `{prefix}/{name}`, and points are placed by `pose`, so shapes are
/// described in their own frame.
pub(crate) struct SolidBuilder<'a> {
    topo: &'a mut Topo,
    prefix: String,
    pose: Pose,
}

impl<'a> SolidBuilder<'a> {
    /// A builder naming entities after `prefix`; fails if any name with
    /// that prefix exists.
    pub(crate) fn new(topo: &'a mut Topo, prefix: &str, pose: Pose) -> anyhow::Result<Self> {
        let prefix = prefix.to_owned();
        let taken = topo.lookup(&prefix).is_some()
            || topo.names().iter().any(|(name, _)| {
                name.strip_prefix(&prefix)
                    .is_some_and(|rest| rest.starts_with('/'))
            });
        anyhow::ensure!(!taken, "topology named {prefix:?} exists");
        Ok(Self { topo, prefix, pose })
    }

    /// A point of the shape's frame, placed.
    pub(crate) fn place(&self, p: Point3) -> Point3 {
        self.pose.transform_point(p)
    }

    /// A direction of the shape's frame, placed.
    pub(crate) fn turn(&self, v: Point3) -> Point3 {
        self.pose.transform_vector(v)
    }

    fn name(
        &mut self,
        id: impl Into<workpiece_core::world::topo::EntityId>,
        name: &str,
    ) -> anyhow::Result<()> {
        let full = format!("{}/{name}", self.prefix);
        self.topo.set_name(id, full)?;
        Ok(())
    }

    /// A vertex at `p` (already placed), named `name`.
    pub(crate) fn vertex(&mut self, p: Point3, name: &str) -> anyhow::Result<VertexId> {
        let id = self.topo.add_vertex(Vertex {
            point: p,
            // (Its edges' ends, rounded a little off it, lie within it.)
            tolerance: 2.0 * TOLERANCE,
        })?;
        self.name(id, name)?;
        Ok(id)
    }

    /// An edge along all of `curve` (already placed) from `start` to
    /// `end`, named `name`.
    pub(crate) fn edge(
        &mut self,
        curve: Curve3,
        start: VertexId,
        end: VertexId,
        name: &str,
    ) -> anyhow::Result<EdgeId> {
        let (t0, t1) = curve.domain();
        let curve = self.topo.add_curve(curve);
        let id = self.topo.add_edge(Edge {
            curve,
            t0,
            t1,
            start,
            end,
            tolerance: TOLERANCE,
        })?;
        self.name(id, name)?;
        Ok(id)
    }

    /// A use of `edge` whose image in the face's (u, v) space is the line
    /// from `from` (at the edge's start) to `to` (at its end): pcurves run
    /// in the edge's direction, whichever way the loop walks it.
    pub(crate) fn coedge(
        &mut self,
        edge: EdgeId,
        orientation: Orientation,
        from: Point2,
        to: Point2,
    ) -> anyhow::Result<CoedgeId> {
        let pcurve = self.topo.add_pcurve(Curve2::line(from, to)?);
        Ok(self
            .topo
            .add_coedge(Coedge::edge(edge, orientation, Some(pcurve)))?)
    }

    /// A pole at `vertex` while the loop runs from `from` to `to` along a
    /// collapsed side of the (u, v) domain.
    pub(crate) fn pole(
        &mut self,
        vertex: VertexId,
        from: Point2,
        to: Point2,
    ) -> anyhow::Result<CoedgeId> {
        let pcurve = self.topo.add_pcurve(Curve2::line(from, to)?);
        Ok(self
            .topo
            .add_coedge(Coedge::pole(vertex, 0.0, 1.0, Some(pcurve)))?)
    }

    /// A face of `surface` (already placed) bounded by one loop of
    /// `coedges`, named `name`.
    pub(crate) fn face(
        &mut self,
        surface: NurbsSurface,
        coedges: Vec<CoedgeId>,
        name: &str,
    ) -> anyhow::Result<FaceId> {
        let surface = self.topo.add_surface(surface);
        let l = self.topo.add_loop(Loop { coedges })?;
        let id = self.topo.add_face(Face {
            surface,
            loops: vec![l],
            tolerance: TOLERANCE,
        })?;
        self.name(id, name)?;
        Ok(id)
    }

    /// The solid body bounded by `faces`, whose natural normals all point
    /// out of it, named after the prefix alone.
    pub(crate) fn solid(self, faces: Vec<FaceId>) -> anyhow::Result<BodyId> {
        let faces = faces
            .into_iter()
            .map(|face| FaceUse {
                face,
                orientation: Orientation::Forward,
            })
            .collect();
        let shell = self.topo.add_shell(Shell { faces })?;
        let lump = self.topo.add_lump(Lump {
            shells: vec![shell],
        })?;
        let body = self.topo.add_body(Body {
            kind: BodyKind::Solid,
            lumps: vec![lump],
            ..Body::default()
        })?;
        self.topo.set_name(body, self.prefix)?;
        Ok(body)
    }
}
