//! How a surface sends light on: a matte part that scatters it all
//! ways, a shiny part that mirrors it more or less sharply, and for
//! glass a part that lets it through, bent.

use std::f32::consts::PI;

use glam::Vec3A;

use crate::Surface;

/// How much more slowly light travels in glass than in air.
const GLASS: f32 = 1.5;
/// What a surface that is no metal mirrors when seen head-on.
const PLAIN_MIRROR: f32 = 0.04;

/// A surface at one point, as seen from one direction.
pub(crate) struct Bsdf {
    /// The normal light is shaded with, on the viewer's side.
    normal: Vec3A,
    /// Two directions along the surface, square to each other.
    along: [Vec3A; 2],
    /// The colour of the matte part.
    matte: Vec3A,
    /// What the shiny part mirrors when seen head-on.
    mirror: Vec3A,
    /// Whether there is a shiny part at all.
    shiny: bool,
    /// How wide the shiny part spreads (the roughness, squared).
    spread: f32,
    /// How often the shiny part is the one tried.
    try_shiny: f32,
    /// Whether this is glass: light is mirrored or goes through.
    pub glass: bool,
    /// The bending of light that goes through.
    bend: f32,
    /// What light that goes through is tinted by.
    tint: Vec3A,
    /// Whether the surface mirrors so sharply that only the mirrored
    /// direction itself matters.
    pub sharp: bool,
}

/// Where a ray goes on from a surface.
pub(crate) struct Bounce {
    /// The new direction.
    pub dir: Vec3A,
    /// What the light along the path is multiplied by.
    pub weight: Vec3A,
    /// How likely this direction was, per solid angle.
    pub chance: f32,
    /// Whether the ray went through the surface.
    pub through: bool,
    /// How widely the part of the surface that sent the ray on
    /// spreads light: 1 for the matte part.
    pub spread: f32,
}

fn luminance(c: Vec3A) -> f32 {
    c.dot(Vec3A::new(0.2126, 0.7152, 0.0722))
}

/// Fresnel's rise of the mirror towards grazing angles, after Schlick.
fn rise(cos: f32) -> f32 {
    let m = (1.0 - cos).clamp(0.0, 1.0);
    let m2 = m * m;
    m2 * m2 * m
}

/// How much of the light glass mirrors, for light that meets it at
/// the given cosine; `bend` is the speed of light beyond the surface
/// over that before it.
fn glass_mirror(cos: f32, bend: f32) -> f32 {
    let sin2 = bend * bend * (1.0 - cos * cos);
    if sin2 >= 1.0 {
        return 1.0;
    }
    let beyond = (1.0 - sin2).sqrt();
    let s = (bend * cos - beyond) / (bend * cos + beyond);
    let p = (cos - bend * beyond) / (cos + bend * beyond);
    0.5 * (s * s + p * p)
}

impl Bsdf {
    /// A matte surface of one colour.
    pub(crate) fn matte(normal: Vec3A, colour: Vec3A) -> Self {
        Self {
            normal,
            along: along(normal),
            matte: colour,
            mirror: Vec3A::ZERO,
            shiny: false,
            spread: 1.0,
            try_shiny: 0.0,
            glass: false,
            bend: 1.0,
            tint: Vec3A::ONE,
            sharp: false,
        }
    }

    /// A surface of the scene, seen from `out` (which points away from
    /// it, on the side of `normal`). `glass` tells whether this ray
    /// takes it as glass, and `inside` whether the ray is within the
    /// glass. The shiny part spreads light at least as widely as
    /// `least`.
    pub(crate) fn of(
        surface: &Surface,
        colour: Vec3A,
        normal: Vec3A,
        out: Vec3A,
        glass: bool,
        inside: bool,
        least: f32,
    ) -> Self {
        let roughness = surface.roughness.clamp(0.0, 1.0);
        let spread = (roughness * roughness).max(least).max(1e-3);
        let mut bsdf = Self::matte(normal, Vec3A::ZERO);
        bsdf.shiny = true;
        bsdf.spread = spread;
        if glass {
            bsdf.glass = true;
            bsdf.try_shiny = 1.0;
            bsdf.bend = if inside { GLASS } else { 1.0 / GLASS };
            bsdf.tint = if inside { Vec3A::ONE } else { colour };
            bsdf.sharp = spread < 0.002;
            return bsdf;
        }
        let metallic = surface.metallic.clamp(0.0, 1.0);
        let facing = normal.dot(out);
        bsdf.mirror = Vec3A::splat(PLAIN_MIRROR).lerp(colour, metallic);
        bsdf.matte = colour
            * ((1.0 - metallic) * (1.0 - PLAIN_MIRROR - (1.0 - PLAIN_MIRROR) * rise(facing)));
        let matte = luminance(bsdf.matte);
        let shiny = luminance(bsdf.mirror + (Vec3A::ONE - bsdf.mirror) * rise(facing));
        bsdf.try_shiny = if matte > 0.0 {
            (shiny / (shiny + matte)).clamp(0.1, 0.9)
        } else {
            1.0
        };
        bsdf.sharp = spread < 0.002 && matte <= 0.0;
        bsdf
    }

    /// The shadowing of the tiny facets of a rough surface, for a
    /// direction at the given cosine to the normal.
    fn hidden(&self, cos: f32) -> f32 {
        let c2 = (cos * cos).max(1e-8);
        0.5 * ((1.0 + self.spread * self.spread * (1.0 - c2) / c2).sqrt() - 1.0)
    }

