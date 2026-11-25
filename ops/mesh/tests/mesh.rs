//! What is done to meshes, and the bodies they make.

use std::collections::BTreeSet;

use workpiece_core::math::Point3;
use workpiece_ops_mesh::poly::{Item, Poly, edge};
use workpiece_ops_mesh::{Primitive, body, ops, subd};

fn cube() -> Poly {
    Primitive::Box.mesh(2.0, 1, Point3::ZERO)
}

/// How many edges of `mesh` are not between exactly two faces that
/// walk them opposite ways.
fn open(mesh: &Poly) -> usize {
    let mut bad = 0;
    for ((a, b), sides) in mesh.edges() {
        let ways: Vec<bool> = sides
            .iter()
            .map(|s| mesh.faces[s.face][s.corner] == a)
            .collect();
        let _ = b;
        if ways.len() != 2 || ways[0] == ways[1] {
            bad += 1;
        }
    }
    bad
}

/// The volume a closed mesh encloses.
fn volume(mesh: &Poly) -> f64 {
    let mut sum = 0.0;
    for ring in &mesh.faces {
        for k in 1..ring.len() - 1 {
            let [a, b, c] = [ring[0], ring[k], ring[k + 1]].map(|v| mesh.points[v]);
            sum += a.dot(b.cross(c));
        }
    }
    sum / 6.0
}

fn top(mesh: &Poly) -> usize {
    (0..mesh.faces.len())
        .find(|&f| mesh.normal(f).z > 0.9)
        .expect("a face up")
}

fn close(got: f64, wanted: f64) {
    assert!((got - wanted).abs() < 1e-9, "{got} is not {wanted}");
}

#[test]
fn faces_are_extruded_inset_and_bevelled() {
    let cube = cube();
    let up = [Item::Face(top(&cube))];
    let (taller, picked) = ops::extrude(&cube, &up, 1.0).unwrap();
    assert_eq!(open(&taller), 0);
    close(volume(&taller), 12.0);
    assert_eq!(taller.faces.len(), 10);
    assert!(matches!(picked[..], [Item::Face(_)]));

    // A rim set in, and what is inside pushed down: a pocket.
    let (pocket, _) = ops::inset(&cube, &up, 0.5, -1.0, false).unwrap();
    assert_eq!(open(&pocket), 0);
    close(volume(&pocket), 8.0 - 7.0 / 3.0);

    // An edge cut away: a prism with legs of the width goes.
    let along = cube
        .edges()
        .into_keys()
        .find(|&(a, b)| cube.points[a].z > 0.0 && cube.points[b].z > 0.0)
        .unwrap();
    let (cut, faces) = ops::bevel(&cube, &[Item::Edge(along.0, along.1)], 0.5).unwrap();
    assert_eq!(open(&cut), 0, "{cut:?}");
    close(volume(&cut), 8.0 - 0.5 * 0.5 * 0.5 * 2.0);
    assert_eq!(faces.len(), 1);

    // All twelve: a face for each edge and one at each corner.
    let all: Vec<Item> = cube
        .edges()
        .into_keys()
        .map(|(a, b)| Item::Edge(a, b))
        .collect();
    let (round, faces) = ops::bevel(&cube, &all, 0.25).unwrap();
    assert_eq!(open(&round), 0);
    assert_eq!(faces.len(), 12 + 8);
    assert_eq!(round.faces.len(), 6 + 12 + 8);
    assert!(volume(&round) < 8.0 && volume(&round) > 7.0);
}

