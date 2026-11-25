//! Each rule is exercised by breaking it once. Inserts run the checks
//! because tests are built with debug assertions.

use anyhow::Result;

use super::ValidationError as V;
use crate::math::{Point2, Point3};
use crate::world::topo::tests::square;
use crate::world::topo::*;

const TOL: f64 = 1e-7;

/// How to break the square face built by [`square_face`].
#[derive(Clone, Copy, Default)]
struct Square {
    /// Moves the whole square along x.
    x: f64,
    /// Shifts the pcurves in (u, v), so they no longer match the edges.
    uv_shift: f64,
    /// Leaves the coedges without pcurves.
    no_pcurves: bool,
}

fn vertex(t: &mut Topo, p: Point3) -> Result<VertexId, TopoError> {
    t.add_vertex(Vertex {
        point: p,
        tolerance: TOL,
    })
}

/// A unit square face at `x` on the XY plane.
fn square_face(t: &mut Topo, s: Square) -> Result<FaceId, TopoError> {
    let o = Point3::new(s.x, 0.0, 0.0);
    let p = [
        o,
        o + Point3::X,
        o + Point3::new(1.0, 1.0, 0.0),
        o + Point3::Y,
    ];
    let uv =
        [Point2::ZERO, Point2::X, Point2::ONE, Point2::Y].map(|q| q + Point2::splat(s.uv_shift));
    let surface = t.add_surface(NurbsSurface::bilinear(p[0], p[1], p[3], p[2]).expect("valid"));
    let mut v = Vec::new();
    for q in p {
        v.push(vertex(t, q)?);
    }
    let mut coedges = Vec::new();
    for i in 0..4 {
        let j = (i + 1) % 4;
        let curve = t.add_curve(Curve3::line(p[i], p[j]).expect("valid"));

        let edge = t.add_edge(Edge {
            curve,
            t0: 0.0,
            t1: 1.0,
            start: v[i],
            end: v[j],
            tolerance: TOL,
        })?;
        let pcurve =
            (!s.no_pcurves).then(|| t.add_pcurve(Curve2::line(uv[i], uv[j]).expect("valid")));
        coedges.push(t.add_coedge(Coedge::edge(edge, Orientation::Forward, pcurve))?);
    }
    let l = t.add_loop(Loop { coedges })?;
    t.add_face(Face {
        surface,
        loops: vec![l],
        tolerance: TOL,
    })
}

fn shell(t: &mut Topo, faces: &[FaceId]) -> Result<ShellId, TopoError> {
    let faces = faces
        .iter()
        .map(|&face| FaceUse {
            face,
            orientation: Orientation::Forward,
        })
        .collect();
    t.add_shell(Shell { faces })
}

fn body(t: &mut Topo, kind: BodyKind, shell: ShellId) -> Result<BodyId, TopoError> {
    let lump = t.add_lump(Lump {
        shells: vec![shell],
    })?;
    t.add_body(Body {
        kind,
        lumps: vec![lump],
        ..Default::default()
    })
}

fn invalid<T: std::fmt::Debug>(r: Result<T, TopoError>) -> V {
    match r {
        Err(TopoError::Invalid(e)) => e,
        other => panic!("expected a validation error, got {other:?}"),
    }
}

#[test]
fn a_valid_sheet_passes_everything() -> Result<()> {
    let (t, _, _) = square()?;
    assert_eq!(t.validate(), Ok(()));
    Ok(())
}

#[test]
fn tolerances_must_be_finite_and_non_negative() {
    let mut t = Topo::new();
    let r = t.add_vertex(Vertex {
        point: Point3::ZERO,
        tolerance: -1.0,
    });
    assert!(matches!(invalid(r), V::BadTolerance { .. }));
    assert!(t.vertices().is_empty(), "a refused entity is not kept");
}

#[test]
fn edge_ends_must_be_inside_their_vertices() -> Result<()> {
    let mut t = Topo::new();
    let a = vertex(&mut t, Point3::ZERO)?;
    let b = vertex(&mut t, Point3::X)?;
    // The curve starts 1e-3 away from `a`.
    let curve = t.add_curve(Curve3::line(Point3::new(1e-3, 0.0, 0.0), Point3::X)?);

    let r = t.add_edge(Edge {
        curve,
        t0: 0.0,
        t1: 1.0,
        start: a,
        end: b,
        tolerance: TOL,
    });
    assert!(matches!(invalid(r), V::EdgeEndOutsideVertex { vertex, .. } if vertex == a));
    // An edge looser than its vertices does not fit inside them either.
    let curve = t.add_curve(Curve3::line(Point3::ZERO, Point3::X)?);

    let r = t.add_edge(Edge {
        curve,
        t0: 0.0,
        t1: 1.0,
        start: a,
        end: b,
        tolerance: 10.0 * TOL,
    });
    assert!(matches!(invalid(r), V::EdgeEndOutsideVertex { .. }));
    Ok(())
}

