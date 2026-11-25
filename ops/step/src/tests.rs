use workpiece_boolean::measure;
use workpiece_core::{Operation, Standalone};

use crate::{ImportStep, ImportStepArgs, import};

/// A rod of radius 1 and height 2, in inches: two circles and no seam,
/// as most systems write a cylinder.
const ROD: &str = include_str!("rod.step");

const PI: f64 = std::f64::consts::PI;

#[test]
fn a_cylinder_without_a_seam_is_cut_open_and_measured_in_millimetres() -> anyhow::Result<()> {
    let found = import(ROD.as_bytes())?;
    assert!(found.refused.is_empty(), "{:?}", found.refused);
    assert_eq!(found.bodies.len(), 1);
    let body = &found.bodies[0];
    assert_eq!(body.name, "rod");
    assert_eq!(body.solid.faces.len(), 3);
    // The side is one loop: both circles and the seam twice.
    let side = &body.solid.faces[2];
    assert_eq!(side.loops.len(), 1);
    assert_eq!(side.loops[0].len(), 4);
    let m = measure(&body.solid);
    let inch = 25.4_f64;
    let (volume, area) = (2.0 * PI * inch.powi(3), 6.0 * PI * inch.powi(2));
    assert!((m.volume / volume - 1.0).abs() < 1e-9, "{}", m.volume);
    assert!((m.area / area - 1.0).abs() < 1e-9, "{}", m.area);
    Ok(())
}

#[test]
fn the_operation_adds_a_files_solids_as_bodies() -> anyhow::Result<()> {
    let path =
        std::env::temp_dir().join(format!("workpiece-step-test-{}.step", std::process::id()));
    std::fs::write(&path, ROD)?;
    let mut b = Standalone::new();
    let op = ImportStep {
        id: "rod".into(),
        args: ImportStepArgs {
            path: path.to_string_lossy().into_owned(),
        },
    };
    let done = op.apply(&mut b);
    std::fs::remove_file(&path)?;
    done?;
    let topo = &b.world.topo;
    assert_eq!(topo.validate(), Ok(()));
    let body = topo
        .resolve::<workpiece_core::world::topo::BodyId>("rod")
        .expect("the body");
    assert_eq!(topo.body_faces(body).map(|f| f.len()), Some(3));
    // No file, no body; and it says so.
    let missing = ImportStep {
        id: "none".into(),
        args: ImportStepArgs {
            path: "/nonexistent/none.step".into(),
        },
    };
    let error = missing.apply(&mut b).expect_err("no such file");
    assert!(format!("{error:#}").contains("none.step"), "{error:#}");
    // What is not a solid of faces is refused, not guessed at.
    let broken = ROD.replace(
        "#33=CLOSED_SHELL('',(#24,#27,#32));",
        "#33=CLOSED_SHELL('',(#24,#32));",
    );
    let found = import(broken.as_bytes())?;
    assert!(found.bodies.is_empty());
    assert!(
        found.refused[0].contains("do not join two faces"),
        "{:?}",
        found.refused
    );
    Ok(())
}

/// What places [`ROD`] twice in an assembly: shifted by 10 along x, and
/// laid along x from (0, 5, 0).
const PLACED_TWICE: &str = "#50=ADVANCED_BREP_SHAPE_REPRESENTATION('',(#4,#34),#45);
#51=SHAPE_REPRESENTATION('',(#4,#60,#63),#45);
#52=PRODUCT_DEFINITION('','',$,$);
#53=PRODUCT_DEFINITION('','',$,$);
#54=PRODUCT_DEFINITION_SHAPE('','',#52);
#55=SHAPE_DEFINITION_REPRESENTATION(#54,#50);
#56=PRODUCT_DEFINITION_SHAPE('','',#53);
#57=SHAPE_DEFINITION_REPRESENTATION(#56,#51);
#58=CARTESIAN_POINT('',(10.,0.,0.));
#60=AXIS2_PLACEMENT_3D('',#58,#2,#3);
#61=CARTESIAN_POINT('',(0.,5.,0.));
#62=DIRECTION('',(1.,0.,0.));
#64=DIRECTION('',(0.,0.,-1.));
#63=AXIS2_PLACEMENT_3D('',#61,#62,#64);
#65=NEXT_ASSEMBLY_USAGE_OCCURRENCE('1','','',#53,#52,$);
#66=PRODUCT_DEFINITION_SHAPE('','',#65);
#67=ITEM_DEFINED_TRANSFORMATION('','',#4,#60);
#68=(REPRESENTATION_RELATIONSHIP('','',#50,#51)REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION(#67)SHAPE_REPRESENTATION_RELATIONSHIP());
#69=CONTEXT_DEPENDENT_SHAPE_REPRESENTATION(#68,#66);
#70=NEXT_ASSEMBLY_USAGE_OCCURRENCE('2','','',#53,#52,$);
#71=PRODUCT_DEFINITION_SHAPE('','',#70);
#72=ITEM_DEFINED_TRANSFORMATION('','',#4,#63);
#73=(REPRESENTATION_RELATIONSHIP('','',#50,#51)REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION(#72)SHAPE_REPRESENTATION_RELATIONSHIP());
#74=CONTEXT_DEPENDENT_SHAPE_REPRESENTATION(#73,#71);
";

