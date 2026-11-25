//! Surfaces as bodies of their own: faces offset (or copied) off a
//! body, surfaces run on, split, and faces of them deleted.

use anyhow::{Context, ensure};
use std::collections::BTreeMap;
use workpiece_boolean::model::Walk;
use workpiece_boolean::{Solid, read, read_sheet, stitched};

use workpiece_core::widgets::{ReferenceList, VBox};
use workpiece_core::world::AreaPick;
use workpiece_core::world::topo::{BodyId, BodyKind, Topo};
use workpiece_core::{
    Builder, Edit, EditContext, EditVisualization, Measure, Operation, ReferenceKind, Widget,
};

use workpiece_shape::outline::{Outline, Outlines, split_faces};
use workpiece_shape::ui;
use workpiece_shape::{faces_of, trimmed, whole};

/// Every solid and sheet body of `topo`, read: its id, its name,
/// whether it is a sheet, and its faces.
fn bodies(topo: &Topo) -> Vec<(BodyId, String, bool, Solid)> {
    let mut all = Vec::new();
    for (id, body) in topo.bodies().iter() {
        let Some(name) = topo.name_of(id) else {
            continue;
        };
        let read = match body.kind {
            BodyKind::Solid => read(topo, id).map(|s| (false, s)),
            BodyKind::Sheet => read_sheet(topo, id).map(|s| (true, s)),
            _ => continue,
        };
        if let Ok((sheet, solid)) = read {
            all.push((id, name.to_owned(), sheet, solid));
        }
    }
    all
}

/// What a surface's faces are split along: the outlines Emboss takes
/// (a sketch thrown onto them, curves on them), or a plane that cuts
/// whole surfaces in two.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum SplitBy {
    /// The areas of a sketch, thrown onto the faces along its normal:
    /// what of the faces each meets first is cut out of them.
    #[default]
    Sketch,
    /// Closed curves on the faces: edges, or curves in space (wire
    /// bodies) such as a projection makes.
    Curves,
    /// A plane: the surfaces cut in two, what is in front of it a
    /// surface of its own.
    Plane,
}

impl workpiece_core::Choices for SplitBy {
    const ALL: &'static [Self] = &[SplitBy::Sketch, SplitBy::Curves, SplitBy::Plane];

    fn label(self) -> &'static str {
        match self {
            SplitBy::Sketch => Outline::Sketch.label(),
            SplitBy::Curves => Outline::Curves.label(),
            SplitBy::Plane => "Plane",
        }
    }

    fn icon(self) -> Option<&'static str> {
        match self {
            SplitBy::Sketch => Outline::Sketch.icon(),
            SplitBy::Curves => Outline::Curves.icon(),
            SplitBy::Plane => Some("split-plane"),
        }
    }
}

impl SplitBy {
    /// The outline it is, if it is one.
    fn outline(self) -> Option<Outline> {
        match self {
            SplitBy::Sketch => Some(Outline::Sketch),
            SplitBy::Curves => Some(Outline::Curves),
            SplitBy::Plane => None,
        }
    }
}

