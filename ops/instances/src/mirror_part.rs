//! [`MirrorPart`]: a placed part's mirror image in a plane.

use std::sync::Arc;

use anyhow::Context;
use shapecraft_core::math::Point3;
use shapecraft_core::world::topo::BodyKind;
use shapecraft_core::world::{Connector, DatumPlane, Instance, Joint, JointKind, Limits};
use shapecraft_core::{Builder, Operation, Pose, Reference};

/// The part mirrored, and the plane it is mirrored in.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Default,
    serde::Serialize,
    serde::Deserialize,
    shapecraft_core::Arguments,
)]
pub struct MirrorPartArgs {
    /// The instance that is mirrored: a part placed in this one.
    #[arg(
        one_of = "context.world.instances.keys().cloned().collect()",
        label = "Part",
        when = "!context.world.instances.is_empty()"
    )]
    pub of: Option<String>,
    /// The plane.
    pub plane: Option<Reference<DatumPlane>>,
}

/// Places the mirror image of a placed part: an instance named after
/// the operation's id whose bodies are the part's, mirrored, on the
/// other side of the plane. It is fastened to the part it mirrors.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MirrorPart {
    /// The operation's id; also the new instance's name.
    pub id: String,
    /// What is mirrored, and in what.
    pub args: MirrorPartArgs,
}

impl Operation for MirrorPart {
    type Args = MirrorPartArgs;
    type EditState = ();

    fn id(&self) -> &str {
        &self.id
    }

    fn id_mut(&mut self) -> &mut String {
        &mut self.id
    }

    fn args(&self) -> &MirrorPartArgs {
        &self.args
    }

    fn args_mut(&mut self) -> &mut MirrorPartArgs {
        &mut self.args
    }

    fn make(&self, builder: &dyn Builder) -> anyhow::Result<shapecraft_core::Made<'_>> {
        // (Nothing is changed here: what it makes is put in by the
        // builder.)
        let mut out = shapecraft_boolean::Out::default();
        let args = &self.args;
        let of = args.of.as_ref().context("pick the part to mirror")?;
        let plane = args.plane.as_ref().context("pick the plane to mirror in")?;
        let plane = builder.resolve(plane)?;
        anyhow::ensure!(
            !builder.peek().instances.contains_key(&self.id),
            "instance {:?} exists",
            self.id
        );
        let first = builder
            .placed(of)
            .with_context(|| format!("there is no placed part {of:?}"))?
            .clone();
        anyhow::ensure!(
            first.world.instances.is_empty(),
            "{of} places parts of its own: an assembly is not mirrored yet"
        );
        // The part's own bodies, mirrored in its xy plane; then placed
        // so that all of it is the part's image in the plane.
        let mut image = (*first.world).clone();
        let bodies: Vec<_> = image
            .topo
            .bodies()
            .iter()
            .filter(|(_, b)| b.kind == BodyKind::Solid)
            .map(|(id, _)| id)
            .collect();
        for body in bodies {
            let name = image.topo.name_of(body).unwrap_or("body").to_owned();
            let solid = shapecraft_boolean::read(&image.topo, body)?;
            let flipped = solid.mapped(&|p| Point3::new(p.x, p.y, -p.z), true, 1.0)?;
            shapecraft_boolean::remove(&mut image.topo, body);
            shapecraft_boolean::write(&mut image.topo, &flipped, &name)?;
        }
        let (origin, normal) = (plane.origin(), plane.normal());
        let point = |p: Point3| p - normal * (2.0 * (p - origin).dot(normal));
        let way = |v: Point3| v - normal * (2.0 * v.dot(normal));
        let pose = Pose::from_axes(
            point(first.pose.transform_point(Point3::ZERO)),
            way(first.pose.transform_vector(Point3::X)),
            way(first.pose.transform_vector(Point3::Y)),
        );
        let mirrored = Instance {
            pose,
            world: Arc::new(image),
            fixed: false,
        };
        out.made.instances.push((self.id.clone(), Some(mirrored)));
        // Fastened to the part it mirrors: it goes where that goes.
        out.made.joints.push(Joint {
            name: self.id.clone(),
            kind: JointKind::Fastened,
            a: Connector {
                instance: Some(of.clone()),
                frame: first.pose.inverse() * pose,
            },
            b: Connector {
                instance: Some(self.id.clone()),
                frame: Pose::IDENTITY,
            },
            limits: Limits::default(),
        });
        Ok(out.made)
    }

    fn summary(&self) -> String {
        format!("of {}", self.args.of.as_deref().unwrap_or("nothing yet"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shapecraft_core::world::ORIGIN;
    use shapecraft_core::{Axis, Measure, Standalone};
    use shapecraft_ops_basic_shapes::{AddBox, AddBoxArgs};

    #[test]
    fn placed_parts_are_mirrored_in_planes() -> anyhow::Result<()> {
        // A part with a box off to one side, placed 5 along x: its image
        // in the yz plane is 5 the other way, the box on the other side.
        let mut part = Standalone::new();
        let pose = Pose::from_translation(Point3::new(1.0, 2.0, 3.0));
        let frame = shapecraft_core::world::DatumFrame::from_pose(pose)?;
        part.world.datums.insert("off".into(), frame.into());
        AddBox {
            id: "box".into(),
            args: AddBoxArgs {
                frame: Some(Reference::datum("off")),
                size: [1.0, 1.0, 1.0].map(Measure::new),
            },
        }
        .apply(&mut part)?;
        let mut b = Standalone::new();
        b.world.instances.insert(
            "one".into(),
            Instance {
                pose: Pose::from_translation(Point3::new(5.0, 0.0, 0.0)),
                world: Arc::new(part.world),
                fixed: true,
            },
        );
        MirrorPart {
            id: "other".into(),
            args: MirrorPartArgs {
                of: Some("one".into()),
                plane: Some(Reference::frame_axis(ORIGIN, Axis::X)),
            },
        }
        .apply(&mut b)?;
        let image = &b.world.instances["other"];
        assert_eq!(image.world.topo.validate(), Ok(()));
        let body = image.world.topo.resolve("box").expect("the box");
        let solid = shapecraft_boolean::read(&image.world.topo, body)?;
        let measured = shapecraft_boolean::measure(&solid);
        assert!((measured.volume - 1.0).abs() < 1e-9, "{}", measured.volume);
        let centre = image.pose.transform_point(measured.centre);
        assert!(
            centre.distance(Point3::new(-6.0, 2.0, 3.0)) < 1e-9,
            "{centre}"
        );
        assert_eq!(b.world.joints.len(), 1);
        Ok(())
    }
}
