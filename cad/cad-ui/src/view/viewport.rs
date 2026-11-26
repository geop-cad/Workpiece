use std::sync::Arc;

use workpiece_rasterize::{Drawing, Pick, Scene};

use workpiece_core::handles::GizmoPart;

use workpiece_core::widgets::Button;
use workpiece_core::{Hotkey, Key};

use crate::model::Camera;
use crate::picking::Candidate;
use crate::{CameraMove, Message, Model};

/// A text field over the scene, with the keyboard: Enter (or a click
/// elsewhere) enters its text, Escape cancels.
pub struct Entry {
    /// What the field is for: another key is another field, which starts
    /// from its own text.
    pub key: String,
    /// Its center, in the scene.
    pub position: glam::Vec3,
    /// The text it starts with, selected.
    pub text: String,
    /// The message for text entered, or why it is not taken.
    pub enter: workpiece_core::handles::CanvasEnter<Message>,
    /// Closes the field without entering anything.
    pub cancel: Message,
}

/// The 3D view: what to draw, from where, and what to highlight.
pub struct Viewport {
    /// The focused part, in color.
    pub scene: Arc<Drawing>,
    /// The parts around it, faded.
    pub ghosts: Arc<Drawing>,
    /// The edited step's handles, drawn on top.
    pub handles: Scene,
    /// The text field the edited step has open in the scene.
    pub entry: Option<Entry>,
    /// The camera.
    pub camera: Camera,
    /// The candidate under the pointer, highlighted.
    pub hovered: Option<Pick>,
    /// Which candidate it is, and how many there are (cycled with the
    /// right mouse button), with a few words on it.
    pub candidate: Option<(usize, usize, String)>,
    /// The handle being dragged.
    pub dragging: Option<Pick>,
    /// The selection set.
    pub selected: Vec<Pick>,
    /// What the faces are painted to show.
    pub analysis: crate::model::display::Analysis,
    /// How the model is displayed: cut open, analysed, its parts apart.
    pub display: crate::model::display::Display,
    /// The sections the part that shows has, by name: what it can be
    /// shown cut open by.
    pub sections: Vec<String>,
    /// Whether a sketch on a plane cuts the model open there (a setting).
    pub sketch_section: bool,
    /// The camera views it has, by name: what the camera can be sent
    /// to.
    pub views: Vec<String>,
    /// Whether the step that is edited takes the view as it is turned
    /// (see [`Message::TakeView`](crate::Message::TakeView)).
    pub takes_view: bool,
    /// What is said of the model on it. With the annotate tab open,
    /// all of it: the sizes the sketches and the steps hold it to as
    /// well.
    pub notes: Vec<NoteShown>,
    /// Why something could not be built.
    pub error: Option<String>,
    /// What the user is told of that is no error.
    pub notice: Option<String>,
    /// Whether the camera is on its way to a new view: time has to pass
    /// ([`Message::Tick`](crate::Message::Tick)) for it to get there.
    pub animating: bool,
    /// Back to the starting view.
    pub home: Button<Message>,
    /// Switches between orthographic and perspective.
    pub projection: Button<Message>,
    /// Faces the plane the edited step is edited in (a sketch's) square
    /// on, while there is one.
    pub normal: Option<Button<Message>>,
}

/// A few words on what `candidate` is.
pub fn describe_candidate(candidate: &Candidate) -> String {
    match candidate {
        Candidate::Reference { instance, source } => {
            let source = super::timeline::describe_source(source);
            if instance.is_empty() {
                source
            } else {
                format!("{} › {source}", instance.join(" › "))
            }
        }
        Candidate::Joint { name, kind } => {
            format!("{} joint {name}", kind.label())
        }
        Candidate::Handle { name, part } => match part {
            Some(GizmoPart::Free) => format!("{name}: move freely"),
            Some(GizmoPart::Translate(a)) => format!("{name}: move along {}", a.name()),
            Some(GizmoPart::Rotate(a)) => format!("{name}: turn about {}", a.name()),
            Some(GizmoPart::Scale) => format!("{name}: scale all at once"),
            None => name.clone(),
        },
    }
}

