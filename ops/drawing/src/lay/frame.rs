//! The frame of a sheet (ISO 5457) and its title block (ISO 7200).

use crate::Sheet;
use crate::sheet::{Format, Projection};
use crate::template::{Cell, Content};
use std::collections::BTreeMap;

use workpiece_core::Unit;
use workpiece_core::math::Point2;
use workpiece_core::world::World;
use workpiece_core::world::topo::BodyKind;

use super::{Laid, Pen, Subject, scale_text};

/// The width of the border at the left (to file the sheet by), and at
/// the other edges.
pub(crate) const FILING: f64 = 20.0;
pub(crate) const BORDER: f64 = 10.0;
/// The width of the band of the grid reference system, outside the
/// frame.
const GRID: f64 = 5.0;
/// The length of a field of the grid.
const FIELD: f64 = 50.0;

/// The lower left and upper right corner of the drawing space.
pub(crate) fn space(sheet: &Sheet) -> (Point2, Point2) {
    let [w, h] = sheet.size();
    (
        Point2::new(FILING, BORDER),
        Point2::new(w - BORDER, h - BORDER),
    )
}

/// The frame: the border of the drawing space, the centring marks, and
/// the grid by which a place on the sheet is named.
pub(crate) fn frame(laid: &mut Laid, sheet: &Sheet) {
    let [w, h] = sheet.size();
    let (lo, hi) = space(sheet);
    let corners = [lo, Point2::new(hi.x, lo.y), hi, Point2::new(lo.x, hi.y)];
    for k in 0..4 {
        laid.line(corners[k], corners[(k + 1) % 4], Pen::Frame);
    }
    // The band of the grid, 5 wide around the frame.
    let (glo, ghi) = (lo - Point2::splat(GRID), hi + Point2::splat(GRID));
    let outer = [
        glo,
        Point2::new(ghi.x, glo.y),
        ghi,
        Point2::new(glo.x, ghi.y),
    ];
    for k in 0..4 {
        laid.line(outer[k], outer[(k + 1) % 4], Pen::Narrow);
    }
    // Centring marks: at the middle of each edge of the sheet, from the
    // grid's band 10 into the drawing space.
    let (mx, my) = (w / 2.0, h / 2.0);
    laid.line(
        Point2::new(mx, glo.y),
        Point2::new(mx, lo.y + 10.0),
        Pen::Frame,
    );
    laid.line(
        Point2::new(mx, ghi.y),
        Point2::new(mx, hi.y - 10.0),
        Pen::Frame,
    );
    laid.line(
        Point2::new(glo.x, my),
        Point2::new(lo.x + 10.0, my),
        Pen::Frame,
    );
    laid.line(
        Point2::new(ghi.x, my),
        Point2::new(hi.x - 10.0, my),
        Pen::Frame,
    );
    // The fields, 50 long, counted from the centring marks outwards;
    // numbered from the left, lettered from the top (without I and O).
    // On an A4 only along the top and the right.
    let letters: Vec<char> = ('A'..='Z').filter(|c| !matches!(c, 'I' | 'O')).collect();
    let cuts = |middle: f64, from: f64, to: f64| -> Vec<f64> {
        let mut all = vec![from];
        let mut k = ((from - middle) / FIELD).ceil();
        while middle + k * FIELD < to - 1e-9 {
            let at = middle + k * FIELD;
            if at > from + 1e-9 {
                all.push(at);
            }
            k += 1.0;
        }
        all.push(to);
        all
    };
    let across = cuts(mx, lo.x, hi.x);
    let up = cuts(my, lo.y, hi.y);
    let small = sheet.format == Format::A4;
    for (k, pair) in across.windows(2).enumerate() {
        let middle = (pair[0] + pair[1]) / 2.0;
        let name = format!("{}", k + 1);
        for (inner, outer, both) in [(hi.y, ghi.y, true), (lo.y, glo.y, !small)] {
            if !both {
                continue;
            }
            if k > 0 {
                laid.line(
                    Point2::new(pair[0], inner),
                    Point2::new(pair[0], outer),
                    Pen::Narrow,
                );
            }
            laid.text(
                Point2::new(middle, (inner + outer) / 2.0),
                name.clone(),
                3.5,
                [0.5, 0.5],
            );
        }
    }
    let rows = up.len() - 1;
    for (k, pair) in up.windows(2).enumerate() {
        let middle = (pair[0] + pair[1]) / 2.0;
        // (From the top down.)
        let name = letters[(rows - 1 - k) % letters.len()].to_string();
        for (inner, outer, both) in [(hi.x, ghi.x, true), (lo.x, glo.x, !small)] {
            if !both {
                continue;
            }
            if k > 0 {
                laid.line(
                    Point2::new(inner, pair[0]),
                    Point2::new(outer, pair[0]),
                    Pen::Narrow,
                );
            }
            laid.text(
                Point2::new((inner + outer) / 2.0, middle),
                name.clone(),
                3.5,
                [0.5, 0.5],
            );
        }
    }
}

