//! The parts list of an assembly's sheet (ISO 7573), over its title
//! block, and the balloons that say which part is which item (ISO 6433):
//! the item's number in a circle, on a narrow leader that ends in a dot
//! on the part.

use std::collections::BTreeMap;

use workpiece_core::math::{Point2, Point3};
use workpiece_core::world::World;

use super::views::Placed;
use super::{Fill, Laid, LaidNote, Pen, Placing, Subject};
use crate::Sheet;
use crate::sheet::{PartsColumn, ViewKind};

/// An item of the parts list: one kind of part, however many of it are
/// placed.
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    /// Its number.
    pub number: usize,
    /// The names its parts are placed under.
    pub placed: Vec<String>,
    /// What it is called.
    pub name: String,
    /// What it is made of, if that is said.
    pub material: Option<String>,
}

/// The items of the parts placed in `world` (not of what is placed in
/// those): a part placed more than once — the same part, as it is built
/// the same, or catalogue parts of one designation — is one item, called
/// by its designation where nothing else names it. Numbered in the order
/// of their names.
pub fn items(world: &World, names: &BTreeMap<String, String>) -> Vec<Item> {
    let mut items: Vec<(u64, usize, Item)> = Vec::new();
    for (placed, instance) in &world.instances {
        let stamp = instance.world.stamp;
        let ptr = std::sync::Arc::as_ptr(&instance.world) as usize;
        let designation = instance.world.designation.clone();
        let name = names.get(placed).cloned().or_else(|| designation.clone());
        let same = items.iter_mut().find(|(s, p, item)| {
            let alike = match (&designation, stamp) {
                (Some(d), _) => *d == item.name,
                (None, 0) => *p == ptr,
                (None, _) => *s == stamp,
            };
            alike && name.as_ref().is_none_or(|n| *n == item.name)
        });
        match same {
            Some((_, _, item)) => item.placed.push(placed.clone()),
            None => items.push((
                stamp,
                ptr,
                Item {
                    number: 0,
                    placed: vec![placed.clone()],
                    name: name.unwrap_or_default(),
                    material: instance.world.material.clone(),
                },
            )),
        }
    }
    let mut items: Vec<Item> = items.into_iter().map(|i| i.2).collect();
    for (k, item) in items.iter_mut().enumerate() {
        item.number = k + 1;
        if item.name.is_empty() {
            item.name = placed_name(&item.placed);
        }
    }
    items
}

/// What parts placed under `placed` are called where nothing else says:
/// the name they are placed under, without the number that tells them
/// apart (`divider1`, `divider2`: `divider`).
fn placed_name(placed: &[String]) -> String {
    let first = &placed[0];
    if placed.len() < 2 {
        return first.clone();
    }
    let trimmed =
        first.trim_end_matches(|c: char| c.is_ascii_digit() || matches!(c, '_' | '-' | '#'));
    if !trimmed.is_empty() && placed.iter().all(|p| p.starts_with(trimmed)) {
        trimmed.to_owned()
    } else {
        first.clone()
    }
}

/// How high the rows of the parts list are, in millimetres.
const ROW: f64 = 7.0;
/// The radius of a balloon's circle, in millimetres.
const BALLOON: f64 = 5.0;

/// Draws the parts list of `sheet` over its title block, and its
/// balloons in its views.
pub(crate) fn draw(laid: &mut Laid, sheet: &Sheet, subject: &Subject<'_>, frames: &[Placed<'_>]) {
    if !sheet.parts_list && !sheet.auto_balloons && sheet.balloons.is_empty() {
        return;
    }
    let items = items(subject.world, subject.parts);
    if items.is_empty() {
        laid.problems
            .push("the sheet has a parts list or balloons, but the model places no parts".into());
        return;
    }
    if sheet.parts_list {
        list(
            laid,
            &items,
            sheet.drafting.letter,
            &sheet.parts_columns,
            subject.densities,
        );
    }
    balloons(laid, sheet, subject, frames, &items);
}

