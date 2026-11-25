//! What an edit's handles look like: their [pieces](workpiece_core::handles::shape)
//! as knobs, arrows and rings, the same size on screen wherever they are.
//! Each primitive picks its handle (and the gizmo part it moves).

use std::collections::BTreeMap;

use workpiece_core::Pose;
use workpiece_core::handles::shape::Shape;
use workpiece_core::handles::{
    Canvas, CanvasShape, CanvasView, Handle, Space, SpaceShape, SpaceView, Tone,
};
use workpiece_core::math::{Point2, Point3};

use crate::palette::{self, AXES, HANDLE};
use crate::{Color, Label, Line, Pick, PickId, Scene, Vertex};

const SHAFT_WIDTH: f32 = 3.0;
const ARROW_WIDTH: f32 = 12.0;
const RING_WIDTH: f32 = 2.6;
const KNOB_SIZE: f32 = 12.0;
const RING_SEGMENTS: usize = 64;
/// A canvas's dashes and the gaps between them, in pixels.
const DASH: f64 = 7.0;
const DASH_GAP: f64 = 4.0;
/// A canvas's arrowheads: long and slender, as on a drawing.
use workpiece_core::handles::{HEAD_LENGTH, HEAD_WIDTH};

/// How a canvas is seen through a camera whose screen-right and
/// screen-up directions are `right` and `up` (in the coordinates `pose`
/// places the canvas in), where a pixel at the canvas's anchor is
/// `pixel` long, and that looks at `middle`.
pub fn canvas_view<M: 'static>(
    canvas: &Canvas<M>,
    pose: Pose,
    right: Point3,
    up: Point3,
    pixel: f64,
    middle: Point3,
) -> CanvasView {
    let local = pose.inverse();
    CanvasView::through(
        canvas.plane.x_dir(),
        canvas.plane.y_dir(),
        local.transform_vector(right),
        local.transform_vector(up),
        pixel,
        canvas.locate(local.transform_point(middle)),
    )
}

/// The color of a canvas's `tone`.
fn tone_color(tone: Tone) -> Color {
    match tone {
        Tone::Free => palette::SKETCH_FREE,
        Tone::Constrained | Tone::Dimension => palette::SKETCH,
        Tone::Construction => palette::SKETCH_CONSTRUCTION,
        Tone::Projected => palette::SKETCH_PROJECTED,
        Tone::Hover => palette::HANDLE_HOVER,
        Tone::Selected => palette::SELECTION,
        Tone::Conflict => palette::CONFLICT,
        Tone::Preview => palette::PREVIEW,
        Tone::Badge => palette::BADGE,
    }
}

