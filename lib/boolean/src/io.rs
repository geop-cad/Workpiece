//! Solids in and out of a topology.

use std::collections::BTreeMap;
use std::sync::Arc;

use shapecraft_core::world::topo::{
    Body, BodyId, BodyKind, Coedge, CoedgeKind, Edge as TopoEdge, EdgeId, EntityId,
    Face as TopoFace, FaceUse, Loop, Lump, Orientation, Shell, Topo, Vertex as TopoVertex,
    VertexId,
};

use crate::Refusal;
use crate::model::{Edge, Face, Solid, Use, Vertex, Walk};

/// Reads solid body `body` of `topo`.
pub fn read(topo: &Topo, body: BodyId) -> Result<Solid, Refusal> {
    read_as(topo, body, BodyKind::Solid)
}

/// Reads sheet body `body` of `topo`: its faces, with the edges and
/// vertices between and around them — open, where a solid's shells are
/// closed. Not something to combine; what a sheet is thickened from.
pub fn read_sheet(topo: &Topo, body: BodyId) -> Result<Solid, Refusal> {
    read_as(topo, body, BodyKind::Sheet)
}

fn read_as(topo: &Topo, body: BodyId, kind: BodyKind) -> Result<Solid, Refusal> {
    let called = || topo.name_of(body).unwrap_or("the body").to_owned();
    let not_solid = || Refusal::NotSolid(called());
    let b = topo.body(body).ok_or_else(not_solid)?;
    if b.kind != kind {
        return Err(not_solid());
    }
    let mut solid = Solid::default();
    let mut vertices: BTreeMap<VertexId, usize> = BTreeMap::new();
    let mut edges: BTreeMap<EdgeId, usize> = BTreeMap::new();
    let mut vertex = |solid: &mut Solid, id: VertexId| -> Result<usize, Refusal> {
        if let Some(&k) = vertices.get(&id) {
            return Ok(k);
        }
        let v = topo.vertex(id).ok_or_else(not_solid)?;
        vertices.insert(id, solid.vertices.len());
        solid.vertices.push(Vertex {
            point: v.point,
            tolerance: v.tolerance,
            name: topo.name_of(id).map(str::to_owned),
        });
        Ok(solid.vertices.len() - 1)
    };
    let uses = b
        .lumps
        .iter()
        .filter_map(|&l| topo.lump(l))
        .flat_map(|l| &l.shells)
        .filter_map(|&s| topo.shell(s))
        .flat_map(|s| &s.faces);
    for face_use in uses {
        let face = topo.face(face_use.face).ok_or_else(not_solid)?;
        let surface = topo.surface(face.surface).ok_or_else(not_solid)?;
        let mut loops = Vec::with_capacity(face.loops.len());
        for &l in &face.loops {
            let mut steps = Vec::new();
            for &c in &topo.loop_(l).ok_or_else(not_solid)?.coedges {
                let coedge = topo.coedge(c).ok_or_else(not_solid)?;
                let pcurve = coedge
                    .pcurve
                    .and_then(|p| topo.pcurve(p))
                    .ok_or_else(not_solid)?;
                let walk = match coedge.kind {
                    CoedgeKind::Edge { edge, orientation } => {
                        let k = match edges.get(&edge) {
                            Some(&k) => k,
                            None => {
                                let e = topo.edge(edge).ok_or_else(not_solid)?;
                                let curve = topo.curve(e.curve).ok_or_else(not_solid)?;
                                let ends =
                                    [vertex(&mut solid, e.start)?, vertex(&mut solid, e.end)?];
                                edges.insert(edge, solid.edges.len());
                                solid.edges.push(Edge {
                                    curve: Arc::new(curve.clone()),
                                    range: (e.t0, e.t1),
                                    ends,
                                    tolerance: e.tolerance,
                                    name: topo.name_of(edge).map(str::to_owned),
                                    also: topo
                                        .also_of(edge)
                                        .into_iter()
                                        .map(str::to_owned)
                                        .collect(),
                                });
                                solid.edges.len() - 1
                            }
                        };
                        Walk::Edge {
                            edge: k,
                            reversed: orientation == Orientation::Reversed,
                        }
                    }
                    CoedgeKind::Pole { vertex: v, t0, t1 } => Walk::Pole {
                        vertex: vertex(&mut solid, v)?,
                        range: (t0, t1),
                    },
                };
                steps.push(Use {
                    walk,
                    pcurve: Arc::new(pcurve.clone()),
                });
            }
            loops.push(steps);
        }
        solid.faces.push(Face {
            surface: Arc::new(surface.clone()),
            outward: face_use.orientation == Orientation::Forward,
            loops,
            tolerance: face.tolerance,
            name: topo.name_of(face_use.face).map(str::to_owned),
            also: topo
                .also_of(face_use.face)
                .into_iter()
                .map(str::to_owned)
                .collect(),
            smooth: Vec::new(),
        });
    }
    Ok(solid)
}

