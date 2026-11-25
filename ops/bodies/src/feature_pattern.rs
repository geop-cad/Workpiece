//! Feature pattern: what steps did to a body — what an extrusion added,
//! a hole took away, a fillet rounded — done again along a line,
//! around an axis, along a path or at the points of a sketch.

use anyhow::{Context, ensure};
use shapecraft_boolean::{Op, Solid, combine, read};
use shapecraft_core::math::Point3;
use shapecraft_core::world::DatumAxis;
use shapecraft_core::world::topo::FaceId;
use shapecraft_core::{Builder, Measure, Operation, Reference};

use crate::arrange::{Along, Places, places, whereabouts};
use crate::features::{Change, Features, Move, steps_of};

/// The features, and where they are repeated.
#[derive(
    Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, shapecraft_core::Arguments,
)]
#[serde(default)]
pub struct FeaturePatternArgs {
    /// Faces of the features that are repeated: one of each is enough.
    /// Features with others between them are repeated with those.
    #[arg(required, label = "Faces of the features")]
    pub features: Vec<Reference<FaceId>>,
    /// Along a line or around a circle.
    #[arg(choice)]
    pub along: Along,
    /// The axis: the line's direction, or what the circle is around.
    #[arg(when = "matches!(self.along, Along::Line | Along::Circle)")]
    pub axis: Option<Reference<DatumAxis>>,
    /// The sketch whose curves are the path, or whose points are the
    /// places.
    #[arg(when = "matches!(self.along, Along::Path | Along::Points)")]
    pub sketch: Option<Reference<shapecraft_core::Sketched>>,
    /// How many there are in the end, the features themselves counted.
    #[serde(deserialize_with = "crate::arrange::whole")]
    #[arg(when = "self.along != Along::Points")]
    pub count: usize,
    /// Along a line: how far each is from the one before.
    #[arg(when = "self.along == Along::Line")]
    pub spacing: Measure,
    /// Around a circle: the angle they are spread over, in degrees (360
    /// or more: all the way around).
    #[arg(label = "Angle °", when = "self.along == Along::Circle")]
    pub angle: Measure,
    /// Along a path: whether the copies keep facing the way the
    /// features do (else each is turned as the path has turned).
    #[arg(
        label = "Copies",
        options = "Turn with the path|Stay as the features are",
        when = "self.along == Along::Path"
    )]
    pub upright: bool,
}

impl Default for FeaturePatternArgs {
    fn default() -> Self {
        Self {
            features: Vec::new(),
            along: Along::Line,
            axis: None,
            sketch: None,
            count: 3,
            spacing: Measure::new(10.0),
            angle: Measure::new(360.0),
            upright: false,
        }
    }
}

/// Repeats features: what the steps that made the picked faces (and
/// the steps between them) added to the bodies and took from them is
/// added and taken again at every place of the pattern.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FeaturePattern {
    /// The operation's id.
    pub id: String,
    /// The features, and where they go.
    pub args: FeaturePatternArgs,
}

impl FeaturePattern {
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
        where_: &Places<'_>,
        near: Point3,
    ) -> anyhow::Result<Option<Solid>> {
        // The moves of the pattern, told from what it does to a small
        // block: where its corner goes, and its three edges from there.
        let axes = [Point3::X, Point3::Y, Point3::Z];
        let probe = shapecraft_boolean::block("probe", near, axes, [1.0; 3])?;
        let corner = |solid: &Solid, at: Point3| {
            let nearest = solid
                .vertices
                .iter()
                .min_by(|a, b| a.point.distance(at).total_cmp(&b.point.distance(at)));
            nearest.map(|v| v.point).context("a block has corners")
        };
        let which: Vec<usize> = [near, near + axes[0], near + axes[1], near + axes[2]]
            .iter()
            .map(|at| {
                let at = corner(&probe, *at)?;
                probe
                    .vertices
                    .iter()
                    .position(|v| v.point == at)
                    .context("a block has corners")
            })
            .collect::<anyhow::Result<_>>()?;
        let mut result = now;
        for (k, moved) in places(where_, builder, &probe, near)?.iter().enumerate() {
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
}

impl Operation for FeaturePattern {
    type Args = FeaturePatternArgs;
    type EditState = ();

    fn id(&self) -> &str {
        &self.id
    }

    fn id_mut(&mut self) -> &mut String {
        &mut self.id
    }

    fn args(&self) -> &FeaturePatternArgs {
        &self.args
    }

    fn args_mut(&mut self) -> &mut FeaturePatternArgs {
        &mut self.args
    }

    fn make(&self, builder: &dyn Builder) -> anyhow::Result<shapecraft_core::Made<'_>> {
        let mut out = shapecraft_shape::bodies::Out::default();
        let args = &self.args;
        ensure!(
            !args.features.is_empty(),
            "pick a face of each feature to repeat"
        );
        // What the features did: the bodies before the first of them
        // and after the last.
        let features = Features::of(builder, &args.features)?;
        let topo = &builder.tracked().topo;
        let where_ = Places {
            along: args.along,
            axis: &args.axis,
            sketch: &args.sketch,
            count: args.count,
            spacing: &args.spacing,
            angle: &args.angle,
            upright: args.upright,
        };
        let mut done = false;
        let mut said = Vec::new();
        for Change {
            name,
            existed,
            added,
            taken,
        } in &features.changes
        {
            // (All of them from about where the features are.)
            let near = whereabouts(if added.is_empty() { taken } else { added });
            // (How they are spread, said once: where the first of them
            // is.)
            if said.is_empty() {
                said = where_.said(builder, &self.id, near);
            }
            // The body as it is now, with the same done again at each
            // place: first what is added, then what is taken.
            let Some(body) = crate::arrange::named_solid(topo, name) else {
                continue;
            };
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
                    .again(builder, &features, name, read(topo, body)?, &where_, near)
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
                for (n, copy) in places(&where_, builder, piece, near)?.iter().enumerate() {
                    k += 1;
                    let copy = crate::arrange::instance(copy.clone(), &format!("{}/{k}", self.id));
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
        let times = features
            .changes
            .iter()
            .find_map(|c| {
                let piece = if c.added.is_empty() {
                    &c.taken
                } else {
                    &c.added
                };
                (!piece.is_empty()).then(|| places(&where_, builder, piece, whereabouts(piece)))
            })
            .transpose()?
            .map_or(args.count, |copies| copies.len() + 1);
        said.extend(crate::features::callouts_times(
            builder,
            &args.features,
            times,
        ));
        out.made.annotations = said;
        Ok(out.made)
    }

    fn summary(&self) -> String {
        let args = &self.args;
        let steps = steps_of(&args.features);
        let of = if steps.is_empty() {
            "(nothing picked)".to_owned()
        } else {
            steps.join(", ")
        };
        let how = match args.along {
            Along::Line => format!("{} apart", args.spacing),
            Along::Circle => format!("around {}°", args.angle),
            Along::Path => "along a path".to_owned(),
            Along::Points => "at the points of a sketch".to_owned(),
        };
        match args.along {
            Along::Points => format!("{of} {how}"),
            _ => format!("{} of {of}, {how}", args.count),
        }
    }
}
