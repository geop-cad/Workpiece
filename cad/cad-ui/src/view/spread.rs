//! What is said of the model, as it shows on it in 3D: each note laid
//! out by the drawing's own rules (`on_model`), and the texts of those
//! that stand where their steps put them moved apart where they would
//! stand on one another.

use workpiece_core::math::Point3;
use workpiece_core::world::Annotation;

use super::viewport::{NOTE_PIXELS, NoteShown, NoteText};
use crate::Model;
use crate::model::viewport::Viewport;

/// `note` (called `name`) with its text at `at` (in the part's own
/// coordinates), as it shows on the model for who sees `right` to the
/// right and `up` up (also the part's): a drawing's lettering, 3.5
/// millimetres high, twelve pixels high.
pub(crate) fn on_the_model(
    model: &Model,
    v: &Viewport,
    (name, note): (&str, &Annotation),
    at: Point3,
    right: Point3,
    up: Point3,
) -> NoteShown {
    let pose = v.shown.pose;
    let place = |p: Point3| pose.transform_point(p).as_vec3();
    let pixel = model.pixel(pose.transform_point(at));
    let drawn = workpiece_ops_drawing::lay::on_model::on_model(
        note,
        at,
        right,
        up,
        pixel * NOTE_PIXELS,
        &v.shown.world.topo,
    )
    .unwrap_or_default();
    NoteShown {
        name: name.to_owned(),
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
    }
}

/// A box on the screen, in pixels: its least and its most corner.
type Box2 = [glam::Vec2; 2];

/// The texts of `notes` kept off one another where there is room: a
/// note that stands where its step put it (none the user stood
/// elsewhere, no sketch's, which the sketch places) and whose text
/// would stand on another's is laid out again with its text moved —
/// the least of a few steps up, down, left or right on the screen, by
/// its own height or width — to where it stands on none; where there is
/// no such place, it stays. Who comes first keeps its place: those the
/// user placed, then by name.
pub(crate) fn spread(
    model: &Model,
    v: &Viewport,
    right: Point3,
    up: Point3,
    notes: &mut [NoteShown],
) {
    let [w, h] = v.size;
    if w < 1.0 || h < 1.0 || notes.len() < 2 {
        return;
    }
    let view_proj = v.camera.view_proj(w / h);
    let screen = |p: glam::Vec3| {
        super::viewport::project(view_proj, p)
            .map(|c| glam::Vec2::new((c.x + 1.0) / 2.0 * w, (1.0 - c.y) / 2.0 * h))
    };
    // (Of its text: the box of its grip, as it is seen.)
    let text_box = |note: &NoteShown| -> Option<Box2> {
        if note.texts.is_empty() {
            return None;
        }
        let corners: Vec<glam::Vec2> = note.grip.iter().filter_map(|p| screen(*p)).collect();
        (corners.len() == 4).then(|| {
            let lo = corners.iter().fold(glam::Vec2::INFINITY, |m, c| m.min(*c));
            let hi = corners
                .iter()
                .fold(glam::Vec2::NEG_INFINITY, |m, c| m.max(*c));
            [lo, hi]
        })
    };
    const MARGIN: f32 = 3.0;
    let overlaps = |a: &Box2, b: &Box2| {
        a[0].x < b[1].x + MARGIN
            && b[0].x < a[1].x + MARGIN
            && a[0].y < b[1].y + MARGIN
            && b[0].y < a[1].y + MARGIN
    };
    let world = &v.shown.world;
    let places = model
        .focused_part()
        .and_then(|p| model.workspace.part(&p).ok())
        .map(|p| &p.note_places);
    // (Which may be moved: what stands where its step put it.)
    let free = |note: &NoteShown| {
        world
            .annotations
            .get(&note.name)
            .is_some_and(|a| a.sketch.is_none())
            && !places.is_some_and(|p| p.contains_key(&note.name))
    };
    // (Its lines as seen, each as points along it: what another's text
    // is kept off, and what is kept off another's.)
    let lines = |note: &NoteShown| -> Vec<glam::Vec2> {
        note.strokes
            .iter()
            .filter_map(|(a, b)| Some((screen(*a)?, screen(*b)?)))
            .flat_map(|(a, b)| (0..=8).map(move |k| a.lerp(b, k as f32 / 8.0)))
            .collect()
    };
    let inside = |p: &glam::Vec2, b: &Box2| {
        p.x > b[0].x - MARGIN
            && p.x < b[1].x + MARGIN
            && p.y > b[0].y - MARGIN
            && p.y < b[1].y + MARGIN
    };
    // (Whether a note with its text in `own` and its lines `drawn`
    // stands on what is taken.)
    let clashes = |own: &Box2, drawn: &[glam::Vec2], taken: &[(Box2, Vec<glam::Vec2>)]| {
        taken.iter().any(|(b, l)| {
            overlaps(own, b)
                || l.iter().any(|p| inside(p, own))
                || drawn.iter().any(|p| inside(p, b))
        })
    };
    let mut order: Vec<usize> = (0..notes.len()).collect();
    order.sort_by_key(|k| (free(&notes[*k]), notes[*k].name.clone()));
    let back = v.shown.pose.inverse();
    let mut taken: Vec<(Box2, Vec<glam::Vec2>)> = Vec::new();
    for k in order {
        let Some(had) = text_box(&notes[k]) else {
            continue;
        };
        if !free(&notes[k]) || !clashes(&had, &lines(&notes[k]), &taken) {
            taken.push((had, lines(&notes[k])));
            continue;
        }
        let Some(note) = world.annotations.get(&notes[k].name) else {
            continue;
        };
        let at = back.transform_point(notes[k].at.as_dvec3());
        let pixel = model.pixel(notes[k].at.as_dvec3());
        let size = had[1] - had[0];
        let tries = (1..=4).flat_map(|n| {
            let n = n as f32;
            [
                glam::Vec2::new(0.0, -(size.y + MARGIN) * n),
                glam::Vec2::new(0.0, (size.y + MARGIN) * n),
                glam::Vec2::new((size.x + MARGIN) * n, 0.0),
                glam::Vec2::new(-(size.x + MARGIN) * n, 0.0),
            ]
        });
        let mut moved = None;
        for shift in tries {
            let to = at + (right * f64::from(shift.x) - up * f64::from(shift.y)) * pixel;
            let again = on_the_model(model, v, (&notes[k].name, note), to, right, up);
            if let Some(there) = text_box(&again)
                && !clashes(&there, &lines(&again), &taken)
            {
                moved = Some((again, there));
                break;
            }
        }
        match moved {
            Some((again, there)) => {
                taken.push((there, lines(&again)));
                notes[k] = again;
            }
            None => taken.push((had, lines(&notes[k]))),
        }
    }
}