/// Removes body `body` from `topo` with everything only it is made of.
pub fn remove(topo: &mut Topo, body: BodyId) {
    topo.remove_body(body);
}

/// A name for something new in `topo`: `wanted`, or `fallback`, with a
/// number after it if taken.
fn free_name(topo: &Topo, wanted: Option<&str>, fallback: &str) -> String {
    let base = wanted.unwrap_or(fallback);
    if topo.lookup(base).is_none() {
        return base.to_owned();
    }
    (2..)
        .map(|n| format!("{base}#{n}"))
        .find(|name| topo.lookup(name).is_none())
        .expect("unbounded range")
}

/// Adds `solid` to `topo` as a solid body called `name`; its vertices,
/// edges and faces keep their names (those without get one after the
/// body's). Nothing is added if it cannot all be.
pub fn write(topo: &mut Topo, solid: &Solid, name: &str) -> Result<BodyId, Refusal> {
    write_as(topo, solid, name, BodyKind::Solid)
}

/// Writes `sheet` — faces that need not close — into `topo` as a sheet
/// body called `name`.
pub fn write_sheet(topo: &mut Topo, sheet: &Solid, name: &str) -> Result<BodyId, Refusal> {
    write_as(topo, sheet, name, BodyKind::Sheet)
}

fn write_as(topo: &mut Topo, solid: &Solid, name: &str, kind: BodyKind) -> Result<BodyId, Refusal> {
    let mut added: Vec<EntityId> = Vec::new();
    // (Writing is no reading of what is there.)
    let watched = topo.pause();
    let result = write_all(topo, solid, name, kind, &mut added);
    topo.resume(watched);
    if result.is_err() {
        for id in added.into_iter().rev() {
            let _ = topo.remove(id);
        }
    }
    result
}