/// Adds what `canvas` draws in `view`.
fn canvas<M: 'static>(scene: &mut Scene, canvas: &Canvas<M>, pose: Pose, view: &CanvasView) {
    let at = |p: Point2| pose.transform_point(canvas.place(p)).as_vec3();
    let direction = |d: Point2| {
        let d = canvas.plane.x_dir() * d.x + canvas.plane.y_dir() * d.y;
        pose.transform_vector(d).as_vec3()
    };
    for shape in canvas.shapes(view) {
        match shape {
            CanvasShape::Polyline {
                points,
                tone,
                width,
                dashed,
            } => {
                let color = tone_color(tone);
                let mut line = |a: Point2, b: Point2| {
                    scene.lines.lines.push(Line {
                        a: at(a),
                        b: at(b),
                        width: [width; 2],
                        color,
                        pick: None,
                    });
                };
                // How far along the dash pattern the polyline is.
                let (dash, period) = (DASH * view.pixel, (DASH + DASH_GAP) * view.pixel);
                let mut along = 0.0;
                for pair in points.windows(2) {
                    let (a, b) = (pair[0], pair[1]);
                    let length = a.distance(b);
                    if !dashed || period <= 0.0 || length / period > 4000.0 {
                        line(a, b);
                        continue;
                    }
                    // The dashes [k·period, k·period + dash] that overlap
                    // this piece, [along, along + length].
                    let mut k = (along / period).floor();
                    while k * period < along + length {
                        let from = (k * period).max(along) - along;
                        let to = (k * period + dash).min(along + length) - along;
                        if to > from {
                            line(a.lerp(b, from / length), a.lerp(b, to / length));
                        }
                        k += 1.0;
                    }
                    along += length;
                }
            }
            CanvasShape::Dot { at: p, tone, size } => {
                scene.vertices.vertices.push(Vertex {
                    position: at(p),
                    size,
                    color: tone_color(tone),
                    pick: None,
                });
            }
            CanvasShape::Arrowhead {
                tip,
                direction: d,
                tone,
            } => {
                let base = tip - d * HEAD_LENGTH * view.pixel;
                scene.lines.lines.push(Line {
                    a: at(base),
                    b: at(tip),
                    width: [HEAD_WIDTH as f32, 0.0],
                    color: tone_color(tone),
                    pick: None,
                });
            }
            CanvasShape::Text {
                at: p,
                along,
                text,
                upper,
                lower,
                tone,
                badge,
            } => {
                scene.labels.push(Label {
                    position: at(p),
                    along: direction(along),
                    text,
                    upper,
                    lower,
                    color: tone_color(tone),
                    badge,
                });
            }
        }
    }
}

/// Adds what `space` draws in `view`: in front of everything, as what
/// is edited through it is what the eye is on.
fn space<M: 'static>(scene: &mut Scene, space: &Space<M>, pose: Pose, view: &SpaceView) {
    let at = |p: Point3| pose.transform_point(p).as_vec3();
    let faded = |tone: Tone, opacity: f32| {
        let [r, g, b, a] = tone_color(tone);
        [r, g, b, a * opacity]
    };
    for shape in space.shapes(view) {
        match shape {
            SpaceShape::Line {
                ends: [a, b],
                tone,
                width,
                opacity,
                dashed,
            } => {
                let color = faded(tone, opacity);
                let mut line = |a: Point3, b: Point3| {
                    scene.front.lines.push(Line {
                        a: at(a),
                        b: at(b),
                        width: [width; 2],
                        color,
                        pick: None,
                    });
                };
                let length = a.distance(b);
                let period = (DASH + DASH_GAP) * view.pixel;
                if !dashed || period <= 0.0 || length / period > 4000.0 {
                    line(a, b);
                    continue;
                }
                let mut from = 0.0;
                while from < length {
                    let to = (from + DASH * view.pixel).min(length);
                    line(a.lerp(b, from / length), a.lerp(b, to / length));
                    from += period;
                }
            }
            SpaceShape::Dot {
                at: p,
                tone,
                size,
                opacity,
            } => scene.front.vertices.push(Vertex {
                position: at(p),
                size,
                color: faded(tone, opacity),
                pick: None,
            }),
            SpaceShape::Triangle {
                corners,
                tone,
                opacity,
            } => {
                let positions = corners.map(at);
                let normal = (positions[1] - positions[0])
                    .cross(positions[2] - positions[0])
                    .normalize_or_zero();
                scene.front.triangles.push(crate::Triangle {
                    positions,
                    normals: [normal; 3],
                    color: faded(tone, opacity),
                    pick: None,
                    texture: [0.0, 0.0],
                });
            }
        }
    }
}

