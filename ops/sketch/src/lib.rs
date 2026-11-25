//! The sketch operation: constrained 2D geometry on a plane.
//!
//! Lines, arcs, circles and splines exist only here. What a sketch leaves
//! in the world is a [`Profile`](workpiece_core::world::Profile): NURBS curves
//! with names and tolerances, for features to build on.
//!
//! * [`model`]: points, entities, constraints and dimensions: the
//!   operation's arguments.
//! * [`solve`]: the constraints' equations, solved with `workpiece-solver`.
//! * [`tools`]: what the pointer does on the sketch's canvas, and the
//!   edit session's state.
//! * [`modify`]: what the buttons do with what is selected.
//!
//! The arguments hold what the user drew, solved as they drew it. When
//! the model under a sketch changes (a projected edge moves), the sketch
//! is solved again where it is applied, and the solved positions go to
//! the part's [`Cache`](workpiece_core::Cache): the arguments stay what the
//! user made, and the next build and the next edit start from the cache,
//! so the sketch stays on the solution it is on. (An edit by the user
//! makes the arguments the newer starting point: a cached solution of
//! other arguments is not used.)

mod annotate;
mod copy;
mod corners;
mod draw;
mod geometry;
pub mod layout;
pub mod model;
pub mod modify;
mod profile;
mod project;
pub mod solve;
pub mod text;
pub mod tools;
mod trim;
mod ui;

pub use model::{
    Constraint, DimTolerance, Dimension, Entity, Id, Item, Kind, Point, Shape, SketchArgs,
    SketchAxis,
};
pub use modify::Action;
pub use solve::{Diagnosis, Var};
pub use tools::{SketchState, Tool};

use std::collections::BTreeMap;
use std::rc::Rc;

use anyhow::Context;
use workpiece_core::handles::Canvas;
use workpiece_core::math::Point2;
use workpiece_core::world::DatumPlane;
use workpiece_core::{
    Builder, Cache, Edit, EditContext, EditVisualization, Operation, Parameters, Reference,
    ReferenceKind, Source, World,
};

/// A sketch, named after the operation's id: its profile is
/// `world.profiles[id]`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Sketch {
    /// The operation's id; also the profile's name.
    pub id: String,
    /// The plane and what is drawn on it.
    pub args: SketchArgs,
}

impl Sketch {
    /// Projects what `source` is in `world` — an edge, a vertex, a face
    /// (its edges), an axis — onto the sketch's plane, as fixed
    /// construction geometry to constrain against: it follows the model
    /// from build to build. Gives the entities made, one per curve.
    pub fn project(
        &mut self,
        world: &World,
        source: &workpiece_core::Source,
    ) -> anyhow::Result<Vec<Id>> {
        let plane = self
            .args
            .plane
            .as_ref()
            .context("the sketch has no plane yet")?;
        let plane = world.resolve(plane)?;
        let found = project::project(world, &plane, source)?;
        Ok(found
            .into_iter()
            .map(|found| project::place(&mut self.args, found))
            .collect())
    }

    /// Its dimensions as a drawing writes them, in the order they were
    /// made: each value with what kind it is (`R`, `Ø`, `°`) and its
    /// tolerance.
    pub fn dimensions(&self) -> Vec<String> {
        let texts = self
            .args
            .constraints
            .values()
            .filter(|c| c.dimension.is_some());
        texts
            .map(|c| {
                let (text, upper, lower) = draw::dimension_text(c);
                if upper.is_empty() && lower.is_empty() {
                    text
                } else {
                    format!("{text} {upper}/{lower}")
                }
            })
            .collect()
    }