#[test]
fn loops_are_cut_and_found() {
    let cube = cube();
    let upright = cube
        .edges()
        .into_keys()
        .find(|&(a, b)| (cube.points[a].z - cube.points[b].z).abs() > 1.0)
        .unwrap();
    let (cut, picked) = ops::loop_cut(&cube, upright, 2, 0.0).unwrap();
    assert_eq!(open(&cut), 0);
    assert_eq!(cut.faces.len(), 6 + 4 * 2);
    assert_eq!(picked.len(), 8);
    close(volume(&cut), 8.0);
    // The new loops are found from any of their edges.
    let Item::Edge(a, b) = picked[0] else {
        panic!()
    };
    assert_eq!(ops::edge_loop(&cut, edge(a, b)).len(), 4);

    // A knife's cut: a point on each of two edges of a face, joined.
    let ring = cube.faces[0].clone();
    let cuts = [
        (edge(ring[0], ring[1]), vec![0.5]),
        (edge(ring[2], ring[3]), vec![0.5]),
    ];
    let (mut split, new) = ops::split_edges(&cube, &cuts.into_iter().collect());
    let ends: Vec<usize> = new.values().flatten().copied().collect();
    assert!(ops::connect(&mut split, ends[0], ends[1]));
    assert_eq!(open(&split), 0);
    assert_eq!(split.faces.len(), 7);
}

#[test]
fn holes_are_made_and_closed_again() {
    let cube = cube();
    let up = top(&cube);
    let (holed, _) = ops::delete(&cube, &[Item::Face(up)]).unwrap();
    assert_eq!(holed.faces.len(), 5);
    assert_eq!(holed.points.len(), 8);
    let rim: Vec<Item> = holed
        .edges()
        .into_iter()
        .filter(|(_, s)| s.len() == 1)
        .map(|((a, b), _)| Item::Edge(a, b))
        .collect();
    assert_eq!(rim.len(), 4);
    let (closed, _) = ops::fill(&holed, &rim).unwrap();
    assert_eq!(open(&closed), 0);
    close(volume(&closed), 8.0);

    // Two holes facing each other are bridged: a tunnel.
    let grid = Primitive::Box.mesh(3.0, 3, Point3::ZERO);
    let middle = |z: f64| {
        (0..grid.faces.len())
            .find(|&f| grid.center(f).distance(Point3::new(0.0, 0.0, z)) < 1e-9)
            .unwrap()
    };
    let (tunnel, walls) =
        ops::bridge(&grid, &[Item::Face(middle(1.5)), Item::Face(middle(-1.5))]).unwrap();
    assert_eq!(open(&tunnel), 0);
    assert_eq!(walls.len(), 4);
    close(volume(&tunnel), 27.0 - 3.0);

    // A hole of eight edges takes a grid of quads.
    let hole: BTreeSet<usize> = (0..grid.faces.len())
        .filter(|&f| grid.normal(f).z > 0.9)
        .collect();
    let (opened, _) = grid.without_faces(&hole);
    let rim: Vec<Item> = opened
        .edges()
        .into_iter()
        .filter(|(_, s)| s.len() == 1)
        .map(|((a, b), _)| Item::Edge(a, b))
        .collect();
    assert_eq!(rim.len(), 12);
    let (filled, faces) = ops::grid_fill(&opened, &rim, 3, 0).unwrap();
    assert_eq!(faces.len(), 9);
    assert_eq!(open(&filled), 0);
    close(volume(&filled), 27.0);
}

#[test]
fn what_is_dissolved_leaves_no_hole() {
    let grid = Primitive::Box.mesh(2.0, 2, Point3::ZERO);
    assert_eq!(grid.faces.len(), 24);
    // The point in the middle of a side: its four faces become one.
    let middle = grid
        .points
        .iter()
        .position(|p| p.distance(Point3::Z) < 1e-9)
        .unwrap();
    let (fewer, _) = ops::dissolve(&grid, &[Item::Vertex(middle)]).unwrap();
    assert_eq!(fewer.faces.len(), 21);
    assert_eq!(open(&fewer), 0);
    close(volume(&fewer), 8.0);
    // An edge: the faces on both sides of it become one.
    let (a, b) = grid
        .edges()
        .into_keys()
        .find(|&(a, b)| a == middle || b == middle)
        .unwrap();
    let (fewer, _) = ops::dissolve(&grid, &[Item::Edge(a, b)]).unwrap();
    assert_eq!(fewer.faces.len(), 23);
    assert_eq!(open(&fewer), 0);
    // Merged points: an edge collapsed takes its faces' corner.
    let (merged, picked) = ops::merge(&grid, &[Item::Edge(a, b)], false).unwrap();
    assert_eq!(merged.points.len(), grid.points.len() - 1);
    assert_eq!(picked.len(), 1);
    assert_eq!(open(&merged), 0);
    // Subdividing and welding back.
    let (finer, _) = ops::subdivide(&grid, &[Item::Face(0)]).unwrap();
    assert_eq!(finer.faces.len(), 27);
    assert_eq!(open(&finer), 0);
    let (tris, _) = ops::triangulate(&grid, &[], false).unwrap();
    assert_eq!(tris.faces.len(), 48);
    let (quads, _) = ops::quads(&tris, &[]).unwrap();
    assert_eq!(quads.faces.len(), 24);
    assert_eq!(open(&quads), 0);
}

