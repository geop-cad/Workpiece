//! Pattern: features — what steps did to a body: what an extrusion
//! added, a hole took away, a fillet rounded — or whole bodies, repeated
//! along a line, around an axis, along a path, at the points of a sketch
//! or filling a face.

use anyhow::{Context, ensure};
use workpiece_boolean::{Op, Solid, combine, read};
use workpiece_core::math::Point3;
use workpiece_core::world::DatumAxis;
use workpiece_core::world::topo::{BodyId, FaceId};
use workpiece_core::{Builder, Measure, Operation, Reference};
use workpiece_shape::bodies::Combine;

use crate::arrange::{Along, Places, instance, places, whereabouts};
use crate::features::{Change, Features, Move, steps_of};
use crate::fill::Grid;

/// What a pattern or a mirror repeats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum Repeat {
    /// Features: what the steps that made the picked faces did to the
    /// bodies, done again.
    #[default]
    Features,
    /// Whole bodies.
    Bodies,
}

impl workpiece_core::Choices for Repeat {
    const ALL: &'static [Self] = &[Repeat::Features, Repeat::Bodies];

    fn label(self) -> &'static str {
        match self {
            Repeat::Features => "Features",
            Repeat::Bodies => "Bodies",
        }
    }

    fn icon(self) -> Option<&'static str> {
        Some(match self {
            Repeat::Features => "repeat-features",
            Repeat::Bodies => "repeat-bodies",
        })
    }
}

/// What becomes of the copies of bodies: bodies of their own, joined
/// with what they are copies of, or cut out of (or kept in common with)
/// another body.
pub(crate) const BODIES_BECOME: [Combine; 4] = [
    Combine::NewBody,
    Combine::Join,
    Combine::Cut,
    Combine::Intersect,
];

/// What is repeated, and where the copies go.
#[derive(
    Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, workpiece_core::Arguments,
)]
#[serde(default)]
pub struct PatternArgs {
    /// Whether features or whole bodies are repeated.
    #[arg(choice, label = "", group = "Repeat")]
    pub of: Repeat,
    /// Faces of the features that are repeated: one of each is enough.
    /// Features with others between them are repeated with those.
    #[arg(
        required,
        label = "Faces of the features",
        when = "self.of == Repeat::Features",
        group = "Repeat"
    )]
    pub features: Vec<Reference<FaceId>>,
    /// The bodies that are repeated.
    #[arg(
        required,
        label = "Bodies",
        when = "self.of == Repeat::Bodies",
        group = "Repeat"
    )]
    pub bodies: Vec<Reference<BodyId>>,
    /// Along a line, around a circle, along a path, at points, or
    /// filling a face.
    #[arg(choice, label = "", group = "Along")]
    pub along: Along,
    /// The flat face filled.
    #[arg(label = "Face", when = "self.along == Along::Fill", group = "Along")]
    pub face: Option<Reference<FaceId>>,
    /// The axis: the line's direction, what the circle is around, or
    /// (filling, where it is picked) what the rows run along.
    #[arg(
        label_by = "match self.along { Along::Circle => \"Around\", Along::Fill => \"Rows along\", _ => \"Direction\" }",
        empty = "The face's x",
        when = "matches!(self.along, Along::Line | Along::Circle | Along::Fill)",
        group = "Along"
    )]
    pub axis: Option<Reference<DatumAxis>>,
    /// The sketch whose curves are the path, or whose points are the
    /// places.
    #[arg(
        when = "matches!(self.along, Along::Path | Along::Points)",
        group = "Along"
    )]
    pub sketch: Option<Reference<workpiece_core::Sketched>>,
    /// How the face is filled.
    #[arg(
        choice,
        label = "",
        when = "self.along == Along::Fill",
        group = "Along"
    )]
    pub grid: Grid,
    /// How many there are in the end, what is repeated counted.
    #[serde(deserialize_with = "crate::arrange::whole")]
    #[arg(
        when = "matches!(self.along, Along::Line | Along::Circle | Along::Path)",
        group = "Along"
    )]
    pub count: usize,
    /// Along a line or a row of a fill: how far each is from the one
    /// before.
    #[arg(
        when = "matches!(self.along, Along::Line | Along::Fill)",
        group = "Along"
    )]
    pub spacing: Measure,
    /// Filling: how far apart the rows (or rings) are, where that is
    /// not what the grid says.
    #[arg(
        label_by = "if matches!(self.grid, Grid::Rings | Grid::Polygon) { \"Rings apart\" } else { \"Rows apart\" }",
        icon = "f-spacing",
        row,
        when = "self.along == Along::Fill && !matches!(self.grid, Grid::Square | Grid::Hexagonal)",
        group = "Along"
    )]
    pub across: Measure,
    /// Around a circle: the angle they are spread over, in degrees (360
    /// or more: all the way around).
    #[arg(
        label = "Angle",
        row,
        when = "self.along == Along::Circle",
        group = "Along"
    )]
    pub angle: Measure,
    /// Filling: the angle the rows are turned by, in degrees.
    #[arg(label = "Angle", when = "self.along == Along::Fill", group = "Along")]
    pub turned: Measure,
    /// Filling with polygons: how many sides they have.
    #[arg(
        label = "Sides",
        row,
        when = "self.along == Along::Fill && self.grid == Grid::Polygon",
        group = "Along"
    )]
    pub sides: usize,
    /// Filling: how far in from the face's edges every copy stays.
    #[arg(label = "Margin", when = "self.along == Along::Fill", group = "Along")]
    pub margin: Measure,
    /// Along a path: whether the copies keep facing the way what is
    /// repeated does (else each is turned as the path has turned).
    #[arg(
        label = "Copies",
        options = "Turn with the path|Stay as they are",
        icons = "path-turn|path-upright",
        when = "self.along == Along::Path",
        group = "Along"
    )]
    pub upright: bool,
    /// Of bodies: what becomes of the copies — bodies of their own, one
    /// body with what they are copies of, or cut out of (or kept in
    /// common with) the target.
    #[arg(
        label = "",
        among = "BODIES_BECOME",
        when = "self.of == Repeat::Bodies",
        group = "Result"
    )]
    pub combine: Combine,
    /// The body they are cut out of.
    #[arg(
        label = "Of",
        when = "self.of == Repeat::Bodies && matches!(self.combine, Combine::Cut | Combine::Intersect)",
        group = "Result"
    )]
    pub target: Option<Reference<BodyId>>,
}

