//! A small assembly to start from: a bracket (a plate, a pin and a ball on
//! a frame) placed twice.

use std::collections::BTreeMap;

use shapecraft_cad_part::{AnyOperation, Part, Workspace};
use shapecraft_cad_ui::{Message, Model};
use shapecraft_core::math::Point3;
use shapecraft_core::world::ORIGIN;
use shapecraft_core::{Pose, Reference};
use shapecraft_ops_basic_shapes::{
    AddBox, AddBoxArgs, AddCylinder, AddCylinderArgs, AddSphere, AddSphereArgs,
};
use shapecraft_ops_datum::AddFrame;
use shapecraft_ops_instances::{AddPart, AddPartArgs};

/// The demo, with the assembly open and its left bracket expanded;
/// `editing` also edits that step of the right bracket from inside the
/// assembly.
pub fn model(editing: Option<&str>) -> anyhow::Result<Model> {
    let bracket = Part {
        program: vec![
            AnyOperation::from(AddBox {
                id: "plate".into(),
                args: AddBoxArgs {
                    frame: Some(Reference::datum(ORIGIN)),
                    size: [2.0, 1.2, 0.3].map(shapecraft_core::Measure::new),
                },
            }),
            AddFrame::at("top", Pose::from_translation(Point3::new(0.5, 0.0, 0.15))).into(),
            AddCylinder {
                id: "pin".into(),
                args: AddCylinderArgs {
                    frame: Some(Reference::datum("top")),
                    radius: 0.25.into(),
                    height: 1.0.into(),
                },
            }
            .into(),
            AddSphere {
                id: "ball".into(),
                args: AddSphereArgs {
                    frame: Some(Reference::datum(ORIGIN)),
                    radius: 0.35.into(),
                },
            }
            .into(),
        ],
        ..Default::default()
    };
    let place = |id: &str| -> AnyOperation {
        AddPart {
            id: id.into(),
            args: AddPartArgs {
                part: "bracket.shapecraft".into(),
                variant: None,
                hold: Default::default(),
                bodies: Vec::new(),
            },
        }
        .into()
    };
    let beside = |y: f64| Pose::from_translation(Point3::new(0.0, y, 0.0));
    let assembly = Part::new(vec![place("left"), place("right")])
        .placing("left", &beside(-1.0))
        .placing("right", &beside(1.0));
    let mut model = Model::new(Workspace::new(BTreeMap::from([
        ("assembly.shapecraft".into(), assembly),
        ("bracket.shapecraft".into(), bracket),
    ])));
    model.update(Message::Open("assembly.shapecraft".into()))?;
    // One placed part open in the timeline.
    model.update(Message::Toggle(vec!["left".into()]))?;
    if editing == Some("plane") {
        // A new plane in the right bracket, from its plate's top face.
        model.update(Message::Focus(vec!["right".into()]))?;
        let world = model.viewport.shown.world.clone();
        let top = vec![shapecraft_core::Source::Topology("plate/top".into())];
        let op = shapecraft_ops_datum::AddPlane::from_selection("plane1".into(), &world, &top);
        model.update(Message::AddOperation {
            part: "bracket.shapecraft".into(),
            op: op.into(),
        })?;
        return Ok(model);
    }
    if let Some(step) = editing {
        model.update(Message::Edit(vec!["right".into(), step.into()]))?;
    }
    Ok(model)
}