#[test]
fn a_part_of_an_assembly_is_at_every_place_it_is_used() -> anyhow::Result<()> {
    let text = ROD.replace("#40=", &format!("{PLACED_TWICE}#40="));
    let found = import(text.as_bytes())?;
    assert!(found.refused.is_empty(), "{:?}", found.refused);
    assert_eq!(found.bodies.len(), 2);
    let inch = 25.4_f64;
    let volume = 2.0 * PI * inch.powi(3);
    let boxes = [
        ([9.0, -1.0, 0.0], [11.0, 1.0, 2.0]),
        ([0.0, 4.0, -1.0], [2.0, 6.0, 1.0]),
    ];
    for (body, (lo, hi)) in found.bodies.iter().zip(boxes) {
        let m = measure(&body.solid);
        assert!((m.volume / volume - 1.0).abs() < 1e-9, "{}", m.volume);
        let (from, to) = body.solid.extent();
        for k in 0..3 {
            assert!((from[k] / inch - lo[k]).abs() < 1e-6, "{from:?}");
            assert!((to[k] / inch - hi[k]).abs() < 1e-6, "{to:?}");
        }
    }
    Ok(())
}

#[test]
fn what_is_written_reads_back_as_it_was() -> anyhow::Result<()> {
    use workpiece_boolean::{Op, combine, read};
    use workpiece_core::world::ORIGIN;
    use workpiece_core::{Measure, Reference};
    use workpiece_ops_basic_shapes::{AddBox, AddBoxArgs, AddCylinder, AddCylinderArgs, AddSphere};
    // A block with a hole through it and a hollow ball inside; and a
    // rod beside it.
    let mut b = Standalone::new();
    AddBox {
        id: "block".into(),
        args: AddBoxArgs {
            frame: Some(Reference::datum(ORIGIN)),
            size: [6.0, 4.0, 3.0].map(Measure::new),
        },
    }
    .apply(&mut b)?;
    AddCylinder {
        id: "rod".into(),
        args: AddCylinderArgs {
            frame: Some(Reference::datum(ORIGIN)),
            radius: Measure::new(0.5),
            height: Measure::new(4.0),
        },
    }
    .apply(&mut b)?;
    let mut ball = AddSphere {
        id: "ball".into(),
        args: Default::default(),
    };
    ball.args.frame = Some(Reference::datum(ORIGIN));
    ball.args.radius = Measure::new(0.4);
    ball.apply(&mut b)?;
    let topo = &b.world.topo;
    let solid = |name: &str| read(topo, topo.resolve(name).expect("a body"));
    let (block, rod, ball) = (solid("block")?, solid("rod")?, solid("ball")?);
    let ball = ball.moved(&|p| p + workpiece_core::math::Point3::new(2.0, 0.0, 0.0))?;
    let drilled = combine(Op::Cut, &combine(Op::Cut, &block, &rod)?.solid, &ball)?.solid;
    let far = rod.moved(&|p| p + workpiece_core::math::Point3::new(10.0, 0.0, 0.0))?;
    let bodies = [("drilled".to_owned(), drilled), ("rod".to_owned(), far)];
    let text = crate::export("part", &bodies, workpiece_core::Unit::Millimetre)?;
    assert!(text.contains("BREP_WITH_VOIDS('drilled'"));
    let back = import(text.as_bytes())?;
    assert!(back.refused.is_empty(), "{:?}", back.refused);
    assert_eq!(back.bodies.len(), 2);
    for (name, was) in &bodies {
        let body = back
            .bodies
            .iter()
            .find(|b| &b.name == name)
            .expect("the body");
        let (a, b) = (measure(was), measure(&body.solid));
        assert!(
            (a.volume - b.volume).abs() < 1e-9 * a.volume,
            "{} {}",
            a.volume,
            b.volume
        );
        assert!(
            (a.area - b.area).abs() < 1e-9 * a.area,
            "{} {}",
            a.area,
            b.area
        );
        assert_eq!(was.faces.len(), body.solid.faces.len());
    }
    // Written in inches, it is read back 25.4 times the size.
    let text = crate::export("part", &bodies, workpiece_core::Unit::Inch)?;
    let back = import(text.as_bytes())?;
    assert!(back.refused.is_empty(), "{:?}", back.refused);
    let (was, is) = (measure(&bodies[1].1), measure(&back.bodies[1].solid));
    let grown = is.volume / was.volume;
    assert!((grown - 25.4_f64.powi(3)).abs() < 1e-6 * grown, "{grown}");
    Ok(())
}