    /// How much of the light from `light` the surface sends to `out`
    /// (times the cosine of the light's angle), and how likely
    /// [`Bsdf::sample`] is to pick `light`. For glass only the
    /// mirrored light counts.
    pub(crate) fn eval(&self, out: Vec3A, light: Vec3A) -> (Vec3A, f32) {
        let nl = self.normal.dot(light);
        let nv = self.normal.dot(out);
        if nl <= 0.0 || nv <= 0.0 {
            return (Vec3A::ZERO, 0.0);
        }
        let mut value = self.matte * (nl / PI);
        let mut chance = (1.0 - self.try_shiny) * nl / PI;
        if self.shiny {
            let half = (out + light).normalize_or_zero();
            let nh = self.normal.dot(half);
            let vh = out.dot(half);
            if nh > 0.0 && vh > 0.0 {
                let a2 = self.spread * self.spread;
                let d = nh * nh * (a2 - 1.0) + 1.0;
                let facets = a2 / (PI * d * d) / (4.0 * nv);
                let hidden_out = self.hidden(nv);
                let both = 1.0 / (1.0 + hidden_out + self.hidden(nl));
                let one = 1.0 / (1.0 + hidden_out);
                if self.glass {
                    let mirrored = glass_mirror(vh, self.bend);
                    value += Vec3A::splat(mirrored * facets * both);
                    chance += mirrored * facets * one;
                } else {
                    let mirrored = self.mirror + (Vec3A::ONE - self.mirror) * rise(vh);
                    value += mirrored * (facets * both);
                    chance += self.try_shiny * facets * one;
                }
            }
        }
        (value, chance)
    }

    /// Picks where a ray that came along `out` goes on, from three
    /// random numbers. `flat` is the true normal of the surface on the
    /// side of `out`; a ray never leaves through it unless it goes
    /// through glass.
    pub(crate) fn sample(&self, out: Vec3A, flat: Vec3A, random: [f32; 3]) -> Option<Bounce> {
        if self.glass {
            let facet = self.facet(out, random[1], random[2]);
            let cos = out.dot(facet);
            if cos <= 0.0 {
                return None;
            }
            let hidden_out = self.hidden(self.normal.dot(out));
            let shadow = |dir: Vec3A| {
                let cos = self.normal.dot(dir).abs().max(1e-4);
                (1.0 + hidden_out) / (1.0 + hidden_out + self.hidden(cos))
            };
            if random[0] < glass_mirror(cos, self.bend) {
                let dir = above(facet * (2.0 * cos) - out, flat);
                let chance = self.eval(out, dir).1;
                return Some(Bounce {
                    dir,
                    weight: Vec3A::splat(shadow(dir)),
                    chance: if chance > 0.0 { chance } else { f32::INFINITY },
                    through: false,
                    spread: self.spread,
                });
            }
            let beyond = (1.0 - self.bend * self.bend * (1.0 - cos * cos))
                .max(0.0)
                .sqrt();
            let dir = (facet * (self.bend * cos - beyond) - out * self.bend).normalize_or_zero();
            if dir.dot(flat) >= 0.0 {
                return None;
            }
            return Some(Bounce {
                dir,
                weight: self.tint * shadow(dir),
                chance: 0.0,
                through: true,
                spread: self.spread,
            });
        }
        let mut spread = 1.0;
        let dir = if self.shiny && random[0] < self.try_shiny {
            let facet = self.facet(out, random[1], random[2]);
            spread = self.spread;
            facet * (2.0 * out.dot(facet)) - out
        } else {
            let r = random[1].sqrt();
            let turn = 2.0 * PI * random[2];
            self.along[0] * (r * turn.cos())
                + self.along[1] * (r * turn.sin())
                + self.normal * (1.0 - random[1]).max(0.0).sqrt()
        };
        let dir = above(dir, flat);
        let (value, chance) = self.eval(out, dir);
        (chance > 0.0).then(|| Bounce {
            dir,
            weight: value / chance,
            chance,
            through: false,
            spread,
        })
    }

    /// Picks one of the tiny facets of the rough surface that `out`
    /// can see, as likely as it is seen (after Heitz).
    fn facet(&self, out: Vec3A, u1: f32, u2: f32) -> Vec3A {
        let [tx, ty] = self.along;
        let a = self.spread;
        let v = Vec3A::new(a * out.dot(tx), a * out.dot(ty), out.dot(self.normal)).normalize();
        let len2 = v.x * v.x + v.y * v.y;
        let t1 = if len2 > 0.0 {
            Vec3A::new(-v.y, v.x, 0.0) / len2.sqrt()
        } else {
            Vec3A::X
        };
        let t2 = v.cross(t1);
        let r = u1.sqrt();
        let turn = 2.0 * PI * u2;
        let p1 = r * turn.cos();
        let s = 0.5 * (1.0 + v.z);
        let p2 = (1.0 - s) * (1.0 - p1 * p1).max(0.0).sqrt() + s * r * turn.sin();
        let n = t1 * p1 + t2 * p2 + v * (1.0 - p1 * p1 - p2 * p2).max(0.0).sqrt();
        let local = Vec3A::new(a * n.x, a * n.y, n.z.max(1e-6)).normalize();
        tx * local.x + ty * local.y + self.normal * local.z
    }
}

/// A direction that would dip below the surface, folded back above it.
fn above(dir: Vec3A, flat: Vec3A) -> Vec3A {
    let d = dir.dot(flat);
    if d < 0.0 { dir - flat * (2.0 * d) } else { dir }
}

/// Two directions square to a unit vector and to each other.
pub(crate) fn along(n: Vec3A) -> [Vec3A; 2] {
    let s = 1f32.copysign(n.z);
    let a = -1.0 / (s + n.z);
    let b = n.x * n.y * a;
    [
        Vec3A::new(1.0 + s * n.x * n.x * a, s * b, -s * n.x),
        Vec3A::new(b, s + n.y * n.y * a, -n.y),
    ]
}
