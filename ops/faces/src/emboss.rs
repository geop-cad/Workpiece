//! Emboss and engrave: the areas of a sketch raised from faces of a
//! body, or sunk into them, following the faces.

use std::collections::BTreeMap;

use anyhow::{Context, ensure};
use workpiece_boolean::{Op, Solid, combine, read};
use workpiece_core::widgets::VBox;
use workpiece_core::world::AreaPick;
use workpiece_core::{Builder, Edit, EditContext, EditVisualization, Measure, Operation, Widget};

use workpiece_shape::sweep::Frame;
use workpiece_shape::ui;

pub use workpiece_shape::outline::{Area, Outline, curve_profiles, sketch_areas};
use workpiece_shape::outline::{Outlines, first_met, prism, reach};

/// What is raised or sunk, on which faces, and by how much.
#[derive(
    Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, workpiece_core::Arguments,
)]
pub struct EmbossArgs {
    /// Whether the areas of a sketch are raised, or what closed curves
    /// on the faces bound.
    #[serde(default)]
    #[arg(choice, label = "")]
    pub outline: Outline,
    /// The sketch: its areas are thrown onto the faces along its
    /// plane's normal.
    #[arg(with = "sketch_fields", when = "self.outline == Outline::Sketch")]
    pub sketch: Option<workpiece_core::Reference<workpiece_core::Sketched>>,
    /// The areas of it, each by a point inside it or by its boundary;
    /// none for the whole sketch.
    #[serde(default)]
    pub areas: Vec<AreaPick>,
    /// The closed curves on the faces: edges, or curves in space (wire
    /// bodies, all their curves). A curve inside another makes a hole
    /// in what that one bounds.
    #[serde(default)]
    #[arg(with = "curves_field", when = "self.outline == Outline::Curves")]
    pub curves: Vec<workpiece_core::Reference<workpiece_core::Anything>>,
    /// The faces of the body they are raised from or sunk into. Where
    /// the sketch's areas are thrown onto them twice (both sides of a
    /// ring), only the first they meet takes them.
    #[arg(required)]
    pub faces: Vec<workpiece_core::Reference<workpiece_core::world::topo::FaceId>>,
    /// Whether the sides of what is raised (or sunk) run along the
    /// sketch's normal, or square to the face. (Of curves on the faces,
    /// always square to the face.)
    #[serde(default)]
    #[arg(
        label = "Sides",
        options = "Sketch normal|Face normal",
        icons = "emboss-along-sketch|emboss-along-face",
        when = "self.outline == Outline::Sketch"
    )]
    pub normal_to_face: bool,
    /// Whether they are sunk into the faces, not raised from them.
    #[serde(default)]
    #[arg(
        label = "Way",
        options = "Emboss (raised)|Engrave (sunk)",
        icons = "emboss-raised|emboss-sunk"
    )]
    pub engrave: bool,
    /// How high they stand, or how deep they are sunk.
    pub height: Measure,
}

impl Default for EmbossArgs {
    fn default() -> Self {
        Self {
            outline: Outline::Sketch,
            sketch: None,
            areas: Vec::new(),
            curves: Vec::new(),
            faces: Vec::new(),
            normal_to_face: false,
            height: Measure::new(0.5),
            engrave: false,
        }
    }
}

/// Raises the areas of a sketch — or what closed curves on the faces
/// bound — from faces of a body, or sinks them into the faces: each
/// area's top (or floor) follows the face, the height away from it.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Emboss {
    /// The operation's id.
    pub id: String,
    /// The areas, the faces and the height.
    pub args: EmbossArgs,
}