/// A field of the title block: its box, the name of what it holds, in
/// small letters at its top left, and what it holds.
fn field(laid: &mut Laid, lo: Point2, hi: Point2, name: &str, value: &str, height: f64) {
    let corners = [lo, Point2::new(hi.x, lo.y), hi, Point2::new(lo.x, hi.y)];
    for k in 0..4 {
        laid.line(corners[k], corners[(k + 1) % 4], Pen::Narrow);
    }
    laid.text(Point2::new(lo.x + 1.2, hi.y - 1.2), name, 1.8, [0.0, 1.0]);
    if !value.is_empty() {
        // (What does not fit is set smaller, from the heights of ISO
        // 3098, down to 1.8; then squeezed to the field's width.)
        let room = hi.x - lo.x - 3.0;
        let height = [height, 3.5, 2.5, 1.8]
            .into_iter()
            .filter(|h| *h <= height)
            .find(|h| super::notes::width(value, *h) <= room)
            .unwrap_or_else(|| (room / super::notes::width(value, 1.0)).clamp(0.5, 1.8));
        laid.text(
            Point2::new(lo.x + 2.0, lo.y + 1.8),
            value,
            height,
            [0.0, 0.0],
        );
    }
}

/// The sign of how the views are arranged (ISO 5456-2): a cone cut off,
/// from the side and from its small end.
fn projection_symbol(laid: &mut Laid, centre: Point2, projection: Projection) {
    // The cone from the side, its small end to the left: 5 long, 5 and
    // 2.5 across. The view from its small end — two circles — stands
    // to its right in first angle, to its left in third.
    let side = match projection {
        Projection::FirstAngle => -1.0,
        Projection::ThirdAngle => 1.0,
    };
    let cone = centre + Point2::new(side * 4.5, 0.0);
    let round = centre - Point2::new(side * 4.5, 0.0);
    let corners = [
        cone + Point2::new(-2.5, -1.25),
        cone + Point2::new(2.5, -2.5),
        cone + Point2::new(2.5, 2.5),
        cone + Point2::new(-2.5, 1.25),
    ];
    for k in 0..4 {
        laid.line(corners[k], corners[(k + 1) % 4], Pen::Wide);
    }
    laid.line(
        cone - Point2::new(3.5, 0.0),
        cone + Point2::new(3.5, 0.0),
        Pen::Chain,
    );
    laid.circle(round, 2.5, Pen::Wide);
    laid.circle(round, 1.25, Pen::Wide);
    laid.line(
        round - Point2::new(3.5, 0.0),
        round + Point2::new(3.5, 0.0),
        Pen::Chain,
    );
    laid.line(
        round - Point2::new(0.0, 3.5),
        round + Point2::new(0.0, 3.5),
        Pen::Chain,
    );
}

/// What the solid bodies of a world and of the parts placed in it weigh.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Weight {
    /// What those whose material is known weigh, in kilograms.
    pub kg: f64,
    /// The solid bodies whose material is not known (or not in the
    /// library): their names, within the parts they are placed in
    /// (`brg/bearing`).
    pub unweighed: Vec<String>,
}

