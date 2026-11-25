//! What shows of a mesh while it is edited: its cage of edges, its
//! points, what is picked and what the tool in use is about to do.

use std::collections::BTreeSet;

use workpiece_core::handles::{SpaceShape, SpaceView, Tone};
use workpiece_core::math::Point3;

use crate::MeshArgs;
use crate::ops;
use crate::poly::{Edge, Item, Poly};
use crate::session::{MeshState, Mode, Tool};

/// How faint what lies on the far side shows.
const BEHIND: f32 = 0.22;

fn line(a: Point3, b: Point3, tone: Tone, width: f32, opacity: f32) -> SpaceShape {
    SpaceShape::Line {
        ends: [a, b],
        tone,
        width,
        opacity,
        dashed: false,
    }
}

fn dashed(a: Point3, b: Point3, tone: Tone) -> SpaceShape {
    SpaceShape::Line {
        ends: [a, b],
        tone,
        width: 1.8,
        opacity: 1.0,
        dashed: true,
    }
}

fn dot(at: Point3, tone: Tone, size: f32, opacity: f32) -> SpaceShape {
    SpaceShape::Dot {
        at,
        tone,
        size,
        opacity,
    }
}

/// The face `f` of `poly`, filled.
fn fill(shapes: &mut Vec<SpaceShape>, poly: &Poly, f: usize, tone: Tone, opacity: f32) {
    let Some(ring) = poly.faces.get(f) else {
        return;
    };
    for k in 1..ring.len().saturating_sub(1) {
        shapes.push(SpaceShape::Triangle {
            corners: [ring[0], ring[k], ring[k + 1]].map(|v| poly.points[v]),
            tone,
            opacity,
        });
    }
}

