//! Surfaces as bodies of their own: faces copied off a body, sheets
//! sewn into one (a solid, if they close), a sheet taken apart.

use anyhow::{Context, ensure};
use workpiece_boolean::model::Walk;
use workpiece_boolean::{Solid, read, read_sheet, stitched};
use workpiece_core::widgets::ReferenceList;
use workpiece_core::world::topo::{BodyId, BodyKind, Topo};
use workpiece_core::{Builder, EditContext, Measure, Operation, ReferenceKind, Widget};

use workpiece_shape::ui;
use workpiece_shape::{faces_of, filled, trimmed, whole};

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

/// What a surface operation works on.
#[derive(
    Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, workpiece_core::Arguments,
)]
pub struct SurfaceArgs {
    /// What it does.
    #[serde(default)]
    #[arg(hidden)]
    pub way: SurfaceWay,
    /// Faces, or whole bodies.
    #[arg(with = "picked_field")]
    pub picked: Vec<workpiece_core::Reference<workpiece_core::Anything>>,
    /// The plane surfaces are trimmed to or split by.
    #[serde(default)]
    #[arg(when = "matches!(self.way, SurfaceWay::Trim | SurfaceWay::Split)")]
    pub plane: Option<workpiece_core::Reference<workpiece_core::world::DatumPlane>>,
    /// Whether it is what is behind the plane that goes (a trim), or
    /// which side of the faces an offset goes to: along their normals
    /// as they are, or against them. (A surface has no front or back of
    /// its own: the way is said here.)
    #[serde(default)]
    #[arg(
        flip,
        row,
        when = "matches!(self.way, SurfaceWay::Trim | SurfaceWay::Offset)"
    )]
    pub flip: bool,
    /// How far an offset surface is from what it is offset from (the
    /// way `flip` says), how far a surface runs on, or a round's radius.
    #[serde(default = "a_tenth")]
    #[arg(
        label_by = "self.way.distance_label()",
        when = "matches!(self.way, SurfaceWay::Offset | SurfaceWay::Extend)"
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
            picked: Vec::new(),
            plane: None,
            flip: false,
            distance: a_tenth(),
        }
    }
}

/// What a surface operation does.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
pub enum SurfaceWay {
    /// Copies the picked faces (or all of the picked bodies' faces) into
    /// a surface of their own, named after the operation.
    #[default]
    Copy,
    /// Sews the picked surfaces into one, where their edges lie on each
    /// other; closed all around, it becomes a solid.
    Stitch,
    /// Takes the picked surfaces apart: every face a surface of its own.
    Unstitch,
    /// Removes the picked faces from their surfaces (or the picked
    /// surfaces altogether); faces of a solid — a hole, a pocket, a
    /// boss — are taken off it and the faces around closed over where
    /// they were.
    Delete,
    /// A surface a distance off the picked faces (or all of the picked
    /// bodies' faces), named after the operation.
    Offset,
    /// The picked surfaces cut back to a plane: what is in front of it
    /// goes.
    Trim,
    /// The picked surfaces cut in two by a plane: what is in front of
    /// it becomes a surface of its own.
    Split,
    /// Every face of the picked surfaces back to all of its surface:
    /// what it was before anything was cut from it.
    Untrim,
    /// The picked surfaces run on past their free edges, by the
    /// distance, as their faces go.
    Extend,
    /// Fills the flat holes of the picked surfaces: every ring of free
    /// edges that lies in a plane gets a face. A surface closed all
    /// around by that becomes a solid.
    Fill,
}

impl SurfaceWay {
    /// Whether it works on whole surfaces (a face picked standing for
    /// its surface).
    pub fn whole(self) -> bool {
        matches!(
            self,
            Self::Stitch
                | Self::Unstitch
                | Self::Trim
                | Self::Split
                | Self::Untrim
                | Self::Extend
                | Self::Fill
        )
    }

    /// What its distance is called.
    fn distance_label(self) -> &'static str {
        match self {
            Self::Extend => "Run on by",
            _ => "Distance",
        }
    }
}