impl Viewport {
    /// The viewport of `model`.
    pub fn new(model: &Model) -> Self {
        let v = &model.viewport;
        let hovered = v.hovered();
        let candidate = hovered.map(|c| (v.candidate, v.candidates.len(), describe_candidate(c)));
        let pick = |c: Option<&Candidate>| c.map(Pick::from);
        Self {
            scene: v.shown.scene.clone(),
            ghosts: {
                // With the part a joint being made would bring along.
                let preview = model.joint_preview();
                if preview.is_empty() {
                    v.shown.ghosts.clone()
                } else {
                    let mut ghosts = (*v.shown.ghosts).clone();
                    ghosts.extend(preview);
                    Arc::new(ghosts)
                }
            },
            handles: model.handle_scene(),
            entry: model.entry(),
            camera: v.camera,
            hovered: pick(v.dragging.as_ref().or(hovered)),
            candidate: candidate.filter(|_| v.dragging.is_none()),
            dragging: pick(v.dragging.as_ref()),
            selected: model.selection.iter().map(Pick::from).collect(),
            analysis: model.display.analysis,
            display: model.display.clone(),
            sections: v.shown.world.sections.keys().cloned().collect(),
            sketch_section: model.settings.sketch_section,
            views: v.shown.world.views.keys().cloned().collect(),
            takes_view: model.takes_view(),
            notes: {
                let pose = v.shown.pose;
                let place = |p: workpiece_core::math::Point3| pose.transform_point(p).as_vec3();
                // Annotations show on the model where what is said of
                // it is looked at: on the Annotate tab, and nowhere else.
                let annotating = model.tab == crate::message::Tab::Annotate;
                let places = model
                    .focused_part()
                    .and_then(|p| model.workspace.part(&p).ok())
                    .map(|p| &p.note_places);
                // As who looks sees it, in the part's own directions.
                let back = pose.inverse();
                let right = back.transform_vector(v.camera.right().as_dvec3());
                let up = back.transform_vector(v.camera.up().as_dvec3());
                // While a sketch is edited its dimensions are drawn by the
                // sketcher, which edits them: nothing is drawn over them,
                // nor of any other sketch or step.
                let sketching = model.editing.last().is_some_and(|e| {
                    model.focused_part().is_some_and(|p| {
                        model.workspace.part(&p).is_ok_and(|p| {
                            p.operation(e).is_some_and(|op| {
                                op.downcast_ref::<workpiece_ops_sketch::Sketch>().is_some()
                            })
                        })
                    })
                });
                let notes = v.shown.world.annotations.iter();
                notes
                    .filter(|_| annotating && !sketching)
                    .filter_map(|(name, note)| {
                        let at = places
                            .and_then(|p| p.get(name))
                            .map_or(note.at, |at| workpiece_core::math::Point3::from_array(*at));
                        // (A drawing's lettering, 3.5 millimetres high,
                        // is twelve pixels high here.)
                        let pixel = model.pixel(pose.transform_point(at));
                        // A sketch's dimension is laid out by the
                        // sketch, in the sketch's plane, as it is in the
                        // sketch: nothing is chosen from the view but
                        // its size and the way it turns in the plane.
                        if let Some(of) = &note.sketch {
                            return sketch_note(name, note, of, pixel, right, up, &place);
                        }
                        let per_mm = pixel * NOTE_PIXELS;
                        let drawn = workpiece_ops_drawing::lay::on_model::on_model(
                            note, at, right, up, per_mm,
                        )
                        .unwrap_or_default();
                        Some(NoteShown {
                            name: name.clone(),
                            at: place(at),
                            text: note.text(),
                            movable: true,
                            strokes: drawn
                                .strokes
                                .iter()
                                .map(|(a, b)| (place(*a), place(*b)))
                                .collect(),
                            fills: drawn.fills.iter().map(|f| f.map(place)).collect(),
                            texts: drawn
                                .texts
                                .iter()
                                .map(|t| NoteText {
                                    at: place(t.at),
                                    towards: place(t.at + t.along * t.height),
                                    text: t.text.clone(),
                                    upper: String::new(),
                                    lower: String::new(),
                                    height: (t.height / pixel) as f32,
                                })
                                .collect(),
                            grip: drawn.grip.map(place),
                        })
                    })
                    .chain(masses(model, v, right, up))
                    .collect()
            },
            error: model.error(),
            notice: model.notice(),
            animating: v.transition.is_some(),
            home: Button::new("Home view", || Message::Camera(CameraMove::Home))
                .with_hotkey(Hotkey::named(Key::Home)),
            projection: Button::new(
                if v.camera.orthographic && v.transition.is_none() {
                    "Perspective"
                } else {
                    "Orthographic"
                },
                || Message::Camera(CameraMove::ToggleProjection),
            )
            .with_hotkey(Hotkey::key('O')),
            normal: crate::edit::edit_plane(model).map(|_| {
                Button::new("Normal view", || Message::Camera(CameraMove::FacePlane))
                    .with_hotkey(Hotkey::key('Q'))
            }),
        }
    }
}

