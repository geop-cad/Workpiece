//! Imports the STEP files named on the command line and says what each
//! holds: `cargo run --release -p shapecraft-ops-step --example import -- a.step`.

use std::time::Instant;

fn main() {
    for path in std::env::args().skip(1) {
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) => {
                println!("{path}: {e}");
                continue;
            }
        };
        let clock = Instant::now();
        match shapecraft_ops_step::import(&bytes) {
            Ok(found) => {
                println!(
                    "{path}: {} solid(s), {} refused, {:.1} MB in {:?}",
                    found.bodies.len(),
                    found.refused.len(),
                    bytes.len() as f64 / 1e6,
                    clock.elapsed()
                );
                for body in &found.bodies {
                    let m = shapecraft_boolean::measure(&body.solid);
                    let (lo, hi) = body.solid.extent();
                    println!(
                        "  {:?}: {} faces, volume {:.6}, area {:.6}, from {:.3?} to {:.3?}",
                        body.name,
                        body.solid.faces.len(),
                        m.volume,
                        m.area,
                        lo.to_array(),
                        hi.to_array()
                    );
                }
                for why in &found.refused {
                    println!("  refused: {why}");
                }
            }
            Err(e) => println!("{path}: {e:#}"),
        }
    }
}