/// A plate with a sketch on its top, open for editing: a rounded outline
/// offset from the plate's projected edges, a toleranced hole and a slot.
pub fn sketch() -> anyhow::Result<Model> {
    use shapecraft_core::math::Point2;
    use shapecraft_ops_sketch::{DimTolerance, Kind, Shape, Sketch, SketchAxis};

    let plate = AddBox {
        id: "plate".into(),
        args: AddBoxArgs {
            frame: Some(Reference::datum(ORIGIN)),
            size: [3.0, 2.0, 0.3].map(shapecraft_core::Measure::new),
        },
    };
    let mut part = Part::default();
    part.program.push(plate.into());
    part.parameters.push(shapecraft_cad_part::Parameter {
        name: "hole".into(),
        value: shapecraft_core::Measure::new(0.6),
    });
    part.variants.push(shapecraft_cad_part::Variant {
        name: "wide".into(),
        values: BTreeMap::from([("hole".to_owned(), shapecraft_core::Measure::new(0.9))]),
    });
    let mut model = Model::new(Workspace::new(BTreeMap::from([(
        "plate.shapecraft".into(),
        part,
    )])));
    model.update(Message::Open("plate.shapecraft".into()))?;
    let world = model.viewport.shown.world.clone();
    let top = shapecraft_core::Source::Topology("plate/top".into());
    let mut sketch = Sketch::from_selection("sketch1", &world, &[top]);
    let a = &mut sketch.args;
    // The top face's plane starts at a corner: the plate spans 3 × 2.
    let corners = [(0.4, 0.3), (2.6, 0.3), (2.6, 1.7), (0.4, 1.7)]
        .map(|(x, y)| a.add_point(Point2::new(x, y)));
    let mut lines = Vec::new();
    for k in 0..4 {
        let (p, q) = (corners[k], corners[(k + 1) % 4]);
        lines.push(a.add_line(p, q));
        let axis = if k % 2 == 0 {
            SketchAxis::U
        } else {
            SketchAxis::V
        };
        a.constrain(Kind::Along { a: p, b: q, axis });
    }
    a.dimension(
        Kind::Distance(corners[0], corners[1]),
        2.2,
        Point2::new(1.5, -0.25),
    );
    let height = a.dimension(
        Kind::Distance(corners[1], corners[2]),
        1.4,
        Point2::new(3.25, 1.0),
    );
    a.dimension(
        Kind::DistanceAlong {
            a: 1,
            b: corners[0],
            axis: SketchAxis::U,
        },
        0.4,
        Point2::new(0.2, -0.55),
    );
    let center = a.add_point(Point2::new(1.0, 1.0));
    let hole = a.add(Shape::Circle {
        center,
        radius: 0.3,
    });
    let diameter = a.dimension_of(Kind::Size(hole), 0.6, Point2::new(1.45, 1.5), true);
    let c2 = a.add_point(Point2::new(2.0, 1.0));
    let small = a.add(Shape::Circle {
        center: c2,
        radius: 0.2,
    });
    a.constrain(Kind::Along {
        a: center,
        b: c2,
        axis: SketchAxis::U,
    });
    a.dimension(Kind::Size(small), 0.2, Point2::new(2.35, 0.65));
    let fit = |a: &mut shapecraft_ops_sketch::SketchArgs, id, tolerance| {
        if let Some(d) = a
            .constraints
            .get_mut(&id)
            .and_then(|c| c.dimension.as_mut())
        {
            d.tolerance = tolerance;
        }
    };
    fit(a, diameter, DimTolerance::Fit("H7".into()));
    if let Some(d) = a
        .constraints
        .get_mut(&diameter)
        .and_then(|c| c.dimension.as_mut())
    {
        d.formula = Some("hole".into());
    }
    fit(
        a,
        height,
        DimTolerance::Deviations {
            upper: 0.05,
            lower: -0.02,
        },
    );
    anyhow::ensure!(a.solve(), "the demo sketch does not solve");
    model.update(Message::AddOperation {
        part: "plate.shapecraft".into(),
        op: sketch.into(),
    })?;
    Ok(model)
}

