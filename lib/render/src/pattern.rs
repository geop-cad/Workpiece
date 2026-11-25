//! The patterns a surface can carry over its colour. They are the same
//! ones the rasterizer draws, so a part looks alike in both.

use glam::Vec3A;

fn fract(x: f32) -> f32 {
    x - x.floor()
}

fn mix(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Zero below `a`, one above `b`, and a smooth rise in between.
pub(crate) fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn hash2(x: f32, y: f32) -> f32 {
    fract((x * 127.1 + y * 311.7).sin() * 43_758.547)
}

fn noise2(x: f32, y: f32) -> f32 {
    let (ix, iy) = (x.floor(), y.floor());
    let (fx, fy) = (x - ix, y - iy);
    let ux = fx * fx * (3.0 - 2.0 * fx);
    let uy = fy * fy * (3.0 - 2.0 * fy);
    mix(
        mix(hash2(ix, iy), hash2(ix + 1.0, iy), ux),
        mix(hash2(ix, iy + 1.0), hash2(ix + 1.0, iy + 1.0), ux),
        uy,
    )
}

/// Whether the cell is an odd one of a checkerboard.
fn odd(x: f32, y: f32) -> bool {
    let s = x + y;
    s - 2.0 * (s * 0.5).floor() > 0.5
}

/// What a pattern multiplies the colour by at a point of its plane.
/// Kind 0 is no pattern, 1 brushed, 2 wood, 3 weave, 4 checker,
/// 5 speckle and anything else ribbed.
pub(crate) fn pattern(kind: u8, x: f32, y: f32) -> f32 {
    match kind {
        0 => 1.0,
        1 => 0.94 + 0.12 * noise2(x * 3.0, y * 160.0),
        2 => {
            let rings = fract(y * 6.0 + 0.6 * noise2(x * 1.5, y * 4.0));
            0.80 + 0.22 * smoothstep(0.0, 0.7, rings) * (1.0 - smoothstep(0.85, 1.0, rings))
                + 0.05 * noise2(x * 80.0, y * 8.0)
        }
        3 => {
            let (cx, cy) = ((x * 4.0).floor(), (y * 4.0).floor());
            let (fx, fy) = (fract(x * 4.0), fract(y * 4.0));
            let over = if odd(cx, cy) { 1.0 } else { 0.0 };
            let across = if over > 0.5 { fx } else { fy };
            0.75 + 0.45 * (across * std::f32::consts::PI).sin() * (0.8 + 0.2 * over)
        }
        4 => {
            if odd((x * 2.0).floor(), (y * 2.0).floor()) {
                1.08
            } else {
                0.80
            }
        }
        5 => 0.86 + 0.24 * noise2(x * 40.0, y * 40.0) * noise2(x * 13.0 + 7.0, y * 13.0 + 7.0),
        _ => 0.82 + 0.22 * smoothstep(0.2, 0.5, (fract(x * 4.0) - 0.5).abs() * 2.0),
    }
}

/// The pattern's factor at a point of a surface: the point is laid on
/// the axis plane the surface most nearly faces, and one repeat of the
/// pattern is `scale` long.
pub(crate) fn factor(kind: u8, scale: f32, point: Vec3A, normal: Vec3A) -> f32 {
    if kind == 0 || scale.is_nan() || scale <= 0.0 {
        return 1.0;
    }
    let n = normal.abs();
    let (x, y) = if n.z >= n.x && n.z >= n.y {
        (point.x, point.y)
    } else if n.y >= n.x {
        (point.x, point.z)
    } else {
        (point.y, point.z)
    };
    pattern(kind, x / scale, y / scale)
}