#[test]
fn a_closed_mesh_is_read_as_the_solid_it_bounds() -> anyhow::Result<()> {
    // A tetrahedron, as text and as a binary file: a solid of four
    // faces, a sixth of the cube around it.
    let corners = [
        [0.0, 0.0, 0.0],
        [3.0, 0.0, 0.0],
        [0.0, 3.0, 0.0],
        [0.0, 0.0, 3.0],
    ];
    let faces = [[0, 2, 1], [0, 1, 3], [1, 2, 3], [0, 3, 2]];
    let mut text = String::from("solid t\n");
    let mut binary = vec![0_u8; 80];
    binary.extend(4_u32.to_le_bytes());
    for face in faces {
        text.push_str("facet normal 0 0 0\n outer loop\n");
        binary.extend([0_u8; 12]);
        for corner in face {
            let [x, y, z]: [f64; 3] = corners[corner];
            text.push_str(&format!("  vertex {x} {y} {z}\n"));
            for number in [x, y, z] {
                binary.extend((number as f32).to_le_bytes());
            }
        }
        text.push_str(" endloop\nendfacet\n");
        binary.extend([0_u8; 2]);
    }
    text.push_str("endsolid t\n");
    for file in [text.as_bytes(), &binary[..]] {
        let solid = crate::import_stl(file, "mesh")?;
        assert_eq!(solid.faces.len(), 4);
        let volume = measure(&solid).volume;
        assert!((volume - 4.5).abs() < 1e-9, "{volume}");
    }
    assert!(crate::import_stl(&binary[..binary.len() - 50], "mesh").is_err());
    // A mesh with a face missing, one there twice and one inside out is
    // mended: a block of 2 x 2 x 2 from ten of its twelve triangles.
    let p: Vec<workpiece_core::math::Point3> = (0..8)
        .map(|i| {
            workpiece_core::math::Point3::new(
                f64::from(i & 1),
                f64::from(i >> 1 & 1),
                f64::from(i >> 2),
            ) * 2.0
        })
        .collect();
    let quads = [
        [0, 2, 3, 1],
        [4, 5, 7, 6],
        [0, 1, 5, 4],
        [2, 6, 7, 3],
        [0, 4, 6, 2],
        [1, 3, 7, 5],
    ];
    let mut broken: Vec<[usize; 3]> = quads
        .iter()
        .flat_map(|q| [[q[0], q[1], q[2]], [q[0], q[2], q[3]]])
        .collect();
    broken.truncate(10);
    broken[3].swap(0, 1);
    broken.push(broken[5]);
    assert!(workpiece_boolean::faceted("m", &p, &broken).is_err());
    let whole = workpiece_boolean::faceted("m", &p, &crate::mended(&p, &broken))?;
    assert_eq!(whole.faces.len(), 6);
    assert!((measure(&whole).volume - 8.0).abs() < 1e-9);
    Ok(())
}

#[test]
#[ignore = "reads the file WORKPIECE_STL names: to try a mesh by hand"]
fn a_mesh_file_by_hand() -> anyhow::Result<()> {
    let path = std::env::var("WORKPIECE_STL")?;
    let started = std::time::Instant::now();
    let solid = crate::import_stl(&std::fs::read(path)?, "mesh")?;
    let mut topo = workpiece_core::world::Topo::new();
    workpiece_boolean::write(&mut topo, &solid, "mesh")?;
    assert_eq!(topo.validate(), Ok(()));
    let m = measure(&solid);
    println!(
        "{} faces, volume {}, area {}, {:?}",
        solid.faces.len(),
        m.volume,
        m.area,
        started.elapsed()
    );
    Ok(())
}

#[test]
#[ignore = "reads the 3MF file WORKPIECE_3MF names: to try one by hand"]
fn a_3mf_file_by_hand() -> anyhow::Result<()> {
    let path = std::env::var("WORKPIECE_3MF")?;
    for solid in crate::import_3mf(&std::fs::read(path)?, "mesh")? {
        let m = measure(&solid);
        println!(
            "{} faces, volume {}, area {}",
            solid.faces.len(),
            m.volume,
            m.area
        );
    }
    Ok(())
}