/// What the solid bodies of `world` and of the parts placed in it (and
/// in those) weigh, each by its own material — the body's, else its
/// part's — and `densities` (kg/m³, by material). Lengths are in
/// `unit`.
pub fn weight(world: &World, unit: Unit, densities: &BTreeMap<String, f64>) -> Weight {
    let cubic = (unit.millimetres() / 1000.0).powi(3);
    let mut all = Weight::default();
    weigh(world, cubic, densities, "", &mut all);
    all
}

fn weigh(
    world: &World,
    cubic: f64,
    densities: &BTreeMap<String, f64>,
    within: &str,
    all: &mut Weight,
) {
    let topo = &world.topo;
    for (id, body) in topo.bodies().iter() {
        if body.kind != BodyKind::Solid {
            continue;
        }
        let (Some(name), Ok(solid)) = (topo.name_of(id), workpiece_boolean::read(topo, id)) else {
            continue;
        };
        let density = world
            .body_look(name)
            .material
            .and_then(|m| densities.get(&m).copied());
        match density {
            Some(d) => all.kg += workpiece_boolean::measure(&solid).volume * cubic * d,
            None => all.unweighed.push(format!("{within}{name}")),
        }
    }
    for (name, instance) in &world.instances {
        // (A placed part's lengths are the assembly's: it is placed as
        // it is.)
        weigh(
            &instance.world,
            cubic,
            densities,
            &format!("{within}{name}/"),
            all,
        );
    }
}

/// How a mass is written in a title block: kilograms (grams under one),
/// or pounds for an inch part; `≥` before it where some bodies are not
/// weighed, nothing where none is.
/// `kg` is what `bodies` solid bodies weigh but `unweighed` of them.
pub fn mass_text(kg: f64, unweighed: usize, bodies: usize, unit: Unit) -> String {
    if bodies == unweighed {
        return String::new();
    }
    let at_least = if unweighed == 0 { "" } else { "≥ " };
    match unit {
        Unit::Inch => format!("{at_least}{:.2} lb", kg / 0.453_592_37),
        _ if kg >= 1.0 => format!("{at_least}{kg:.2} kg"),
        _ => format!("{at_least}{:.1} g", kg * 1000.0),
    }
}

/// `world` with only the bodies and placed parts `names` names (by
/// their names; a placed part's as `part`).
fn shown_only(world: &World, names: &[&String]) -> World {
    let mut only = world.clone();
    let others: Vec<_> = only
        .topo
        .bodies()
        .ids()
        .filter(|b| {
            only.topo
                .name_of(*b)
                .is_none_or(|name| !names.iter().any(|n| *n == name))
        })
        .collect();
    for body in others {
        workpiece_boolean::remove(&mut only.topo, body);
    }
    only.instances.retain(|name, _| names.contains(&name));
    only
}

/// How many solid bodies `world` and the parts placed in it have.
pub fn solid_bodies(world: &World) -> usize {
    let own = world
        .topo
        .bodies()
        .iter()
        .filter(|(_, b)| b.kind == BodyKind::Solid)
        .count();
    own + world
        .instances
        .values()
        .map(|i| solid_bodies(&i.world))
        .sum::<usize>()
}

/// What `cell` of the title block of `sheet` says of `subject`: a
/// property the part gives, else what the sheet and the model say of
/// it — its title (its `Title`, else its name), its material, its mass
/// (of what is placed in it too), whether it is a part or an assembly.
fn said(cell: &Cell, sheet: &Sheet, subject: &Subject<'_>) -> String {
    let given = |name: &str| {
        subject
            .properties
            .get(name)
            .filter(|v| !v.trim().is_empty())
            .cloned()
    };
    match &cell.content {
        Content::Text(text) => text.clone(),
        Content::Property(name) => match name.as_str() {
            "Scale" => scale_text(sheet.scale),
            "Format" => sheet.format.label().to_owned(),
            "Sheet" => "1/1".to_owned(),
            "Projection" => String::new(),
            "General tolerances" => sheet.general_tolerance.clone().unwrap_or_default(),
            "Title" => given("Title")
                .or_else(|| given("Name"))
                .unwrap_or_else(|| subject.name.to_owned()),
            "Name" => given("Name").unwrap_or_else(|| subject.name.to_owned()),
            "Material" => given("Material")
                .or_else(|| subject.material.clone())
                .unwrap_or_default(),
            "Mass" => given("Mass").unwrap_or_else(|| {
                // (Of what the sheet shows, where each of its views
                // says which bodies or parts it shows: a flat pattern's
                // sheet weighs the flat pattern.)
                let shown: Vec<&String> = sheet.views.iter().flat_map(|v| &v.bodies).collect();
                let only = (!sheet.views.is_empty()
                    && sheet.views.iter().all(|v| !v.bodies.is_empty()))
                .then(|| shown_only(subject.world, &shown));
                let world = only.as_ref().unwrap_or(subject.world);
                let w = weight(world, subject.unit, subject.densities);
                mass_text(w.kg, w.unweighed.len(), solid_bodies(world), subject.unit)
            }),
            "Document type" => given("Document type").unwrap_or_else(|| {
                match subject.world.instances.is_empty() {
                    true => "Part drawing",
                    false => "Assembly drawing",
                }
                .to_owned()
            }),
            name => given(name).unwrap_or_default(),
        },
    }
}

