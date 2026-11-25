//! Following the light backwards: a ray leaves the eye, bounces from
//! surface to surface, and gathers the light it finds on the way.

use glam::Vec3A;

use crate::film::Sum;
use crate::light::Light;
use crate::material::Bsdf;
use crate::pattern::{factor, smoothstep};
use crate::{Camera, Lighting, Scene, Surface};

/// The most surfaces a ray visits.
const BOUNCES: u32 = 8;
/// After this many bounces, rays that carry little light are dropped
/// at random.
const SURE_BOUNCES: u32 = 3;
/// The most light a path may bring in over more than one bounce. It
/// keeps rare, very bright paths from leaving sparks in the picture.
const MOST: f32 = 3.0;
/// The same for the light on the floor, where 1 is the backdrop.
const MOST_FLOOR: f32 = 2.0;
/// The most a shiny surface is blurred for a path that has left a
/// rougher one before.
const BLUR: f32 = 0.3;
/// How bright the picture is for a strength of 1.
const EXPOSURE: f32 = 0.55;
/// What a surface without an entry in the scene's list looks like.
const PLAIN: Surface = Surface {
    colour: [0.6, 0.6, 0.6],
    roughness: 0.6,
    metallic: 0.0,
    transparency: 0.0,
    texture: 0,
    scale: 1.0,
};

/// A source of random numbers, small and fast (PCG).
struct Random(u64);

impl Random {
    /// The numbers for one pixel in one pass; always the same ones.
    fn new(pixel: u64, pass: u32) -> Self {
        let mut z = (pixel << 24 ^ u64::from(pass)).wrapping_add(0x9E37_79B9_7F4A_7C15);
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        Self(z ^ (z >> 31))
    }

    /// A number from 0 up to, but not including, 1.
    fn next(&mut self) -> f32 {
        let old = self.0;
        self.0 = old
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        let word = (((old >> 18) ^ old) >> 27) as u32;
        let word = word.rotate_right((old >> 59) as u32);
        (word >> 8) as f32 * (1.0 / 16_777_216.0)
    }
}

/// What one ray from the eye brought back.
#[derive(Default)]
struct Seen {
    /// The light of the model.
    model: Vec3A,
    /// Whether the ray met the model.
    met: bool,
    /// The light of the backdrop and the floor.
    back: Vec3A,
}

/// How much two ways of finding the same light count against each
/// other: the likelier way counts more.
fn share(this: f32, other: f32) -> f32 {
    if this.is_infinite() {
        1.0
    } else {
        let (a, b) = (this * this, other * other);
        if a + b > 0.0 { a / (a + b) } else { 0.0 }
    }
}

/// All that one pass needs, worked out once.
pub(crate) struct Frame<'a> {
    scene: &'a Scene,
    light: Light,
    eye: Vec3A,
    right: Vec3A,
    up: Vec3A,
    back: Vec3A,
    orthographic: bool,
    /// One over the width and the height of the film.
    pixel: (f32, f32),
    width: usize,
    pass: u32,
    exposure: f32,
    backdrop: Vec3A,
    /// The height of the floor, when there is one.
    floor: Option<f32>,
    /// The floor's colour for light that bounces off it.
    floor_colour: Vec3A,
    /// What the floor's light is multiplied by where the eye sees it,
    /// so that open floor shows exactly the backdrop.
    floor_gain: Vec3A,
}

impl<'a> Frame<'a> {
    pub(crate) fn new(
        scene: &'a Scene,
        camera: &Camera,
        lighting: &Lighting,
        width: usize,
        height: usize,
        pass: u32,
    ) -> Self {
        let light = Light::new(lighting);
        let backdrop = Vec3A::from_array(lighting.backdrop).max(Vec3A::ZERO);
        let back = Vec3A::from(camera.back);
        let mut eye = Vec3A::from(camera.eye);
        if camera.orthographic {
            // Rays start behind the whole scene, wherever the eye is.
            let behind = (scene.centre - eye).dot(back) + scene.radius * 1.01;
            eye += back * behind.max(0.0);
        }
        let aspect = width as f32 / height as f32;
        Self {
            scene,
            eye,
            right: Vec3A::from(camera.right) * (camera.height * aspect),
            up: Vec3A::from(camera.up) * camera.height,
            back,
            orthographic: camera.orthographic,
            pixel: (1.0 / width as f32, 1.0 / height as f32),
            width,
            pass,
            exposure: lighting.strength.max(0.0) * EXPOSURE,
            backdrop,
            floor: (lighting.ground && scene.bounds.is_some())
                .then_some(scene.low - 4.0 * scene.nudge),
            floor_colour: backdrop.min(Vec3A::splat(0.9)),
            floor_gain: backdrop / light.open,
            light,
        }
    }

