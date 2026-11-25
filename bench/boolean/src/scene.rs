//! Building the solids of a case, and grading what a boolean made of
//! them.

use glam::DQuat;
use shapecraft_boolean::{Op, Refusal, Solid, combine, measure, read, write};
use shapecraft_core::math::{Axis, Point2, Point3};
use shapecraft_core::world::topo::Topo;
use shapecraft_core::world::{Datum, DatumFrame};
use shapecraft_core::{Measure, Operation, Pose, Reference, Standalone};
use shapecraft_ops_basic_shapes::{
    AddBox, AddBoxArgs, AddCylinder, AddCylinderArgs, AddSphere, AddSphereArgs,
};
use shapecraft_ops_sketch::{Shape, Sketch};
use shapecraft_ops_sweep::{Revolve, RevolveAxis};

use crate::Grade;

/// How close a volume or an area has to be, relative to what is
/// expected.
pub const CLOSE: f64 = 1e-6;

/// A world to build solids in.
pub struct Scene {
    b: Standalone,
    made: usize,
}

impl Default for Scene {
    fn default() -> Self {
        Self::new()
    }
}

impl Scene {
    /// An empty scene.
    pub fn new() -> Self {
        Self {
            b: Standalone::new(),
            made: 0,
        }
    }

    fn name(&mut self, kind: &str) -> String {
        self.made += 1;
        format!("{kind}{}", self.made)
    }

    fn frame(&mut self, id: &str, pose: Pose) -> anyhow::Result<Reference<DatumFrame>> {
        let name = format!("{id}-frame");
        let frame = DatumFrame::from_pose(pose)?;
        self.b
            .world
            .datums
            .insert(name.clone(), Datum::Frame(frame));
        Ok(Reference::datum(name))
    }

    fn take(&self, id: &str) -> anyhow::Result<Solid> {
        let body = self
            .b
            .world
            .topo
            .resolve(id)
            .ok_or_else(|| anyhow::anyhow!("no body {id}"))?;
        Ok(read(&self.b.world.topo, body)?)
    }

    /// A box of `size` centred at `at`, turned by `turn`.
    pub fn cube(&mut self, at: [f64; 3], size: [f64; 3], turn: DQuat) -> anyhow::Result<Solid> {
        let id = self.name("box");
        let frame = self.frame(&id, Pose::from_rotation_translation(turn, at.into()))?;
        AddBox {
            id: id.clone(),
            args: AddBoxArgs {
                frame: Some(frame),
                size: size.map(Measure::new),
            },
        }
        .apply(&mut self.b)?;
        self.take(&id)
    }

    /// A cylinder of `radius` standing on `base`, `height` along the z
    /// of `turn`.
    pub fn cylinder(
        &mut self,
        base: [f64; 3],
        radius: f64,
        height: f64,
        turn: DQuat,
    ) -> anyhow::Result<Solid> {
        let id = self.name("cylinder");
        let frame = self.frame(&id, Pose::from_rotation_translation(turn, base.into()))?;
        AddCylinder {
            id: id.clone(),
            args: AddCylinderArgs {
                frame: Some(frame),
                radius: Measure::new(radius),
                height: Measure::new(height),
            },
        }
        .apply(&mut self.b)?;
        self.take(&id)
    }

    /// A ball of `radius` around `at`.
    pub fn ball(&mut self, at: [f64; 3], radius: f64, turn: DQuat) -> anyhow::Result<Solid> {
        let id = self.name("ball");
        let frame = self.frame(&id, Pose::from_rotation_translation(turn, at.into()))?;
        AddSphere {
            id: id.clone(),
            args: AddSphereArgs {
                frame: Some(frame),
                radius: Measure::new(radius),
            },
        }
        .apply(&mut self.b)?;
        self.take(&id)
    }

