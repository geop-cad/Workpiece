//! What a laid-out sheet holds, as data an agent can read instead of
//! looking at pixels: each view with its kind, scale, place and box, and
//! in it every dimension and note with its text exactly as printed, what
//! it measures, whether its ends are on drawn geometry, and where its
//! text stands; the centre lines, cutting lines, balloons, the parts list
//! and the problems.

use std::collections::BTreeMap;

use serde_json::{Value, json};
use workpiece_core::math::Point2;
use workpiece_core::world::World;
use workpiece_core::world::annotation::{Measured, Note};
use workpiece_ops_drawing::lay::{Laid, Placing, parts};
use workpiece_ops_drawing::sheet::{Beside, Projection, Sheet, Side, ViewKind};

/// How far (sheet millimetres) from a drawn line a dimension's end may
/// be to count as on it.
const ON: f64 = 0.35;

fn r(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

fn pt(p: Point2) -> Value {
    json!([r(p.x), r(p.y)])
}

fn rel(p: Point2, centre: Point2) -> Value {
    pt(p - centre)
}

fn near_segment(p: Point2, a: Point2, b: Point2) -> f64 {
    let d = b - a;
    let l = d.length_squared();
    let t = if l < 1e-12 {
        0.0
    } else {
        ((p - a).dot(d) / l).clamp(0.0, 1.0)
    };
    p.distance(a + d * t)
}

/// How far each end of a length is from a line or a round the view
/// draws (nothing if it is on one, or the line the extension line runs
/// back along from it meets one within `gap`).
fn ends_off(
    l: &workpiece_ops_drawing::lay::LaidLinear,
    v: &workpiece_ops_drawing::lay::LaidView,
    gap: f64,
) -> Vec<f64> {
    let away = |p: Point2| {
        v.lines
            .iter()
            .map(|(a, b)| near_segment(p, *a, *b))
            .chain(
                v.rounds
                    .iter()
                    .map(|(c, rad)| (p.distance(*c) - rad).abs().min(p.distance(*c))),
            )
            .fold(f64::INFINITY, f64::min)
    };
    (0..2)
        .map(|k| {
            let (p, way) = (l.from[k], l.ways[k]);
            let steps = ((gap + 0.5) / 0.25).ceil() as usize;
            (0..=steps)
                .map(|i| away(p - way * (i as f64 * 0.25)))
                .fold(away(p), f64::min)
        })
        .collect()
}

/// The side of the model a view of a sheet shows, where it is one of the
/// six, by the way a base view's sides stand.
fn sides(side: Side) -> Option<([i8; 3], [i8; 3])> {
    // (What looks on the screen to the right, and up.)
    Some(match side {
        Side::Front => ([1, 0, 0], [0, 0, 1]),
        Side::Back => ([-1, 0, 0], [0, 0, 1]),
        Side::Left => ([0, -1, 0], [0, 0, 1]),
        Side::Right => ([0, 1, 0], [0, 0, 1]),
        Side::Top => ([1, 0, 0], [0, 1, 0]),
        Side::Bottom => ([1, 0, 0], [0, -1, 0]),
        _ => return None,
    })
}

fn side_of(normal: [i8; 3]) -> &'static str {
    match normal {
        [0, -1, 0] => "Front",
        [0, 1, 0] => "Back",
        [-1, 0, 0] => "Left",
        [1, 0, 0] => "Right",
        [0, 0, 1] => "Top",
        [0, 0, -1] => "Bottom",
        _ => "Projected",
    }
}

