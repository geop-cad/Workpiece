//! Surfaces as bodies of their own: faces copied off a body, sheets
//! sewn into one (a solid, if they close), a sheet taken apart.

use anyhow::{Context, ensure};
use shapecraft_boolean::model::Walk;
use shapecraft_boolean::{Solid, read, read_sheet, stitched};
use shapecraft_core::widgets::ReferenceList;
use shapecraft_core::world::topo::{BodyId, BodyKind, Topo};
use shapecraft_core::{Builder, EditContext, Measure, Operation, ReferenceKind, Widget};

use shapecraft_shape::ui;
use shapecraft_shape::{between, faces_of, filled, rounded, ruled, trimmed, whole};

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
    Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, shapecraft_core::Arguments,
)]
pub struct SurfaceArgs {
    /// What it does.
    #[serde(default)]
    #[arg(hidden)]
    pub way: SurfaceWay,
    /// Faces, or whole bodies.
    #[arg(with = "picked_field")]
    pub picked: Vec<shapecraft_core::Reference<shapecraft_core::Anything>>,
    /// The way a ruled surface runs from its edges.
    #[serde(default)]
    #[arg(label = "Along", when = "self.way == SurfaceWay::Ruled")]
    pub axis: Option<shapecraft_core::Reference<shapecraft_core::world::DatumAxis>>,
    /// The plane surfaces are trimmed to or split by.
    #[serde(default)]
    #[arg(when = "matches!(self.way, SurfaceWay::Trim | SurfaceWay::Split)")]
    pub plane: Option<shapecraft_core::Reference<shapecraft_core::world::DatumPlane>>,
    /// Whether it is what is behind the plane that goes.
    #[serde(default)]
    #[arg(flip, row, when = "self.way == SurfaceWay::Trim")]
    pub flip: bool,
    /// How far an offset surface is from what it is offset from: to
    /// the faces' front, or (less than nothing) to their back; or how
    /// far a ruled surface runs from its edges.
    #[serde(default = "a_tenth")]
    #[arg(
        when = "matches!(self.way, SurfaceWay::Offset | SurfaceWay::Ruled | SurfaceWay::Extend | SurfaceWay::Fillet)"
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
            axis: None,
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
    /// Turns the picked surfaces around: their back is their front.
    Reverse,
    /// Removes the picked faces from their surfaces (or the picked
    /// surfaces altogether).
    Delete,
    /// A surface a distance off the picked faces (or all of the picked
    /// bodies' faces), named after the operation.
    Offset,
    /// A surface from the picked edges straight along an axis, a
    /// distance: each edge sweeps a face, sewn where the edges meet.
    Ruled,
    /// A surface between four picked edges that meet end to end around
    /// it, running along each of them.
    Boundary,
    /// The picked edges, each between two faces of a surface, rounded
    /// to the distance as a radius.
    Fillet,
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

/// An operation on surfaces: copy faces off bodies, sew surfaces
/// together, take them apart, turn them around, or delete of them.
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