#[test]
fn bodies_written_as_iges_read_back_as_they_were() -> anyhow::Result<()> {
    use workpiece_boolean::{Op, combine, read};
    use workpiece_core::math::Point3;
    use workpiece_core::world::ORIGIN;
    use workpiece_core::{Measure, Reference};
    use workpiece_ops_basic_shapes::{AddBox, AddBoxArgs, AddCylinder, AddCylinderArgs, AddSphere};
    // A block with a hole drilled through it, and a ball (whose face
    // comes to a point at each pole): written as trimmed surfaces and
    // read back, sewn into solids of the same size.
    let mut b = Standalone::new();
    AddBox {
        id: "block".into(),
        args: AddBoxArgs {
            frame: Some(Reference::datum(ORIGIN)),
            size: [4.0, 3.0, 2.0].map(Measure::new),
        },
    }
    .apply(&mut b)?;
    AddCylinder {
        id: "rod".into(),
        args: AddCylinderArgs {
            frame: Some(Reference::datum(ORIGIN)),
            radius: Measure::new(0.5),
            height: Measure::new(4.0),
        },
    }
    .apply(&mut b)?;
    let mut ball = AddSphere {
        id: "ball".into(),
        args: Default::default(),
    };
    ball.args.frame = Some(Reference::datum(ORIGIN));
    ball.args.radius = Measure::new(1.5);
    ball.apply(&mut b)?;
    let topo = &b.world.topo;
    let solid = |name: &str| read(topo, topo.resolve(name).expect("a body"));
    let rod = solid("rod")?.moved(&|p| p - Point3::Z)?;
    let drilled = combine(Op::Cut, &solid("block")?, &rod)?.solid;
    for (name, was) in [("drilled", drilled), ("ball", solid("ball")?)] {
        let text = crate::export_iges(
            name,
            &[(name.to_owned(), was.clone())],
            workpiece_core::Unit::Millimetre,
        )?;
        assert!(text.lines().all(|l| l.len() == 80), "lines are 80 long");
        if let Ok(dir) = std::env::var("WORKPIECE_IGES_OUT") {
            std::fs::write(format!("{dir}/{name}.igs"), &text)?;
        }
        let back = crate::import_iges(text.as_bytes(), name)?;
        assert!(back.refused.is_empty(), "{:?}", back.refused);
        let [(body, closed)] = &back.bodies[..] else {
            panic!("one body");
        };
        assert!(closed, "{name} closes");
        let mut topo = workpiece_core::world::Topo::new();
        workpiece_boolean::write(&mut topo, body, name)?;
        assert_eq!(topo.validate(), Ok(()));
        let (a, b) = (measure(&was), measure(body));
        assert!(
            (a.volume - b.volume).abs() < 1e-6 * a.volume,
            "{name}: {} {}",
            a.volume,
            b.volume
        );
        assert!(
            (a.area - b.area).abs() < 1e-6 * a.area,
            "{name}: {} {}",
            a.area,
            b.area
        );
    }
    Ok(())
}

#[test]
#[ignore = "reads the IGES file WORKPIECE_IGES names: to try one by hand"]
fn an_iges_file_by_hand() -> anyhow::Result<()> {
    let path = std::env::var("WORKPIECE_IGES")?;
    let found = crate::import_iges(&std::fs::read(path)?, "iges")?;
    for why in &found.refused {
        println!("refused: {why}");
    }
    for (body, closed) in &found.bodies {
        let m = measure(body);
        println!(
            "{} faces, closed {closed}, volume {}, area {}",
            body.faces.len(),
            m.volume,
            m.area
        );
    }
    Ok(())
}

/// A ring's one face is closed both ways: bounded by the circle around
/// its tube and the circle around its axis, each walked there and back.
/// Written as it is here (a B-spline surface), whole and cut in half
/// through its axis, it reads back as it was.
#[test]
fn a_ring_closed_both_ways_reads_back_as_it_was() -> anyhow::Result<()> {
    use workpiece_boolean::{Op, combine, read};
    use workpiece_core::world::ORIGIN;
    use workpiece_core::{Measure, Reference};
    use workpiece_ops_basic_shapes::{AddBox, AddBoxArgs, AddTorus, AddTorusArgs};
    let mut b = Standalone::new();
    AddTorus {
        id: "ring".into(),
        args: AddTorusArgs {
            frame: Some(Reference::datum(ORIGIN)),
            radius: Measure::new(10.0),
            tube: Measure::new(2.0),
        },
    }
    .apply(&mut b)?;
    AddBox {
        id: "block".into(),
        args: AddBoxArgs {
            frame: Some(Reference::datum(ORIGIN)),
            size: [30.0, 30.0, 30.0].map(Measure::new),
        },
    }
    .apply(&mut b)?;
    let topo = &b.world.topo;
    let solid = |name: &str| read(topo, topo.resolve(name).expect("a body"));
    let (ring, block) = (solid("ring")?, solid("block")?);
    let volume = 2.0 * PI * PI * 10.0 * 4.0;
    assert!((measure(&ring).volume / volume - 1.0).abs() < 1e-9);
    // Half of it, and a quarter with a bite out of the tube's side.
    let half = block.moved(&|p| p + workpiece_core::math::Point3::new(0.0, 15.0, 0.0))?;
    let bite = block.moved(&|p| p + workpiece_core::math::Point3::new(26.0, 15.0, 15.0))?;
    let halved = combine(Op::Cut, &ring, &half)?.solid;
    let bitten = combine(Op::Cut, &ring, &bite)?.solid;
    for (name, was) in [("ring", ring), ("halved", halved), ("bitten", bitten)] {
        let bodies = [(name.to_owned(), was)];
        let text = crate::export(name, &bodies, workpiece_core::Unit::Millimetre)?;
        let back = import(text.as_bytes())?;
        assert!(back.refused.is_empty(), "{name}: {:?}", back.refused);
        assert_eq!(back.bodies.len(), 1, "{name}");
        let (a, b) = (measure(&bodies[0].1), measure(&back.bodies[0].solid));
        assert!(
            (a.volume - b.volume).abs() < 1e-9 * a.volume,
            "{name}: {} {}",
            a.volume,
            b.volume
        );
        assert!((a.area - b.area).abs() < 1e-9 * a.area, "{name}");
    }
    Ok(())
}