/// A bracket with blended edges: a block whose upright edges are
/// rounded, a boss on it whose root runs into the block in a round and
/// whose rim is cut off flat. The chamfer is open for editing.
pub fn blends() -> anyhow::Result<Model> {
    use shapecraft_core::math::Point3;
    use shapecraft_core::{Measure, Source};
    use shapecraft_ops_basic_shapes::{AddCylinder, AddCylinderArgs};
    use shapecraft_ops_blend::{Chamfer, ChamferArgs, Fillet, FilletArgs};
    use shapecraft_ops_bodies::{CombineArgs, CombineBodies};
    use shapecraft_shape::Combine;

    const PART: &str = "bracket.shapecraft";
    let body = |name: &str| Some(Source::Topology(name.into()));
    let mut part = Part::default();
    part.program.push(
        AddBox {
            id: "block".into(),
            args: AddBoxArgs {
                frame: Some(Reference::datum(ORIGIN)),
                size: [3.0, 2.0, 0.6].map(Measure::new),
            },
        }
        .into(),
    );
    let mut model = Model::new(Workspace::new(BTreeMap::from([(PART.into(), part)])));
    model.update(Message::Open(PART.into()))?;
    let add = |model: &mut Model, op: shapecraft_cad_part::AnyOperation| {
        model.update(Message::AddOperation {
            part: PART.into(),
            op,
        })
    };
    // The edges of what is shown that `take` takes by their ends.
    let edges = |model: &Model, take: &dyn Fn(Point3, Point3) -> bool| -> Vec<Source> {
        let topo = &model.viewport.shown.world.topo;
        topo.edges()
            .iter()
            .filter_map(|(id, edge)| {
                let a = topo.vertex(edge.start)?.point;
                let b = topo.vertex(edge.end)?.point;
                take(a, b)
                    .then(|| topo.name_of(id).map(|n| Source::Topology(n.to_owned())))
                    .flatten()
            })
            .collect()
    };
    let rounds = Fillet {
        id: "rounds".into(),
        args: FilletArgs {
            edges: shapecraft_core::Reference::each(edges(&model, &|a, b| (a.z - b.z).abs() > 0.1)),
            size: Measure::new(0.4),
            ..Default::default()
        },
    };
    add(&mut model, rounds.into())?;
    model.update(Message::FinishEdit)?;
    let boss = AddCylinder {
        id: "boss".into(),
        args: AddCylinderArgs {
            frame: Some(Reference::datum(ORIGIN)),
            radius: Measure::new(0.5),
            height: Measure::new(1.0),
        },
    };
    add(&mut model, boss.into())?;
    model.update(Message::FinishEdit)?;
    let join = CombineBodies {
        id: "join".into(),
        args: CombineArgs {
            target: body("block").map(Into::into),
            tool: body("boss").map(Into::into),
            how: Combine::Join,
        },
    };
    add(&mut model, join.into())?;
    model.update(Message::FinishEdit)?;
    // The boss's two circles: where it leaves the block, and its rim.
    let circle = |top: bool| {
        move |a: Point3, b: Point3| {
            a.distance(b) < 1e-9 && (a.truncate().length() - 0.5).abs() < 1e-6 && (a.z > 0.9) == top
        }
    };
    let root = Fillet {
        id: "root".into(),
        args: FilletArgs {
            edges: shapecraft_core::Reference::each(edges(&model, &circle(false))),
            size: Measure::new(0.2),
            ..Default::default()
        },
    };
    add(&mut model, root.into())?;
    model.update(Message::FinishEdit)?;
    let rim = Chamfer {
        id: "rim".into(),
        args: ChamferArgs {
            edges: shapecraft_core::Reference::each(edges(&model, &circle(true))),
            distance: Measure::new(0.1),
            ..Default::default()
        },
    };
    add(&mut model, rim.into())?;
    Ok(model)
}