/// Where `p` is on the screen, in clip coordinates (`x` and `y` from -1
/// to 1 across the view), if it is in front of the camera's near plane:
/// what is behind it has no place on the screen (dividing by its depth
/// would put it anywhere, mirrored and without bound).
#[must_use]
pub fn project(view_proj: glam::Mat4, p: glam::Vec3) -> Option<glam::Vec2> {
    let c = view_proj * p.extend(1.0);
    (c.z + c.w > 1e-6 && c.w > 1e-6).then(|| glam::Vec2::new(c.x / c.w, c.y / c.w))
}

/// The part of the polygon `corners` (a line for two, a point for one)
/// that is in front of the near plane, on the screen in clip
/// coordinates. Whatever the camera and the plane the polygon lies in:
/// every point is finite, and as far from the middle of the view as
/// the near plane lets what is in front of it be.
#[must_use]
pub fn clip_polygon(view_proj: glam::Mat4, corners: &[glam::Vec3]) -> Vec<glam::Vec2> {
    // (How far in front of the near plane a clip point is.)
    let depth = |c: glam::Vec4| c.z + c.w - 1e-6;
    let clip: Vec<glam::Vec4> = corners.iter().map(|p| view_proj * p.extend(1.0)).collect();
    let mut kept = Vec::new();
    let count = if clip.len() == 2 { 1 } else { clip.len() };
    for k in 0..count {
        let (a, b) = (clip[k], clip[(k + 1) % clip.len()]);
        let (da, db) = (depth(a), depth(b));
        if da > 0.0 {
            kept.push(a);
        }
        if (da > 0.0) != (db > 0.0) {
            kept.push(a.lerp(b, da / (da - db)));
        }
    }
    if clip.len() == 2 && depth(clip[1]) > 0.0 {
        kept.push(clip[1]);
    }
    kept.iter()
        .filter(|c| c.w > 1e-6)
        .map(|c| glam::Vec2::new(c.x / c.w, c.y / c.w))
        .filter(|p| p.is_finite())
        .collect()
}

/// How high the capitals of a sketch dimension's value are, in pixels:
/// those of the sketcher's own (13.5 pixel) text.
const SKETCH_TEXT_PX: f32 = 13.5 * 0.7;