impl Default for PatternArgs {
    fn default() -> Self {
        Self {
            of: Repeat::Features,
            features: Vec::new(),
            bodies: Vec::new(),
            along: Along::Line,
            axis: None,
            sketch: None,
            face: None,
            grid: Grid::Square,
            count: 3,
            spacing: Measure::new(10.0),
            across: Measure::new(10.0),
            angle: Measure::new(360.0),
            turned: Measure::new(0.0),
            sides: 6,
            margin: Measure::new(2.0),
            upright: false,
            combine: Combine::NewBody,
            target: None,
        }
    }
}

impl PatternArgs {
    fn places(&self) -> Places<'_> {
        Places {
            along: self.along,
            axis: &self.axis,
            sketch: &self.sketch,
            count: self.count,
            spacing: &self.spacing,
            angle: &self.angle,
            upright: self.upright,
            face: &self.face,
            grid: self.grid,
            across: &self.across,
            turned: &self.turned,
            sides: self.sides,
            margin: &self.margin,
        }
    }
}

/// Repeats features or bodies. Features: what the steps that made the
/// picked faces (and the steps between them) added to the bodies and
/// took from them is added and taken again at every place of the
/// pattern. Bodies: copies (`{id}/1`, `{id}/2`, …) of their own, joined
/// with the body, or — all of them, the body too — cut out of another.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Pattern {
    /// The operation's id.
    pub id: String,
    /// What is repeated, and where.
    pub args: PatternArgs,
}