/// The parts list (ISO 7573), standing on the title block, as wide as
/// it: its heads at the foot, the items numbered upwards.
fn list(
    laid: &mut Laid,
    items: &[Item],
    letter: f64,
    columns: &[PartsColumn],
    densities: &BTreeMap<String, f64>,
) {
    let block = laid.cells.iter().fold(None, |b: Option<[Point2; 2]>, c| {
        Some(match b {
            Some([lo, hi]) => [lo.min(c.rect[0]), hi.max(c.rect[1])],
            None => c.rect,
        })
    });
    let [lo, hi] = match block {
        Some(b) => b,
        None => {
            let [w, _] = laid.size;
            [Point2::new(w - 190.0, 10.0), Point2::new(w - 10.0, 10.0)]
        }
    };
    let width = hi.x - lo.x;
    let columns: Vec<PartsColumn> = if columns.is_empty() {
        PartsColumn::USUAL.to_vec()
    } else {
        columns.to_vec()
    };
    // (How wide each is: the name takes what the others leave.)
    let fixed = |c: PartsColumn| match c {
        PartsColumn::Item | PartsColumn::Qty => Some(14.0),
        PartsColumn::Material => Some(46.0),
        PartsColumn::Density => Some(32.0),
        PartsColumn::Name => None,
    };
    let taken: f64 = columns.iter().filter_map(|c| fixed(*c)).sum();
    let names = columns
        .iter()
        .filter(|c| fixed(**c).is_none())
        .count()
        .max(1) as f64;
    let mut edges = vec![0.0];
    for c in &columns {
        let w = fixed(*c).unwrap_or(((width - taken) / names).max(20.0));
        edges.push(edges[edges.len() - 1] + w);
    }
    let width = edges[edges.len() - 1];
    let rows = items.len() + 1;
    let top = hi.y + ROW * rows as f64;
    let at = |x: f64, y: f64| Point2::new(lo.x + x, y);
    // Its frame wide, its rules narrow.
    laid.line(at(0.0, hi.y), at(0.0, top), Pen::Wide);
    laid.line(at(width, hi.y), at(width, top), Pen::Wide);
    laid.line(at(0.0, top), at(width, top), Pen::Wide);
    laid.line(at(0.0, hi.y + ROW), at(width, hi.y + ROW), Pen::Wide);
    for k in 2..rows {
        let y = hi.y + ROW * k as f64;
        laid.line(at(0.0, y), at(width, y), Pen::Narrow);
    }
    for x in &edges[1..edges.len() - 1] {
        laid.line(at(*x, hi.y), at(*x, top), Pen::Narrow);
    }
    let height = letter.min(ROW - 2.5);
    let row = |laid: &mut Laid, k: usize, cells: Vec<String>| {
        let y = hi.y + ROW * k as f64 + ROW / 2.0;
        for (c, text) in cells.into_iter().enumerate() {
            // (Numbers in the middle, words from the left.)
            let (x, anchor) = match columns[c] {
                PartsColumn::Item | PartsColumn::Qty | PartsColumn::Density => {
                    ((edges[c] + edges[c + 1]) / 2.0, [0.5, 0.5])
                }
                _ => (edges[c] + 1.5, [0.0, 0.5]),
            };
            laid.text(at(x, y), text, height, anchor);
        }
    };
    row(
        laid,
        0,
        columns.iter().map(|c| c.head().to_owned()).collect(),
    );
    for item in items {
        let cells = columns
            .iter()
            .map(|c| match c {
                PartsColumn::Item => item.number.to_string(),
                PartsColumn::Qty => item.placed.len().to_string(),
                PartsColumn::Name => item.name.clone(),
                PartsColumn::Material => item.material.clone().unwrap_or_default(),
                PartsColumn::Density => item
                    .material
                    .as_ref()
                    .and_then(|m| densities.get(m))
                    .map(|d| format!("{d} kg/m³"))
                    .unwrap_or_default(),
            })
            .collect();
        row(laid, item.number, cells);
    }
}