#[test]
fn a_mirrored_half_makes_a_whole() {
    let cube = cube();
    // The half with x ≥ 0: the box cut by a loop, the far side gone.
    let across = cube
        .edges()
        .into_keys()
        .find(|&(a, b)| (cube.points[a].x - cube.points[b].x).abs() > 1.0)
        .unwrap();
    let (cut, _) = ops::loop_cut(&cube, across, 1, 0.0).unwrap();
    let far: Vec<Item> = (0..cut.points.len())
        .filter(|&v| cut.points[v].x < -0.5)
        .map(Item::Vertex)
        .collect();
    let (half, _) = ops::delete(&cut, &far).unwrap();
    assert_eq!(open(&half), 4);
    let whole = ops::mirrored(&half, [true, false, false], 1e-9);
    assert_eq!(open(&whole), 0);
    close(volume(&whole), 8.0);
    // Thickened, an open skin is closed.
    let grid = Primitive::Grid.mesh(2.0, 2, Point3::ZERO);
    let (slab, _) = ops::solidify(&grid, &[], 0.5).unwrap();
    assert_eq!(open(&slab), 0);
    close(volume(&slab), 2.0);
}

#[test]
fn meshes_make_sound_bodies() -> anyhow::Result<()> {
    // Flat: the box itself.
    let pieces = body::bodies(&cube(), false, 0, "m")?;
    assert_eq!(pieces.len(), 1);
    assert!(pieces[0].closed);
    let (v, a) = workpiece_ops_testing::sound(&pieces[0].solid)?;
    workpiece_ops_testing::close(v, 8.0, 1e-9);
    workpiece_ops_testing::close(a, 24.0, 1e-9);

    // Smooth: something round inside the box, a face for each quad.
    let pieces = body::bodies(&cube(), true, 0, "m")?;
    assert!(pieces[0].closed);
    assert_eq!(pieces[0].solid.faces.len(), 24);
    let (v, _) = workpiece_ops_testing::sound(&pieces[0].solid)?;
    // (What the mesh goes to, subdivided again and again.)
    let mut fine = cube();
    for _ in 0..6 {
        fine = subd::refined(&fine);
    }
    assert!(
        (v - volume(&fine)).abs() < 0.003 * v,
        "{v} and {}",
        volume(&fine)
    );
    // Where four quads meet at every corner of a face, the face is
    // one face of the body, and the very surface: only those at the
    // eight corners of a finer box are in four.
    let finer = Primitive::Box.mesh(2.0, 3, Point3::ZERO);
    let pieces = body::bodies(&finer, true, 0, "m")?;
    assert_eq!(pieces[0].solid.faces.len(), 54 - 24 + 4 * 24);
    workpiece_ops_testing::sound(&pieces[0].solid)?;

    // Creased all around its top, the surface ends there in an edge:
    // more of the box is kept.
    let mut creased = cube();
    let up = top(&creased);
    let ring = creased.faces[up].clone();
    for k in 0..4 {
        creased.creases.insert(edge(ring[k], ring[(k + 1) % 4]));
    }
    let sharp = body::bodies(&creased, true, 0, "m")?;
    let (u, _) = workpiece_ops_testing::sound(&sharp[0].solid)?;
    assert!(u > v, "{u} and {v}");

    // Other faces than quads: a cone, flat and smooth.
    let cone = Primitive::Cone.mesh(2.0, 8, Point3::ZERO);
    let flat = body::bodies(&cone, false, 0, "m")?;
    let (v, _) = workpiece_ops_testing::sound(&flat[0].solid)?;
    workpiece_ops_testing::close(v, volume(&cone), 1e-9);
    let round = body::bodies(&cone, true, 0, "m")?;
    workpiece_ops_testing::sound(&round[0].solid)?;

    // An open mesh makes a sheet.
    let grid = Primitive::Grid.mesh(2.0, 3, Point3::ZERO);
    let sheet = body::bodies(&grid, true, 0, "m")?;
    assert!(!sheet[0].closed);
    assert!(sheet[0].solid.faces.len() >= 9);
    let mut topo = workpiece_core::world::topo::Topo::new();
    workpiece_boolean::io::write_sheet(&mut topo, &sheet[0].solid, "it")?;
    assert_eq!(topo.validate(), Ok(()));

    // The surface is where the rules of subdivision say: a point of
    // the mesh subdivided again and again goes to the patch's corner.
    let mut mesh = cube();
    for _ in 0..5 {
        mesh = subd::refined(&mesh);
    }
    let limit = subd::patches(&cube()).corners[0];
    assert!(
        mesh.points[0].distance(limit) < 2e-3,
        "{:?} {limit:?}",
        mesh.points[0]
    );
    Ok(())
}