fn write_all(
    topo: &mut Topo,
    solid: &Solid,
    name: &str,
    kind: BodyKind,
    added: &mut Vec<EntityId>,
) -> Result<BodyId, Refusal> {
    let failed = |e: shapecraft_core::world::topo::TopoError| Refusal::Open(e.to_string());
    let mut keep = |id: EntityId| {
        added.push(id);
        id
    };
    // What has no name of its own is called by its history: an edge by
    // the faces it lies between, a vertex by the faces that meet at it
    // (`body/edge[top+hole/side]`). Those names stay what they are when
    // something upstream changes how many edges there are, which a
    // number would not; only a face that came with no name is numbered.
    let face_name = |k: usize| -> String {
        let own = solid.faces[k].name.clone();
        let full = own.unwrap_or_else(|| format!("{name}/face{k}"));
        let short = full
            .strip_prefix(name)
            .and_then(|rest| rest.strip_prefix('/'));
        short.map_or(full.clone(), str::to_owned)
    };
    let mut faces_of_edge: Vec<Vec<String>> = vec![Vec::new(); solid.edges.len()];
    let mut faces_of_vertex: Vec<Vec<String>> = vec![Vec::new(); solid.vertices.len()];
    for (k, face) in solid.faces.iter().enumerate() {
        for step in face.loops.iter().flatten() {
            match step.walk {
                Walk::Edge { edge, .. } => {
                    faces_of_edge[edge].push(face_name(k));
                    for end in solid.edges[edge].ends {
                        faces_of_vertex[end].push(face_name(k));
                    }
                }
                Walk::Pole { vertex, .. } => faces_of_vertex[vertex].push(face_name(k)),
            }
        }
    }
    let between = |faces: &[String]| -> Option<String> {
        let mut faces = faces.to_vec();
        faces.sort();
        faces.dedup();
        (!faces.is_empty()).then(|| faces.join("+"))
    };
    let named = |topo: &mut Topo, id: EntityId, wanted: &Option<String>, kind: &str, k: usize| {
        let by_history = match kind {
            "edge" => between(&faces_of_edge[k]),
            "vertex" => between(&faces_of_vertex[k]),
            _ => None,
        };
        let fallback = match by_history {
            Some(faces) => format!("{name}/{kind}[{faces}]"),
            None => format!("{name}/{kind}{k}"),
        };
        let free = free_name(topo, wanted.as_deref(), &fallback);
        topo.set_name(id, free).map_err(failed)?;
        // (What was called by its number before edges were called by
        // their faces is still known by it: parts saved then refer to
        // it so.)
        Ok(())
    };
    // (Named once everything is there: what has a name gets it before
    // anything without one is called by its number, which might be the
    // very name another came with.)
    let mut pending: Vec<(EntityId, Option<String>, &str, usize)> = Vec::new();
    let (vertex_tolerances, edge_tolerances) = solid.tolerances();
    let mut vertices: Vec<VertexId> = Vec::with_capacity(solid.vertices.len());
    for (k, v) in solid.vertices.iter().enumerate() {
        let id = topo
            .add_vertex(TopoVertex {
                point: v.point,
                tolerance: vertex_tolerances[k],
            })
            .map_err(failed)?;
        keep(id.into());
        pending.push((id.into(), v.name.clone(), "vertex", k));
        vertices.push(id);
    }
    let mut curves = BTreeMap::new();
    let mut edges: Vec<EdgeId> = Vec::with_capacity(solid.edges.len());
    for (k, e) in solid.edges.iter().enumerate() {
        let curve = *curves.entry(Arc::as_ptr(&e.curve)).or_insert_with(|| {
            let id = topo.add_curve((*e.curve).clone());
            keep(id.into());
            id
        });
        let id = topo
            .add_edge(TopoEdge {
                curve,
                t0: e.range.0,
                t1: e.range.1,
                start: vertices[e.ends[0]],
                end: vertices[e.ends[1]],
                tolerance: edge_tolerances[k],
            })
            .map_err(failed)?;
        keep(id.into());
        pending.push((id.into(), e.name.clone(), "edge", k));
        edges.push(id);
    }
    let mut surfaces = BTreeMap::new();
    let mut faces = Vec::with_capacity(solid.faces.len());
    for (k, face) in solid.faces.iter().enumerate() {
        let surface = *surfaces
            .entry(Arc::as_ptr(&face.surface))
            .or_insert_with(|| {
                let id = topo.add_surface((*face.surface).clone());
                keep(id.into());
                id
            });
        let mut loops = Vec::with_capacity(face.loops.len());
        for steps in &face.loops {
            let mut coedges = Vec::with_capacity(steps.len());
            for step in steps {
                let pcurve = topo.add_pcurve((*step.pcurve).clone());
                keep(pcurve.into());
                let coedge = match step.walk {
                    Walk::Edge { edge, reversed } => Coedge::edge(
                        edges[edge],
                        if reversed {
                            Orientation::Reversed
                        } else {
                            Orientation::Forward
                        },
                        Some(pcurve),
                    ),
                    Walk::Pole { vertex, range } => {
                        Coedge::pole(vertices[vertex], range.0, range.1, Some(pcurve))
                    }
                };
                let id = topo.add_coedge(coedge).map_err(failed)?;
                keep(id.into());
                coedges.push(id);
            }
            let id = topo.add_loop(Loop { coedges }).map_err(failed)?;
            keep(id.into());
            loops.push(id);
        }
        let id = topo
            .add_face(TopoFace {
                surface,
                loops,
                tolerance: face.tolerance,
            })
            .map_err(failed)?;
        keep(id.into());
        pending.push((id.into(), face.name.clone(), "face", k));
        faces.push(id);
    }
    // Where several lie between the same faces (the two edges a hole
    // leaves between a plate's top and its side), they are numbered in
    // the order of where they are, which is theirs whatever was made
    // before them: by x, then y, then z, of a vertex or an edge's middle.
    let place = |kind: &str, k: usize| -> [f64; 3] {
        // (Rounded: what is one place within the tolerance is one.)
        let at = |p: shapecraft_core::math::Point3| p.to_array().map(|x| (x * 1e6).round() / 1e6);
        match kind {
            "vertex" => at(solid.vertices[k].point),
            "edge" => {
                let e = &solid.edges[k];
                at(e.curve.point_at(0.5 * (e.range.0 + e.range.1)))
            }
            _ => [k as f64, 0.0, 0.0],
        }
    };
    pending.sort_by(|a, b| {
        let unnamed = |p: &(EntityId, Option<String>, &str, usize)| p.1.is_none();
        let (pa, pb) = (place(a.2, a.3), place(b.2, b.3));
        let by_place = pa.iter().zip(&pb).map(|(x, y)| x.total_cmp(y));
        let by_place = by_place.fold(std::cmp::Ordering::Equal, std::cmp::Ordering::then);
        (unnamed(a), a.2).cmp(&(unnamed(b), b.2)).then(by_place)
    });
    for (id, wanted, kind, k) in &pending {
        named(topo, *id, wanted, kind, *k)?;
    }
    // What each is known by besides, once everything has its own name.
    for (id, e) in edges.iter().zip(&solid.edges) {
        for name in &e.also {
            topo.also(*id, name.as_str());
        }
    }
    for (id, face) in faces.iter().zip(&solid.faces) {
        for name in &face.also {
            topo.also(*id, name.as_str());
        }
    }
    // What was called by its number before edges and vertices were
    // called by their faces is still known by it, where nothing that
    // came with that name has it: parts saved then refer to it so.
    for (id, wanted, kind, k) in &pending {
        if wanted.is_none() && (*kind == "edge" || *kind == "vertex") {
            topo.also(*id, format!("{name}/{kind}{k}"));
        }
    }
    let mut lumps = Vec::new();
    // (A sheet's faces that hang together by their edges are a shell.)
    let grouped = if kind == BodyKind::Solid {
        solid.lump_shells()?
    } else {
        let mut of_edge: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        for (f, face) in solid.faces.iter().enumerate() {
            for step in face.loops.iter().flatten() {
                if let Walk::Edge { edge, .. } = step.walk {
                    of_edge.entry(edge).or_default().push(f);
                }
            }
        }
        let mut seen = vec![false; solid.faces.len()];
        let mut pieces = Vec::new();
        for start in 0..solid.faces.len() {
            if seen[start] {
                continue;
            }
            seen[start] = true;
            let mut piece = vec![start];
            let mut k = 0;
            while k < piece.len() {
                let f = piece[k];
                k += 1;
                for step in solid.faces[f].loops.iter().flatten() {
                    let Walk::Edge { edge, .. } = step.walk else {
                        continue;
                    };
                    for &g in &of_edge[&edge] {
                        if !seen[g] {
                            seen[g] = true;
                            piece.push(g);
                        }
                    }
                }
            }
            piece.sort_unstable();
            pieces.push(vec![piece]);
        }
        pieces
    };
    for shells in grouped {
        let mut ids = Vec::with_capacity(shells.len());
        for shell in shells {
            let uses = shell
                .iter()
                .map(|&f| FaceUse {
                    face: faces[f],
                    orientation: if solid.faces[f].outward {
                        Orientation::Forward
                    } else {
                        Orientation::Reversed
                    },
                })
                .collect();
            let id = topo.add_shell(Shell { faces: uses }).map_err(failed)?;
            keep(id.into());
            ids.push(id);
        }
        let id = topo.add_lump(Lump { shells: ids }).map_err(failed)?;
        keep(id.into());
        lumps.push(id);
    }
    let body = topo
        .add_body(Body {
            kind,
            lumps,
            ..Body::default()
        })
        .map_err(failed)?;
    keep(body.into());
    topo.set_name(body, free_name(topo, Some(name), name))
        .map_err(failed)?;
    Ok(body)
}