/// Adds `handles`, placed by `pose`, to `scene`. `pixel` gives the size of
/// a pixel in model units at a (placed) point; `right` is the screen's
/// right direction there and `up` its up direction (for canvases and
/// spaces, which draw by them); `middle` is what the view looks at.
pub fn handles<M: 'static>(
    scene: &mut Scene,
    handles: &BTreeMap<String, Handle<M>>,
    pose: Pose,
    pixel: &dyn Fn(Point3) -> f64,
    right: Point3,
    up: Point3,
    middle: Point3,
) {
    // (A space first: the other handles show over what it draws.)
    let spaces = handles.values().filter(|h| matches!(h, Handle::Space(_)));
    let others = handles
        .iter()
        .filter(|(_, h)| !matches!(h, Handle::Space(_)));
    for handle in spaces {
        let scale = pixel(pose.transform_point(handle.anchor()));
        if let Handle::Space(s) = handle {
            let local = pose.inverse();
            let right = local.transform_vector(right);
            let up = local.transform_vector(up);
            let view = SpaceView {
                right,
                up,
                pixel: scale,
            };
            space(scene, s, pose, &view);
        }
    }
    for (name, handle) in others {
        let scale = pixel(pose.transform_point(handle.anchor()));
        if let Handle::Canvas(c) = handle {
            canvas(
                scene,
                c,
                pose,
                &canvas_view(c, pose, right, up, scale, middle),
            );
            continue;
        }
        for piece in handle.pieces(scale) {
            let pick = scene.pick_id(Some(Pick::Handle {
                name: name.clone(),
                part: piece.part,
            }));
            let color = piece.axis.map_or(HANDLE, |a| AXES[a.index()]);
            let mut add = Painter {
                scene,
                pose,
                color,
                pick,
            };
            match piece.shape {
                Shape::Knob(at) => add.knob(at),
                Shape::Arrow(from, to) => add.arrow(from, to),
                Shape::Ring {
                    center,
                    normal,
                    radius,
                } => add.ring(center, normal, radius),
            }
        }
    }
}

/// Adds one piece's primitives, placed.
struct Painter<'a> {
    scene: &'a mut Scene,
    pose: Pose,
    color: Color,
    pick: Option<PickId>,
}