/// A whole ring as other systems write it: a `TOROIDAL_SURFACE` with
/// its two seams as edges, each used twice by the one loop.
const RING: &str = "ISO-10303-21;
HEADER;
FILE_DESCRIPTION((''),'2;1');
FILE_NAME('ring.step','',(''),(''),'','','');
FILE_SCHEMA(('AUTOMOTIVE_DESIGN'));
ENDSEC;
DATA;
#1=CARTESIAN_POINT('',(0.,0.,0.));
#2=DIRECTION('',(0.,0.,1.));
#3=DIRECTION('',(1.,0.,0.));
#4=AXIS2_PLACEMENT_3D('',#1,#2,#3);
#5=CARTESIAN_POINT('',(10.,0.,0.));
#6=DIRECTION('',(0.,-1.,0.));
#7=AXIS2_PLACEMENT_3D('',#5,#6,#3);
#8=TOROIDAL_SURFACE('',#4,10.,2.);
#9=CIRCLE('',#4,12.);
#10=CIRCLE('',#7,2.);
#11=CARTESIAN_POINT('',(12.,0.,0.));
#12=VERTEX_POINT('',#11);
#13=EDGE_CURVE('',#12,#12,#9,.T.);
#14=EDGE_CURVE('',#12,#12,#10,.T.);
#15=ORIENTED_EDGE('',*,*,#13,.T.);
#16=ORIENTED_EDGE('',*,*,#14,.T.);
#17=ORIENTED_EDGE('',*,*,#13,.F.);
#18=ORIENTED_EDGE('',*,*,#14,.F.);
#19=EDGE_LOOP('',(#15,#16,#17,#18));
#20=FACE_OUTER_BOUND('',#19,.T.);
#21=ADVANCED_FACE('',(#20),#8,.T.);
#22=CLOSED_SHELL('',(#21));
#23=MANIFOLD_SOLID_BREP('ring',#22);
ENDSEC;
END-ISO-10303-21;
";

#[test]
fn a_whole_torus_with_its_seams_as_edges_is_read() -> anyhow::Result<()> {
    // As it is, and with its loop started at its other edge.
    let other = RING.replace("(#15,#16,#17,#18)", "(#16,#17,#18,#15)");
    for text in [RING, &other] {
        let found = import(text.as_bytes())?;
        assert!(found.refused.is_empty(), "{:?}", found.refused);
        assert_eq!(found.bodies.len(), 1);
        let m = measure(&found.bodies[0].solid);
        let (volume, area) = (2.0 * PI * PI * 10.0 * 4.0, 4.0 * PI * PI * 10.0 * 2.0);
        assert!((m.volume / volume - 1.0).abs() < 1e-9, "{}", m.volume);
        assert!((m.area / area - 1.0).abs() < 1e-9, "{}", m.area);
    }
    Ok(())
}