    /// What the outline `corners` (in a plane through the axis: distance
    /// from it, height along it), the circle `ring` (centre and radius,
    /// there) or the parabola `bowl` (from its vertex on the axis out to
    /// a distance and height, closed flat across its top) sweeps turning around the y axis of the frame at
    /// `at` turned by `turn`.
    pub fn turned(
        &mut self,
        at: [f64; 3],
        turn: DQuat,
        corners: &[[f64; 2]],
        ring: Option<([f64; 2], f64)>,
        bowl: Option<[f64; 2]>,
    ) -> anyhow::Result<Solid> {
        let id = self.name("turned");
        self.frame(&id, Pose::from_rotation_translation(turn, at.into()))?;
        let mut sketch = Sketch::new(format!("{id}-sketch"));
        sketch.args.plane = Some(Reference::frame_axis(format!("{id}-frame"), Axis::Z));
        let points: Vec<_> = corners
            .iter()
            .map(|p| sketch.args.add_point(Point2::from_array(*p)))
            .collect();
        for k in 0..points.len() {
            sketch
                .args
                .add_line(points[k], points[(k + 1) % points.len()]);
        }
        if let Some((centre, radius)) = ring {
            let center = sketch.args.add_point(Point2::from_array(centre));
            sketch.args.add(Shape::Circle { center, radius });
        }
        // A parabola from the axis out: its hull of three points.
        if let Some([reach, height]) = bowl {
            let control = [
                [0.0, 0.0],
                [reach / 2.0, 0.0],
                [reach, height],
                [0.0, height],
            ]
            .map(|p| sketch.args.add_point(Point2::from_array(p)));
            sketch.args.add(Shape::Spline {
                control: control[..3].to_vec(),
                degree: 2,
                knots: Vec::new(),
                weights: Vec::new(),
            });
            sketch.args.add_line(control[2], control[3]);
            sketch.args.add_line(control[3], control[0]);
        }
        sketch.apply(&mut self.b)?;
        let mut revolve = Revolve::of(id.clone(), Some(format!("{id}-sketch")), false);
        revolve.args.axis = RevolveAxis::SketchV;
        revolve.args.combine = shapecraft_shape::Combine::NewBody;
        revolve.apply(&mut self.b)?;
        self.take(&id)
    }
}

/// How an outline that is turned goes on from where it is to the next
/// of its points.
#[derive(Debug, Clone)]
pub enum Piece {
    /// Straight.
    Line,
    /// Around a circle about `center`, counter-clockwise or `clockwise`.
    Arc {
        /// The circle's center.
        center: [f64; 2],
        /// Whether it goes around clockwise.
        clockwise: bool,
    },
    /// Along a spline that starts and ends at the two points: the
    /// control points between them, its degree, its knots (none for one
    /// Bézier piece) and the weights of all its control points (none
    /// for all 1).
    Spline {
        /// The control points between the two ends.
        between: Vec<[f64; 2]>,
        /// The degree.
        degree: usize,
        /// The knots.
        knots: Vec<f64>,
        /// The weights.
        weights: Vec<f64>,
    },
}

impl Scene {
    /// What a closed outline sweeps turning around the y axis of the
    /// frame at `at` turned by `turn`. The outline lies in a plane
    /// through the axis (distance from it, height along it): it starts
    /// at `start` and goes on by each of `pieces` to the point given
    /// with it, the last one back to `start`.
    pub fn outlined(
        &mut self,
        at: [f64; 3],
        turn: DQuat,
        start: [f64; 2],
        pieces: &[(Piece, [f64; 2])],
    ) -> anyhow::Result<Solid> {
        let id = self.name("outlined");
        self.frame(&id, Pose::from_rotation_translation(turn, at.into()))?;
        let mut sketch = Sketch::new(format!("{id}-sketch"));
        sketch.args.plane = Some(Reference::frame_axis(format!("{id}-frame"), Axis::Z));
        let first = sketch.args.add_point(Point2::from_array(start));
        let mut from = first;
        for (k, (piece, to)) in pieces.iter().enumerate() {
            let to = if k + 1 == pieces.len() {
                first
            } else {
                sketch.args.add_point(Point2::from_array(*to))
            };
            match piece {
                Piece::Line => {
                    sketch.args.add_line(from, to);
                }
                Piece::Arc { center, clockwise } => {
                    let center = sketch.args.add_point(Point2::from_array(*center));
                    let (start, end) = if *clockwise { (to, from) } else { (from, to) };
                    sketch.args.add(Shape::Arc { center, start, end });
                }
                Piece::Spline {
                    between,
                    degree,
                    knots,
                    weights,
                } => {
                    let mut control = vec![from];
                    for p in between {
                        control.push(sketch.args.add_point(Point2::from_array(*p)));
                    }
                    control.push(to);
                    sketch.args.add(Shape::Spline {
                        control,
                        degree: *degree,
                        knots: knots.clone(),
                        weights: weights.clone(),
                    });
                }
            }
            from = to;
        }
        sketch.apply(&mut self.b)?;
        let mut revolve = Revolve::of(id.clone(), Some(format!("{id}-sketch")), false);
        revolve.args.axis = RevolveAxis::SketchV;
        revolve.args.combine = shapecraft_shape::Combine::NewBody;
        revolve.apply(&mut self.b)?;
        self.take(&id)
    }
}