    fn make(&self, builder: &dyn Builder) -> anyhow::Result<shapecraft_core::Made<'_>> {
        // (Nothing is changed here: the bodies it makes are written by
        // the builder.)
        let mut out = shapecraft_shape::bodies::Out::default();
        let picked: Vec<&str> = self.args.picked.iter().map(|r| r.name()).collect();
        ensure!(!picked.is_empty(), "pick the faces or surfaces");
        let distance = (*builder).measure(&self.args.distance, "the distance")?;
        if self.args.way == SurfaceWay::Boundary {
            ensure!(picked.len() == 4, "pick the four edges around the surface");
            let topo = &builder.tracked().topo;
            let all = bodies(topo);
            let mut sides = Vec::new();
            for wanted in &picked {
                let found = all.iter().find_map(|(_, _, _, solid)| {
                    let named = |e: &&shapecraft_boolean::model::Edge| {
                        e.name.as_deref() == Some(*wanted) || e.also.iter().any(|n| n == wanted)
                    };
                    solid.edges.iter().find(named).cloned()
                });
                let edge = found.with_context(|| format!("there is no edge {wanted:?}"))?;
                sides.push((edge.curve.clone(), edge.range));
            }
            let sides: [_; 4] = sides
                .try_into()
                .map_err(|_| anyhow::anyhow!("pick the four edges around the surface"))?;
            let sheet = between(&sides, &self.id)?;
            out.write_sheet(&sheet, &self.id);
            return Ok(out.made);
        }
        if self.args.way == SurfaceWay::Ruled {
            let axis = self
                .args
                .axis
                .as_ref()
                .context("pick the axis to run along")?;
            let by = builder.tracked().resolve(axis)?.direction() * distance;
            let topo = &builder.tracked().topo;
            let mut sheets = Vec::new();
            for wanted in &picked {
                let found = bodies(topo).into_iter().find_map(|(_, _, _, solid)| {
                    let named = |e: &&shapecraft_boolean::model::Edge| {
                        e.name.as_deref() == Some(*wanted) || e.also.iter().any(|n| n == wanted)
                    };
                    solid.edges.iter().find(named).cloned()
                });
                let edge = found.with_context(|| format!("there is no edge {wanted:?}"))?;
                let id = format!("{}/e{}", self.id, sheets.len());
                sheets.push(ruled(&edge.curve, edge.range, by, &id)?);
            }
            let (sheet, _) = stitched(&sheets, by.length() * 1e-7)?;
            out.write_sheet(&sheet, &self.id);
            return Ok(out.made);
        }
        if self.args.way == SurfaceWay::Fillet {
            ensure!(!picked.is_empty(), "pick the edges to round");
            let topo = &builder.tracked().topo;
            let mut left: Vec<&str> = picked.clone();
            for (body, name, is_sheet, sheet) in bodies(topo) {
                let edges: Vec<usize> = (0..sheet.edges.len())
                    .filter(|&e| {
                        let edge = &sheet.edges[e];
                        let names = edge.name.iter().chain(&edge.also);
                        names.into_iter().any(|n| picked.contains(&n.as_str()))
                    })
                    .collect();
                if !is_sheet || edges.is_empty() {
                    continue;
                }
                left.retain(|wanted| {
                    !edges.iter().any(|&e| {
                        let edge = &sheet.edges[e];
                        edge.name.iter().chain(&edge.also).any(|n| n == wanted)
                    })
                });
                let made = rounded(&sheet, &edges, distance, &self.id)
                    .with_context(|| format!("rounding {name}"))?;
                out.remove(topo, body);
                out.write_sheet(&made, &name);
            }
            if let Some(missing) = left.first() {
                anyhow::bail!("there is no edge {missing:?} of a surface");
            }
            return Ok(out.made);
        }
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
                chosen.push((k, faces, whole.is_some()));
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
                            out.made.annotations.push(shapecraft_shape::on::held(
                                &self.id,
                                "distance",
                                (of, f),
                                (distance, None),
                                &self.args.distance,
                                distance.abs(),
                            ));
                        }
                        let slab = shapecraft_shape::thick::slab(of, f, distance, &id)
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
            // (Done above: they work on edges, not faces.)
            SurfaceWay::Ruled | SurfaceWay::Boundary | SurfaceWay::Fillet => {}
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
                                shapecraft_shape::thick::run_on(sheet, f, &free)
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
            SurfaceWay::Unstitch | SurfaceWay::Reverse | SurfaceWay::Delete => {
                ensure!(
                    chosen.iter().all(|(k, ..)| all[*k].2),
                    "this works on surfaces: copy the faces off the solid first"
                );
                let mut made = 0;
                for (k, faces, whole) in &chosen {
                    let (body, name, _, sheet) = &all[*k];
                    out.remove(topo, *body);
                    match self.args.way {
                        SurfaceWay::Unstitch => {
                            for f in 0..sheet.faces.len() {
                                made += 1;
                                let called = format!("{}/{made}", self.id);
                                out.write_sheet(&faces_of(sheet, &[f]), &called);
                            }
                        }
                        SurfaceWay::Reverse => {
                            let mut turned = sheet.clone();
                            for face in &mut turned.faces {
                                face.outward = !face.outward;
                            }
                            out.write_sheet(&turned, name);
                        }
                        _ => {
                            let rest: Vec<usize> = (0..sheet.faces.len())
                                .filter(|f| !faces.contains(f))
                                .collect();
                            if !*whole && !rest.is_empty() {
                                out.write_sheet(&faces_of(sheet, &rest), name);
                            }
                        }
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
            SurfaceWay::Reverse => "turned around",
            SurfaceWay::Delete => "deleted",
            SurfaceWay::Offset => "offset",
            SurfaceWay::Ruled => "ruled",
            SurfaceWay::Boundary => "edges bounded",
            SurfaceWay::Fillet => "edges rounded",
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
    let kinds = match args.way {
        SurfaceWay::Ruled | SurfaceWay::Boundary | SurfaceWay::Fillet => {
            vec![ReferenceKind::Edge]
        }
        SurfaceWay::Stitch
        | SurfaceWay::Unstitch
        | SurfaceWay::Reverse
        | SurfaceWay::Trim
        | SurfaceWay::Split
        | SurfaceWay::Untrim
        | SurfaceWay::Extend
        | SurfaceWay::Fill => vec![ReferenceKind::Body],
        _ => vec![ReferenceKind::Face, ReferenceKind::Body],
    };
    let picked = shapecraft_core::Reference::sources(&args.picked);
    let set = ui::set(args, |a, picked| {
        a.picked = shapecraft_core::Reference::each(picked)
    });
    vec![
        ReferenceList::new("Of", kinds, picked, set)
            .required()
            .into(),
    ]
}