/// The dimension `of` a sketch, `note`, as the sketcher lays it out, set
/// in the sketch's plane: every shape of the sketcher's layout (in the
/// sketch's own coordinates) taken through the plane's frame, so that it
/// is in the plane and as it is in the sketch, and nowhere else. `pixel`
/// is the length of a pixel there; `right` and `up` are where the
/// screen's right and up point (in the part's directions).
fn sketch_note(
    name: &str,
    note: &workpiece_core::world::Annotation,
    of: &workpiece_core::world::SketchOf,
    pixel: f64,
    right: workpiece_core::math::Point3,
    up: workpiece_core::math::Point3,
    place: &dyn Fn(workpiece_core::math::Point3) -> glam::Vec3,
) -> Option<NoteShown> {
    use workpiece_core::handles::{CanvasShape, CanvasView, HEAD_LENGTH, HEAD_WIDTH, Tone};
    use workpiece_core::math::Point2;
    let to_3d = |p: Point2| place(of.origin + of.x * p.x + of.y * p.y);
    // The sketch's coordinates of where the text stands.
    let label = Point2::new(
        (note.at - of.origin).dot(of.x),
        (note.at - of.origin).dot(of.y),
    );
    let view = CanvasView::through(of.x, of.y, right, up, pixel, label);
    let mut shown = NoteShown {
        name: name.to_owned(),
        at: place(note.at),
        text: note.text(),
        // (Dragged, it moves in its sketch.)
        movable: true,
        strokes: Vec::new(),
        fills: Vec::new(),
        texts: Vec::new(),
        grip: [place(note.at); 4],
    };
    for shape in workpiece_ops_sketch::layout::annotation(of, &view, Tone::Dimension)? {
        match shape {
            CanvasShape::Polyline { points, .. } => shown
                .strokes
                .extend(points.windows(2).map(|w| (to_3d(w[0]), to_3d(w[1])))),
            CanvasShape::Arrowhead { tip, direction, .. } => {
                let base = tip - direction * HEAD_LENGTH * pixel;
                let half = direction.perp() * HEAD_WIDTH * 0.5 * pixel;
                shown
                    .fills
                    .push([to_3d(tip), to_3d(base + half), to_3d(base - half)]);
            }
            CanvasShape::Text {
                at,
                along,
                text,
                upper,
                lower,
                ..
            } => {
                let (at3, towards) = (to_3d(at), to_3d(at + along * pixel * 12.0));
                // The box of the text: about as wide as its letters.
                let (half_w, half_h) = (
                    text.chars().count() as f64 * f64::from(SKETCH_TEXT_PX) * 0.8 * pixel / 2.0,
                    f64::from(SKETCH_TEXT_PX) * pixel,
                );
                let (a, b) = (along, along.perp());
                shown.grip = [
                    at - a * half_w - b * half_h,
                    at + a * half_w - b * half_h,
                    at + a * half_w + b * half_h,
                    at - a * half_w + b * half_h,
                ]
                .map(to_3d);
                shown.texts.push(NoteText {
                    at: at3,
                    towards,
                    text,
                    upper,
                    lower,
                    height: SKETCH_TEXT_PX,
                });
            }
            CanvasShape::Dot { .. } => {}
        }
    }
    Some(shown)
}

/// How many pixels a millimetre of a drawing's lettering is, where
/// notes are drawn on the model.
const NOTE_PIXELS: f64 = 12.0 / 3.5;