/// The volume and area of `solid`, once it is seen to be a valid closed
/// solid: written to a topology of its own and checked there.
pub fn sound(solid: &Solid) -> Result<(f64, f64), String> {
    if solid.is_empty() {
        return Ok((0.0, 0.0));
    }
    let mut topo = Topo::new();
    write(&mut topo, solid, "result").map_err(|e| format!("not a solid: {e}"))?;
    topo.validate().map_err(|e| match e.first() {
        Some(first) => format!("invalid ({} faults): {first}", e.len()),
        None => "invalid".to_owned(),
    })?;
    let m = measure(solid);
    Ok((m.volume, m.area))
}

fn near(got: f64, wanted: f64, scale: f64) -> bool {
    (got - wanted).abs() <= CLOSE * scale.abs().max(wanted.abs()) + 1e-12
}

/// Grades `op` of `a` and `b` against the volume (and area) expected.
pub fn expect(op: Op, a: &Solid, b: &Solid, volume: f64, area: Option<f64>) -> Grade {
    let scale = measure(a).volume.abs().max(measure(b).volume.abs());
    match combine(op, a, b) {
        Err(why) => Grade::Refused(why.to_string()),
        Ok(outcome) => match sound(&outcome.solid) {
            Err(why) => Grade::Wrong(why),
            Ok((v, s)) => {
                if !near(v, volume, scale) {
                    return Grade::Wrong(format!("volume {v} is not {volume}"));
                }
                match area {
                    Some(area) if !near(s, area, area) => {
                        Grade::Wrong(format!("area {s} is not {area}"))
                    }
                    _ => Grade::Pass,
                }
            }
        },
    }
}

/// The volume `op` of `a` and `b` has, if it is done and sound.
pub fn volume_of(op: Op, a: &Solid, b: &Solid) -> Result<f64, Grade> {
    let outcome =
        combine(op, a, b).map_err(|why: Refusal| Grade::Refused(format!("{op:?}: {why}")))?;
    sound(&outcome.solid)
        .map(|m| m.0)
        .map_err(|why| Grade::Wrong(format!("{op:?}: {why}")))
}

/// Grades all four booleans of `a` and `b` by what must hold between
/// them, whatever the solids: `|A ∪ B| = |A| + |B| − |A ∩ B|`,
/// `|A − B| = |A| − |A ∩ B|` and `|B − A| = |B| − |A ∩ B|`. If `common`
/// is given, `|A ∩ B|` must be it, to within `within`.
pub fn consistent(a: &Solid, b: &Solid, common: Option<(f64, f64)>) -> Grade {
    let (va, vb) = (measure(a).volume, measure(b).volume);
    let scale = va.abs().max(vb.abs());
    // Solids of a file whose faces do not quite close up have no volume
    // closer than that.
    let open = 2.0 * (shapecraft_boolean::measure::open(a) + shapecraft_boolean::measure::open(b));
    let run = || -> Result<(), Grade> {
        let both = volume_of(Op::Intersect, a, b)?;
        if let Some((wanted, within)) = common
            && (both - wanted).abs() > within
        {
            return Err(Grade::Wrong(format!(
                "the common volume {both} is not {wanted} (± {within})"
            )));
        }
        let checks = [
            ("union", volume_of(Op::Union, a, b)?, va + vb - both),
            ("a − b", volume_of(Op::Cut, a, b)?, va - both),
            ("b − a", volume_of(Op::Cut, b, a)?, vb - both),
        ];
        for (what, got, wanted) in checks {
            if !near(got, wanted, scale) && (got - wanted).abs() > open {
                return Err(Grade::Wrong(format!(
                    "{what} is {got}, not {wanted} (|a| {va}, |b| {vb}, common {both})"
                )));
            }
        }
        Ok(())
    };
    run().err().unwrap_or(Grade::Pass)
}

/// The corners of a box around `solid`.
pub fn bounds(solid: &Solid) -> (Point3, Point3) {
    solid.extent()
}