    /// An empty sketch: only its origin, a fixed construction point to
    /// constrain against.
    pub fn new(id: impl Into<String>) -> Self {
        let mut args = SketchArgs::default();
        let origin = args.add_point(Point2::ZERO);
        if let Some(point) = args.points.get_mut(&origin) {
            point.fixed = true;
        }
        let dot = args.add(Shape::Point(origin));
        if let Some(entity) = args.entities.get_mut(&dot) {
            entity.construction = true;
        }
        // The plane's axes: construction lines to snap to, measure from
        // and mirror in.
        for axis in [SketchAxis::U, SketchAxis::V] {
            let end = args.add_point(axis.unit());
            if let Some(point) = args.points.get_mut(&end) {
                point.fixed = true;
            }
            let line = args.add_line(origin, end);
            if let Some(entity) = args.entities.get_mut(&line) {
                entity.construction = true;
                entity.axis = Some(axis);
            }
        }
        Self {
            id: id.into(),
            args,
        }
    }

    /// An empty sketch on the first plane of `selection` (in `world`), if
    /// there is one.
    pub fn from_selection(id: impl Into<String>, world: &World, selection: &[Source]) -> Self {
        let mut sketch = Self::new(id);
        sketch.args.plane = selection
            .iter()
            .find(|s| ReferenceKind::Plane.accepts(world, s))
            .map(|s| Reference::new(s.clone()));
        // It starts with what lies in its plane.
        if let Some(plane) = sketch
            .args
            .plane
            .as_ref()
            .and_then(|p| world.resolve(p).ok())
        {
            project::auto(&mut sketch.args, world, &plane);
        }
        sketch
    }

    /// A number that changes when the arguments do: what a cached
    /// solution was solved from. (FNV-1a over their debug form, as two
    /// exact floats.)
    fn fingerprint(&self) -> Vec<f64> {
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for byte in format!("{:?}", self.args).bytes() {
            hash = (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3);
        }
        vec![(hash >> 32) as f64, (hash & 0xffff_ffff) as f64]
    }

    /// The arguments with the positions `cache` holds for them, if it
    /// holds a solution of these very arguments.
    fn cached(&self, cache: &Cache) -> Option<SketchArgs> {
        let mut entries: BTreeMap<&str, &[f64]> = cache.of(&self.id).collect();
        if entries.remove("args") != Some(&self.fingerprint()[..]) {
            return None;
        }
        let mut args = self.args.clone();
        for (name, values) in entries {
            let id = || name[1..].parse::<Id>().ok();
            match (name.as_bytes().first(), values) {
                (Some(b'p'), &[x, y]) => {
                    if let Some(p) = id().and_then(|id| args.points.get_mut(&id)) {
                        p.at = Point2::new(x, y);
                    }
                }
                (Some(b'r'), &[r]) => {
                    if let Some(Shape::Circle { radius, .. }) = id()
                        .and_then(|id| args.entities.get_mut(&id))
                        .map(|e| &mut e.shape)
                    {
                        *radius = r;
                    }
                }
                (Some(b'a'), &[u, v]) => {
                    if let Some(Shape::Axis { direction, .. }) = id()
                        .and_then(|id| args.entities.get_mut(&id))
                        .map(|e| &mut e.shape)
                    {
                        *direction = Point2::new(u, v);
                    }
                }
                _ => {}
            }
        }
        Some(args)
    }

    /// The sketch as it is shown and edited: from the cache if that holds
    /// its solution, else as stored.
    fn shown(&self, cache: &Cache) -> SketchArgs {
        self.cached(cache).unwrap_or_else(|| self.args.clone())
    }