/// An operation on surfaces: copy faces off bodies, sew surfaces
/// together (all one way), take them apart, offset, fill, trim, split,
/// untrim, extend them, or delete faces of them.
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
        let picked: Vec<&str> = self.args.picked.iter().map(|r| r.name()).collect();
        ensure!(!picked.is_empty(), "pick the faces or surfaces");
        let distance = (*builder).measure(&self.args.distance, "the distance")?;
        // (An offset goes to the other side of the faces, flipped.)
        let distance = if self.args.flip && self.args.way == SurfaceWay::Offset {
            -distance
        } else {
            distance
        };
        let plane = match self.args.way {
            SurfaceWay::Trim | SurfaceWay::Split => {
                let plane = self
                    .args
                    .plane
                    .as_ref()
                    .context("pick the plane to cut at")?;
                let plane = builder.tracked().resolve(plane)?;
                let normal = if self.args.flip {
                    -plane.normal()
                } else {
                    plane.normal()
                };
                Some((plane.origin(), normal))
            }
            _ => None,
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
                let whole = whole.is_some() || self.args.way.whole();
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
        match self.args.way {
            SurfaceWay::Copy => {
                let sheets: Vec<Solid> = chosen
                    .iter()
                    .map(|(k, faces, _)| faces_of(&all[*k].3, faces))
                    .collect();
                let within = sheets.iter().map(size).fold(0.0, f64::max) * 1e-7;
                let (sheet, _) = stitched(&sheets, within)?;
                out.write_sheet(&sheet, &self.id);
            }
            SurfaceWay::Offset => {
                ensure!(distance != 0.0, "offset by some distance");
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
                                &self.args.distance,
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
            SurfaceWay::Stitch => {
                ensure!(
                    chosen.iter().all(|(k, _, whole)| all[*k].2 && *whole),
                    "surfaces are sewn whole: pick surface bodies"
                );
                let sheets: Vec<Solid> = chosen.iter().map(|(k, ..)| all[*k].3.clone()).collect();
                let within = sheets.iter().map(size).fold(0.0, f64::max) * 1e-6;
                let (sewn, closed) = stitched(&sheets, within).context("sewing the surfaces")?;
                for (k, ..) in &chosen {
                    out.remove(topo, all[*k].0);
                }
                if closed {
                    out.write(&sewn, &self.id);
                } else {
                    out.write_sheet(&sewn, &self.id);
                }
            }
            SurfaceWay::Trim | SurfaceWay::Split => {
                ensure!(
                    chosen.iter().all(|(k, _, whole)| all[*k].2 && *whole),
                    "this works on whole surfaces: pick surface bodies"
                );
                let (origin, normal) = plane.context("pick the plane to cut at")?;
                for (n, (k, ..)) in chosen.iter().enumerate() {
                    let (body, name, _, sheet) = &all[*k];
                    let behind = trimmed(sheet, origin, normal)
                        .with_context(|| format!("cutting {name}"))?;
                    let front = match self.args.way {
                        SurfaceWay::Split => Some(
                            trimmed(sheet, origin, -normal)
                                .with_context(|| format!("cutting {name}"))?,
                        ),
                        _ => None,
                    };
                    out.remove(topo, *body);
                    if !behind.faces.is_empty() {
                        out.write_sheet(&behind, name);
                    }
                    if let Some(front) = front.filter(|f| !f.faces.is_empty()) {
                        let called = if n == 0 {
                            self.id.clone()
                        } else {
                            format!("{}/{}", self.id, n + 1)
                        };
                        out.write_sheet(&front, &called);
                    }
                }
            }
            SurfaceWay::Untrim | SurfaceWay::Extend => {
                ensure!(
                    chosen.iter().all(|(k, _, whole)| all[*k].2 && *whole),
                    "this works on whole surfaces: pick surface bodies"
                );
                ensure!(
                    self.args.way == SurfaceWay::Untrim || distance > 0.0,
                    "run on by some distance"
                );
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
                        let mut piece = if self.args.way == SurfaceWay::Untrim {
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
            SurfaceWay::Fill => {
                ensure!(
                    chosen.iter().all(|(k, _, whole)| all[*k].2 && *whole),
                    "holes of surfaces are filled: pick surface bodies"
                );
                for (k, ..) in &chosen {
                    let (body, name, _, sheet) = &all[*k];
                    let (with, made) = filled(sheet)?;
                    ensure!(made > 0, "{name} has no flat hole to fill");
                    let (sewn, closed) = stitched(&[with], size(sheet) * 1e-7)?;
                    out.remove(topo, *body);
                    if closed {
                        out.write(&sewn, name);
                    } else {
                        out.write_sheet(&sewn, name);
                    }
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
            SurfaceWay::Unstitch => {
                // (A solid taken apart is its faces, each a surface.)
                let mut made = 0;
                for (k, ..) in &chosen {
                    let (body, _, _, sheet) = &all[*k];
                    out.remove(topo, *body);
                    for f in 0..sheet.faces.len() {
                        made += 1;
                        let called = format!("{}/{made}", self.id);
                        out.write_sheet(&faces_of(sheet, &[f]), &called);
                    }
                }
            }
        }
        Ok(out.made)
    }

    fn summary(&self) -> String {
        let n = self.args.picked.len();
        let what = match self.args.way {
            SurfaceWay::Copy => "copied",
            SurfaceWay::Stitch => "sewn",
            SurfaceWay::Unstitch => "taken apart",
            SurfaceWay::Delete => "deleted",
            SurfaceWay::Offset => "offset",
            SurfaceWay::Trim => "trimmed",
            SurfaceWay::Split => "split",
            SurfaceWay::Untrim => "untrimmed",
            SurfaceWay::Extend => "run on",
            SurfaceWay::Fill => "filled",
        };
        format!("{n} {what}")
    }
}

/// The box that takes what it works on, of the kinds its way takes.
fn picked_field(args: &SurfaceArgs, _: &EditContext<'_>) -> Vec<Widget<SurfaceArgs>> {
    // (A face picked of what works on whole surfaces stands for its
    // surface.)
    let kinds = if args.way.whole() {
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