/// An assembly with joints: a base that is fixed, an arm hinged to it
/// that turns between two stops, a slide on the arm that travels along
/// it, and a wheel beside the arm that a gear mechanism turns twice as
/// fast the other way. Drag the arm, the slide or the wheel.
pub fn joints() -> anyhow::Result<Model> {
    use shapecraft_core::world::{CouplingKind, JointKind};
    use shapecraft_ops_instances::connector::Snap;
    use shapecraft_ops_instances::{Anchor, Constraint, Hold, MateArgs, Mechanism, MechanismArgs};

    let block = |size: [f64; 3], frames: &[(&str, Pose)]| {
        let mut program = vec![AnyOperation::from(AddBox {
            id: "body".into(),
            args: AddBoxArgs {
                frame: Some(Reference::datum(ORIGIN)),
                size: size.map(shapecraft_core::Measure::new),
            },
        })];
        for (name, pose) in frames {
            program.push(AddFrame::at(*name, *pose).into());
        }
        Part::new(program)
    };
    let at = |x: f64, y: f64, z: f64| Pose::from_translation(Point3::new(x, y, z));
    // Along x: a frame whose z is the part's x.
    let along_x = |x: f64, z: f64| Pose::from_axes(Point3::new(x, 0.0, z), Point3::Y, Point3::Z);
    let frame = |instance: Option<&str>, name: &str| {
        Some(Anchor {
            instance: instance.into_iter().map(str::to_owned).collect(),
            source: shapecraft_core::Source::Datum(name.into()),
            snap: Snap::Datum,
        })
    };
    // A part added with its joint: onto a frame of a part there
    // already, with a frame of its own.
    let place = |id: &str, part: &str, joint: Option<MateArgs>| -> AnyOperation {
        AddPart {
            id: id.into(),
            args: AddPartArgs {
                part: part.into(),
                variant: None,
                hold: match joint {
                    Some(joint) => Hold {
                        onto: joint.a,
                        with: joint.b,
                        constraint: Constraint::Joint(joint.kind),
                        flip: joint.flip,
                        angle: joint.angle,
                        distance: joint.distance,
                        offset: [0.0; 3],
                    },
                    None => Hold::fixed(),
                },
                bodies: Vec::new(),
            },
        }
        .into()
    };
    let hinge = MateArgs {
        kind: JointKind::Revolute,
        a: frame(Some("base"), "pivot"),
        b: frame(None, "pivot"),
        angle: Some([-60.0, 60.0]),
        ..MateArgs::default()
    };
    let slider = MateArgs {
        kind: JointKind::Slider,
        a: frame(Some("arm"), "rail"),
        b: frame(None, "rail"),
        distance: Some([0.0, 2.6]),
        ..MateArgs::default()
    };
    let axle = MateArgs {
        kind: JointKind::Revolute,
        a: frame(Some("base"), "axle"),
        b: frame(None, "hub"),
        ..MateArgs::default()
    };
    let gears = Mechanism {
        id: "gears".into(),
        args: MechanismArgs {
            kind: CouplingKind::Gear { ratio: -2.0 },
            a: Some("arm".into()),
            b: Some("wheel".into()),
        },
    };

    let mut model = Model::new(Workspace::new(BTreeMap::from([
        (
            "base.shapecraft".into(),
            block(
                [2.4, 4.4, 0.4],
                &[("pivot", at(0.0, -1.0, 0.2)), ("axle", at(0.0, 1.4, 0.2))],
            ),
        ),
        (
            "arm.shapecraft".into(),
            block(
                [4.0, 0.6, 0.3],
                &[
                    ("pivot", at(-1.6, 0.0, -0.15)),
                    ("rail", along_x(-1.0, 0.15)),
                ],
            ),
        ),
        (
            "slide.shapecraft".into(),
            block([0.8, 1.0, 0.4], &[("rail", along_x(0.0, -0.2))]),
        ),
        (
            "wheel.shapecraft".into(),
            block([1.2, 1.2, 0.3], &[("hub", at(0.0, 0.0, -0.15))]),
        ),
        (
            "joints.shapecraft".into(),
            Part::new(vec![
                place("base", "base.shapecraft", None),
                place("arm", "arm.shapecraft", Some(hinge)),
                place("slide", "slide.shapecraft", Some(slider)),
                place("wheel", "wheel.shapecraft", Some(axle)),
                gears.into(),
            ]),
        ),
    ])));
    model.update(Message::Open("joints.shapecraft".into()))?;
    Ok(model)
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_blends_demo_builds() -> anyhow::Result<()> {
        let model = super::blends()?;
        assert_eq!(model.viewport.shown.error, None);
        let topo = &model.viewport.shown.world.topo;
        let blended = |step: &str| {
            topo.faces()
                .iter()
                .filter(|(id, _)| topo.name_of(*id).is_some_and(|n| n.starts_with(step)))
                .count()
        };
        assert_eq!((blended("root/"), blended("rim/")), (1, 1));
        assert_eq!(blended("rounds/"), 4);
        Ok(())
    }

    #[test]
    fn the_joints_demo_closes_its_joints() -> anyhow::Result<()> {
        let model = super::joints()?;
        assert_eq!(model.error(), None);
        let world = &model.viewport.shown.world;
        assert_eq!(world.joints.len(), 3);
        assert_eq!(world.couplings.len(), 1);
        // The arm's pivot is on the base's, the slide on the arm's rail.
        let arm = world.instances["arm"].pose;
        let pivot = arm.transform_point(shapecraft_core::math::Point3::new(-1.6, 0.0, -0.15));
        assert!(
            pivot.abs_diff_eq(shapecraft_core::math::Point3::new(0.0, -1.0, 0.2), 1e-6),
            "{pivot}"
        );
        let slide = world.instances["slide"].pose;
        let rail = arm.inverse() * slide;
        let off = rail.transform_point(shapecraft_core::math::Point3::new(0.0, 0.0, -0.2));
        assert!((off.y).abs() < 1e-6 && (off.z - 0.15).abs() < 1e-6, "{off}");
        Ok(())
    }
}