impl Painter<'_> {
    fn line(&mut self, a: Point3, b: Point3, width: [f32; 2]) {
        self.scene.lines.lines.push(Line {
            a: self.pose.transform_point(a).as_vec3(),
            b: self.pose.transform_point(b).as_vec3(),
            width,
            color: self.color,
            pick: self.pick,
        });
    }

    fn arrow(&mut self, from: Point3, to: Point3) {
        let head = from + (to - from) * 0.78;
        self.line(from, head, [SHAFT_WIDTH; 2]);
        self.line(head, to, [ARROW_WIDTH, 0.0]);
    }

    fn knob(&mut self, at: Point3) {
        self.scene.vertices.vertices.push(Vertex {
            position: self.pose.transform_point(at).as_vec3(),
            size: KNOB_SIZE,
            color: self.color,
            pick: self.pick,
        });
    }

    fn ring(&mut self, center: Point3, normal: Point3, radius: f64) {
        let u = normal.any_orthonormal_vector();
        let v = normal.cross(u);
        let at = |k: usize| {
            let a = std::f64::consts::TAU * k as f64 / RING_SEGMENTS as f64;
            center + (u * a.cos() + v * a.sin()) * radius
        };
        for k in 0..RING_SEGMENTS {
            self.line(at(k), at(k + 1), [RING_WIDTH; 2]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use workpiece_core::Axis;
    use workpiece_core::handles::{Gizmo, GizmoPart, LinearHandle};

    #[test]
    fn gizmo_parts_are_picked_apart() {
        let mut scene = Scene::default();
        let g = Gizmo::new(Pose::IDENTITY, 100.0, |p| p);
        let all = BTreeMap::from([("pose".to_owned(), Handle::from(g))]);
        handles(
            &mut scene,
            &all,
            Pose::IDENTITY,
            &|_| 0.01,
            Point3::X,
            Point3::Y,
            Point3::ZERO,
        );
        // The center, three arrows and three rings.
        assert_eq!(scene.picks().len(), 7);
        let ring = Pick::Handle {
            name: "pose".into(),
            part: Some(GizmoPart::Rotate(Axis::Z)),
        };
        let id = scene.index_of(&ring);
        let ring_lines = scene.lines.lines.iter().filter(|l| l.pick == id).count();
        assert_eq!(ring_lines, RING_SEGMENTS);
    }

    #[test]
    fn canvases_draw_their_shapes_in_the_plane() {
        use workpiece_core::world::DatumPlane;
        let shapes = |_: &CanvasView| {
            vec![
                CanvasShape::Polyline {
                    points: vec![Point2::ZERO, Point2::new(1.0, 0.0)],
                    tone: Tone::Construction,
                    width: 1.0,
                    dashed: true,
                },
                CanvasShape::Arrowhead {
                    tip: Point2::new(0.0, 1.0),
                    direction: Point2::Y,
                    tone: Tone::Dimension,
                },
                CanvasShape::Text {
                    at: Point2::new(2.0, 3.0),
                    along: Point2::Y,
                    text: "Ø10 H7".into(),
                    upper: String::new(),
                    lower: String::new(),
                    tone: Tone::Dimension,
                    badge: false,
                },
            ]
        };
        // On the yz plane: the plane's x is the model's y.
        let c: Canvas<()> = Canvas::new(DatumPlane::yz(), Point2::ZERO, shapes, |_| None);
        let view = canvas_view(&c, Pose::IDENTITY, Point3::Z, Point3::X, 0.01, Point3::ZERO);
        assert!(
            view.right.abs_diff_eq(Point2::Y, 1e-12),
            "the screen's right is the plane's y"
        );
        let all = BTreeMap::from([("sketch".to_owned(), Handle::from(c))]);
        let mut scene = Scene::default();
        handles(
            &mut scene,
            &all,
            Pose::IDENTITY,
            &|_| 0.01,
            Point3::Z,
            Point3::Y,
            Point3::ZERO,
        );
        // A unit line in dashes of 7 pixels with gaps of 4: 9 full dashes
        // and the start of a tenth, then the arrowhead.
        let lines = &scene.lines.lines;
        assert_eq!(lines.len(), 10 + 1);
        assert!(lines.iter().all(|l| l.a.x.abs() < 1e-6), "in the yz plane");
        assert_eq!(lines[10].width, [HEAD_WIDTH as f32, 0.0]);
        assert!((lines[10].a.z - 0.88).abs() < 1e-6 && (lines[10].b.z - 1.0).abs() < 1e-6);
        let [label] = &scene.labels[..] else {
            panic!("one label");
        };
        assert_eq!(label.text, "Ø10 H7");
        assert!(
            label
                .position
                .abs_diff_eq(glam::Vec3::new(0.0, 2.0, 3.0), 1e-6)
        );
        assert!(label.along.abs_diff_eq(glam::Vec3::Z, 1e-6));
        assert!(scene.picks().is_empty());
    }

    #[test]
    fn linear_handles_keep_their_size() {
        let size = |value: f64| {
            let mut scene = Scene::default();
            let h = LinearHandle::new(Point3::ZERO, Point3::Z, value, |x| x).expect("handle");
            let all = BTreeMap::from([("h".to_owned(), Handle::from(h))]);
            handles(
                &mut scene,
                &all,
                Pose::IDENTITY,
                &|_| 0.01,
                Point3::X,
                Point3::Y,
                Point3::ZERO,
            );
            let lines = &scene.lines.lines;
            (
                lines[0].a.z,
                lines.last().map_or(0.0, |l| l.b.z - lines[0].a.z),
            )
        };
        // The arrow starts at the tip and is as long at any value.
        let ((start1, length1), (start5, length5)) = (size(1.0), size(5.0));
        assert_eq!((start1, start5), (1.0, 5.0));
        assert!((length1 - length5).abs() < 1e-5);
        assert!((length1 - 0.6).abs() < 1e-5, "60 pixels of 0.01");
    }
}