/// A ball stretched to an egg is no sphere any more: it is written as
/// the B-spline surface it is, which closes around and comes to a point
/// at both ends, with its seam from point to point as its one edge. The
/// seam is walked up at one side of the surface and down at the other.
#[test]
fn a_spline_surface_with_poles_and_a_seam_reads_back_as_it_was() -> anyhow::Result<()> {
    use workpiece_boolean::read;
    use workpiece_core::math::Point3;
    use workpiece_core::world::ORIGIN;
    use workpiece_core::{Measure, Reference};
    use workpiece_ops_basic_shapes::AddSphere;
    let mut b = Standalone::new();
    let mut ball = AddSphere {
        id: "ball".into(),
        args: Default::default(),
    };
    ball.args.frame = Some(Reference::datum(ORIGIN));
    ball.args.radius = Measure::new(2.0);
    ball.apply(&mut b)?;
    let topo = &b.world.topo;
    let ball = read(topo, topo.resolve("ball").expect("a body"))?;
    for stretch in [Point3::new(1.0, 1.0, 1.5), Point3::new(1.5, 1.0, 0.5)] {
        let egg = ball.sheared(&|p| p * stretch, false, 1.5)?;
        let bodies = [("egg".to_owned(), egg)];
        let text = crate::export("egg", &bodies, workpiece_core::Unit::Millimetre)?;
        assert!(!text.contains("SPHERICAL_SURFACE"));
        let back = import(text.as_bytes())?;
        assert!(back.refused.is_empty(), "{:?}", back.refused);
        let (a, b) = (measure(&bodies[0].1), measure(&back.bodies[0].solid));
        let volume = 4.0 / 3.0 * PI * 8.0 * stretch.x * stretch.y * stretch.z;
        assert!((a.volume / volume - 1.0).abs() < 1e-9, "{}", a.volume);
        assert!((b.volume / volume - 1.0).abs() < 1e-9, "{}", b.volume);
        assert!((a.area - b.area).abs() < 1e-9 * a.area);
    }
    Ok(())
}

/// A sphere's patch may come to its points at the ends of its first
/// parameter and close around its second, and lie any way in space. It
/// is written as a sphere about the axis through those points, so that
/// its seam, from point to point, is a seam of the sphere read back.
#[test]
fn a_ball_with_its_poles_along_the_first_parameter_reads_back() -> anyhow::Result<()> {
    use std::sync::Arc;
    use workpiece_boolean::read;
    use workpiece_core::math::{Point2, Point3};
    use workpiece_core::world::ORIGIN;
    use workpiece_core::world::topo::Curve2;
    use workpiece_core::{Measure, Reference};
    use workpiece_ops_basic_shapes::AddSphere;
    let mut b = Standalone::new();
    let mut ball = AddSphere {
        id: "ball".into(),
        args: Default::default(),
    };
    ball.args.frame = Some(Reference::datum(ORIGIN));
    ball.args.radius = Measure::new(2.0);
    ball.apply(&mut b)?;
    let topo = &b.world.topo;
    let ball = read(topo, topo.resolve("ball").expect("a body"))?;
    // Turned, so that its poles are on no axis of the world; and its
    // surface's two parameters swapped, the face turned over with them.
    let (x, y, z) = (
        Point3::new(2.0, 1.0, 2.0) / 3.0,
        Point3::new(1.0, 2.0, -2.0) / 3.0,
        Point3::new(-2.0, 2.0, 1.0) / 3.0,
    );
    let mut ball = ball.moved(&|p| x * p.x + y * p.y + z * p.z + Point3::new(5.0, 0.0, 0.0))?;
    for face in &mut ball.faces {
        face.surface = Arc::new(crate::geometry::swapped(&face.surface)?);
        face.outward = !face.outward;
        for step in face.loops.iter_mut().flatten() {
            let c = &step.pcurve;
            step.pcurve = Arc::new(Curve2::new(
                c.degree(),
                c.knots().to_vec(),
                c.control_points()
                    .iter()
                    .map(|q| Point2::new(q.y, q.x))
                    .collect(),
                Some(c.weights().to_vec()),
            )?);
        }
        for lp in &mut face.loops {
            lp.reverse();
            for step in lp.iter_mut() {
                match &mut step.walk {
                    workpiece_boolean::model::Walk::Edge { reversed, .. } => *reversed = !*reversed,
                    workpiece_boolean::model::Walk::Pole { .. } => {
                        step.pcurve = Arc::new(step.pcurve.reversed());
                    }
                }
            }
        }
    }
    let volume = 4.0 / 3.0 * PI * 8.0;
    assert!((measure(&ball).volume / volume - 1.0).abs() < 1e-9);
    let text = crate::export(
        "ball",
        &[("ball".to_owned(), ball)],
        workpiece_core::Unit::Millimetre,
    )?;
    assert!(text.contains("SPHERICAL_SURFACE"));
    let back = import(text.as_bytes())?;
    assert!(back.refused.is_empty(), "{:?}", back.refused);
    let m = measure(&back.bodies[0].solid);
    assert!((m.volume / volume - 1.0).abs() < 1e-9, "{}", m.volume);
    Ok(())
}