    /// The plane, and the sketch solved against `world`: what is projected
    /// projected again, dimensions given by formulas valued with
    /// `parameters`, then the constraints solved from the arguments if
    /// they hold already, else from the cached solution, else from the
    /// arguments.
    fn solved(
        &self,
        world: &World,
        cache: &Cache,
        parameters: &Parameters,
    ) -> anyhow::Result<(DatumPlane, SketchArgs)> {
        let plane = self
            .args
            .plane
            .as_ref()
            .context("pick a plane to sketch on")?;
        let plane = world.resolve(plane)?;
        // What the formulas give, here.
        let mut driven = Vec::new();
        for (&id, constraint) in &self.args.constraints {
            let Some(dimension) = &constraint.dimension else {
                continue;
            };
            if let Some(formula) = &dimension.formula {
                let value = workpiece_core::measure::evaluate(formula, parameters)
                    .with_context(|| format!("dimension {:?}", constraint.kind))?;
                anyhow::ensure!(
                    value > 0.0,
                    "{formula:?} gives {value}: dimensions are positive"
                );
                driven.push((id, value));
            }
        }
        // Each value is gone to in steps, so the sketch does not flip.
        let drive = |args: &mut SketchArgs| {
            driven.iter().all(|&(id, value)| {
                let now = args.constraints[&id].dimension.as_ref().map(|d| d.value);
                now == Some(value) || args.set_value(id, value)
            })
        };
        let mut args = self.args.clone();
        project::refresh(&mut args, world, &plane)?;
        let as_stored = driven.iter().all(|&(id, value)| {
            args.constraints[&id].dimension.as_ref().map(|d| d.value) == Some(value)
        });
        if as_stored && args.holds() {
            return Ok((plane, args));
        }
        if let Some(mut cached) = self.cached(cache) {
            project::refresh(&mut cached, world, &plane)?;
            if drive(&mut cached) && cached.solve() {
                return Ok((plane, cached));
            }
        }
        if !(drive(&mut args) && args.solve()) {
            let diagnosis = args.diagnose();
            let named: Vec<String> = diagnosis
                .conflicting
                .iter()
                .filter_map(|id| Some(format!("{:?}", args.constraints.get(id)?.kind)))
                .collect();
            if named.is_empty() {
                anyhow::bail!("the sketch's constraints cannot be solved from where it is");
            }
            anyhow::bail!("the sketch's constraints conflict: {}", named.join(", "));
        }
        Ok((plane, args))
    }
}

impl Operation for Sketch {
    type Args = SketchArgs;
    type EditState = SketchState;

    fn id(&self) -> &str {
        &self.id
    }

    fn id_mut(&mut self) -> &mut String {
        &mut self.id
    }

    fn args(&self) -> &SketchArgs {
        &self.args
    }

    fn args_mut(&mut self) -> &mut SketchArgs {
        &mut self.args
    }

    fn make(&self, builder: &dyn Builder) -> anyhow::Result<workpiece_core::Made<'_>> {
        anyhow::ensure!(
            !builder.tracked().profiles.contains_key(&self.id),
            "sketch {:?} exists",
            self.id
        );
        let (plane, args) =
            self.solved(builder.tracked(), builder.cache(), builder.parameters())?;
        // The solution, for the next build and the next edit.
        let points = args
            .points
            .iter()
            .map(|(id, p)| (format!("p{id}"), vec![p.at.x, p.at.y]));
        let radii = args.entities.iter().filter_map(|(id, e)| match e.shape {
            Shape::Circle { radius, .. } => Some((format!("r{id}"), vec![radius])),
            Shape::Axis { direction, .. } => {
                Some((format!("a{id}"), vec![direction.x, direction.y]))
            }
            _ => None,
        });
        let entries: Vec<_> = points
            .chain(radii)
            .chain([("args".to_owned(), self.fingerprint())])
            .collect();
        Ok(workpiece_core::Made {
            // (Its dimensions are said of the model: a drawing shows
            // them.)
            annotations: annotate::annotations(&self.id, &args, &plane),
            profiles: vec![(self.id.clone(), profile::profile(&args, plane))],
            kept: vec![(self.id.clone(), entries)],
            ..Default::default()
        })
    }

    fn references(&self) -> Vec<Source> {
        let plane = self.args.plane.iter().map(|r| r.source.clone());
        let projected = self
            .args
            .entities
            .values()
            .filter_map(|e| e.projected.clone());
        plane.chain(projected).collect()
    }

    fn references_mut(&mut self) -> Vec<&mut Source> {
        let plane = self.args.plane.iter_mut().map(|r| &mut r.source);
        let projected = self
            .args
            .entities
            .values_mut()
            .filter_map(|e| e.projected.as_mut());
        plane.chain(projected).collect()
    }

    fn visualize_edit(
        &self,
        context: &EditContext<'_>,
        state: &SketchState,
    ) -> EditVisualization<Edit<SketchArgs, SketchState>> {
        self.edit_on(context, state, self.edit_plane(context))
    }

    fn edit_plane(&self, context: &EditContext<'_>) -> Option<DatumPlane> {
        context.world.resolve(self.args.plane.as_ref()?).ok()
    }

    fn summary(&self) -> String {
        let op = self;
        let drawn = op
            .args
            .entities
            .values()
            .filter(|e| !e.construction)
            .count();
        let on = op.args.plane.as_ref().map(|r| r.source.clone());
        format!(
            "{drawn} {} on {}",
            if drawn == 1 { "curve" } else { "curves" },
            on.as_ref().map_or_else(
                || "(not picked)".to_owned(),
                workpiece_core::Source::described
            )
        )
    }
}