/// The title block, in the lower right corner of the drawing space:
/// the cells the sheet has of it (those of ISO 7200, unless its
/// template has others), each with what it says — text of its own, or
/// a property of the part or of the sheet.
pub(crate) fn title_block(laid: &mut Laid, sheet: &Sheet, subject: &Subject<'_>) {
    let (_, hi) = space(sheet);
    let (width, height) = sheet.block.iter().fold((0.0_f64, 0.0_f64), |(w, h), c| {
        (w.max(c.at[0] + c.size[0]), h.max(c.at[1] + c.size[1]))
    });
    if sheet.block.is_empty() {
        return;
    }
    let lo = Point2::new(hi.x - width, BORDER);
    for cell in &sheet.block {
        let from = lo + Point2::from_array(cell.at);
        let to = from + Point2::from_array(cell.size);
        let value = said(cell, sheet, subject);
        field(laid, from, to, &cell.label, &value, cell.height);
        if cell.content == Content::Property("Projection".to_owned()) {
            let centre = Point2::new((from.x + to.x) / 2.0 + 2.5, (from.y + to.y) / 2.0 - 0.7);
            projection_symbol(laid, centre, sheet.projection);
        }
        laid.cells.push(super::LaidCell {
            rect: [from, to],
            label: cell.label.clone(),
            value,
            property: match &cell.content {
                Content::Property(name) => Some(name.clone()),
                Content::Text(_) => None,
            },
        });
    }
    // (Its outline as wide as the edges in view.)
    let corners = [
        lo,
        Point2::new(hi.x, lo.y),
        Point2::new(hi.x, lo.y + height),
        Point2::new(lo.x, lo.y + height),
    ];
    for k in 0..4 {
        laid.line(corners[k], corners[(k + 1) % 4], Pen::Wide);
    }
    // The surface texture of the faces that say none of their own, over
    // the title block at its right: its symbol, then the basic symbol in
    // brackets (ISO 1302, ISO 21920-1: "the others as this").
    if let Some(text) = sheet
        .surface_texture
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())
    {
        let h = super::lettering();
        let wide = super::notes::width(text, h);
        // (From the right: `(`, the basic symbol, `)`; the symbol with
        // its text before them.)
        let foot = lo.y + height + 4.0;
        let close = Point2::new(hi.x - 2.0, foot);
        laid.text(close, ")", h * 1.4, [1.0, 0.0]);
        let basic = close - Point2::new(h * 1.4 * 0.45 + h * 0.9, -0.3);
        super::notes::texture_symbol(
            laid,
            basic,
            h * 0.55,
            "",
            workpiece_core::world::Removal::Any,
        );
        laid.text(
            basic - Point2::new(h * 0.8 * 0.9 + 0.4, 0.3),
            "(",
            h * 1.4,
            [1.0, 0.0],
        );
        let apex = basic - Point2::new(h * 0.8 * 0.9 + 0.4 + h * 1.4 * 0.45 + wide + h * 2.6, 0.0);
        super::notes::texture_symbol(laid, apex, h, text, workpiece_core::world::Removal::Any);
    }
}
