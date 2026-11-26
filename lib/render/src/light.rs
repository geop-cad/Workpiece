//! The light of a picture: a sky all around, and one key light in it
//! that casts the shadows.

use std::f32::consts::PI;

use glam::Vec3A;

use crate::material::along;
use crate::pattern::smoothstep;
use crate::{Lighting, Sky};

/// The sky and the key light of one picture, worked out once.
pub(crate) struct Light {
    sky: Sky,
    /// Where the key light stands.
    pub key: Vec3A,
    /// Two directions square to the key light's.
    key_along: [Vec3A; 2],
    /// The cosine of the key light's angular radius.
    key_cos: f32,
    /// How bright the key light's disc is.
    pub key_glow: Vec3A,
    /// How likely each direction of the disc is picked, per solid angle.
    pub key_chance: f32,
    /// Where the two bright bands of the studio stand, around the
    /// horizon.
    bands: [(f32, f32); 2],
    /// Where the faint fill of the dark sky comes from.
    fill: Vec3A,
    /// What an open, level, white floor sends back up under this light.
    pub open: Vec3A,
}

fn direction(azimuth: f32, elevation: f32) -> Vec3A {
    let (a, e) = (azimuth.to_radians(), elevation.to_radians());
    Vec3A::new(
        libm::cosf(e) * libm::cosf(a),
        libm::cosf(e) * libm::sinf(a),
        libm::sinf(e),
    )
}

impl Light {
    pub(crate) fn new(lighting: &Lighting) -> Self {
        let [azimuth, elevation] = lighting.sun;
        let key = direction(azimuth, elevation.clamp(-90.0, 90.0));
        // The key light's radius in degrees, its colour, and the light
        // it throws on a surface that faces it.
        let (radius, colour, power) = match lighting.sky {
            Sky::Studio => (12.0f32, Vec3A::ONE, 3.4),
            Sky::Outdoor => (2.0, Vec3A::new(1.0, 0.94, 0.84), 3.0),
            Sky::Dark => (5.0, Vec3A::new(1.0, 0.97, 0.93), 3.4),
        };
        let key_cos = libm::cosf(radius.to_radians());
        let angle = 2.0 * PI * (1.0 - key_cos);
        let band = |turn: f32| {
            let d = direction(azimuth + turn, 0.0);
            (d.x, d.y)
        };
        let mut light = Self {
            sky: lighting.sky,
            key,
            key_along: along(key),
            key_cos,
            key_glow: colour * (power / angle),
            key_chance: 1.0 / angle,
            bands: [band(115.0), band(-140.0)],
            fill: direction(azimuth + 180.0, 35.0),
            open: Vec3A::ONE,
        };
        light.open = light.level();
        light
    }

    /// What an open, level, white floor sends back up: the light of
    /// the whole sky and the key light on it, over pi.
    fn level(&self) -> Vec3A {
        const RINGS: usize = 16;
        const TURNS: usize = 48;
        let mut sum = Vec3A::ZERO;
        for i in 0..RINGS {
            let z = (i as f32 + 0.5) / RINGS as f32;
            let r = (1.0 - z * z).sqrt();
            for j in 0..TURNS {
                let turn = 2.0 * PI * (j as f32 + 0.5) / TURNS as f32;
                sum += self.sky(Vec3A::new(r * libm::cosf(turn), r * libm::sinf(turn), z)) * z;
            }
        }
        let sky = sum * (2.0 / (RINGS * TURNS) as f32);
        const SPOTS: usize = 64;
        let mut up = 0.0;
        for i in 0..SPOTS {
            let u = (i as f32 + 0.5) / SPOTS as f32;
            up += self
                .key_sample(u, (i as f32 * 0.618_034).fract())
                .z
                .max(0.0);
        }
        let key = self.key_glow * (up / SPOTS as f32 / self.key_chance / PI);
        (sky + key).max(Vec3A::splat(1e-3))
    }

    /// The light of the sky from a direction, without the key light.
    pub(crate) fn sky(&self, dir: Vec3A) -> Vec3A {
        let z = dir.z;
        match self.sky {
            Sky::Studio => {
                // A dome that is brightest overhead, with two broad
                // bright bands standing around the model.
                let dome = if z >= 0.0 {
                    0.3 + 0.35 * z
                } else {
                    0.3 + 0.2 * z
                };
                let flat = (dir.x * dir.x + dir.y * dir.y).sqrt().max(1e-6);
                let (x, y) = (dir.x / flat, dir.y / flat);
                let rise = smoothstep(0.1, 0.28, z) * (1.0 - smoothstep(0.62, 0.82, z));
                let [a, b] = self.bands;
                let bands = 1.3 * smoothstep(0.55, 0.85, x * a.0 + y * a.1)
                    + 0.8 * smoothstep(0.7, 0.9, x * b.0 + y * b.1);
                Vec3A::splat(dome + rise * bands)
            }
            Sky::Outdoor => {
                let horizon = Vec3A::new(0.78, 0.86, 0.98);
                let zenith = Vec3A::new(0.20, 0.38, 0.85);
                let earth = Vec3A::new(0.26, 0.24, 0.21);
                let sky = if z >= 0.0 {
                    horizon.lerp(zenith, z.sqrt())
                } else {
                    horizon.lerp(earth, smoothstep(0.0, 0.15, -z))
                };
                let near = dir.dot(self.key).max(0.0);
                let near = near * near;
                let near = near * near;
                let near = near * near;
                sky * 0.85 + Vec3A::new(1.0, 0.9, 0.7) * (0.25 * near * near * near * near)
            }
            Sky::Dark => {
                let fill = smoothstep(0.2, 1.0, dir.dot(self.fill));
                Vec3A::splat(0.012) + Vec3A::new(0.10, 0.12, 0.16) * fill
            }
        }
    }

    /// Whether a direction points at the key light's disc.
    pub(crate) fn at_key(&self, dir: Vec3A) -> bool {
        dir.dot(self.key) >= self.key_cos
    }

    /// A direction to a point of the key light's disc, each as likely
    /// as the other.
    pub(crate) fn key_sample(&self, u1: f32, u2: f32) -> Vec3A {
        let cos = 1.0 - u1 * (1.0 - self.key_cos);
        let sin = (1.0 - cos * cos).max(0.0).sqrt();
        let turn = 2.0 * PI * u2;
        self.key_along[0] * (sin * libm::cosf(turn))
            + self.key_along[1] * (sin * libm::sinf(turn))
            + self.key * cos
    }
}