#[test]
fn pcurves_must_cover_the_edge_range() -> Result<()> {
    let mut t = Topo::new();
    let a = vertex(&mut t, Point3::ZERO)?;
    let b = vertex(&mut t, Point3::X)?;
    let curve = t.add_curve(Curve3::new(
        1,
        vec![0.0, 0.0, 2.0, 2.0],
        vec![Point3::ZERO, Point3::X],
        None,
    )?);

    let edge = t.add_edge(Edge {
        curve,
        t0: 0.0,
        t1: 2.0,
        start: a,
        end: b,
        tolerance: TOL,
    })?;
    let pcurve = Some(t.add_pcurve(Curve2::line(Point2::ZERO, Point2::X)?)); // domain [0, 1]
    let r = t.add_coedge(Coedge::edge(edge, Orientation::Forward, pcurve));
    assert!(matches!(invalid(r), V::PCurveRangeMismatch(_)));
    Ok(())
}

#[test]
fn face_edges_need_pcurves_that_lie_on_the_edges() {
    let mut t = Topo::new();
    let r = square_face(
        &mut t,
        Square {
            no_pcurves: true,
            ..Default::default()
        },
    );
    assert!(matches!(invalid(r), V::MissingPCurve { .. }));
    let r = square_face(
        &mut t,
        Square {
            x: 5.0,
            uv_shift: 0.1,
            ..Default::default()
        },
    );
    assert!(matches!(invalid(r), V::EdgeOffFace { .. }));
}

#[test]
fn edges_must_not_be_tighter_than_their_faces() -> Result<()> {
    let mut t = Topo::new();
    let f = square_face(&mut t, Square::default())?;
    let face = t.face(f).expect("exists").clone();
    let surface = face.surface;
    // A second face on the same loop is refused anyway (shared loop), so
    // build a fresh loop and give the face a tolerance above the edges'.
    let loop_ = t.loop_(face.loops[0]).expect("exists").clone();
    let coedges = loop_
        .coedges
        .iter()
        .map(|&c| t.add_coedge(*t.coedge(c).expect("exists")))
        .collect::<Result<_, _>>()?;
    let l = t.add_loop(Loop { coedges })?;
    let r = t.add_face(Face {
        surface,
        loops: vec![l],
        tolerance: 2.0 * TOL,
    });
    assert!(matches!(invalid(r), V::EdgeTighterThanFace { .. }));
    Ok(())
}

#[test]
fn children_have_a_single_owner() -> Result<()> {
    let mut t = Topo::new();
    let f = square_face(&mut t, Square::default())?;
    shell(&mut t, &[f])?;
    assert!(matches!(
        invalid(shell(&mut t, &[f])),
        V::MultipleOwners { .. }
    ));
    Ok(())
}

#[test]
fn shells_are_non_empty_and_connected() -> Result<()> {
    let mut t = Topo::new();
    assert!(matches!(invalid(shell(&mut t, &[])), V::EmptyShell(_)));
    let a = square_face(&mut t, Square::default())?;
    let b = square_face(
        &mut t,
        Square {
            x: 5.0,
            ..Default::default()
        },
    )?;
    assert!(matches!(
        invalid(shell(&mut t, &[a, b])),
        V::DisconnectedShell(_)
    ));
    Ok(())
}

#[test]
fn solids_need_every_edge_twice() -> Result<()> {
    let mut t = Topo::new();
    let f = square_face(&mut t, Square::default())?;
    let s = shell(&mut t, &[f])?;
    let r = body(&mut t, BodyKind::Solid, s);
    assert!(matches!(invalid(r), V::BadEdgeUseCount { uses: 1, .. }));
    // The same shell is a fine sheet (the refused body's lump is still
    // there, so build the sheet from a fresh face).
    let f = square_face(
        &mut t,
        Square {
            x: 5.0,
            ..Default::default()
        },
    )?;
    let s = shell(&mut t, &[f])?;
    body(&mut t, BodyKind::Sheet, s)?;
    Ok(())
}