/// The mass properties the part's steps ask for, as they show on the
/// model: the ellipsoid of one density that weighs and turns as the
/// bodies do, its axes, and what they weigh and how they turn in
/// lines of text beside it. They show on the Properties tab, and while
/// their step is edited or selected.
fn masses(
    model: &Model,
    v: &crate::model::viewport::Viewport,
    right: workpiece_core::math::Point3,
    up: workpiece_core::math::Point3,
) -> Vec<NoteShown> {
    use workpiece_core::math::Point3;
    let world = &v.shown.world;
    let pose = v.shown.pose;
    let place = |p: Point3| pose.transform_point(p).as_vec3();
    let on_tab = model.tab == crate::message::Tab::Properties;
    let asked_for = |name: &String| {
        model.editing.last() == Some(name)
            || model.step.as_ref().and_then(|s| s.last()) == Some(name)
    };
    // What a unit of volume of each body weighs, in kilograms: by what
    // the body is made of, or else the part.
    let part = model
        .focused_part()
        .and_then(|p| model.workspace.part(&p).ok());
    let cubic = part.map_or(1e-9, |p| (p.unit.millimetres() / 1000.0).powi(3));
    let of_part = model.density();
    let density = |body: &str| {
        let own = world.body_look(body).material;
        let own = own.as_ref().and_then(|m| model.workspace.library.get(m));
        own.map(|m| m.density * cubic).or(of_part)
    };
    let unit = model.unit().label();
    let mut all = Vec::new();
    for (name, asked) in &world.masses {
        if !(on_tab || asked_for(name)) {
            continue;
        }
        // (Where nothing says what they are made of, they are taken as
        // of one density: where their centre is and how they turn then
        // still holds; what they weigh is not known.)
        let known = asked.bodies.iter().all(|b| density(&b.body).is_some());
        let Some(w) = asked.weighed(|b| {
            if known {
                density(b).unwrap_or(1.0)
            } else {
                1.0
            }
        }) else {
            continue;
        };
        let mut strokes = Vec::new();
        // The ellipsoid: its three outlines in its principal planes.
        const STEPS: usize = 48;
        for (i, j) in [(0, 1), (1, 2), (0, 2)] {
            let at = |k: usize| {
                let t = std::f64::consts::TAU * k as f64 / STEPS as f64;
                w.centre
                    + w.axes[i] * (w.half[i] * libm::cos(t))
                    + w.axes[j] * (w.half[j] * libm::sin(t))
            };
            strokes.extend((0..STEPS).map(|k| (place(at(k)), place(at(k + 1)))));
        }
        // Its axes, each a little past it.
        let reach = w.half.iter().copied().fold(0.0, f64::max);
        for k in 0..3 {
            let out = w.axes[k] * (w.half[k] + reach * 0.15);
            strokes.push((place(w.centre - out), place(w.centre + out)));
        }
        // The centre of gravity: a small cross as who looks sees it.
        let pixel = model.pixel(pose.transform_point(w.centre));
        for way in [right, up] {
            let arm = way * (pixel * 5.0);
            strokes.push((place(w.centre - arm), place(w.centre + arm)));
        }
        let number = |x: f64| {
            // (Of nothing the logarithm is no number: no digits.)
            let digits = 3i32
                .saturating_sub(libm::log10(x.abs()).floor() as i32)
                .clamp(0, 9) as usize;
            if x == 0.0 {
                "0".to_owned()
            } else {
                format!("{x:.digits$}")
            }
        };
        let mut lines = Vec::new();
        if known {
            lines.push(format!("Mass {} kg", number(w.mass)));
        } else {
            lines.push("Mass: give the part or its bodies a material".to_owned());
        }
        lines.push(format!("Volume {} {unit}³", number(w.volume)));
        let c = w.centre;
        lines.push(format!(
            "Centre of gravity {}, {}, {}",
            number(c.x),
            number(c.y),
            number(c.z)
        ));
        let of = if known {
            format!("kg·{unit}²")
        } else {
            format!("{unit}⁵ (density 1)")
        };
        let [a, b, c] = w.principal;
        lines.push(format!(
            "Principal moments {}, {}, {} {of}",
            number(a),
            number(b),
            number(c)
        ));
        let m = w.inertia;
        lines.push(format!(
            "Ixx {}  Iyy {}  Izz {}",
            number(m[0][0]),
            number(m[1][1]),
            number(m[2][2])
        ));
        lines.push(format!(
            "Ixy {}  Ixz {}  Iyz {}",
            number(-m[0][1]),
            number(-m[0][2]),
            number(-m[1][2])
        ));
        // Under the ellipsoid, one line under the other, each about
        // the middle.
        let height = 12.0_f64;
        let start = w.centre - up * (reach * 0.7 + pixel * height * 2.0);
        let texts = lines
            .iter()
            .enumerate()
            .map(|(n, line)| {
                let at = start - up * (pixel * height * 1.5 * n as f64);
                NoteText {
                    at: place(at),
                    towards: place(at + right * (pixel * height)),
                    text: line.clone(),
                    height: height as f32,
                    upper: String::new(),
                    lower: String::new(),
                }
            })
            .collect();
        let at = place(w.centre);
        all.push(NoteShown {
            name: name.clone(),
            at,
            movable: true,
            text: lines.join(" · "),
            strokes,
            fills: Vec::new(),
            texts,
            grip: [at; 4],
        });
    }
    all
}

/// A line of a note's lettering.
#[derive(Debug, Clone, PartialEq)]
pub struct NoteText {
    /// Its middle.
    pub at: glam::Vec3,
    /// A point from its middle the way it is read.
    pub towards: glam::Vec3,
    /// What it says.
    pub text: String,
    /// How high its capitals are, in pixels.
    pub height: f32,
    /// Small text after it, above its line: an upper deviation.
    pub upper: String,
    /// Small text after it, below its line: a lower deviation.
    pub lower: String,
}

/// Something said of the model, as it shows on it: drawn as a drawing
/// has it, in the plane it lies in.
#[derive(Debug, Clone, PartialEq)]
pub struct NoteShown {
    /// The annotation's name in the model.
    pub name: String,
    /// Where its text stands.
    pub at: glam::Vec3,
    /// What it says, in a line.
    pub text: String,
    /// Whether it is dragged to stand elsewhere (a sketch's dimension
    /// is moved in its sketch, where the sketch has it).
    pub movable: bool,
    /// Its lines: dimension and extension lines, leaders, frames.
    pub strokes: Vec<(glam::Vec3, glam::Vec3)>,
    /// Its arrowheads and other filled areas.
    pub fills: Vec<[glam::Vec3; 3]>,
    /// Its lettering.
    pub texts: Vec<NoteText>,
    /// The corners of the box of its text: what is pointed at and
    /// dragged.
    pub grip: [glam::Vec3; 4],
}