/// What a view is, for counting views against what a drawing has: the
/// side of an orthographic one, `Pictorial`, `Section`, `Detail`,
/// `Auxiliary`.
fn kind_of(sheet: &Sheet, id: &str, depth: usize) -> String {
    let Some(view) = sheet.views.iter().find(|v| v.id == id) else {
        return "Projected".into();
    };
    match &view.kind {
        ViewKind::Base { side } => match side {
            Side::Front | Side::Back | Side::Left | Side::Right | Side::Top | Side::Bottom => {
                format!("{side:?}")
            }
            _ => "Pictorial".into(),
        },
        ViewKind::Named { .. } => "Pictorial".into(),
        ViewKind::Section { .. } => "Section".into(),
        ViewKind::Detail { .. } => "Detail".into(),
        ViewKind::Auxiliary { .. } => "Auxiliary".into(),
        ViewKind::Projected { of, beside } => {
            let parent = sheet.views.iter().find(|v| v.id == *of);
            let Some(ViewKind::Base { side }) = parent.map(|p| &p.kind) else {
                if depth < 4 {
                    // (A view projected from a projected one: by what that is.)
                    let from = kind_of(sheet, of, depth + 1);
                    if let Some(s) = [
                        Side::Front,
                        Side::Back,
                        Side::Left,
                        Side::Right,
                        Side::Top,
                        Side::Bottom,
                    ]
                    .into_iter()
                    .find(|s| format!("{s:?}") == from)
                    {
                        return projected_side(sheet, s, *beside);
                    }
                }
                return "Projected".into();
            };
            projected_side(sheet, *side, *beside)
        }
    }
}

fn projected_side(sheet: &Sheet, side: Side, beside: Beside) -> String {
    let Some((right, up)) = sides(side) else {
        return "Projected".into();
    };
    let neg = |v: [i8; 3]| [-v[0], -v[1], -v[2]];
    let towards = match beside {
        Beside::Right => right,
        Beside::Left => neg(right),
        Beside::Above => up,
        Beside::Below => neg(up),
    };
    // Third angle: a view stands on the side it is seen from; first
    // angle: opposite.
    let normal = match sheet.projection {
        Projection::ThirdAngle => towards,
        Projection::FirstAngle => neg(towards),
    };
    side_of(normal).into()
}

fn describe(kind: &ViewKind) -> String {
    match kind {
        ViewKind::Base { side } => format!("{side:?}"),
        ViewKind::Projected { of, beside } => format!("projected from {of}, {beside:?}"),
        ViewKind::Named { view } => format!("camera view {view}"),
        ViewKind::Section {
            of,
            section,
            letter,
            ..
        } => format!(
            "section {letter}-{letter} of {section}{}",
            of.as_ref()
                .map(|o| format!(", cut shown in {o}"))
                .unwrap_or_default()
        ),
        ViewKind::Auxiliary { of, angle } => format!("auxiliary from {of} at {angle} deg"),
        ViewKind::Detail {
            of, letter, radius, ..
        } => format!("detail {letter} of {of} (radius {radius})"),
    }
}

/// What one sketch dimension measures, by the names the recipe gave.
fn constraint_text(
    kind: &workpiece_ops_sketch::Kind,
    diameter: bool,
    points: &BTreeMap<u32, String>,
    curves: &BTreeMap<u32, String>,
) -> String {
    use workpiece_ops_sketch::Kind as K;
    let p = |i: &u32| points.get(i).cloned().unwrap_or_else(|| format!("#{i}"));
    let c = |i: &u32| curves.get(i).cloned().unwrap_or_else(|| format!("#{i}"));
    match kind {
        K::Distance(a, b) => format!("distance {} to {}", p(a), p(b)),
        K::DistanceAlong { a, b, axis } => {
            format!("distance {} to {} along {axis:?}", p(a), p(b))
        }
        K::PointLine { point, line } if diameter => {
            format!("point {} across line {}", p(point), c(line))
        }
        K::PointLine { point, line } => format!("point {} to line {}", p(point), c(line)),
        K::PointRound { point, round, far } => format!(
            "point {} to {}side of {}",
            p(point),
            if *far { "far " } else { "near " },
            c(round)
        ),
        K::LineRound { line, round, far } => format!(
            "line {} to {}side of {}",
            c(line),
            if *far { "far " } else { "near " },
            c(round)
        ),
        K::Gap(a, b) => format!("gap between {} and {}", c(a), c(b)),
        K::Size(e) if diameter => format!("diameter of {}", c(e)),
        K::Size(e) => format!("radius of {}", c(e)),
        K::Angle(a, b) => format!("angle from {} to {}", c(a), c(b)),
        K::Offset { a, b, .. } => format!("offset of {} to {}", c(a), c(b)),
        _ => String::new(),
    }
}

