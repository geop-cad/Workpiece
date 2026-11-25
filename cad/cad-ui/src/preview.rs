//! Previews: small pictures of parts, for lists of them. A preview is
//! taken of a part as it is built, from the front right and above, all
//! of it in view, and kept in the part's file (see
//! [`Preview`](shapecraft_cad_part::Preview)).

use std::sync::Arc;

use shapecraft_cad_part::{Cancel, Engine, Library, Preview, Workspace};
use shapecraft_core::world::{Visibility, World};

use crate::model::Camera;

/// How wide a preview is, in pixels.
pub const WIDTH: usize = 192;
/// How high a preview is, in pixels.
pub const HEIGHT: usize = 144;
/// How many samples a pixel of a preview gets: enough for a picture
/// this small to come out smooth.
const SAMPLES: usize = 96;

/// A picture of `world`, made of what `library` has: [`WIDTH`] by
/// [`HEIGHT`] pixels of red, green, blue and alpha. Nothing, for a
/// world with no body to see.
pub fn picture(world: &World, library: &Library) -> Option<Vec<u8>> {
    shot(
        world,
        library,
        &Taking::new([WIDTH, HEIGHT], SAMPLES),
        &|| true,
    )
}

/// How a picture is taken (see [`shot`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Taking {
    /// How many pixels wide and high.
    pub size: [usize; 2],
    /// How many rays each pixel gets, at most.
    pub samples: usize,
    /// From where: the way `back` points to, with `up` up in the
    /// picture; or from the front right and above as the view starts
    /// out.
    pub from: Option<(glam::Vec3, glam::Vec3)>,
    /// Whether the model stands on a floor that catches its shadow.
    pub ground: bool,
}

impl Taking {
    /// `size` pixels of `samples` rays, from the front right and above,
    /// on no floor, however long it takes.
    pub fn new(size: [usize; 2], samples: usize) -> Self {
        Self {
            size,
            samples,
            from: None,
            ground: false,
        }
    }
}

/// A picture of `world`, taken as `taking` says while `more` says so
/// (asked before each ray a pixel after the first): its pixels' red,
/// green, blue and alpha, all of the model in view. Nothing, for a
/// world with no body to see.
pub fn shot(
    world: &World,
    library: &Library,
    taking: &Taking,
    more: &dyn Fn() -> bool,
) -> Option<Vec<u8>> {
    let visibility = Visibility {
        shades: library.shades(),
        ..Visibility::placed(world, &Default::default())
    };
    let scene = crate::view::scene(world, &visibility, library);
    let (lo, hi) = scene.bounds()?;
    let fov_y = 30f32.to_radians();
    let start = Camera {
        target: (lo + hi) / 2.0,
        distance: (hi - lo).length().max(1e-6) * 0.58 / (fov_y / 2.0).tan(),
        fov_y,
        orthographic: false,
        ..Camera::default()
    };
    let from = match taking.from {
        Some((back, up)) => start.facing(start.target, back, up),
        None => start,
    };
    let shot = shapecraft_ops_appearance::Shot {
        ground: taking.ground,
        ..Default::default()
    };
    let camera = crate::view::camera(&from, &scene);
    let lighting = crate::view::lighting(&shot);
    let [width, height] = taking.size;
    let mut film = shapecraft_render::Film::new(width, height);
    for k in 0..taking.samples.max(1) {
        shapecraft_core::cancel::checkpoint();
        if k > 0 && !more() {
            break;
        }
        shapecraft_render::pass(&scene, &camera, &lighting, &mut film);
    }
    Some(film.rgba())
}

/// `rgba` ([`WIDTH`] by [`HEIGHT`] pixels) as a PNG file's bytes.
fn png(rgba: &[u8]) -> anyhow::Result<Vec<u8>> {
    // (Without alpha: a preview has a backdrop all over.)
    let rgb: Vec<u8> = rgba
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|p| [p[0], p[1], p[2]])
        .collect();
    let mut bytes = Vec::new();
    let mut encoder = png::Encoder::new(&mut bytes, WIDTH as u32, HEIGHT as u32);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(png::Compression::High);
    encoder.write_header()?.write_image_data(&rgb)?;
    Ok(bytes)
}

/// The pixels of `preview`: its width, its height, and red, green, blue
/// and alpha of each. Nothing, if it holds no picture that can be read.
pub fn pixels(preview: &Preview) -> Option<(usize, usize, Vec<u8>)> {
    let bytes = preview.png()?;
    let mut reader = png::Decoder::new(std::io::Cursor::new(bytes))
        .read_info()
        .ok()?;
    let mut buffer = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buffer).ok()?;
    buffer.truncate(info.buffer_size());
    let rgba = match info.color_type {
        png::ColorType::Rgb => buffer
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        png::ColorType::Rgba => buffer,
        _ => return None,
    };
    Some((info.width as usize, info.height as usize, rgba))
}

/// A preview of the part at `path` of `workspace` as it is now, built
/// by `engine`. Nothing, for a part with no body to see.
pub fn take(
    workspace: &Arc<Workspace>,
    engine: &Engine,
    path: &str,
    cancel: &Cancel,
) -> anyhow::Result<Option<Preview>> {
    let stamp = workspace.part(path)?.stamp();
    let built = engine.build(workspace, path, None, &|_| true, cancel)?;
    let Some(rgba) = picture(&built.world, &workspace.library) else {
        return Ok(None);
    };
    Ok(Some(Preview::new(stamp, &png(&rgba)?)))
}

impl crate::Model {
    /// Takes a preview of the part at `path` as it is now, unless it
    /// has one: what is saved with it. A part that does not build keeps
    /// the preview it had.
    pub fn take_preview(&mut self, path: &str) -> anyhow::Result<()> {
        if self.workspace.part(path)?.fresh_preview().is_some() {
            return Ok(());
        }
        let workspace = Arc::new(self.workspace.clone());
        let cancel = Cancel::new();
        let taken = cancel.run(|| take(&workspace, &self.engine, path, &cancel));
        if let Ok(Ok(Some(preview))) = taken {
            self.workspace.part_mut(path)?.preview.0 = Some(preview);
        }
        Ok(())
    }
}