#[test]
fn vertices_of_a_body_must_not_overlap() -> Result<()> {
    let mut t = Topo::new();
    let a = vertex(&mut t, Point3::ZERO)?;
    let b = vertex(&mut t, Point3::new(TOL, 0.0, 0.0))?;
    let r = t.add_body(Body {
        kind: BodyKind::General,
        free_vertices: vec![a, b],
        ..Default::default()
    });
    assert!(matches!(invalid(r), V::VerticesOverlap { .. }));
    Ok(())
}

#[test]
fn edges_must_have_length() -> Result<()> {
    let mut t = Topo::new();
    let a = vertex(&mut t, Point3::ZERO)?;
    // Open edge whose vertices overlap.
    let b = vertex(&mut t, Point3::new(TOL, 0.0, 0.0))?;
    let curve = t.add_curve(Curve3::line(Point3::ZERO, Point3::new(TOL, 0.0, 0.0))?);

    let r = t.add_edge(Edge {
        curve,
        t0: 0.0,
        t1: 1.0,
        start: a,
        end: b,
        tolerance: 0.0,
    });
    assert!(matches!(invalid(r), V::ZeroLengthEdge { .. }));
    // Closed edge that never leaves its vertex.
    let curve = t.add_curve(Curve3::line(Point3::ZERO, Point3::ZERO)?);

    let r = t.add_edge(Edge {
        curve,
        t0: 0.0,
        t1: 1.0,
        start: a,
        end: a,
        tolerance: TOL,
    });
    assert!(matches!(invalid(r), V::ZeroLengthEdge { .. }));
    // A real closed edge (out to x = 1 and back) is fine.
    let curve = t.add_curve(Curve3::new(
        1,
        vec![0.0, 0.0, 0.5, 1.0, 1.0],
        vec![Point3::ZERO, Point3::X, Point3::ZERO],
        None,
    )?);

    t.add_edge(Edge {
        curve,
        t0: 0.0,
        t1: 1.0,
        start: a,
        end: a,
        tolerance: TOL,
    })?;
    Ok(())
}

/// A triangle A, B, P on a bilinear patch whose v = 1 side collapses to
/// the apex P. The loop runs A → B → P, stays at P while its pcurve
/// crosses the collapsed side (a pole coedge), then P → A. `pole_uv` gives
/// the pole pcurve's ends.
fn apex_face(t: &mut Topo, pole_uv: [Point2; 2]) -> Result<FaceId, TopoError> {
    let (a, b, p) = (Point3::ZERO, Point3::X, Point3::new(0.5, 1.0, 0.0));
    let surface = t.add_surface(NurbsSurface::bilinear(a, b, p, p).expect("valid"));
    let [va, vb, vp] = [vertex(t, a)?, vertex(t, b)?, vertex(t, p)?];
    let edge = |t: &mut Topo, from: (VertexId, Point3, Point2), to: (VertexId, Point3, Point2)| {
        let curve = t.add_curve(Curve3::line(from.1, to.1).expect("valid"));
        let e = t.add_edge(Edge {
            curve,
            t0: 0.0,
            t1: 1.0,
            start: from.0,
            end: to.0,
            tolerance: TOL,
        })?;
        let pcurve = t.add_pcurve(Curve2::line(from.2, to.2).expect("valid"));
        t.add_coedge(Coedge::edge(e, Orientation::Forward, Some(pcurve)))
    };
    let ab = edge(t, (va, a, Point2::ZERO), (vb, b, Point2::X))?;
    let bp = edge(t, (vb, b, Point2::X), (vp, p, Point2::ONE))?;
    let pa = edge(t, (vp, p, Point2::Y), (va, a, Point2::ZERO))?;
    let pcurve = t.add_pcurve(Curve2::line(pole_uv[0], pole_uv[1]).expect("valid"));
    let pole = t.add_coedge(Coedge::pole(vp, 0.0, 1.0, Some(pcurve)))?;
    let l = t.add_loop(Loop {
        coedges: vec![ab, bp, pole, pa],
    })?;
    t.add_face(Face {
        surface,
        loops: vec![l],
        tolerance: TOL,
    })
}

#[test]
fn poles_are_coedges_on_a_collapsed_side() -> Result<()> {
    let mut t = Topo::new();
    let f = apex_face(&mut t, [Point2::ONE, Point2::Y])?;
    let s = shell(&mut t, &[f])?;
    body(&mut t, BodyKind::Sheet, s)?;
    assert_eq!(t.validate(), Ok(()));
    // A pole whose pcurve runs along a side that does not collapse.
    let mut t = Topo::new();
    let r = apex_face(&mut t, [Point2::X, Point2::ZERO]);
    assert!(matches!(invalid(r), V::EdgeOffFace { .. }));
    Ok(())
}
