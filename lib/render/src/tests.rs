use glam::{Vec3, Vec3A};

use super::*;

const MATTE: Surface = Surface {
    colour: [0.8, 0.8, 0.8],
    roughness: 1.0,
    metallic: 0.0,
    transparency: 0.0,
    texture: 0,
    scale: 1.0,
};

/// A box from `min` to `max`, as twelve triangles.
fn cuboid(min: Vec3, max: Vec3) -> Vec<Triangle> {
    let corner = |i: usize| {
        Vec3::new(
            if i & 1 == 0 { min.x } else { max.x },
            if i & 2 == 0 { min.y } else { max.y },
            if i & 4 == 0 { min.z } else { max.z },
        )
    };
    let faces = [
        ([0, 2, 3, 1], Vec3::NEG_Z),
        ([4, 5, 7, 6], Vec3::Z),
        ([0, 1, 5, 4], Vec3::NEG_Y),
        ([2, 6, 7, 3], Vec3::Y),
        ([0, 4, 6, 2], Vec3::NEG_X),
        ([1, 3, 7, 5], Vec3::X),
    ];
    let mut triangles = Vec::new();
    for (quad, normal) in faces {
        for [a, b, c] in [[0, 1, 2], [0, 2, 3]] {
            triangles.push(Triangle {
                positions: [corner(quad[a]), corner(quad[b]), corner(quad[c])],
                normals: [normal; 3],
                surface: 0,
            });
        }
    }
    triangles
}

/// A camera that looks straight down, without perspective, on a square
/// `size` wide around the origin.
fn from_above(size: f32) -> Camera {
    Camera {
        eye: Vec3::new(0.0, 0.0, 10.0),
        right: Vec3::X,
        up: Vec3::Y,
        back: Vec3::Z,
        height: size,
        orthographic: true,
    }
}

fn studio(backdrop: [f32; 3]) -> Lighting {
    Lighting {
        sky: Sky::Studio,
        sun: [0.0, 30.0],
        strength: 1.0,
        ground: true,
        backdrop,
    }
}

/// A sequence of numbers from 0 to 1 that looks random.
fn numbers(mut seed: u32) -> impl FnMut() -> f32 {
    move || {
        seed = seed.wrapping_mul(747_796_405).wrapping_add(2_891_336_453);
        let word = ((seed >> ((seed >> 28) + 4)) ^ seed).wrapping_mul(277_803_737);
        ((word >> 22) ^ word) as f32 / u32::MAX as f32
    }
}

#[test]
fn an_empty_scene_shows_the_backdrop() {
    let scene = Scene::new(Vec::new(), Vec::new());
    assert_eq!(scene.bounds(), None);
    let mut camera = from_above(4.0);
    for orthographic in [true, false] {
        camera.orthographic = orthographic;
        let mut film = Film::new(16, 8);
        pass(&scene, &camera, &studio([1.0, 0.5, 0.0]), &mut film);
        pass(&scene, &camera, &studio([1.0, 0.5, 0.0]), &mut film);
        for pixel in film.rgba().chunks(4) {
            assert_eq!(pixel, [255, 188, 0, 255]);
        }
    }
}

#[test]
fn a_box_casts_a_shadow_on_the_floor() {
    let scene = Scene::new(
        cuboid(Vec3::new(-1.0, -1.0, 0.0), Vec3::new(1.0, 1.0, 1.0)),
        vec![MATTE],
    );
    let mut film = Film::new(32, 32);
    for _ in 0..16 {
        pass(
            &scene,
            &from_above(8.0),
            &studio([0.5, 0.5, 0.5]),
            &mut film,
        );
    }
    let rgba = film.rgba();
    // The brightness around a point of the floor plane, from 0 to 255.
    let around = |x: f32, y: f32| {
        let (px, py) = (
            ((x / 8.0 + 0.5) * 32.0) as usize,
            ((0.5 - y / 8.0) * 32.0) as usize,
        );
        let mut sum = 0.0;
        for row in py - 1..=py + 1 {
            for column in px - 1..=px + 1 {
                let at = (row * 32 + column) * 4;
                sum += f32::from(rgba[at]) + f32::from(rgba[at + 1]) + f32::from(rgba[at + 2]);
            }
        }
        sum / 27.0
    };
    // The light comes from +x, so the shadow falls towards -x.
    let (top, shadow, open) = (around(0.0, 0.0), around(-1.8, 0.0), around(0.0, 2.0));
    assert!(top > shadow + 20.0, "top {top}, shadow {shadow}");
    assert!(open > shadow + 20.0, "open {open}, shadow {shadow}");
    assert!(top > 120.0 && top < 250.0, "top {top}");
    assert!(shadow > 5.0, "shadow {shadow}");
    // Open floor shows the backdrop: half grey is 188 on a screen.
    assert!((open - 188.0).abs() < 12.0, "open {open}");
}

#[test]
fn the_tree_finds_what_trying_every_triangle_finds() {
    let mut next = numbers(7);
    let mut point = |size: f32| Vec3::new(next() - 0.5, next() - 0.5, next() - 0.5) * size;
    let triangles: Vec<Triangle> = (0..400)
        .map(|_| {
            let at = point(20.0);
            Triangle {
                positions: [at + point(3.0), at + point(3.0), at + point(3.0)],
                normals: [Vec3::Z; 3],
                surface: 0,
            }
        })
        .collect();
    let scene = Scene::new(triangles, vec![MATTE]);
    let mut hits = 0;
    for _ in 0..1000 {
        let origin = Vec3A::from(point(40.0));
        let dir = (Vec3A::from(point(12.0)) - origin).normalize();
        let fast = scene.bvh.nearest(origin, dir, f32::INFINITY);
        let slow = scene.bvh.nearest_slow(origin, dir);
        assert_eq!(
            fast.map(|hit| (hit.t, hit.slot)),
            slow.map(|hit| (hit.t, hit.slot))
        );
        hits += usize::from(fast.is_some());
        // Whatever stands in a ray's way is also found without order.
        let mut any = false;
        scene.bvh.each(origin, dir, f32::INFINITY, |_| {
            any = true;
            false
        });
        assert_eq!(any, slow.is_some());
    }
    assert!(hits > 200, "only {hits} rays hit anything");
}

#[test]
fn the_same_passes_give_the_same_picture() {
    let scene = Scene::new(
        cuboid(Vec3::new(-1.0, -1.0, 0.0), Vec3::new(1.0, 1.0, 1.0)),
        vec![MATTE],
    );
    let camera = Camera {
        eye: Vec3::new(0.0, -6.0, 2.0),
        right: Vec3::X,
        up: Vec3::new(0.0, 0.316_227_77, 0.948_683_3),
        back: Vec3::new(0.0, -0.948_683_3, 0.316_227_77),
        height: 1.0,
        orthographic: false,
    };
    let lighting = Lighting {
        sky: Sky::Outdoor,
        ..studio([0.3, 0.3, 0.3])
    };
    let picture = || {
        let mut film = Film::new(32, 24);
        pass(&scene, &camera, &lighting, &mut film);
        pass(&scene, &camera, &lighting, &mut film);
        assert_eq!(film.passes(), 2);
        film.rgba()
    };
    let first = picture();
    assert_eq!(first, picture());
    // The box is in the picture, lit.
    let middle = (12 * 32 + 16) * 4;
    assert!(first[middle] > 60, "{}", first[middle]);
}