impl shapecraft_core::Shape for Solid {
    fn write(&self, topo: &mut Topo, name: &str) -> anyhow::Result<()> {
        write(topo, self, name)?;
        Ok(())
    }
}

/// A body that is a sheet: faces that close no volume.
#[derive(Debug, Clone)]
pub struct Sheet(pub Solid);

impl shapecraft_core::Shape for Sheet {
    fn write(&self, topo: &mut Topo, name: &str) -> anyhow::Result<()> {
        write_sheet(topo, &self.0, name)?;
        Ok(())
    }
}

/// What an operation makes of bodies, gathered as it works them out:
/// the bodies that are no more, and those written, for the builder to
/// put in the world. Nothing is changed where they are read from.
#[derive(Default)]
pub struct Out<'a> {
    /// What was gathered.
    pub made: shapecraft_core::Made<'a>,
}

impl Out<'_> {
    /// The body `body` of `topo` is no more.
    pub fn remove(&mut self, topo: &Topo, body: BodyId) {
        self.made
            .removed
            .extend(topo.name_of(body).map(str::to_owned));
    }

    /// There is the solid `solid`, called `name`.
    pub fn write(&mut self, solid: &Solid, name: &str) {
        self.made
            .bodies
            .push((name.to_owned(), Box::new(solid.clone())));
    }

    /// There is the body `body` of `scratch`, a solid or a sheet,
    /// under the name it has there.
    pub fn take(&mut self, scratch: &Topo, body: BodyId) -> anyhow::Result<()> {
        let name = scratch
            .name_of(body)
            .ok_or_else(|| anyhow::anyhow!("a body without a name"))?;
        match scratch.body(body).map(|b| b.kind) {
            Some(BodyKind::Sheet) => self.write_sheet(&read_sheet(scratch, body)?, name),
            _ => self.write(&read(scratch, body)?, name),
        }
        Ok(())
    }

    /// There is the sheet `sheet`, called `name`.
    pub fn write_sheet(&mut self, sheet: &Solid, name: &str) {
        let sheet = Sheet(sheet.clone());
        self.made.bodies.push((name.to_owned(), Box::new(sheet)));
    }
}
