//! Emboss and engrave: the areas of a sketch raised from faces of a
//! body, or sunk into them, following the faces.

use std::collections::BTreeMap;

use anyhow::{Context, ensure};
use shapecraft_boolean::{Op, Solid, combine, read};
use shapecraft_core::widgets::VBox;
use shapecraft_core::world::topo::Topo;
use shapecraft_core::world::{AreaPick, Profile};
use shapecraft_core::{Builder, Edit, EditContext, EditVisualization, Measure, Operation, Widget};

use shapecraft_shape::sweep::{self, Frame, Make};
use shapecraft_shape::ui;

/// What is raised or sunk, on which faces, and by how much.
#[derive(
    Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, shapecraft_core::Arguments,
)]
pub struct EmbossArgs {
    /// The sketch: its areas are thrown onto the faces along its
    /// plane's normal.
    #[arg(with = "sketch_fields")]
    pub sketch: Option<shapecraft_core::Reference<shapecraft_core::Sketched>>,
    /// The areas of it, each by a point inside it or by its boundary;
    /// none for the whole sketch.
    #[serde(default)]
    pub areas: Vec<AreaPick>,
    /// The faces of the body they are raised from or sunk into.
    #[arg(required)]
    pub faces: Vec<shapecraft_core::Reference<shapecraft_core::world::topo::FaceId>>,
    /// Whether they are sunk into the faces, not raised from them.
    #[serde(default)]
    #[arg(label = "Way", options = "Emboss (raised)|Engrave (sunk)")]
    pub engrave: bool,
    /// How high they stand, or how deep they are sunk.
    pub height: Measure,
}

impl Default for EmbossArgs {
    fn default() -> Self {
        Self {
            sketch: None,
            areas: Vec::new(),
            faces: Vec::new(),
            height: Measure::new(0.5),
            engrave: false,
        }
    }
}

/// Raises the areas of a sketch from faces of a body, or sinks them
/// into the faces: each area's top (or floor) follows the face, the
/// height away from it.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Emboss {
    /// The operation's id.
    pub id: String,
    /// The areas, the faces and the height.
    pub args: EmbossArgs,
}

/// `solid` with the areas `regions` of `profile` raised by `height`
/// from its faces `faces` — or, with less than nothing, sunk that deep
/// into them. Each area takes what of the faces lies under it nearest
/// the sketch's plane.
pub fn embossed(
    id: &str,
    solid: &Solid,
    faces: &[usize],
    profile: &Profile,
    regions: &[usize],
    height: f64,
) -> anyhow::Result<Solid> {
    ensure!(height != 0.0 && height.is_finite(), "by some height");
    let frame = Frame::of(profile);
    let (origin, normal) = (frame.origin, frame.normal());
    // All within the height of the faces, on the side it goes to.
    let slab = shapecraft_shape::thick::thicken(solid, faces, height, false, true)
        .context("following the faces")?;
    let mut far = 0.0_f64;
    for face in &solid.faces {
        for p in face.surface.control_points() {
            far = far.max(p.distance(origin));
        }
    }
    let far = far * 1.5 + height.abs() + 1.0;
    let mut made = solid.clone();
    for &region in regions {
        let walks = sweep::outline(profile, &[region])?;
        let mut scratch = Topo::new();
        let name = format!("{id}/a{region}");
        let through = sweep::extrude(
            &mut scratch,
            &name,
            profile,
            &walks,
            true,
            (-far, far),
            Make::Solid,
        )?;
        let through = read(&scratch, through)?;
        let under = combine(Op::Intersect, &slab, &through)
            .context("throwing an area onto the faces")?
            .solid;
        let near = |lump: &Solid| {
            lump.vertices
                .iter()
                .map(|v| (v.point - origin).dot(normal).abs())
                .fold(f64::INFINITY, f64::min)
        };
        let Some(piece) = under
            .lumps()?
            .into_iter()
            .min_by(|a, b| near(a).total_cmp(&near(b)))
        else {
            anyhow::bail!("an area of the sketch is not over the faces");
        };
        let op = if height > 0.0 { Op::Union } else { Op::Cut };
        made = combine(op, &made, &piece)
            .context("putting an area on the body")?
            .solid;
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

    fn make(&self, builder: &dyn Builder) -> anyhow::Result<shapecraft_core::Made<'_>> {
        // (Nothing is changed here: the bodies it makes are written by
        // the builder.)
        let mut out = shapecraft_shape::bodies::Out::default();
        let args = &self.args;
        let height = (*builder).measure(&args.height, "the height")?;
        ensure!(height > 0.0, "by some height");
        ensure!(!args.faces.is_empty(), "pick the faces to emboss");
        let world = builder.tracked();
        let sketch = args.sketch.as_ref().context("pick a sketch")?.name();
        let profile = world
            .profiles
            .get(sketch)
            .with_context(|| format!("there is no sketch {sketch:?}"))?
            .clone();
        let regions = sweep::regions(&profile, &args.areas)?;
        ensure!(
            !regions.is_empty(),
            "the sketch {sketch:?} has no closed area{}",
            profile.open_note()
        );
        let picked: Vec<&str> = args.faces.iter().map(|r| r.name()).collect();
        let topo = &world.topo;
        let mut found = vec![false; picked.len()];
        for (body, name) in shapecraft_shape::bodies::solids_with(topo, &picked) {
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
            let by = if args.engrave { -height } else { height };
            if out.made.annotations.is_empty() {
                out.made.annotations.push(shapecraft_shape::on::held(
                    &self.id,
                    "height",
                    (&solid, faces[0]),
                    (by, None),
                    &args.height,
                    height,
                ));
            }
            let made = embossed(&self.id, &solid, &faces, &profile, &regions, by)?;
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
        let fields = shapecraft_core::Arguments::fields(args, context);
        let mut handles = BTreeMap::new();
        if let Some(profile) = ui::profile(context.world, &args.sketch) {
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