    /// One more sample of a pixel.
    pub(crate) fn sample(&self, x: usize, y: usize) -> Sum {
        let mut random = Random::new((y * self.width + x) as u64, self.pass);
        let u = (x as f32 + random.next()) * self.pixel.0 - 0.5;
        let v = 0.5 - (y as f32 + random.next()) * self.pixel.1;
        let aim = self.right * u + self.up * v;
        let (origin, dir) = if self.orthographic {
            (self.eye + aim, -self.back)
        } else {
            (self.eye, (aim - self.back).normalize())
        };
        let seen = self.path(origin, dir, &mut random);
        let (m, b) = (seen.model, seen.back);
        [
            m.x,
            m.y,
            m.z,
            if seen.met { 1.0 } else { 0.0 },
            b.x,
            b.y,
            b.z,
        ]
    }

    /// Adds light a path found to what the ray brings back. `floor`
    /// tells that the eye sees it on the floor, and `far` that it came
    /// over more than one bounce.
    fn add(&self, seen: &mut Seen, light: Vec3A, floor: bool, far: bool) {
        let (mut light, most) = if floor {
            (light * self.floor_gain, MOST_FLOOR)
        } else {
            (light * self.exposure, MOST)
        };
        if !light.is_finite() {
            return;
        }
        let top = light.max_element();
        if far && top > most {
            light *= most / top;
        }
        if floor {
            seen.back += light;
        } else {
            seen.model += light;
        }
    }

    /// How much of the key light reaches a point from a direction:
    /// nothing behind an opaque surface, and a tinted part of it
    /// through glass.
    fn reaches(&self, origin: Vec3A, dir: Vec3A) -> Vec3A {
        if let Some(floor) = self.floor
            && dir.z < 0.0
            && origin.z > floor
        {
            return Vec3A::ZERO;
        }
        let scene = self.scene;
        let mut through = Vec3A::ONE;
        scene.bvh.each(origin, dir, f32::INFINITY, |slot| {
            let surface = scene.surface(scene.triangles[scene.bvh.id(slot)].surface);
            let clear = surface.transparency.clamp(0.0, 1.0);
            if clear <= 0.0 {
                through = Vec3A::ZERO;
                return false;
            }
            let tint = Vec3A::from_array(surface.colour).clamp(Vec3A::ZERO, Vec3A::ONE);
            through *= Vec3A::new(tint.x.sqrt(), tint.y.sqrt(), tint.z.sqrt()) * (0.92 * clear);
            true
        });
        through
    }

