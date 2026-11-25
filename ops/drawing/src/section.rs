//! Section: the step that says where the model is cut open to show its
//! inside — along a plane, or along the lines of a sketch (an offset
//! or an aligned section). The 3D view shows the model cut so, and the
//! section views of drawings are of it.

use anyhow::{Context, ensure};
use workpiece_core::math::Point2;
use workpiece_core::world::{Cutting, DatumPlane, Profile};
use workpiece_core::{Builder, Choices, Made, Measure, Operation, Reference, Sketched};

/// What a section cuts along.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
pub enum Along {
    /// A plane.
    #[default]
    Plane,
    /// The lines of a sketch, one after the other, square to the
    /// sketch's plane.
    Sketch,
}

impl Choices for Along {
    const ALL: &'static [Self] = &[Along::Plane, Along::Sketch];

    fn label(self) -> &'static str {
        match self {
            Along::Plane => "A plane",
            Along::Sketch => "A sketch's lines",
        }
    }

    fn icon(self) -> Option<&'static str> {
        Some(match self {
            Along::Plane => "section-plane",
            Along::Sketch => "section-sketch",
        })
    }
}

/// Where a section cuts.
#[derive(
    Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, workpiece_core::Arguments,
)]
pub struct SectionArgs {
    /// What it cuts along.
    #[arg(choice, label = "Along")]
    pub along: Along,
    /// The plane: a datum plane, a frame's plane or a flat face.
    #[serde(default)]
    #[arg(when = "self.along == Along::Plane")]
    pub plane: Option<Reference<DatumPlane>>,
    /// How far from the plane it cuts, along the plane's normal.
    #[arg(when = "self.along == Along::Plane")]
    pub offset: Measure,
    /// The sketch whose lines it cuts along: one line, or several end
    /// to end.
    #[serde(default)]
    #[arg(when = "self.along == Along::Sketch")]
    pub sketch: Option<Reference<Sketched>>,
    /// Whether the other side is taken away.
    #[serde(default)]
    #[arg(
        label = "Takes away",
        options = "This side|The other side",
        icons = "side-front|side-back"
    )]
    pub flip: bool,
}

impl Default for SectionArgs {
    fn default() -> Self {
        Self {
            along: Along::Plane,
            plane: None,
            offset: Measure::new(0.0),
            sketch: None,
            flip: false,
        }
    }
}

/// `world` cut open as `cut` cuts it (`far`: farther than all of it
/// reaches): what is to the left of its line taken away — on past the
/// line's ends, or, of a cut that is `bounded` (along a sketch's
/// lines), only as far as the line goes: a line from a hub's middle to
/// its rim takes away the one quarter, a half section.
pub fn opened(world: &workpiece_core::World, cut: &Cutting, far: f64) -> workpiece_core::World {
    if !cut.bounded {
        return workpiece_shape::cut::cut_left_of(world, cut.origin, cut.x, cut.y, &cut.path, far);
    }
    let normal = cut.x.cross(cut.y);
    let at = |p: Point2| cut.origin + cut.x * p.x + cut.y * p.y;
    // What is taken away: of each piece of the line, the block to its
    // left, as long as the piece, through all of it.
    let blocks: Vec<_> = cut
        .path
        .windows(2)
        .filter_map(|w| {
            let along = (w[1] - w[0]).try_normalize()?;
            let (d, left) = (
                cut.x * along.x + cut.y * along.y,
                cut.x * -along.y + cut.y * along.x,
            );
            workpiece_boolean::block(
                "section",
                at(w[0]) - normal * far,
                [d, left, d.cross(left)],
                [w[0].distance(w[1]), far, 2.0 * far],
            )
            .ok()
        })
        .collect();
    let mut out = world.clone();
    let bodies: Vec<_> = out
        .topo
        .bodies()
        .iter()
        .filter(|(_, b)| b.kind == workpiece_core::world::topo::BodyKind::Solid)
        .map(|(id, _)| id)
        .collect();
    for body in bodies {
        let Ok(mut solid) = workpiece_boolean::read(&out.topo, body) else {
            continue;
        };
        let Some(name) = out.topo.name_of(body).map(str::to_owned) else {
            continue;
        };
        for block in &blocks {
            if let Ok(kept) = workpiece_boolean::combine(workpiece_boolean::Op::Cut, &solid, block)
            {
                solid = kept.solid;
            }
        }
        workpiece_boolean::remove(&mut out.topo, body);
        if !solid.is_empty() {
            let _ = workpiece_boolean::write(&mut out.topo, &solid, &name);
        }
    }
    for instance in out.instances.values_mut() {
        let back = instance.pose.inverse();
        let inner = Cutting {
            origin: back.transform_point(cut.origin),
            x: back.transform_vector(cut.x),
            y: back.transform_vector(cut.y),
            ..cut.clone()
        };
        if workpiece_shape::cut::left_whole(&instance.world, &inner) {
            continue;
        }
        instance.world = std::sync::Arc::new(opened(&instance.world, &inner, far));
    }
    out
}