#[test]
fn a_mirror_takes_the_upper_half() {
    // A box about the origin, mirrored across x: its half, cut along
    // the plane — and whole again with its image.
    let half = ops::upper_half(&cube(), 0, 1e-9);
    assert!(half.points.iter().all(|p| p.x >= 0.0));
    assert_eq!(open(&half), 4);
    let whole = ops::mirrored(&half, [true, false, false], 1e-9);
    assert_eq!(open(&whole), 0);
    close(volume(&whole), 8.0);
    // Faces split off move by themselves; blown up, a box is larger.
    let (loose, picked) = ops::detach(&cube(), &[Item::Face(0)]).unwrap();
    assert_eq!(loose.points.len(), 12);
    assert_eq!(picked, [Item::Face(0)]);
    let (larger, _) = ops::inflate(&cube(), &[], 0.1).unwrap();
    assert!(volume(&larger) > 8.0);
}

#[test]
fn a_mesh_step_is_written_read_and_built() -> anyhow::Result<()> {
    use workpiece_core::Operation;
    use workpiece_ops_mesh::Mesh;
    let mut step = Mesh::starting("subd1", 2.0, true);
    step.args.mesh.creases.insert((0, 1));
    step.args.mirror = [true, false, false];
    let text = serde_json::to_string(&step)?;
    let read: Mesh = serde_json::from_str(&text)?;
    assert_eq!(read, step);
    // Built: a solid called after the step, or — with a face gone — a
    // sheet.
    let mut b = workpiece_ops_testing::Standalone::new();
    let whole = Mesh::starting("subd1", 2.0, true);
    whole.apply(&mut b)?;
    workpiece_ops_testing::check(&b);
    assert!(b.world.topo.lookup("subd1").is_some());
    let mut open = Mesh::starting("mesh1", 2.0, false);
    open.args.mesh.faces.pop();
    open.apply(&mut b)?;
    workpiece_ops_testing::check(&b);
    assert!(b.world.topo.lookup("mesh1/f0").is_some());
    // A finer mesh is still quick to make a surface of.
    let fine = Primitive::Box.mesh(2.0, 8, Point3::ZERO);
    let started = std::time::Instant::now();
    let pieces = body::bodies(&fine, true, 0, "m")?;
    let mut topo = workpiece_core::world::topo::Topo::new();
    workpiece_boolean::write(&mut topo, &pieces[0].solid, "it")?;
    eprintln!(
        "{} faces in {:?}",
        pieces[0].solid.faces.len(),
        started.elapsed()
    );
    assert!(started.elapsed().as_secs_f64() < 5.0);
    Ok(())
}