impl Pattern {
    /// The body `now` (called `name`) with the steps done on it again at
    /// every place of the pattern after the first: for each, the body
    /// moved back from there, the steps done on it, and what comes of it
    /// moved there again. What they make at place `k` is named under
    /// `{id}/{k}`.
    fn again(
        &self,
        builder: &dyn Builder,
        features: &Features,
        name: &str,
        now: Solid,
        (where_, piece, near): (&Places<'_>, &Solid, Point3),
    ) -> anyhow::Result<Option<Solid>> {
        // The moves of the pattern, told from what it does to a small
        // block: where its corner goes, and its three edges from there.
        let axes = [Point3::X, Point3::Y, Point3::Z];
        let probe = workpiece_boolean::block("probe", near, axes, [1.0; 3])?;
        let corner = |at: Point3| {
            probe
                .vertices
                .iter()
                .position(|v| v.point.distance(at) < 1e-9)
                .context("a block has corners")
        };
        let which = [near, near + axes[0], near + axes[1], near + axes[2]]
            .map(corner)
            .into_iter()
            .collect::<anyhow::Result<Vec<usize>>>()?;
        let mut result = now;
        for (k, moved) in places(where_, builder, &probe, near, piece)?
            .iter()
            .enumerate()
        {
            let at = |i: usize| moved.vertices[which[i]].point;
            let origin = at(0);
            let cols = [at(1) - origin, at(2) - origin, at(3) - origin];
            let there = |p: Point3| {
                let d = p - near;
                origin + cols[0] * d.x + cols[1] * d.y + cols[2] * d.z
            };
            let back = |p: Point3| {
                let d = p - origin;
                near + Point3::new(d.dot(cols[0]), d.dot(cols[1]), d.dot(cols[2]))
            };
            let under = format!("{}/{}", self.id, k + 1);
            let by = Move {
                back: &back,
                there: &there,
                mirrored: false,
            };
            let Some(made) = features.redone(builder, name, &result, &by, &under)? else {
                return Ok(None);
            };
            result = made;
        }
        Ok(Some(result))
    }

    /// The features repeated: see [`Pattern`].
    fn features(
        &self,
        builder: &dyn Builder,
        out: &mut workpiece_shape::bodies::Out,
    ) -> anyhow::Result<()> {
        let args = &self.args;
        ensure!(
            !args.features.is_empty(),
            "pick a face of each feature to repeat"
        );
        // What the features did: the bodies before the first of them
        // and after the last.
        let features = Features::of(builder, &args.features)?;
        let topo = &builder.tracked().topo;
        let where_ = args.places();
        let mut done = false;
        let mut said = Vec::new();
        let mut times = None;
        for Change {
            name,
            existed,
            added,
            taken,
        } in &features.changes
        {
            // (All of them from about where the features are.)
            let piece = if added.is_empty() { taken } else { added };
            let near = whereabouts(piece);
            // (How they are spread, said once: where the first of them
            // is; and as how many holes their callouts say they are.)
            if said.is_empty() {
                said = where_.said(builder, &self.id, near);
            }
            // The body as it is now, with the same done again at each
            // place: first what is added, then what is taken.
            let Some(body) = crate::arrange::named_solid(topo, name) else {
                continue;
            };
            if times.is_none() {
                times = Some(places(&where_, builder, piece, near, piece)?.len() + 1);
            }
            // The steps themselves done again where each copy goes, as
            // far as that can be: a cut then takes what its tool meets
            // there, not a copy of what it met where it was first.
            // (Steps that place themselves by the body's own faces do the
            // same again wherever the body is moved to: of those, what
            // they added and took is repeated instead. Steps that fail
            // where a copy goes say so: a copy of what they did elsewhere
            // would not be what they do there.)
            if *existed {
                let again = self
                    .again(
                        builder,
                        &features,
                        name,
                        read(topo, body)?,
                        (&where_, piece, near),
                    )
                    .with_context(|| format!("doing the features again on {name}"))?;
                if let Some(again) = again {
                    out.remove(topo, body);
                    out.write(&again, name);
                    done = true;
                    continue;
                }
            }
            let mut result = read(topo, body)?;
            let mut k = 0;
            for (op, piece, what) in [
                (Op::Union, added, "adding"),
                (Op::Cut, taken, "taking away"),
            ] {
                if piece.is_empty() {
                    continue;
                }
                for (n, copy) in places(&where_, builder, piece, near, piece)?
                    .iter()
                    .enumerate()
                {
                    k += 1;
                    let copy = instance(copy.clone(), &format!("{}/{k}", self.id));
                    result = combine(op, &result, &copy)
                        .with_context(|| format!("{what} number {} at {name}", n + 2))?
                        .solid;
                }
            }
            out.remove(topo, body);
            if !result.is_empty() {
                out.write(&result, name);
            }
            done = true;
        }
        ensure!(
            done,
            "the features picked changed no body: nothing to repeat"
        );
        // (The holes repeated: their callouts say as many more.)
        said.extend(crate::features::callouts_times(
            builder,
            &args.features,
            times.unwrap_or(args.count),
        ));
        out.made.annotations = said;
        Ok(())
    }

    /// The bodies repeated: see [`Pattern`].
    fn bodies(
        &self,
        builder: &dyn Builder,
        out: &mut workpiece_shape::bodies::Out,
    ) -> anyhow::Result<()> {
        let args = &self.args;
        ensure!(!args.bodies.is_empty(), "pick the bodies to repeat");
        ensure!(
            BODIES_BECOME.contains(&args.combine),
            "the copies of a body are bodies, joined or cut"
        );
        let topo = &builder.tracked().topo;
        let where_ = args.places();
        let target = match args.combine {
            Combine::Cut | Combine::Intersect => {
                let target = args.target.as_ref().context("pick the body to cut")?;
                let id = topo
                    .resolve::<BodyId>(target.name())
                    .with_context(|| format!("there is no body {:?}", target.name()))?;
                Some((id, target.name().to_owned(), read(topo, id)?))
            }
            _ => None,
        };
        let mut result = target.as_ref().map(|t| t.2.clone());
        let mut k = 0;
        let mut said = Vec::new();
        for body in &args.bodies {
            let name = body.name();
            let id = topo
                .resolve::<BodyId>(name)
                .with_context(|| format!("there is no body {name:?}"))?;
            ensure!(
                target.as_ref().is_none_or(|t| t.0 != id),
                "a body is not cut out of itself"
            );
            let solid = read(topo, id)?;
            let near = whereabouts(&solid);
            if said.is_empty() {
                said = where_.said(builder, &self.id, near);
            }
            // Each copy's faces, edges and vertices are called after the
            // copy (`pattern1/2/…`): what refers to one of an instance
            // finds it again, whatever the other instances are.
            let copies: Vec<Solid> = places(&where_, builder, &solid, near, &solid)?
                .into_iter()
                .map(|copy| {
                    k += 1;
                    instance(copy, &format!("{}/{k}", self.id))
                })
                .collect();
            let first = k + 1 - copies.len();
            match args.combine {
                Combine::NewBody => {
                    for (n, copy) in copies.iter().enumerate() {
                        out.write(copy, &format!("{}/{}", self.id, first + n));
                    }
                }
                Combine::Join => {
                    let mut all = solid;
                    for (n, copy) in copies.iter().enumerate() {
                        all = combine(Op::Union, &all, copy)
                            .with_context(|| format!("joining copy {} of {name}", n + 1))?
                            .solid;
                    }
                    out.remove(topo, id);
                    out.write(&all, name);
                }
                Combine::Cut | Combine::Intersect => {
                    let Some(now) = result.as_mut() else {
                        continue;
                    };
                    if args.combine == Combine::Cut {
                        for (n, tool) in std::iter::once(&solid).chain(&copies).enumerate() {
                            *now = combine(Op::Cut, now, tool)
                                .with_context(|| format!("cutting out number {}", n + 1))?
                                .solid;
                        }
                    } else {
                        // What the target shares with any of them.
                        let mut all = solid;
                        for copy in &copies {
                            all = combine(Op::Union, &all, copy)?.solid;
                        }
                        *now = combine(Op::Intersect, now, &all)?.solid;
                    }
                    out.remove(topo, id);
                }
            }
        }
        if let (Some((id, name, _)), Some(result)) = (target, result) {
            out.remove(topo, id);
            if !result.is_empty() {
                out.write(&result, &name);
            }
        }
        out.made.annotations = said;
        Ok(())
    }
}

impl Operation for Pattern {
    type Args = PatternArgs;
    type EditState = ();