/// What shows of the mesh of `args`, edited in `state`, seen as `view`
/// says.
pub fn shapes(args: &MeshArgs, state: &MeshState, view: &SpaceView) -> Vec<SpaceShape> {
    let poly = &args.mesh;
    let mut shapes = Vec::new();
    let forward = view.forward();
    // What faces the eye: a face whose normal looks back at it.
    let front: Vec<bool> = (0..poly.faces.len())
        .map(|f| poly.area_normal(f).dot(forward) <= 0.0)
        .collect();
    let sides = poly.edges();
    // The mirror images, faintly.
    if args.mirror.contains(&true) {
        let whole = args.whole();
        let own: BTreeSet<Edge> = sides.keys().copied().collect();
        for &(a, b) in whole.edges().keys() {
            if !own.contains(&(a, b)) {
                shapes.push(line(
                    whole.points[a],
                    whole.points[b],
                    Tone::Construction,
                    1.0,
                    0.45,
                ));
            }
        }
    }
    let picked_faces = poly.faces_of(&state.picked);
    for &f in &picked_faces {
        fill(&mut shapes, poly, f, Tone::Selected, 0.3);
    }
    if let Some(Item::Face(f)) = state.hover
        && state.tool != Tool::LoopCut
    {
        fill(&mut shapes, poly, f, Tone::Hover, 0.22);
    }
    let picked_edges: BTreeSet<Edge> = state
        .picked
        .iter()
        .filter_map(|i| match *i {
            Item::Edge(a, b) => Some((a, b)),
            _ => None,
        })
        .collect();
    let mut seen = vec![false; poly.points.len()];
    let mut used = vec![false; poly.points.len()];
    for (&(a, b), beside) in &sides {
        used[a] = true;
        used[b] = true;
        let faces_eye = beside.iter().any(|s| front[s.face]);
        if faces_eye {
            seen[a] = true;
            seen[b] = true;
        }
        let opacity = if faces_eye { 1.0 } else { BEHIND };
        let (pa, pb) = (poly.points[a], poly.points[b]);
        let of_picked_face = beside.iter().any(|s| picked_faces.contains(&s.face));
        let hovered = state.hover == Some(Item::Edge(a, b)) && state.tool != Tool::LoopCut;
        let (tone, width) = if picked_edges.contains(&(a, b)) {
            (Tone::Selected, 3.2)
        } else if hovered {
            (Tone::Hover, 3.0)
        } else if of_picked_face {
            (Tone::Selected, 1.8)
        } else if poly.creases.contains(&(a, b)) {
            (Tone::Projected, 2.6)
        } else if beside.len() == 1 {
            (Tone::Constrained, 2.2)
        } else {
            (Tone::Constrained, 1.2)
        };
        shapes.push(line(pa, pb, tone, width, opacity));
    }
    // The lines: the control polygons of curves.
    for &(a, b) in &poly.lines {
        used[a] = true;
        used[b] = true;
        let hovered = state.hover == Some(Item::Edge(a, b));
        let (tone, width) = if picked_edges.contains(&(a, b)) {
            (Tone::Selected, 3.2)
        } else if hovered {
            (Tone::Hover, 3.0)
        } else {
            (Tone::Constrained, 1.6)
        };
        shapes.push(line(poly.points[a], poly.points[b], tone, width, 1.0));
    }
    // The points: all of them where points are picked or cut at, else
    // those that are picked.
    let all_points =
        state.mode == Mode::Vertex || matches!(state.tool, Tool::Knife | Tool::Draw | Tool::Line);
    let picked_points: BTreeSet<usize> = state
        .picked
        .iter()
        .filter_map(|i| match *i {
            Item::Vertex(v) => Some(v),
            _ => None,
        })
        .collect();
    for (v, &p) in poly.points.iter().enumerate() {
        // (A point of no face shows: a corner drawn.)
        let loose = !used[v];
        let opacity = if seen[v] || loose { 1.0 } else { BEHIND };
        if picked_points.contains(&v) {
            shapes.push(dot(p, Tone::Selected, 9.0, 1.0));
        } else if state.hover == Some(Item::Vertex(v)) {
            shapes.push(dot(p, Tone::Hover, 9.0, 1.0));
        } else if poly.corners.contains(&v) {
            shapes.push(dot(p, Tone::Projected, 7.0, opacity));
        } else if all_points || loose {
            shapes.push(dot(p, Tone::Constrained, 5.0, opacity));
        }
    }
    match state.tool {
        Tool::Line => {
            let from = state
                .drawn
                .last()
                .and_then(|&v| poly.points.get(v).copied());
            let cursor = state.cursor.and_then(|c| c.at(poly));
            if let (Some(from), Some(to)) = (from, cursor) {
                shapes.push(dashed(from, to, Tone::Preview));
            }
            if let Some(at) = cursor {
                shapes.push(dot(at, Tone::Preview, 9.0, 1.0));
            }
        }
        Tool::Select => {
            if let Some(band) = &state.band {
                for k in 0..4 {
                    shapes.push(dashed(
                        band.corners[k],
                        band.corners[(k + 1) % 4],
                        Tone::Hover,
                    ));
                }
            }
        }
        Tool::Knife => {
            let cursor = state.cursor.and_then(|c| c.at(poly));
            let from = state.knife.and_then(|v| poly.points.get(v).copied());
            if let (Some(from), Some(to)) = (from, cursor) {
                shapes.push(dashed(from, to, Tone::Preview));
            }
            if let Some(from) = from {
                shapes.push(dot(from, Tone::Preview, 8.0, 1.0));
            }
            if let Some(at) = cursor {
                shapes.push(dot(at, Tone::Preview, 9.0, 1.0));
            }
        }
        Tool::LoopCut => {
            if let Some(Item::Edge(a, b)) = state.hover {
                let (across, closed) = ops::ring(poly, (a, b));
                let middles: Vec<Point3> = across
                    .iter()
                    .map(|&(a, b)| (poly.points[a] + poly.points[b]) / 2.0)
                    .collect();
                for pair in middles.windows(2) {
                    shapes.push(dashed(pair[0], pair[1], Tone::Preview));
                }
                if closed && middles.len() > 2 {
                    shapes.push(dashed(
                        middles[middles.len() - 1],
                        middles[0],
                        Tone::Preview,
                    ));
                }
            }
        }
        Tool::Draw => {
            let corners: Vec<Point3> = state
                .drawn
                .iter()
                .filter_map(|&v| poly.points.get(v).copied())
                .collect();
            for pair in corners.windows(2) {
                shapes.push(line(pair[0], pair[1], Tone::Preview, 2.0, 1.0));
            }
            let cursor = state.cursor.and_then(|c| c.at(poly));
            if let (Some(&last), Some(to)) = (corners.last(), cursor) {
                shapes.push(dashed(last, to, Tone::Preview));
                if corners.len() >= 2 {
                    shapes.push(dashed(to, corners[0], Tone::Preview));
                }
            }
            for &p in &corners {
                shapes.push(dot(p, Tone::Preview, 8.0, 1.0));
            }
            if let Some(at) = cursor {
                shapes.push(dot(at, Tone::Preview, 9.0, 1.0));
            }
        }
    }
    shapes
}