impl Sketch {
    /// The tools and the canvas that edit the sketch as it lies on
    /// `plane`, for an edit session in `state`: what
    /// [`Operation::visualize_edit`] gives of a sketch on its own
    /// plane, and what draws by hand on anything else that is flat (a
    /// drawing's sheet).
    pub fn edit_on(
        &self,
        context: &EditContext<'_>,
        state: &SketchState,
        plane: Option<DatumPlane>,
    ) -> EditVisualization<Edit<SketchArgs, SketchState>> {
        let args = Rc::new(self.shown(context.cache));
        let session = ui::Session {
            diagnosis: Rc::new(args.diagnose()),
            args,
            state: Rc::new(state.clone()),
            world: context.world.clone(),
            parameters: context.parameters.clone(),
            plane,
        };
        let mut handles = BTreeMap::new();
        if let Some(plane) = plane {
            let (args, state, diagnosis) = (
                session.args.clone(),
                session.state.clone(),
                session.diagnosis.clone(),
            );
            let draw = move |view: &_| draw::shapes(&args, &state, &diagnosis, view);
            let (args, state) = (session.args.clone(), session.state.clone());
            let on_event = move |event| tools::handle(&args, &state, event);
            let canvas = Canvas::new(plane, session.args.center(), draw, on_event);
            handles.insert("sketch".to_owned(), canvas.into());
        }
        EditVisualization::new(session.ribbon(), handles)
            .in_ribbon()
            .with_inspector(session.inspector())
            .with_menu(session.menu())
    }

    /// What is drawn of the sketch when nothing of it is edited, seen as
    /// `view` says: its curves, its dimensions and its lettering.
    pub fn drawn(
        &self,
        view: &workpiece_core::handles::CanvasView,
    ) -> Vec<workpiece_core::handles::CanvasShape> {
        draw::shapes(
            &self.args,
            &SketchState::default(),
            &self.args.diagnose(),
            view,
        )
    }
}

#[cfg(test)]
mod tests;

