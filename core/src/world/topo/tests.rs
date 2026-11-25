use anyhow::{Context, Result};

use super::*;
use crate::math::{Point2, Point3};

/// Tolerance given to every test entity.
const TOL: f64 = 1e-7;

/// A unit-square sheet body on a bilinear patch, with every entity named.
pub(crate) fn square() -> Result<(Topo, BodyId, [EdgeId; 4])> {
    let mut t = Topo::new();
    let p = [
        Point3::ZERO,
        Point3::X,
        Point3::new(1.0, 1.0, 0.0),
        Point3::Y,
    ];
    let uv = [Point2::ZERO, Point2::X, Point2::ONE, Point2::Y];
    let mut v = Vec::new();
    for point in p {
        v.push(t.add_vertex(Vertex {
            point,
            tolerance: TOL,
        })?);
    }
    let surface = t.add_surface(NurbsSurface::bilinear(p[0], p[1], p[3], p[2])?);

    let mut edges = [EdgeId::from_raw(0); 4];
    let mut coedges = Vec::new();
    for i in 0..4 {
        let j = (i + 1) % 4;
        let curve = t.add_curve(Curve3::line(p[i], p[j])?);
        let e = t.add_edge(Edge {
            curve,
            t0: 0.0,
            t1: 1.0,
            start: v[i],
            end: v[j],
            tolerance: TOL,
        })?;
        let pc = t.add_pcurve(Curve2::line(uv[i], uv[j])?);
        coedges.push(t.add_coedge(Coedge::edge(e, Orientation::Forward, Some(pc)))?);
        t.set_name(e, format!("square/edge/{i}"))?;
        t.set_name(v[i], format!("square/vertex/{i}"))?;
        edges[i] = e;
    }
    let l = t.add_loop(Loop { coedges })?;
    let f = t.add_face(Face {
        surface,
        loops: vec![l],
        tolerance: TOL,
    })?;
    let s = t.add_shell(Shell {
        faces: vec![FaceUse {
            face: f,
            orientation: Orientation::Forward,
        }],
    })?;
    let lump = t.add_lump(Lump { shells: vec![s] })?;
    let b = t.add_body(Body {
        kind: BodyKind::Sheet,
        lumps: vec![lump],
        ..Default::default()
    })?;
    t.set_name(f, "square/face")?;
    t.set_name(b, "square")?;
    Ok((t, b, edges))
}

#[test]
fn builds_and_walks_a_sheet() -> Result<()> {
    let (t, b, edges) = square()?;
    let faces = t.body_faces(b).context("body exists")?;
    assert_eq!(faces.len(), 1);
    assert_eq!(t.coedges_of_edge(edges[2]).len(), 1);
    let l = t.face(faces[0]).context("face exists")?.loops[0];
    assert_eq!(t.face_of_loop(l), Some(faces[0]));
    let vs = t.loop_vertices(l).context("loop exists")?;
    assert_eq!(t.name_of(vs[3]), Some("square/vertex/3"));
    Ok(())
}

#[test]
fn names_resolve_with_their_kind() -> Result<()> {
    let (t, b, edges) = square()?;
    assert_eq!(t.resolve::<BodyId>("square"), Some(b));
    assert_eq!(t.resolve::<EdgeId>("square/edge/1"), Some(edges[1]));
    assert_eq!(t.resolve::<FaceId>("square/edge/1"), None);
    assert_eq!(t.names().with_prefix("square/edge/").count(), 4);
    Ok(())
}

#[test]
fn open_loops_are_refused() -> Result<()> {
    let (mut t, _, edges) = square()?;
    let c0 = t.add_coedge(Coedge::edge(edges[0], Orientation::Forward, None))?;
    let c2 = t.add_coedge(Coedge::edge(edges[2], Orientation::Forward, None))?;
    assert_eq!(
        t.add_loop(Loop {
            coedges: vec![c0, c2]
        }),
        Err(TopoError::OpenLoop { index: 0 })
    );
    // An edge and its reverse use close up (a slit).
    let c0r = t.add_coedge(Coedge::edge(edges[0], Orientation::Reversed, None))?;
    assert!(
        t.add_loop(Loop {
            coedges: vec![c0, c0r]
        })
        .is_ok()
    );
    Ok(())
}

#[test]
fn ranges_are_checked() -> Result<()> {
    let (mut t, _, edges) = square()?;
    let e = *t.edge(edges[0]).context("edge exists")?;
    let bad = Edge { t1: 2.0, ..e };
    assert!(matches!(
        t.add_edge(bad),
        Err(TopoError::RangeOutsideDomain { .. })
    ));
    assert!(matches!(
        t.add_coedge(Coedge::pole(e.start, 1.0, 1.0, None)),
        Err(TopoError::BadRange { .. })
    ));
    let pole = t.add_coedge(Coedge::pole(e.start, 0.0, 1.0, None))?;
    assert_eq!(
        t.coedge_ends(t.coedge(pole).context("exists")?),
        (e.start, e.start)
    );
    Ok(())
}

#[test]
fn removal_is_top_down_and_prunes_names() -> Result<()> {
    let (mut t, b, edges) = square()?;
    assert!(matches!(
        t.remove(edges[0]),
        Err(TopoError::StillReferenced { .. })
    ));
    t.remove(b)?;
    assert_eq!(t.lookup("square"), None);
    assert!(!t.contains(b));
    // A stale id no longer names anything, and the name is free again.
    assert!(matches!(
        t.set_name(b, "square"),
        Err(TopoError::Missing(_))
    ));
    Ok(())
}

#[test]
fn duplicate_names_are_refused() -> Result<()> {
    let (mut t, _, edges) = square()?;
    assert!(matches!(
        t.set_name(edges[0], "square"),
        Err(TopoError::Naming(NamingError::NameTaken { .. }))
    ));
    t.rename(edges[0], "square/edge/bottom")?;
    assert_eq!(t.resolve::<EdgeId>("square/edge/bottom"), Some(edges[0]));
    assert_eq!(t.lookup("square/edge/0"), None);
    Ok(())
}