/// `solid` with `areas` raised by `height` from its faces `faces` — or,
/// with less than nothing, sunk that deep into them. Each area takes
/// what of the faces it meets first; its sides run along the profile's
/// normal, or (`square`) square to the faces.
pub fn embossed(
    solid: &Solid,
    faces: &[usize],
    areas: &[Area<'_>],
    height: f64,
    square: bool,
) -> anyhow::Result<Solid> {
    ensure!(height != 0.0 && height.is_finite(), "by some height");
    // All within the height of the faces, on the side it goes to.
    let slab = workpiece_shape::thick::thicken(solid, faces, height, false, true)
        .context("following the faces")?;
    let mut made = solid.clone();
    for area in areas {
        let origin = Frame::of(area.profile).origin;
        let far = reach(solid, origin) + height.abs();
        let through = prism(area, (-far, far))?;
        let under = combine(Op::Intersect, &slab, &through)
            .context("throwing an area onto the faces")?
            .solid;
        let pieces = first_met(under.lumps()?, origin, area.toward);
        ensure!(!pieces.is_empty(), "an area is not over the faces");
        let op = if height > 0.0 { Op::Union } else { Op::Cut };
        for piece in pieces {
            let piece = if square {
                square_to_faces(&piece, height.abs(), &area.name)?
            } else {
                piece
            };
            made = combine(op, &made, &piece)
                .context("putting an area on the body")?
                .solid;
        }
    }
    Ok(made)
}

/// What `piece` (an area thrown onto faces, within `height` of them)
/// stands on, raised from the faces by `height` with its sides square to
/// them: the faces of it that lie in the faces it stands on, thickened
/// (away from the body for what is raised, into it for what is sunk).
/// Its faces are named under `name`.
fn square_to_faces(piece: &Solid, height: f64, name: &str) -> anyhow::Result<Solid> {
    let on: Vec<usize> = (0..piece.faces.len())
        .filter(|&f| {
            piece.faces[f]
                .name
                .as_deref()
                .is_some_and(|n| n.starts_with("thick/") && n.contains("/near"))
        })
        .collect();
    ensure!(!on.is_empty(), "an area has no foot on the faces");
    let foot = workpiece_shape::surface::faces_of(piece, &on);
    let all: Vec<usize> = (0..foot.faces.len()).collect();
    // (The foot's faces look out of the piece, into what it stands on:
    // into the piece is less than nothing.)
    let mut made = workpiece_shape::thick::thicken(&foot, &all, -height, false, true)
        .context("raising an area square to the faces")?;
    for face in &mut made.faces {
        if let Some(rest) = face.name.as_deref().and_then(|n| n.strip_prefix("thick/")) {
            face.name = Some(format!("{name}/{rest}"));
        }
    }
    Ok(made)
}

impl Operation for Emboss {
    type Args = EmbossArgs;
    type EditState = ();

    fn id(&self) -> &str {
        &self.id
    }

    fn id_mut(&mut self) -> &mut String {
        &mut self.id
    }

    fn args(&self) -> &EmbossArgs {
        &self.args
    }

    fn args_mut(&mut self) -> &mut EmbossArgs {
        &mut self.args
    }

    fn make(&self, builder: &dyn Builder) -> anyhow::Result<workpiece_core::Made<'_>> {
        // (Nothing is changed here: the bodies it makes are written by
        // the builder.)
        let mut out = workpiece_shape::bodies::Out::default();
        let args = &self.args;
        let height = (*builder).measure(&args.height, "the height")?;
        ensure!(height > 0.0, "by some height");
        ensure!(!args.faces.is_empty(), "pick the faces to emboss");
        let world = builder.tracked();
        let topo = &world.topo;
        // What is raised: the areas of the sketch, or what the curves
        // bound (each outer ring with its holes, a profile of its own).
        let outlines =
            Outlines::read(world, args.outline, &args.sketch, &args.areas, &args.curves)?;
        let square = args.normal_to_face || args.outline == Outline::Curves;
        let picked: Vec<&str> = args.faces.iter().map(|r| r.name()).collect();
        let mut found = vec![false; picked.len()];
        for (body, name) in workpiece_shape::bodies::solids_with(topo, &picked) {
            let solid = read(topo, body)?;
            let faces: Vec<usize> = (0..solid.faces.len())
                .filter(|&f| {
                    let named = solid.faces[f].name.as_deref();
                    let also = solid.faces[f].also.iter().map(String::as_str);
                    let hit = named
                        .into_iter()
                        .chain(also)
                        .find_map(|n| picked.iter().position(|p| *p == n));
                    if let Some(k) = hit {
                        found[k] = true;
                    }
                    hit.is_some()
                })
                .collect();
            if faces.is_empty() {
                continue;
            }
            let areas = outlines.areas(&self.id, &solid, &faces);
            let by = if args.engrave { -height } else { height };
            if out.made.annotations.is_empty() {
                out.made.annotations.push(workpiece_shape::on::held(
                    &self.id,
                    "height",
                    (&solid, faces[0]),
                    (by, None),
                    &args.height,
                    height,
                ));
            }
            let made = embossed(&solid, &faces, &areas, by, square)?;
            out.remove(topo, body);
            out.write(&made, &name);
        }
        if let Some(k) = found.iter().position(|f| !f) {
            anyhow::bail!("there is no face {:?} to emboss", picked[k]);
        }
        Ok(out.made)
    }

    fn summary(&self) -> String {
        let what = if self.args.engrave { "sunk" } else { "raised" };
        format!("{what} by {}", self.args.height)
    }

    fn visualize_edit(
        &self,
        context: &EditContext<'_>,
        (): &(),
    ) -> EditVisualization<Edit<EmbossArgs, ()>> {
        let args = &self.args;
        // The fields the arguments give; the handle is its own.
        let fields = workpiece_core::Arguments::fields(args, context);
        let mut handles = BTreeMap::new();
        if let Some(profile) =
            ui::profile(context.world, &args.sketch).filter(|_| args.outline == Outline::Sketch)
        {
            let picker = ui::area_picker(
                profile,
                &args.areas,
                ui::set(args, |a, areas| a.areas = areas),
            );
            handles.insert("areas".to_owned(), picker);
        }
        EditVisualization::new(VBox::new(fields), handles).map(Edit::args)
    }
}

/// The field that picks the sketch (what was picked of another sketch
/// goes with it), and what of it is taken.
fn sketch_fields(args: &EmbossArgs, _: &EditContext<'_>) -> Vec<Widget<EmbossArgs>> {
    let set = ui::set(args, |a, sketch| {
        a.sketch = sketch;
        a.areas.clear();
    });
    ui::sketch_fields(&args.sketch, args.areas.len(), set)
}

/// The box of the curves that bound what is raised: edges, or curves in
/// space (wire bodies).
fn curves_field(args: &EmbossArgs, _: &EditContext<'_>) -> Vec<Widget<EmbossArgs>> {
    let set = ui::set(args, |a, picked: Vec<workpiece_core::Source>| {
        a.curves = workpiece_core::Reference::each(picked)
    });
    vec![ui::curves_list(&args.curves, set).required().into()]
}