/// The balloons: those put by hand where they are put; of the items
/// that have none, where `sheet.auto_balloons` says, one by itself in
/// the first view that shows the part.
fn balloons(
    laid: &mut Laid,
    sheet: &Sheet,
    subject: &Subject<'_>,
    frames: &[Placed<'_>],
    items: &[Item],
) {
    let item_of = |part: &str| items.iter().find(|i| i.placed.iter().any(|p| p == part));
    for b in &sheet.balloons {
        if item_of(&b.part).is_none() {
            laid.problems.push(format!(
                "a balloon points at {}, which is no part placed in the model",
                b.part
            ));
        }
    }
    // (What hides a part: all of the model, as each view sees it — cut
    // open in a section.)
    let reach = subject
        .world
        .extent()
        .map_or(1.0, |(lo, hi)| (hi - lo).length().max(1e-9));
    type Seen = (std::sync::Arc<World>, std::sync::Arc<crate::quick::Shape>);
    let mut seen_by: Vec<(String, Seen)> = Vec::new();
    let mut seen = |frame: &Placed<'_>| -> Seen {
        if let Some((_, kept)) = seen_by.iter().find(|s| s.0 == frame.view.id) {
            return kept.clone();
        }
        let world = match &frame.cutting {
            Some(cut) => {
                let far = reach * 4.0 + cut.origin.length() + 1.0;
                std::sync::Arc::new(crate::section::opened(subject.world, cut, far))
            }
            None => std::sync::Arc::new(subject.world.clone()),
        };
        let mut all = crate::quick::Shape::default();
        crate::quick::shape(&world, workpiece_core::Pose::IDENTITY, &mut all);
        let kept = (world, std::sync::Arc::new(all));
        seen_by.push((frame.view.id.clone(), kept.clone()));
        kept
    };
    let mut done: Vec<(Point2, usize)> = Vec::new();
    // Where a balloon's leader ends, on the part `placed` in the view
    // `placed_view` (on the sheet), and how far that is in.
    let mut on_part = |frame: &Placed<'_>, placed: &str| -> Option<Point2> {
        let (world, all) = seen(frame);
        let instance = world.instances.get(placed)?;
        let mut own = crate::quick::Shape::default();
        crate::quick::shape(&instance.world, instance.pose, &mut own);
        let towards = frame.basis[2];
        // Its faces that look at who looks, the largest first; the
        // first whose middle nothing hides.
        let mut faces: Vec<(f64, Point3)> = own
            .triangles
            .iter()
            .filter_map(|(p, n)| {
                let normal = (n[0] + n[1] + n[2]).normalize_or_zero();
                let area = (p[1] - p[0]).cross(p[2] - p[0]).length() / 2.0;
                (normal.dot(towards) > 0.3 && area > 0.0)
                    .then(|| (area, (p[0] + p[1] + p[2]) / 3.0))
            })
            .collect();
        faces.sort_by(|a, b| b.0.total_cmp(&a.0));
        faces
            .iter()
            .take(60)
            .map(|(_, m)| *m)
            .find(|m| !crate::quick::hidden(&all.triangles, *m, towards, reach * 2e-3))
            .map(|m| frame.sheet(frame.flat(m)))
    };
    let shows = |frame: &&Placed<'_>| {
        frame.cutting.is_none() && !matches!(frame.view.kind, ViewKind::Detail { .. })
    };
    for item in items {
        // Put by hand.
        let said = sheet
            .balloons
            .iter()
            .enumerate()
            .find(|(_, b)| item_of(&b.part).is_some_and(|i| i.number == item.number));
        let (index, frame, centre, tip) = match said {
            Some((index, b)) => {
                let Some(frame) = frames.iter().find(|f| f.view.id == b.view) else {
                    laid.problems.push(format!(
                        "the balloon of {} is in the view {}, which the sheet has not",
                        b.part, b.view
                    ));
                    continue;
                };
                let middle = frame.centre;
                let tip = match b.to {
                    Some(to) => Some(middle + Point2::from_array(to)),
                    None => on_part(frame, &b.part),
                };
                let Some(tip) = tip else {
                    laid.problems.push(format!(
                        "the balloon of {} points at nothing the view {} shows of it",
                        b.part, b.view
                    ));
                    continue;
                };
                (index, frame, middle + Point2::from_array(b.at), tip)
            }
            None if sheet.auto_balloons => {
                let Some((frame, tip)) = frames
                    .iter()
                    .filter(shows)
                    .find_map(|f| Some((f, on_part(f, &item.placed[0])?)))
                else {
                    laid.problems.push(format!(
                        "no view shows the part {} for its balloon",
                        item.name
                    ));
                    continue;
                };
                let centre = clear_spot(laid, frame, tip, &done);
                (usize::MAX, frame, centre, tip)
            }
            None => continue,
        };
        done.push((centre, item.number));
        let way = (tip - centre).normalize_or_zero();
        laid.circle(centre, BALLOON, Pen::Narrow);
        // (Its number a size over the dimensions' lettering, as ISO 6433
        // has it.)
        laid.text(
            centre,
            item.number.to_string(),
            super::title_letter(),
            [0.5, 0.5],
        );
        if tip.distance(centre) > BALLOON + 1.0 {
            laid.line(centre + way * BALLOON, tip, Pen::Narrow);
            dot(laid, tip);
        }
        laid.notes.push(LaidNote {
            index,
            kind: Placing::Balloon(item.placed[0].clone()),
            rect: [
                centre - Point2::splat(BALLOON),
                centre + Point2::splat(BALLOON),
            ],
            view: frame.view.id.clone(),
            preview: None,
            linear: None,
            path: Vec::new(),
            tips: vec![tip],
        });
    }
}