    fn id(&self) -> &str {
        &self.id
    }

    fn id_mut(&mut self) -> &mut String {
        &mut self.id
    }

    fn args(&self) -> &PatternArgs {
        &self.args
    }

    fn args_mut(&mut self) -> &mut PatternArgs {
        &mut self.args
    }

    fn make(&self, builder: &dyn Builder) -> anyhow::Result<workpiece_core::Made<'_>> {
        // (Nothing is changed here: the bodies it makes are written by
        // the builder.)
        let mut out = workpiece_shape::bodies::Out::default();
        match self.args.of {
            Repeat::Features => self.features(builder, &mut out)?,
            Repeat::Bodies => self.bodies(builder, &mut out)?,
        }
        Ok(out.made)
    }

    fn summary(&self) -> String {
        let args = &self.args;
        let of = match args.of {
            Repeat::Features => {
                let steps = steps_of(&args.features);
                if steps.is_empty() {
                    "(nothing picked)".to_owned()
                } else {
                    steps.join(", ")
                }
            }
            Repeat::Bodies if args.bodies.is_empty() => "(nothing picked)".to_owned(),
            Repeat::Bodies => {
                let names: Vec<&str> = args.bodies.iter().map(|b| b.name()).collect();
                names.join(", ")
            }
        };
        let how = match args.along {
            Along::Line => format!("{} of {of}, {} apart", args.count, args.spacing),
            Along::Circle => format!("{} of {of}, around {}°", args.count, args.angle),
            Along::Path => format!("{} of {of}, along a path", args.count),
            Along::Points => format!("{of} at the points of a sketch"),
            Along::Fill => format!(
                "{of} filling a face, {} {} apart",
                args.grid.label().to_lowercase(),
                args.spacing
            ),
        };
        match args.of {
            Repeat::Bodies => format!("{how}, {}", args.combine.label().to_lowercase()),
            Repeat::Features => how,
        }
    }
}

use workpiece_core::Choices as _;
