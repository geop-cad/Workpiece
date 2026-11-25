//! Renders a part file without a window: `cargo run --release -p
//! workpiece-ui --example picture -- part.workpiece out.png [samples]`.
//! The picture is the part's last Render step, or, if it has none, one
//! as a Render step starts out, from the front right; the materials
//! are those of `materials.json` in the working directory.

use std::collections::BTreeMap;

use workpiece_core::world::Visibility;
use workpiece_ops_appearance::{Render, Shot};
use workpiece_part::{Library, Part, Workspace};
use workpiece_ui::model::Camera;
use workpiece_ui::view::{camera, camera_of, lighting, scene};

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let usage = || anyhow::anyhow!("picture <part.workpiece> <out.png> [samples]");
    let path = args.next().ok_or_else(usage)?;
    let out = args.next().ok_or_else(usage)?;
    let part = Part::from_text(&std::fs::read_to_string(&path)?)?;
    let mut shot = part
        .program
        .iter()
        .rev()
        .find_map(|op| op.downcast_ref::<Render>())
        .map_or_else(Shot::default, |render| render.args.clone());
    if let Some(samples) = args.next() {
        shot.samples = samples.parse()?;
    }
    let mut workspace = Workspace::new(BTreeMap::from([(path.clone(), part)]));
    if let Ok(text) = std::fs::read_to_string(workpiece_part::LIBRARY_FILE) {
        workspace.library = Library::from_text(&text)?;
    }
    let world = workspace.build(&path)?;
    let visibility = Visibility {
        shades: workspace.library.shades(),
        ..Visibility::placed(&world, &Default::default())
    };
    let traced = scene(&world, &visibility, &workspace.library);
    // From where it was taken, or looking at all of it.
    let from = match &shot.view {
        Some(view) => camera_of(view),
        None => {
            let (lo, hi) = traced.bounds().unwrap_or_default();
            let fov_y = 30f32.to_radians();
            Camera {
                target: (lo + hi) / 2.0,
                distance: (hi - lo).length() * 0.62 / (fov_y / 2.0).tan(),
                fov_y,
                orthographic: false,
                ..Camera::default()
            }
        }
    };
    let (camera, lighting) = (camera(&from, &traced), lighting(&shot));
    let mut film = workpiece_render::Film::new(shot.width, shot.height);
    let started = std::time::Instant::now();
    for _ in 0..shot.samples {
        workpiece_render::pass(&traced, &camera, &lighting, &mut film);
    }
    println!("{} samples in {:?}", shot.samples, started.elapsed());
    let file = std::io::BufWriter::new(std::fs::File::create(&out)?);
    let mut encoder = png::Encoder::new(file, shot.width as u32, shot.height as u32);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header()?.write_image_data(&film.rgba())?;
    Ok(())
}
