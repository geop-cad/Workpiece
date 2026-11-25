//! A small path tracer: it makes a picture of a scene of triangles by
//! following rays of light from the eye back to the sky, so that soft
//! shadows, mirrored metal and glass come out as they would in a
//! photograph.
//!
//! A [`Scene`] is built once from triangles and their [`Surface`]s.
//! A [`Film`] gathers the picture: every [`pass`] adds one more sample
//! to each pixel, so the picture starts out grainy and clears up pass
//! by pass. The world's z axis points up.

mod bvh;
mod film;
mod light;
mod material;
mod pattern;
mod trace;

use std::sync::{Mutex, PoisonError};

use glam::{Vec3, Vec3A};

pub use film::Film;

/// How a surface takes light.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Surface {
    /// The colour, as linear red, green and blue from 0 to 1.
    pub colour: [f32; 3],
    /// How rough the surface is: 0 is a mirror, 1 is matte.
    pub roughness: f32,
    /// How much of a metal the surface is: 0 is none (plastic, paint),
    /// 1 is bare metal.
    pub metallic: f32,
    /// How much light goes through: 0 is opaque, 1 is clear glass.
    /// Light bends as it does in glass, so the surface should close a
    /// solid.
    pub transparency: f32,
    /// The pattern drawn over the colour, by its number: 0 none,
    /// 1 brushed, 2 wood, 3 weave, 4 checker, 5 speckle, 6 ribbed.
    pub texture: u8,
    /// The length of one repeat of the pattern, in the scene's units.
    pub scale: f32,
}

/// A triangle of a scene.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Triangle {
    /// The three corners.
    pub positions: [Vec3; 3],
    /// The normals at the corners, which light is shaded with. They
    /// may point to either side: a triangle shows from both.
    pub normals: [Vec3; 3],
    /// Which of the scene's surfaces the triangle has.
    pub surface: u32,
}

/// What is in the picture: triangles and their surfaces, sorted so
/// that a ray finds what it hits quickly.
pub struct Scene {
    triangles: Vec<Triangle>,
    surfaces: Vec<Surface>,
    bvh: bvh::Bvh,
    bounds: Option<(Vec3, Vec3)>,
    /// The middle of the scene.
    centre: Vec3A,
    /// Half the scene's diagonal.
    radius: f32,
    /// The lowest z of the scene.
    low: f32,
    /// A length that is tiny against the scene; rays start this far
    /// off a surface so that they do not hit it again.
    nudge: f32,
}

impl Scene {
    /// A scene of the given triangles. `surfaces` are what the
    /// triangles' `surface` numbers point into; a triangle with a
    /// number beyond the list shows in a plain grey.
    pub fn new(triangles: Vec<Triangle>, surfaces: Vec<Surface>) -> Self {
        let (bvh, around) = bvh::Bvh::build(&triangles);
        let (min, max) = around.unwrap_or((Vec3A::ZERO, Vec3A::ZERO));
        let reach = min.abs().max(max.abs()).max_element();
        let radius = 0.5 * (max - min).length();
        Self {
            triangles,
            surfaces,
            bvh,
            bounds: around.map(|(min, max)| (min.into(), max.into())),
            centre: (min + max) * 0.5,
            radius: if radius > 0.0 { radius } else { 1.0 },
            low: min.z,
            nudge: if reach > 0.0 { reach * 1e-5 } else { 1e-5 },
        }
    }

    /// The lowest and the highest corner of the box around the scene,
    /// or `None` when the scene is empty.
    pub fn bounds(&self) -> Option<(Vec3, Vec3)> {
        self.bounds
    }
}

/// Where the picture is taken from.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    /// Where the eye is.
    pub eye: Vec3,
    /// The direction to the right in the picture, of length one.
    pub right: Vec3,
    /// The direction upwards in the picture, of length one.
    pub up: Vec3,
    /// The direction from the scene towards the eye, of length one and
    /// square to the other two.
    pub back: Vec3,
    /// With perspective: the height of the picture one unit in front
    /// of the eye (twice the tangent of half the angle of view).
    /// Without: the height of the picture in the scene's units.
    pub height: f32,
    /// Whether the picture has no perspective: all rays run alongside
    /// each other.
    pub orthographic: bool,
}

/// What surrounds the scene and lights it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sky {
    /// A photo studio: soft, neutral light from all around, brightest
    /// overhead, and a large soft key light.
    Studio,
    /// A clear day: a blue sky and a sun that casts crisp shadows.
    Outdoor,
    /// A dark room with one key light and a faint fill.
    Dark,
}

/// How a scene is lit.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Lighting {
    /// What surrounds the scene.
    pub sky: Sky,
    /// Where the key light (the sun, outdoors) stands, in degrees: its
    /// azimuth, turning from +x towards +y, and its elevation above
    /// the horizon.
    pub sun: [f32; 2],
    /// How bright the picture is. With 1 a mid-grey part comes out
    /// well exposed.
    pub strength: f32,
    /// Whether the scene stands on a matte floor, at its lowest z,
    /// that catches its shadows.
    pub ground: bool,
    /// The colour behind the model, as linear red, green and blue. The
    /// floor has it too, so that it fades into the background.
    pub backdrop: [f32; 3],
}

/// Adds one more sample per pixel to the film, working on all cores.
/// Each sample looks through a random point of its pixel, which
/// smooths the edges. The same scene, camera, lighting and number of
/// passes so far always give the same picture.
pub fn pass(scene: &Scene, camera: &Camera, lighting: &Lighting, film: &mut Film) {
    let (width, height) = (film.width, film.height);
    if width > 0 && height > 0 {
        let frame = trace::Frame::new(scene, camera, lighting, width, height, film.passes);
        // Bands of a few rows are handed out to the threads one by
        // one, so that none idles while another has the hard rows.
        const ROWS: usize = 4;
        let bands = Mutex::new(film.sums.chunks_mut(width * ROWS).enumerate());
        let threads = std::thread::available_parallelism()
            .map_or(1, usize::from)
            .min(height.div_ceil(ROWS));
        let work = || {
            loop {
                let next = bands.lock().unwrap_or_else(PoisonError::into_inner).next();
                let Some((band, pixels)) = next else {
                    break;
                };
                for (i, sum) in pixels.iter_mut().enumerate() {
                    let seen = frame.sample(i % width, band * ROWS + i / width);
                    for (total, part) in sum.iter_mut().zip(seen) {
                        *total += part;
                    }
                }
            }
        };
        std::thread::scope(|scope| {
            for _ in 1..threads {
                scope.spawn(work);
            }
            work();
        });
    }
    film.passes += 1;
}

#[cfg(test)]
mod tests;