/// A cone that opens by next to nothing has its tip far beyond the
/// body: it is read where the body is, as exactly as a cylinder. And a
/// surface that widens by less than surfaces are told apart by is
/// written as the cylinder it is, not as such a cone.
#[test]
fn a_cone_that_hardly_opens_is_read_and_written_as_a_cylinder() -> anyhow::Result<()> {
    use workpiece_boolean::read;
    use workpiece_core::world::ORIGIN;
    use workpiece_core::{Measure, Reference};
    use workpiece_ops_basic_shapes::{AddCone, AddConeArgs};
    let text = ROD.replace(
        "#7=CYLINDRICAL_SURFACE('',#4,1.);",
        "#7=CONICAL_SURFACE('',#4,1.,1.E-11);",
    );
    assert!(text.contains("CONICAL_SURFACE"));
    let found = import(text.as_bytes())?;
    assert!(found.refused.is_empty(), "{:?}", found.refused);
    let inch = 25.4_f64;
    let volume = measure(&found.bodies[0].solid).volume;
    assert!(
        (volume / (2.0 * PI * inch.powi(3)) - 1.0).abs() < 1e-9,
        "{volume}"
    );
    // A cone to its point, with no vertex there: only its base's circle
    // says where the body is, and the profile still starts at the tip.
    let pointed = "ISO-10303-21;
HEADER;
FILE_DESCRIPTION((''),'2;1');
FILE_NAME('cone.step','',(''),(''),'','','');
FILE_SCHEMA(('AUTOMOTIVE_DESIGN'));
ENDSEC;
DATA;
#1=CARTESIAN_POINT('',(0.,0.,0.));
#2=DIRECTION('',(0.,0.,1.));
#3=DIRECTION('',(1.,0.,0.));
#4=AXIS2_PLACEMENT_3D('',#1,#2,#3);
#5=DIRECTION('',(0.,0.,-1.));
#6=AXIS2_PLACEMENT_3D('',#1,#5,#3);
#7=CONICAL_SURFACE('',#6,1.,0.4636476090008061);
#8=PLANE('',#4);
#10=CIRCLE('',#4,1.);
#12=CARTESIAN_POINT('',(1.,0.,0.));
#14=VERTEX_POINT('',#12);
#16=EDGE_CURVE('',#14,#14,#10,.T.);
#18=ORIENTED_EDGE('',*,*,#16,.T.);
#19=ORIENTED_EDGE('',*,*,#16,.F.);
#22=EDGE_LOOP('',(#19));
#23=FACE_OUTER_BOUND('',#22,.T.);
#24=ADVANCED_FACE('',(#23),#8,.F.);
#28=EDGE_LOOP('',(#18));
#30=FACE_OUTER_BOUND('',#28,.T.);
#32=ADVANCED_FACE('',(#30),#7,.T.);
#33=CLOSED_SHELL('',(#24,#32));
#34=MANIFOLD_SOLID_BREP('cone',#33);
ENDSEC;
END-ISO-10303-21;
";
    let found = import(pointed.as_bytes())?;
    assert!(found.refused.is_empty(), "{:?}", found.refused);
    let volume = measure(&found.bodies[0].solid).volume;
    assert!((volume / (2.0 * PI / 3.0) - 1.0).abs() < 1e-9, "{volume}");
    let mut b = Standalone::new();
    AddCone {
        id: "rod".into(),
        args: AddConeArgs {
            frame: Some(Reference::datum(ORIGIN)),
            radius: Measure::new(1.0),
            top: Measure::new(1.0 + 2e-10),
            height: Measure::new(2.0),
        },
    }
    .apply(&mut b)?;
    let topo = &b.world.topo;
    let rod = read(topo, topo.resolve("rod").expect("a body"))?;
    let text = crate::export(
        "rod",
        &[("rod".to_owned(), rod)],
        workpiece_core::Unit::Millimetre,
    )?;
    assert!(text.contains("CYLINDRICAL_SURFACE") && !text.contains("CONICAL_SURFACE"));
    let back = import(text.as_bytes())?;
    assert!(back.refused.is_empty(), "{:?}", back.refused);
    let volume = measure(&back.bodies[0].solid).volume;
    assert!((volume / (2.0 * PI) - 1.0).abs() < 1e-9, "{volume}");
    Ok(())
}

