//! What the operations of this crate make.

use shapecraft_ops_testing::*;

#[test]
fn a_flange_is_bent_up_along_a_plates_edge() -> anyhow::Result<()> {
    use shapecraft_core::math::Point3;
    use shapecraft_ops_sheet_metal::{flanged, flat_length};
    use std::f64::consts::FRAC_PI_2;
    // A plate 4 × 4 × 0.2: along its top edge at x = 4, a bend of inside
    // radius 0.3 through a right angle, and a flange 1 long.
    let axes = [Point3::X, Point3::Y, Point3::Z];
    let plate = shapecraft_boolean::block("p", Point3::ZERO, axes, [4.0, 4.0, 0.2])?;
    let edge = (0..plate.edges.len())
        .find(|&e| {
            let ends = plate.edges[e].ends.map(|v| plate.vertices[v].point);
            ends.iter().all(|p| p.x == 4.0 && (p.z - 0.2).abs() < 1e-12)
        })
        .expect("the edge");
    let (t, r, l) = (0.2, 0.3, 1.0);
    let bent = flanged(&plate, edge, (t, r, FRAC_PI_2, l), false, "flange")?;
    let mut topo = shapecraft_core::world::Topo::new();
    shapecraft_boolean::write(&mut topo, &bent, "p")?;
    assert_eq!(topo.validate(), Ok(()));
    let bend = PI / 4.0 * ((r + t) * (r + t) - r * r) * 4.0;
    close(
        shapecraft_boolean::measure(&bent).volume,
        3.2 + bend + l * t * 4.0,
        1e-9,
    );
    // It rises from the top, out past the end.
    let (_, hi) = bent.extent();
    close(hi.z, 0.2 + r + l, 1e-6);
    close(hi.x, 4.0 + r + t, 1e-6);
    // Laid out flat again from its top: a plate as long as the plate,
    // the bend's neutral layer and the flange together, as thick as
    // the sheet.
    let top = (0..bent.faces.len())
        .max_by(|a, b| {
            let up = |f: &usize| {
                let n = shapecraft_boolean::face_area(&bent, *f);
                let flat = bent.faces[*f].surface.degrees() == (1, 1);
                let high = bent.faces[*f].loops[0].iter().all(|s| match s.walk {
                    shapecraft_boolean::model::Walk::Edge { edge, .. } => bent.edges[edge]
                        .ends
                        .iter()
                        .all(|v| (bent.vertices[*v].point.z - 0.2).abs() < 1e-9),
                    _ => false,
                });
                if flat && high { n } else { 0.0 }
            };
            up(a).total_cmp(&up(b))
        })
        .expect("the top");
    let k = 0.44;
    let (flat, bends) = shapecraft_ops_sheet_metal::unfolded(&bent, top, t, k, "flat")?;
    // One bend, its centre line across the plate (along y, 4 long) where
    // the bend's neutral layer is half laid out: up, through 90°.
    assert_eq!(bends.len(), 1);
    let [a, b] = bends[0].line;
    close(a.distance(b), 4.0, 1e-9);
    close((a - b).x.abs(), 0.0, 1e-9);
    close(a.x, 4.0 + FRAC_PI_2 * (r + k * t) / 2.0, 1e-6);
    close(bends[0].angle, FRAC_PI_2, 1e-9);
    close(bends[0].radius, r, 1e-9);
    assert!(bends[0].up);
    let mut topo = shapecraft_core::world::Topo::new();
    shapecraft_boolean::write(&mut topo, &flat, "flat")?;
    assert_eq!(topo.validate(), Ok(()));
    let long = 4.0 + flat_length(r, t, FRAC_PI_2, l, k);
    close(
        shapecraft_boolean::measure(&flat).volume,
        long * 4.0 * t,
        1e-9,
    );
    let (lo, hi) = flat.extent();
    close(hi.x - lo.x, long, 1e-6);
    close(hi.z - lo.z, t, 1e-6);
    // Flat, the bend is as long as its neutral layer.
    close(
        flat_length(r, t, FRAC_PI_2, l, 0.5),
        l + FRAC_PI_2 * (r + 0.1),
        1e-12,
    );
    Ok(())
}