/// Tells `r` of this crate's operations: their kinds, the tools that
/// add them, and their icons.
pub fn register(r: &mut workpiece_core::Registry) {
    use workpiece_core::Tool;
    r.kind::<Sketch>("sketch");
    r.tool(Tool {
        tab: "Modeling",
        group: "Sketch",
        label: "Sketch",
        icon: "sketch",
        key: 'K',
        hint: "Sketch on a plane: the one selected, or one picked next",
        small: false,
        make: |cx| Sketch::from_selection((cx.fresh_id)("sketch"), cx.world, cx.selection).into(),
    });
    r.tool(Tool {
        tab: "Surfacing",
        group: "Sketch",
        label: "Sketch",
        icon: "sketch",
        key: 'K',
        hint: "Sketch on a plane: the one selected, or one picked next",
        small: false,
        make: |cx| Sketch::from_selection((cx.fresh_id)("sketch"), cx.world, cx.selection).into(),
    });
    r.tool(Tool {
        tab: "Sheet metal",
        group: "Sketch",
        label: "Sketch",
        icon: "sketch",
        key: 'K',
        hint: "Sketch on a plane: the one selected, or one picked next",
        small: false,
        make: |cx| Sketch::from_selection((cx.fresh_id)("sketch"), cx.world, cx.selection).into(),
    });
    r.icon("arc", include_str!("../icons/arc.svg"));
    r.icon("center-arc", include_str!("../icons/center-arc.svg"));
    r.icon(
        "center-rectangle",
        include_str!("../icons/center-rectangle.svg"),
    );
    r.icon("circle", include_str!("../icons/circle.svg"));
    r.icon("circle-2", include_str!("../icons/circle-2.svg"));
    r.icon("circle-3", include_str!("../icons/circle-3.svg"));
    r.icon("coincident", include_str!("../icons/coincident.svg"));
    r.icon("collinear", include_str!("../icons/collinear.svg"));
    r.icon("construction", include_str!("../icons/construction.svg"));
    r.icon("curvature", include_str!("../icons/curvature.svg"));
    r.icon("delete", include_str!("../icons/delete.svg"));
    r.icon("dimension", include_str!("../icons/dimension.svg"));
    r.icon("ellipse", include_str!("../icons/ellipse.svg"));
    r.icon("equal", include_str!("../icons/equal.svg"));
    r.icon("fix", include_str!("../icons/fix.svg"));
    r.icon("grid", include_str!("../icons/grid.svg"));
    r.icon("horizontal", include_str!("../icons/horizontal.svg"));
    r.icon("line", include_str!("../icons/line.svg"));
    r.icon("midpoint", include_str!("../icons/midpoint.svg"));
    r.icon("mirror", include_str!("../icons/mirror.svg"));
    r.icon("offset", include_str!("../icons/offset.svg"));
    r.icon("parallel", include_str!("../icons/parallel.svg"));
    r.icon("perpendicular", include_str!("../icons/perpendicular.svg"));
    r.icon("point", include_str!("../icons/point.svg"));
    r.icon("polygon", include_str!("../icons/polygon.svg"));
    r.icon("rectangle", include_str!("../icons/rectangle.svg"));
    r.icon("rotation-axis", include_str!("../icons/rotation-axis.svg"));
    r.icon("select", include_str!("../icons/select.svg"));
    r.icon("sketch", include_str!("../icons/sketch.svg"));
    r.icon(
        "sketch-chamfer",
        include_str!("../icons/sketch-chamfer.svg"),
    );
    r.icon(
        "sketch-circular",
        include_str!("../icons/sketch-circular.svg"),
    );
    r.icon("sketch-copy", include_str!("../icons/sketch-copy.svg"));
    r.icon("sketch-detach", include_str!("../icons/sketch-detach.svg"));
    r.icon("sketch-extend", include_str!("../icons/sketch-extend.svg"));
    r.icon("sketch-fillet", include_str!("../icons/sketch-fillet.svg"));
    r.icon("sketch-linear", include_str!("../icons/sketch-linear.svg"));
    r.icon("sketch-rotate", include_str!("../icons/sketch-rotate.svg"));
    r.icon("sketch-scale", include_str!("../icons/sketch-scale.svg"));
    r.icon("sketch-split", include_str!("../icons/sketch-split.svg"));
    r.icon("sketch-text", include_str!("../icons/sketch-text.svg"));
    r.icon("slot", include_str!("../icons/slot.svg"));
    r.icon("spline", include_str!("../icons/spline.svg"));
    r.icon(
        "spline-through",
        include_str!("../icons/spline-through.svg"),
    );
    r.icon("symmetric", include_str!("../icons/symmetric.svg"));
    r.icon("tangent", include_str!("../icons/tangent.svg"));
    r.icon("trim", include_str!("../icons/trim.svg"));
    r.icon("vertical", include_str!("../icons/vertical.svg"));
}