/// What a surface operation works on.
#[derive(
    Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, workpiece_core::Arguments,
)]
pub struct SurfaceArgs {
    /// What it does.
    #[serde(default)]
    #[arg(hidden)]
    pub way: SurfaceWay,
    /// What a split goes along: a sketch, curves on the faces, or a
    /// plane.
    #[serde(default)]
    #[arg(choice, label = "", when = "self.way == SurfaceWay::Split")]
    pub split: SplitBy,
    /// The sketch a split goes along: its areas are thrown onto the
    /// faces along its plane's normal.
    #[serde(default)]
    #[arg(
        with = "sketch_fields",
        when = "self.way == SurfaceWay::Split && self.split == SplitBy::Sketch"
    )]
    pub sketch: Option<workpiece_core::Reference<workpiece_core::Sketched>>,
    /// The areas of it, each by a point inside it or by its boundary;
    /// none for the whole sketch.
    #[serde(default)]
    pub areas: Vec<AreaPick>,
    /// The closed curves on the faces a split goes along: edges, or
    /// curves in space (wire bodies, all their curves).
    #[serde(default)]
    #[arg(
        with = "curves_field",
        when = "self.way == SurfaceWay::Split && self.split == SplitBy::Curves"
    )]
    pub curves: Vec<workpiece_core::Reference<workpiece_core::Anything>>,
    /// Faces, or whole bodies.
    #[arg(with = "picked_field")]
    pub picked: Vec<workpiece_core::Reference<workpiece_core::Anything>>,
    /// The plane surfaces are split by.
    #[serde(default)]
    #[arg(when = "self.way == SurfaceWay::Split && self.split == SplitBy::Plane")]
    pub plane: Option<workpiece_core::Reference<workpiece_core::world::DatumPlane>>,
    /// Which side of the faces an offset goes to: along their normals
    /// as they are, or against them. (A surface has no front or back of
    /// its own: the way is said here.)
    #[serde(default)]
    #[arg(flip, row, when = "self.way == SurfaceWay::Offset")]
    pub flip: bool,
    /// Whether a surface runs on to all of its faces' own surfaces —
    /// what they were before anything was cut from them — rather than
    /// by a distance.
    #[serde(default)]
    #[arg(
        label = "Run on",
        options = "By a distance|To the whole surface",
        icons = "extendsurface|untrim",
        when = "self.way == SurfaceWay::Extend"
    )]
    pub whole: bool,
    /// How far an offset surface is from what it is offset from (the
    /// way `flip` says; none for a copy of the faces), or how far a
    /// surface runs on.
    #[serde(default = "a_tenth")]
    #[arg(
        label_by = "self.way.distance_label()",
        when = "self.way == SurfaceWay::Offset || (self.way == SurfaceWay::Extend && !self.whole)"
    )]
    pub distance: Measure,
}

fn a_tenth() -> Measure {
    Measure::new(1.0)
}

impl Default for SurfaceArgs {
    fn default() -> Self {
        Self {
            way: SurfaceWay::default(),
            split: SplitBy::default(),
            sketch: None,
            areas: Vec::new(),
            curves: Vec::new(),
            picked: Vec::new(),
            plane: None,
            flip: false,
            whole: false,
            distance: a_tenth(),
        }
    }
}

impl SurfaceArgs {
    /// Whether it works on whole surfaces (a face picked standing for
    /// its surface).
    pub fn whole_surfaces(&self) -> bool {
        match self.way {
            SurfaceWay::Extend => true,
            SurfaceWay::Split => self.split == SplitBy::Plane,
            SurfaceWay::Offset | SurfaceWay::Delete => false,
        }
    }
}

/// What a surface operation does.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
pub enum SurfaceWay {
    /// A surface a distance off the picked faces (or all of the picked
    /// bodies' faces), named after the operation; by no distance, a
    /// copy of them (closed all around, a solid).
    #[default]
    Offset,
    /// The picked surfaces run on past their free edges, by the
    /// distance, as their faces go; or every face back to all of its
    /// surface, what it was before anything was cut from it.
    Extend,
    /// The picked faces split along the areas of a sketch thrown onto
    /// them or along curves on them; or the picked surfaces cut in two
    /// by a plane, what is in front of it a surface of its own.
    Split,
    /// Removes the picked faces from their surfaces (or the picked
    /// surfaces altogether); faces of a solid — a hole, a pocket, a
    /// boss — are taken off it and the faces around closed over where
    /// they were.
    Delete,
}

impl SurfaceWay {
    /// What its distance is called.
    fn distance_label(self) -> &'static str {
        match self {
            Self::Extend => "Run on by",
            _ => "Distance",
        }
    }
}

/// An operation on surfaces: offset (or copy) faces, extend surfaces,
/// split them, or delete faces of them.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(from = "Saved")]
pub struct Surface {
    /// The operation's id; also the name of what it makes.
    pub id: String,
    /// What it does, and what it works on.
    pub args: SurfaceArgs,
}

/// A surface operation as a part's file has it: one saved before what
/// it does was among its arguments has that beside them.
#[derive(serde::Deserialize)]
struct Saved {
    id: String,
    #[serde(default)]
    way: Option<SurfaceWay>,
    args: SurfaceArgs,
}