    /// Follows one ray from the eye.
    fn path(&self, mut origin: Vec3A, mut dir: Vec3A, random: &mut Random) -> Seen {
        let scene = self.scene;
        let light = &self.light;
        let mut seen = Seen::default();
        // What the light found from here on is multiplied by.
        let mut carry = Vec3A::ONE;
        // Whether the eye sees the floor along this path.
        let mut on_floor = false;
        // Whether the ray has only gone straight through glass so far,
        // so that it still shows what the eye looks at.
        let mut straight = true;
        // Whether the ray is within glass.
        let mut inside = false;
        // Whether the key light must not be counted where the ray
        // leaves the scene, because it was counted before.
        let mut key_counted = false;
        // How likely the last bounce was to take the way it took.
        let mut chance = f32::INFINITY;
        // How widely shiny surfaces spread light at least. It grows
        // once the path has left a matte or rough surface: what a
        // mirror or glass throws on those is found far more surely
        // when it is blurred a little, and the blur does not show.
        let mut least = 0.0f32;

        for bounce in 0..BOUNCES {
            let hit = scene.bvh.nearest(origin, dir, f32::INFINITY);
            let mut floor_at = None;
            if let Some(floor) = self.floor
                && dir.z < 0.0
                && origin.z > floor
            {
                let t = (floor - origin.z) / dir.z;
                if hit.is_none_or(|hit| t < hit.t) {
                    floor_at = Some((t, floor));
                }
            }
            let out = -dir;
            let (point, flat, bsdf) = if let Some((t, floor)) = floor_at {
                let mut point = origin + dir * t;
                point.z = floor;
                let bsdf = if straight {
                    // Far from the model the floor fades into the
                    // backdrop, so that no horizon shows.
                    let away = (point - scene.centre).truncate().length();
                    let fade = smoothstep(1.5 * scene.radius, 4.0 * scene.radius, away);
                    if fade >= 1.0 || random.next() < fade {
                        seen.back += carry * self.backdrop;
                        break;
                    }
                    on_floor = true;
                    Bsdf::matte(Vec3A::Z, Vec3A::ONE)
                } else {
                    Bsdf::matte(Vec3A::Z, self.floor_colour)
                };
                (point, Vec3A::Z, bsdf)
            } else if let Some(hit) = hit {
                if bounce == 0 {
                    seen.met = true;
                }
                let [corner, e1, e2] = scene.bvh.corner(hit.slot);
                let triangle = &scene.triangles[scene.bvh.id(hit.slot)];
                let point = corner + e1 * hit.u + e2 * hit.v;
                let true_normal = e1.cross(e2).normalize_or_zero();
                let flat = if true_normal.dot(out) < 0.0 {
                    -true_normal
                } else {
                    true_normal
                };
                let [n0, n1, n2] = triangle.normals.map(Vec3A::from);
                let mut normal =
                    (n0 * (1.0 - hit.u - hit.v) + n1 * hit.u + n2 * hit.v).normalize_or_zero();
                if !normal.is_finite() || normal == Vec3A::ZERO {
                    normal = flat;
                }
                if normal.dot(flat) < 0.0 {
                    normal = -normal;
                }
                // A smoothed normal may face away from the eye near
                // the outline; lean it back until it faces the eye.
                let facing = normal.dot(out);
                if facing < 0.02 {
                    normal = (normal + out * (0.02 - facing)).normalize_or(flat);
                }
                let surface = scene.surface(triangle.surface);
                let colour = Vec3A::from_array(surface.colour)
                    * factor(surface.texture, surface.scale, point, true_normal);
                let colour = colour.clamp(Vec3A::ZERO, Vec3A::ONE);
                let glass = surface.transparency > 0.0 && random.next() < surface.transparency;
                (
                    point,
                    flat,
                    Bsdf::of(surface, colour, normal, out, glass, inside, least),
                )
            } else {
                if straight {
                    seen.back += carry * self.backdrop;
                } else {
                    let mut sky = light.sky(dir);
                    if !key_counted && light.at_key(dir) {
                        sky += light.key_glow * share(chance, light.key_chance);
                    }
                    self.add(&mut seen, carry * sky, on_floor, bounce >= 2);
                }
                break;
            };

            // The key light, asked for directly.
            let ask = !bsdf.sharp && !(bsdf.glass && inside);
            if ask {
                let to_key = light.key_sample(random.next(), random.next());
                if to_key.dot(flat) > 0.0 {
                    let (value, chance) = bsdf.eval(out, to_key);
                    if chance > 0.0 && value.max_element() > 0.0 {
                        let through = self.reaches(point + flat * scene.nudge, to_key);
                        if through.max_element() > 0.0 {
                            let weight = share(light.key_chance, chance) / light.key_chance;
                            let found = carry * value * through * light.key_glow * weight;
                            self.add(&mut seen, found, on_floor, bounce >= 1);
                        }
                    }
                }
            }

            let Some(next) = bsdf.sample(out, flat, [random.next(), random.next(), random.next()])
            else {
                break;
            };
            carry *= next.weight;
            least = least.max(next.spread.min(BLUR));
            if next.through {
                inside = !inside;
                key_counted = true;
            } else {
                key_counted = false;
                straight = false;
            }
            chance = if ask { next.chance } else { f32::INFINITY };
            if bounce >= SURE_BOUNCES {
                let keep = carry.max_element().min(0.95);
                if keep.is_nan() || keep <= 0.0 || random.next() >= keep {
                    break;
                }
                carry /= keep;
            }
            let side = if next.through { -flat } else { flat };
            origin = point + side * scene.nudge;
            dir = next.dir;
        }
        seen
    }
}

impl Scene {
    /// The surface with a number, or a plain grey one when the scene
    /// has none with that number.
    fn surface(&self, index: u32) -> &Surface {
        self.surfaces.get(index as usize).unwrap_or(&PLAIN)
    }
}