fn p3(p: workpiece_core::math::Point3) -> String {
    format!("({:.3} {:.3} {:.3})", p.x, p.y, p.z)
}

fn measured_text(m: &Measured) -> (String, String) {
    match m {
        Measured::Length { from, to, along } => (
            "length".into(),
            format!(
                "{} to {}{}",
                p3(*from),
                p3(*to),
                along
                    .map(|a| format!(" along {}", p3(a)))
                    .unwrap_or_default()
            ),
        ),
        Measured::Radial { centre, .. } => ("radial".into(), format!("about {}", p3(*centre))),
        Measured::Diameter { centre, radius, .. } => (
            "diameter".into(),
            format!("round at {} r {radius:.3}", p3(*centre)),
        ),
        Measured::Radius { centre, radius, .. } => (
            "radius".into(),
            format!("round at {} r {radius:.3}", p3(*centre)),
        ),
        Measured::Angle { at, .. } => ("angle".into(), format!("at {}", p3(*at))),
    }
}

/// Everything of one laid-out sheet, as data.
pub fn of_sheet(
    sheet: &Sheet,
    laid: &Laid,
    world: &World,
    names: &BTreeMap<String, crate::Named>,
    sketches: &BTreeMap<String, workpiece_ops_sketch::Sketch>,
    steps: &BTreeMap<String, Value>,
    part_names: &BTreeMap<String, String>,
) -> Value {
    // Each dimension's name in the model, by the constraint it is.
    let mut said_by: BTreeMap<String, (String, String, bool)> = BTreeMap::new();
    for (sketch, named) in names {
        let Some(sk) = sketches.get(sketch) else {
            continue;
        };
        for (key, c) in &sk.args.constraints {
            let Some(d) = &c.dimension else { continue };
            let said = match &d.name {
                Some(name) => format!("{sketch}/{name}"),
                None => format!("{sketch}/d{key}"),
            };
            let text = constraint_text(&c.kind, d.diameter, &named.0, &named.1);
            let reference = d.tolerance == workpiece_core::world::Tolerance::Reference;
            said_by.insert(said, (sketch.clone(), text, reference));
        }
    }
    let item_names: Vec<parts::Item> = parts::items(world, part_names);
    let belongs = |rect: &[Point2; 2], t: &workpiece_ops_drawing::lay::Text| {
        let slack = 0.8;
        t.at.x >= rect[0].x - slack
            && t.at.x <= rect[1].x + slack
            && t.at.y >= rect[0].y - slack
            && t.at.y <= rect[1].y + slack
    };
    let text_of = |rect: &[Point2; 2]| -> String {
        let mut found: Vec<&workpiece_ops_drawing::lay::Text> =
            laid.texts.iter().filter(|t| belongs(rect, t)).collect();
        found.sort_by(|a, b| {
            b.at.y
                .round()
                .total_cmp(&a.at.y.round())
                .then(a.at.x.total_cmp(&b.at.x))
        });
        found
            .iter()
            .map(|t| t.text.as_str())
            .collect::<Vec<_>>()
            .join(" ")
    };
    let mut views = Vec::new();
    for v in &laid.views {
        let own = sheet.views.iter().find(|s| s.id == v.id);
        let scale = own.and_then(|o| o.scale).unwrap_or(sheet.scale);
        let mut lo = v.centre - v.half;
        let mut hi = v.centre + v.half;
        let mut items: Vec<Value> = Vec::new();
        for n in laid.notes.iter().filter(|n| n.preview.is_none()) {
            let in_view = match &n.kind {
                Placing::Balloon(_) | Placing::Line(_) => false,
                _ => n.view == v.id,
            };
            let mut mine = in_view;
            let text = text_of(&n.rect);
            let mut item = json!({});
            match &n.kind {
                Placing::Note => {
                    let Some(placed) = sheet.notes.get(n.index) else {
                        continue;
                    };
                    let name = &placed.note;
                    let (mut dim_kind, mut measures) = ("note".to_owned(), String::new());
                    let mut reference = false;
                    if let Some(a) = world.said(name) {
                        match &a.note {
                            Note::Dimension {
                                measured,
                                tolerance,
                                ..
                            } => {
                                (dim_kind, measures) = measured_text(measured);
                                reference =
                                    *tolerance == workpiece_core::world::Tolerance::Reference;
                            }
                            Note::Hole { centre, radius, .. } => {
                                dim_kind = "hole".into();
                                measures = format!("hole at {} r {radius:.3}", p3(*centre));
                            }
                            Note::Datum { letter, .. } => {
                                dim_kind = "datum".into();
                                measures = format!("datum {letter}");
                            }
                            Note::Geometric { characteristic, .. } => {
                                dim_kind = "tolerance".into();
                                measures = characteristic.label().to_owned();
                            }
                            Note::Texture { text, .. } => {
                                dim_kind = "texture".into();
                                measures = text.clone();
                            }
                        }
                    }
                    if let Some((sketch, what, is_ref)) = said_by.get(name) {
                        measures = format!("sketch {sketch}: {what}");
                        reference |= *is_ref;
                    } else if let Some(step) = steps.get(name.split('/').next().unwrap_or(name))
                        && let Some(refs) = step.get("references")
                    {
                        measures = format!("{measures} [{refs}]");
                    }
                    let ends = n.linear.map(|l| ends_off(&l, v, sheet.spacing.gap));
                    item = json!({
                        "type": "dim", "id": name, "text": text, "kind": dim_kind,
                        "measures": measures, "reference": reference,
                        "ends_off_by": ends.map(|e| e.iter().map(|x| if *x <= ON { 0.0 } else { r(*x) }).collect::<Vec<f64>>()), "at": rel(
                            (n.rect[0] + n.rect[1]) * 0.5, v.centre),
                    });
                }
                Placing::Own => {
                    let ends = n.linear.map(|l| ends_off(&l, v, sheet.spacing.gap));
                    item = json!({
                        "type": "dim", "id": format!("own{}", n.index), "text": text,
                        "kind": "sheet's own", "measures": "points picked on the view",
                        "reference": false, "ends_off_by": ends.map(|e| e.iter().map(|x| if *x <= ON { 0.0 } else { r(*x) }).collect::<Vec<f64>>()),
                        "at": rel((n.rect[0] + n.rect[1]) * 0.5, v.centre),
                    });
                }
                Placing::Words => {
                    let w = sheet.words.get(n.index);
                    // (Words that belong to a dimension stand in its view.)
                    let home = w.and_then(|w| {
                        w.view.clone().or_else(|| {
                            let d = w.dimension.as_deref()?;
                            sheet
                                .notes
                                .iter()
                                .find(|p| p.note == d || p.note.starts_with(&format!("{d}/")))
                                .map(|p| p.view.clone())
                        })
                    });
                    item = json!({
                        "type": "words", "id": format!("words{}", n.index),
                        "text": w.map_or(text, |w| w.text.replace('\n', " / ")),
                        "of": w.and_then(|w| w.dimension.clone()),
                        "at": rel((n.rect[0] + n.rect[1]) * 0.5, v.centre),
                    });
                    mine = home.as_deref() == Some(v.id.as_str());
                }
                _ => {}
            }
            if mine && !item.is_null() && item != json!({}) {
                lo = lo.min(n.rect[0]);
                hi = hi.max(n.rect[1]);
                if let Some(l) = n.linear {
                    for q in l.from.iter().chain(l.line.iter()) {
                        lo = lo.min(*q);
                        hi = hi.max(*q);
                    }
                }
                items.push(item);
            }
        }
        // Centre lines in this view, balloons, cutting lines.
        for n in laid.notes.iter().filter(|n| n.preview.is_none()) {
            match &n.kind {
                Placing::Line(of) if n.view == v.id => {
                    let a = n.path.first().map(|s| s.0);
                    let b = n.path.last().map(|s| s.1);
                    let own = sheet.lines.get(n.index);
                    items.push(json!({
                        "type": "line", "id": of.label(),
                        "label": own.and_then(|l| l.label.clone()),
                        "symmetry": own.is_some_and(|l| l.symmetry),
                        "from": a.map(|p| rel(p, v.centre)), "to": b.map(|p| rel(p, v.centre)),
                    }));
                }
                Placing::Balloon(name) if n.view == v.id => {
                    let number = item_names
                        .iter()
                        .find(|i| i.placed.iter().any(|p| p == name))
                        .map(|i| i.number);
                    items.push(json!({
                        "type": "balloon", "id": name, "item": number,
                        "at": rel((n.rect[0] + n.rect[1]) * 0.5, v.centre),
                    }));
                }
                _ => {}
            }
        }
        for cut in laid.cuts.iter().filter(|c| c.of == v.id) {
            let letter =
                sheet
                    .views
                    .iter()
                    .find(|s| s.id == cut.view)
                    .and_then(|s| match &s.kind {
                        ViewKind::Section { letter, .. } => Some(letter.clone()),
                        _ => None,
                    });
            items.push(json!({
                "type": "cut", "id": cut.view, "letter": letter,
                "path": cut.path.iter().map(|p| rel(*p, v.centre)).collect::<Vec<_>>(),
            }));
            for p in &cut.path {
                lo = lo.min(*p - Point2::splat(6.0));
                hi = hi.max(*p + Point2::splat(6.0));
            }
        }
        views.push(json!({
            "id": v.id,
            "kind": kind_of(sheet, &v.id, 0),
            "what": own.map(|o| describe(&o.kind)).unwrap_or_else(|| v.label.clone()),
            "label": v.label,
            "scale": format!("{}:{}", scale[0], scale[1]),
            "style": own.map(|o| format!("{:?}", o.style)),
            "at": pt(v.centre),
            "size": pt(v.half * 2.0),
            "box": [r(lo.x), r(lo.y), r(hi.x), r(hi.y)],
            "items": items,
        }));
    }
    let rows: Vec<Value> = item_names
        .iter()
        .map(|i| {
            json!({"item": i.number, "qty": i.placed.len(), "name": i.name,
                   "material": i.material, "placed": i.placed})
        })
        .collect();
    json!({
        "sheet": sheet.name, "size": laid.size, "projection": format!("{:?}", sheet.projection),
        "scale": format!("{}:{}", sheet.scale[0], sheet.scale[1]),
        "views": views, "parts_list": rows, "problems": laid.problems,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_point_is_near_a_segment_by_its_closest_point() {
        let (a, b) = (Point2::new(0.0, 0.0), Point2::new(10.0, 0.0));
        assert!((near_segment(Point2::new(5.0, 3.0), a, b) - 3.0).abs() < 1e-9);
        assert!((near_segment(Point2::new(13.0, 4.0), a, b) - 5.0).abs() < 1e-9);
    }

    #[test]
    fn a_view_beside_the_front_is_the_side_it_is_seen_from() {
        let sheet = |projection| Sheet {
            projection,
            ..Sheet::default()
        };
        // Third angle: the view above the front is the top, the one to the right the right side.
        let third = sheet(Projection::ThirdAngle);
        assert_eq!(projected_side(&third, Side::Front, Beside::Above), "Top");
        assert_eq!(projected_side(&third, Side::Front, Beside::Right), "Right");
        assert_eq!(projected_side(&third, Side::Front, Beside::Below), "Bottom");
        // First angle: the other way about.
        let first = sheet(Projection::FirstAngle);
        assert_eq!(projected_side(&first, Side::Front, Beside::Above), "Bottom");
        assert_eq!(projected_side(&first, Side::Front, Beside::Right), "Left");
    }
}