/// The lines of `profile` as one path, end to end, in its plane.
pub fn path_of(profile: &Profile) -> anyhow::Result<Vec<Point2>> {
    let mut left: Vec<(Point2, Point2)> = profile
        .edges
        .iter()
        .filter(|e| e.curve.degree() == 1 && e.curve.control_points().len() == 2)
        .map(|e| (e.curve.point_at(e.t0), e.curve.point_at(e.t1)))
        .collect();
    ensure!(
        !left.is_empty(),
        "the sketch has no line to cut along: draw one, or several end to end"
    );
    let reach = left
        .iter()
        .fold(0.0_f64, |m, (a, b)| m.max(a.length()).max(b.length()))
        .max(1e-9);
    let near = |a: Point2, b: Point2| a.distance(b) <= reach * 1e-7;
    let (a, b) = left.remove(0);
    let mut path = std::collections::VecDeque::from([a, b]);
    while !left.is_empty() {
        let (first, last) = (path[0], path[path.len() - 1]);
        let next = left.iter().position(|(a, b)| {
            near(*a, last) || near(*b, last) || near(*a, first) || near(*b, first)
        });
        let (a, b) = left.remove(next.context(
            "the sketch's lines do not run end to end: a section cuts along one line of them",
        )?);
        if near(a, last) {
            path.push_back(b);
        } else if near(b, last) {
            path.push_back(a);
        } else if near(b, first) {
            path.push_front(a);
        } else {
            path.push_front(b);
        }
    }
    // (Pieces that run straight on are one.)
    let mut out: Vec<Point2> = Vec::new();
    for p in path {
        if let [.., a, b] = out[..]
            && (b - a).perp_dot(p - b).abs() <= 1e-9 * (b - a).length() * (p - b).length()
            && (b - a).dot(p - b) > 0.0
        {
            *out.last_mut().expect("has two") = p;
        } else {
            out.push(p);
        }
    }
    Ok(out)
}

/// Where the model is cut open: what the 3D view shows of it cut, and
/// what the section views of drawings are of. Nothing of the model
/// follows from it.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Section {
    /// The operation's id: what the section is called.
    pub id: String,
    /// Where it cuts.
    pub args: SectionArgs,
}

impl Section {
    /// Where it cuts `world`; `offset` is its offset's value.
    fn cutting(&self, world: &workpiece_core::World, offset: f64) -> anyhow::Result<Cutting> {
        let args = &self.args;
        let cut = match args.along {
            Along::Plane => {
                let plane =
                    world.resolve(args.plane.as_ref().context("pick the plane to cut along")?)?;
                Cutting::plane(plane.origin() + plane.normal() * offset, plane.normal())
            }
            Along::Sketch => {
                let sketch = args
                    .sketch
                    .as_ref()
                    .context("pick the sketch whose lines it cuts along")?;
                let profile = world
                    .profiles
                    .get(sketch.name())
                    .with_context(|| format!("there is no sketch {:?}", sketch.name()))?;
                Cutting {
                    origin: profile.plane.origin(),
                    x: profile.plane.x_dir(),
                    y: profile.plane.y_dir(),
                    path: path_of(profile)?,
                    // (As far as the lines go: a line from a hub's
                    // middle to its rim cuts the one half.)
                    bounded: true,
                }
            }
        };
        Ok(if args.flip { cut.flipped() } else { cut })
    }
}

impl Operation for Section {
    type Args = SectionArgs;
    type EditState = ();

    fn id(&self) -> &str {
        &self.id
    }

    fn id_mut(&mut self) -> &mut String {
        &mut self.id
    }

    fn args(&self) -> &SectionArgs {
        &self.args
    }

    fn args_mut(&mut self) -> &mut SectionArgs {
        &mut self.args
    }

    fn make(&self, builder: &dyn Builder) -> anyhow::Result<Made<'_>> {
        let offset = builder.measure(&self.args.offset, "the offset")?;
        let cut = self.cutting(builder.tracked(), offset)?;
        Ok(Made {
            sections: vec![(self.id.clone(), cut)],
            ..Made::default()
        })
    }

    fn visualize_edit(
        &self,
        context: &workpiece_core::EditContext<'_>,
        (): &(),
    ) -> workpiece_core::EditVisualization<workpiece_core::Edit<SectionArgs, ()>> {
        let args = &self.args;
        let fields = workpiece_core::Arguments::fields(args, context);
        let mut handles = std::collections::BTreeMap::new();
        // An arrow square to the plane, from where the middle of the
        // model is over it: dragging it sets how far off the plane the
        // model is cut.
        let plane = args
            .plane
            .as_ref()
            .filter(|_| args.along == Along::Plane && args.offset.is_plain())
            .and_then(|plane| context.world.resolve(plane).ok());
        if let Some(plane) = plane {
            let normal = plane.normal();
            let middle = context
                .world
                .extent()
                .map_or(plane.origin(), |(lo, hi)| (lo + hi) / 2.0);
            let from = middle - normal * (middle - plane.origin()).dot(normal);
            let drag = {
                let args = args.clone();
                move |offset: f64| SectionArgs {
                    offset: Measure::new(offset),
                    ..args.clone()
                }
            };
            let arrow =
                workpiece_core::handles::LinearHandle::new(from, normal, args.offset.value, drag);
            handles.extend(arrow.map(|h| ("offset".to_owned(), h.into())));
        }
        workpiece_core::EditVisualization::new(workpiece_core::widgets::VBox::new(fields), handles)
            .map(workpiece_core::Edit::args)
    }

    fn summary(&self) -> String {
        match self.args.along {
            Along::Plane => format!(
                "along {}",
                workpiece_core::reference::described(&self.args.plane)
            ),
            Along::Sketch => format!(
                "along {}",
                workpiece_core::reference::described(&self.args.sketch)
            ),
        }
    }
}