impl From<Saved> for Surface {
    fn from(saved: Saved) -> Self {
        let mut args = saved.args;
        args.way = saved.way.unwrap_or(args.way);
        Self { id: saved.id, args }
    }
}

impl Operation for Surface {
    type Args = SurfaceArgs;
    type EditState = ();

    fn id(&self) -> &str {
        &self.id
    }

    fn id_mut(&mut self) -> &mut String {
        &mut self.id
    }

    fn args(&self) -> &SurfaceArgs {
        &self.args
    }

    fn args_mut(&mut self) -> &mut SurfaceArgs {
        &mut self.args
    }

    fn make(&self, builder: &dyn Builder) -> anyhow::Result<workpiece_core::Made<'_>> {
        // (Nothing is changed here: the bodies it makes are written by
        // the builder.)
        let mut out = workpiece_shape::bodies::Out::default();
        let args = &self.args;
        let picked: Vec<&str> = args.picked.iter().map(|r| r.name()).collect();
        ensure!(!picked.is_empty(), "pick the faces or surfaces");
        let distance = (*builder).measure(&args.distance, "the distance")?;
        // (An offset goes to the other side of the faces, flipped.)
        let distance = if args.flip && args.way == SurfaceWay::Offset {
            -distance
        } else {
            distance
        };
        let plane = match (args.way, args.split) {
            (SurfaceWay::Split, SplitBy::Plane) => {
                let plane = args.plane.as_ref().context("pick the plane to cut at")?;
                let plane = builder.tracked().resolve(plane)?;
                Some((plane.origin(), plane.normal()))
            }
            _ => None,
        };
        let outlines = match args
            .split
            .outline()
            .filter(|_| args.way == SurfaceWay::Split)
        {
            Some(outline) => Some(Outlines::read(
                builder.tracked(),
                outline,
                &args.sketch,
                &args.areas,
                &args.curves,
            )?),
            None => None,
        };
        let topo = &builder.tracked().topo;
        let all = bodies(topo);
        let mut found = vec![false; picked.len()];
        // Of each body, the faces picked: all of them, for a body picked
        // itself.
        let mut chosen: Vec<(usize, Vec<usize>, bool)> = Vec::new();
        for (k, (_, name, _, solid)) in all.iter().enumerate() {
            let whole = picked.iter().position(|p| p == name);
            if let Some(at) = whole {
                found[at] = true;
            }
            let faces: Vec<usize> = (0..solid.faces.len())
                .filter(|&f| {
                    let named = solid.faces[f].name.as_deref();
                    let also = solid.faces[f].also.iter().map(String::as_str);
                    let hit = named
                        .into_iter()
                        .chain(also)
                        .find_map(|n| picked.iter().position(|p| *p == n));
                    if let Some(at) = hit {
                        found[at] = true;
                    }
                    hit.is_some() || whole.is_some()
                })
                .collect();
            if !faces.is_empty() {
                // (What works on whole surfaces takes a face of one for
                // all of it.)
                let whole = whole.is_some() || args.whole_surfaces();
                let faces = if whole {
                    (0..solid.faces.len()).collect()
                } else {
                    faces
                };
                chosen.push((k, faces, whole));
            }
        }
        if let Some(k) = found.iter().position(|f| !f) {
            anyhow::bail!("there is no face or body {:?}", picked[k]);
        }
        let size = |solid: &Solid| {
            let (lo, hi) = solid.extent();
            (hi - lo).length()
        };
        match args.way {
            SurfaceWay::Offset if distance == 0.0 => {
                // By no distance: the faces copied, sewn where they meet;
                // closed all around, a solid facing out.
                let sheets: Vec<Solid> = chosen
                    .iter()
                    .map(|(k, faces, _)| faces_of(&all[*k].3, faces))
                    .collect();
                let within = sheets.iter().map(size).fold(0.0, f64::max) * 1e-6;
                let (sheet, closed) = stitched(&sheets, within).context("sewing the faces")?;
                if closed {
                    out.write(&sheet, &self.id);
                } else {
                    out.write_sheet(&sheet, &self.id);
                }
            }
            SurfaceWay::Offset => {
                // Of each face, the far side of the slab that thick on
                // it; sewn where the faces were.
                let mut sheets = Vec::new();
                for (k, faces, _) in &chosen {
                    let of = &all[*k].3;
                    for &f in faces {
                        let id = format!("{}/f{}", self.id, sheets.len());
                        if out.made.annotations.is_empty() {
                            out.made.annotations.push(workpiece_shape::on::held(
                                &self.id,
                                "distance",
                                (of, f),
                                (distance, None),
                                &args.distance,
                                distance.abs(),
                            ));
                        }
                        let slab = workpiece_shape::thick::slab(of, f, distance, &id)
                            .with_context(|| format!("offsetting {:?}", of.faces[f].name))?;
                        let far = format!("{id}/far");
                        let found = slab
                            .faces
                            .iter()
                            .position(|g| g.name.as_deref() == Some(&far));
                        let found = found.context("a face has no offset")?;
                        let mut sheet = faces_of(&slab, &[found]);
                        // (Facing as what it is offset from.)
                        sheet.faces[0].outward = of.faces[f].outward;
                        sheets.push(sheet);
                    }
                }
                let within = sheets.iter().map(size).fold(0.0, f64::max) * 1e-6;
                let (sheet, _) = stitched(&sheets, within).context("sewing the offset faces")?;
                out.write_sheet(&sheet, &self.id);
            }
            SurfaceWay::Split => match (&outlines, plane) {
                (Some(outlines), _) => {
                    // Each body with its faces split in place, under its
                    // own name.
                    for (k, faces, _) in &chosen {
                        let (body, name, _, solid) = &all[*k];
                        let areas = outlines.areas(&self.id, solid, faces);
                        let (split, closed) = split_faces(solid, faces, &areas)
                            .with_context(|| format!("splitting {name}"))?;
                        out.remove(topo, *body);
                        if closed {
                            out.write(&split, name);
                        } else {
                            out.write_sheet(&split, name);
                        }
                    }
                }
                (None, Some((origin, normal))) => {
                    ensure!(
                        chosen.iter().all(|(k, _, whole)| all[*k].2 && *whole),
                        "a plane splits whole surfaces: pick surface bodies"
                    );
                    for (n, (k, ..)) in chosen.iter().enumerate() {
                        let (body, name, _, sheet) = &all[*k];
                        let behind = trimmed(sheet, origin, normal)
                            .with_context(|| format!("cutting {name}"))?;
                        let front = trimmed(sheet, origin, -normal)
                            .with_context(|| format!("cutting {name}"))?;
                        out.remove(topo, *body);
                        if !behind.faces.is_empty() {
                            out.write_sheet(&behind, name);
                        }
                        if !front.faces.is_empty() {
                            let called = if n == 0 {
                                self.id.clone()
                            } else {
                                format!("{}/{}", self.id, n + 1)
                            };
                            out.write_sheet(&front, &called);
                        }
                    }
                }
                (None, None) => anyhow::bail!("pick what to split along"),
            },
            SurfaceWay::Extend => {
                ensure!(
                    chosen.iter().all(|(k, _, whole)| all[*k].2 && *whole),
                    "this works on whole surfaces: pick surface bodies"
                );
                ensure!(args.whole || distance > 0.0, "run on by some distance");
                for (k, ..) in &chosen {
                    let (body, name, _, sheet) = &all[*k];
                    // Which of its edges are free: walked once.
                    let mut walked = vec![0; sheet.edges.len()];
                    for step in sheet.faces.iter().flat_map(|f| f.loops.iter().flatten()) {
                        if let Walk::Edge { edge, .. } = step.walk {
                            walked[edge] += 1;
                        }
                    }
                    let mut pieces = Vec::new();
                    for (f, face) in sheet.faces.iter().enumerate() {
                        let id = format!("{name}/f{f}");
                        let mut piece = if args.whole {
                            whole(&face.surface, &id)?
                        } else {
                            let free: Vec<(usize, f64)> = face
                                .loops
                                .iter()
                                .flatten()
                                .filter_map(|step| match step.walk {
                                    Walk::Edge { edge, .. } if walked[edge] == 1 => {
                                        Some((edge, distance))
                                    }
                                    _ => None,
                                })
                                .collect();
                            if free.is_empty() {
                                faces_of(sheet, &[f])
                            } else {
                                workpiece_shape::thick::run_on(sheet, f, &free)
                                    .with_context(|| format!("running {name} on"))?
                            }
                        };
                        piece.faces[0].outward = face.outward;
                        piece.faces[0].name = face.name.clone();
                        pieces.push(piece);
                    }
                    let (sewn, _) = stitched(&pieces, size(sheet) * 1e-7)?;
                    out.remove(topo, *body);
                    out.write_sheet(&sewn, name);
                }
            }
            SurfaceWay::Delete => {
                for (k, faces, whole) in &chosen {
                    let (body, name, sheet, of) = &all[*k];
                    out.remove(topo, *body);
                    let rest: Vec<usize> =
                        (0..of.faces.len()).filter(|f| !faces.contains(f)).collect();
                    if *whole || rest.is_empty() {
                        continue;
                    }
                    if *sheet {
                        out.write_sheet(&faces_of(of, &rest), name);
                    } else {
                        // Faces of a solid: taken off, and the faces
                        // around closed over where they were.
                        let healed = workpiece_shape::heal::faces_deleted(of, faces)
                            .with_context(|| format!("deleting faces of {name}"))?;
                        out.write(&healed, name);
                    }
                }
            }
        }
        Ok(out.made)
    }

    fn summary(&self) -> String {
        let n = self.args.picked.len();
        let what = match self.args.way {
            SurfaceWay::Offset if self.args.distance.value == 0.0 => "copied",
            SurfaceWay::Offset => "offset",
            SurfaceWay::Delete => "deleted",
            SurfaceWay::Split => "split",
            SurfaceWay::Extend if self.args.whole => "made whole",
            SurfaceWay::Extend => "run on",
        };
        format!("{n} {what}")
    }

    fn visualize_edit(
        &self,
        context: &EditContext<'_>,
        (): &(),
    ) -> EditVisualization<Edit<SurfaceArgs, ()>> {
        let args = &self.args;
        let fields = workpiece_core::Arguments::fields(args, context);
        let mut handles = BTreeMap::new();
        let splits = args.way == SurfaceWay::Split && args.split == SplitBy::Sketch;
        if let Some(profile) = ui::profile(context.world, &args.sketch).filter(|_| splits) {
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

/// The field that picks the sketch a split goes along (what was picked
/// of another sketch goes with it), and what of it is taken.
fn sketch_fields(args: &SurfaceArgs, _: &EditContext<'_>) -> Vec<Widget<SurfaceArgs>> {
    let set = ui::set(args, |a, sketch| {
        a.sketch = sketch;
        a.areas.clear();
    });
    ui::sketch_fields(&args.sketch, args.areas.len(), set)
}

/// The box of the curves on the faces a split goes along.
fn curves_field(args: &SurfaceArgs, _: &EditContext<'_>) -> Vec<Widget<SurfaceArgs>> {
    let set = ui::set(args, |a, picked: Vec<workpiece_core::Source>| {
        a.curves = workpiece_core::Reference::each(picked)
    });
    vec![ui::curves_list(&args.curves, set).required().into()]
}

/// The box that takes what it works on, of the kinds its way takes.
fn picked_field(args: &SurfaceArgs, _: &EditContext<'_>) -> Vec<Widget<SurfaceArgs>> {
    // (A face picked of what works on whole surfaces stands for its
    // surface.)
    let kinds = if args.whole_surfaces() {
        vec![ReferenceKind::Body, ReferenceKind::Face]
    } else {
        vec![ReferenceKind::Face, ReferenceKind::Body]
    };
    let picked = workpiece_core::Reference::sources(&args.picked);
    let set = ui::set(args, |a, picked| {
        a.picked = workpiece_core::Reference::each(picked)
    });
    vec![
        ReferenceList::new("Of", kinds, picked, set)
            .required()
            .into(),
    ]
}