/// A file says how near two of its points are one. An edge's end that
/// near to its curve is on it, however short the curve: a body with
/// faces a hair apart has edges a thousandth of a millimetre long whose
/// vertices are a tenth of that off them.
#[test]
fn an_end_as_near_its_curve_as_the_file_says_is_on_it() -> anyhow::Result<()> {
    use crate::geometry::{CurveKind, Reader, edge_curve};
    use workpiece_core::math::Point3;
    use workpiece_core::world::topo::Curve3;
    let text = ROD.replace(
        "#45=(GEOMETRIC_REPRESENTATION_CONTEXT(3)GLOBAL_UNIT_ASSIGNED_CONTEXT((#42,#44))",
        "#46=UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.E-6),#42,'distance_accuracy_value','');
#45=(GEOMETRIC_REPRESENTATION_CONTEXT(3)GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((#46))GLOBAL_UNIT_ASSIGNED_CONTEXT((#42,#44))",
    );
    let file = crate::part21::File::new(text.as_bytes())?;
    let reader = Reader::new(&file);
    assert!(
        (reader.closeness - 25.4e-6).abs() < 1e-15,
        "{}",
        reader.closeness
    );
    let file = crate::part21::File::new(ROD.as_bytes())?;
    assert_eq!(Reader::new(&file).closeness, 0.0);
    // A curve of a thousandth of a millimetre, and a vertex a little
    // more than a ten thousandth off its start.
    let (a, b) = (Point3::new(5.0, 0.0, 0.0), Point3::new(5.000_001, 0.0, 0.0));
    let kind = CurveKind::Spline(Curve3::line(a, b)?);
    let from = a + Point3::new(0.0, 1.1e-7, 0.0);
    assert!(edge_curve(&kind, from, b, false, true, 1e-7).is_err());
    let (_, range) = edge_curve(&kind, from, b, false, true, 1e-6)?;
    assert_eq!(range, (0.0, 1.0));
    Ok(())
}

/// An AP242 file with no exact solid, only its triangles: a wedge of
/// 3 x 4 x 2 centimetres as a tessellated solid, its ends two triangles,
/// its sides a strip and a fan over the face's own numbering of the
/// points; and the same triangles as a shell that is left open.
#[test]
fn a_tessellated_solid_is_read_as_the_solid_its_triangles_bound() -> anyhow::Result<()> {
    let text = "ISO-10303-21;
HEADER;
FILE_DESCRIPTION((''),'2;1');
FILE_NAME('wedge.stp','',(''),(''),'','','');
FILE_SCHEMA(('AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF { 1 0 10303 442 1 1 4 }'));
ENDSEC;
DATA;
#1=(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.CENTI.,.METRE.));
#2=(NAMED_UNIT(*)PLANE_ANGLE_UNIT()SI_UNIT($,.RADIAN.));
#3=(GEOMETRIC_REPRESENTATION_CONTEXT(3)GLOBAL_UNIT_ASSIGNED_CONTEXT((#1,#2))REPRESENTATION_CONTEXT('',''));
#4=COORDINATES_LIST('',6,((0.,0.,0.),(3.,0.,0.),(0.,4.,0.),(0.,0.,2.),(3.,0.,2.),(0.,4.,2.)));
#5=TRIANGULATED_FACE('',#4,0,(),$,(),((1,3,2),(4,5,6)));
#6=COMPLEX_TRIANGULATED_FACE('',#4,0,(),$,(1,2,4,5,3,6),((1,2,3,4),(2,5,4,6)),((5,1,3,6)));
#7=TESSELLATED_SOLID('wedge',(#5,#6),$);
#8=TESSELLATED_SHAPE_REPRESENTATION('',(#7),#3);
ENDSEC;
END-ISO-10303-21;
";
    let found = import(text.as_bytes())?;
    assert!(found.refused.is_empty(), "{:?}", found.refused);
    assert_eq!(found.bodies.len(), 1);
    let body = &found.bodies[0];
    assert_eq!(body.name, "wedge");
    assert_eq!(body.solid.faces.len(), 5);
    let m = measure(&body.solid);
    assert!((m.volume - 12_000.0).abs() < 1e-6, "{}", m.volume);
    assert!((m.area - 3_600.0).abs() < 1e-6, "{}", m.area);
    // Triangles that do not close, as a shell: a surface, which is no
    // solid and is not read as one.
    let open = text.replace(
        "#7=TESSELLATED_SOLID('wedge',(#5,#6),$)",
        "#7=TESSELLATED_SHELL('wedge',(#6),$)",
    );
    let found = import(open.as_bytes())?;
    assert!(found.bodies.is_empty() && found.refused.is_empty());
    Ok(())
}

#[test]
#[ignore = "reads the STEP file WORKPIECE_STEP names: to try one by hand"]
fn a_step_file_by_hand() -> anyhow::Result<()> {
    let path = std::env::var("WORKPIECE_STEP")?;
    let found = import(&std::fs::read(&path)?)?;
    for why in &found.refused {
        println!("refused: {why}");
    }
    for body in &found.bodies {
        let m = measure(&body.solid);
        println!(
            "{:?}: {} faces, volume {}, area {}",
            body.name,
            body.solid.faces.len(),
            m.volume,
            m.area
        );
        for (f, face) in body.solid.faces.iter().enumerate() {
            let steps: Vec<usize> = face.loops.iter().map(Vec::len).collect();
            println!("  face {f}: loops of {steps:?} steps");
        }
    }
    Ok(())
}