/// A small filled dot at `at`: where a leader ends on a face.
fn dot(laid: &mut Laid, at: Point2) {
    let r = 0.6;
    for k in 0..8 {
        let a = std::f64::consts::TAU * f64::from(k) / 8.0;
        let b = std::f64::consts::TAU * f64::from(k + 1) / 8.0;
        laid.fills.push(Fill {
            corners: [
                at,
                at + Point2::from_angle(a) * r,
                at + Point2::from_angle(b) * r,
            ],
            colour: [0, 0, 0],
        });
    }
}

/// Where a balloon pointing at `tip` stands by itself: outside the view
/// `frame`, out from its middle the way `tip` is, clear of the balloons
/// `done`, of what is written and of the lines drawn.
fn clear_spot(laid: &Laid, frame: &Placed<'_>, tip: Point2, done: &[(Point2, usize)]) -> Point2 {
    let (lo, hi) = frame.bounds();
    let middle = (lo + hi) / 2.0;
    let half = (hi - lo) / 2.0;
    let boxes: Vec<[Point2; 2]> = laid
        .texts
        .iter()
        .filter(|t| !t.preview)
        .map(super::notes::text_box)
        .collect();
    let free = |c: Point2| {
        let r = BALLOON + 1.5;
        let [w, h] = laid.size;
        c.x - r > 15.0
            && c.x + r < w - 15.0
            && c.y - r > 15.0
            && c.y + r < h - 15.0
            && done
                .iter()
                .all(|(d, _)| d.distance(c) > 2.0 * BALLOON + 2.0)
            && boxes.iter().all(|b| {
                let q = c.clamp(b[0], b[1]);
                q.distance(c) > r
            })
            && laid.strokes.iter().all(|s| {
                let d = s.b - s.a;
                let t = ((c - s.a).dot(d) / d.length_squared().max(1e-12)).clamp(0.0, 1.0);
                (s.a + d * t).distance(c) > r
            })
    };
    let base = (tip - middle).normalize_or(Point2::new(1.0, 1.0).normalize());
    for out in [12.0, 20.0, 30.0] {
        for k in 0..36 {
            // (The way to the tip first, then turning off it either way.)
            let turn =
                (k as f64 / 2.0).ceil() * 10f64.to_radians() * if k % 2 == 0 { 1.0 } else { -1.0 };
            let way = Point2::from_angle(base.to_angle() + turn);
            // Out of the view's box that way.
            let tx = if way.x.abs() > 1e-9 {
                half.x / way.x.abs()
            } else {
                f64::INFINITY
            };
            let ty = if way.y.abs() > 1e-9 {
                half.y / way.y.abs()
            } else {
                f64::INFINITY
            };
            let c = middle + way * (tx.min(ty) + out);
            if free(c) {
                return c;
            }
        }
    }
    let tx = if base.x.abs() > 1e-9 {
        half.x / base.x.abs()
    } else {
        f64::INFINITY
    };
    let ty = if base.y.abs() > 1e-9 {
        half.y / base.y.abs()
    } else {
        f64::INFINITY
    };
    middle + base * (tx.min(ty) + 12.0)
}
