//! Standard parts: screws, nuts, washers and bearings by their sizes in
//! the standards, in millimetres.

use std::f64::consts::TAU;

use anyhow::Context;
use workpiece_boolean::{Op, Solid, combine};
use workpiece_core::math::{Point2, Point3};
use workpiece_core::widgets::{Choice, HBox};
use workpiece_core::world::topo::Curve3;
use workpiece_core::{Builder, Edit, EditContext, EditVisualization, Operation, Widget};
use workpiece_ops_instances::placing::{self, Hold};

use workpiece_shape::swept::{Outline, Sides, Weighted, circle, polygon, steady, swept};
use workpiece_shape::ui;

/// What kind of part.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
pub enum StandardKind {
    /// A hexagon head screw (ISO 4017), its thread drawn plain.
    #[default]
    HexScrew,
    /// A hexagon socket head cap screw (ISO 4762), its thread and its
    /// socket drawn plain.
    SocketScrew,
    /// A hexagon nut (ISO 4032), its thread drawn plain.
    HexNut,
    /// A plain washer (ISO 7089).
    Washer,
    /// A deep groove ball bearing (ISO 15), drawn as its two rings.
    Bearing,
}

/// A thread size: its name, the thread's diameter, the hexagon's width
/// across flats, the head's height, the nut's height, and the washer's
/// hole, width and thickness.
type Thread = (&'static str, f64, f64, f64, f64, [f64; 3]);

/// The diameter of a socket head cap screw's head (ISO 4762), by its
/// thread's; the head is as high as the thread is wide.
fn socket_head(d: f64) -> f64 {
    match d {
        d if d <= 3.0 => 5.5,
        d if d <= 4.0 => 7.0,
        d if d <= 5.0 => 8.5,
        d if d <= 6.0 => 10.0,
        d if d <= 8.0 => 13.0,
        d if d <= 10.0 => 16.0,
        d if d <= 12.0 => 18.0,
        _ => 24.0,
    }
}

/// What a kind of part is, to look for it by: the catalogue's filter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    /// Screws of every head.
    Screws,
    /// Nuts.
    Nuts,
    /// Washers.
    Washers,
    /// Bearings.
    Bearings,
}

impl Family {
    /// Every family.
    pub const ALL: [Family; 4] = [
        Family::Screws,
        Family::Nuts,
        Family::Washers,
        Family::Bearings,
    ];

    /// What it is called.
    pub fn label(self) -> &'static str {
        match self {
            Family::Screws => "Screws",
            Family::Nuts => "Nuts",
            Family::Washers => "Washers",
            Family::Bearings => "Bearings",
        }
    }
}

/// The thread sizes, as the standards have them.
const THREADS: [Thread; 8] = [
    ("M3", 3.0, 5.5, 2.0, 2.4, [3.2, 7.0, 0.5]),
    ("M4", 4.0, 7.0, 2.8, 3.2, [4.3, 9.0, 0.8]),
    ("M5", 5.0, 8.0, 3.5, 4.7, [5.3, 10.0, 1.0]),
    ("M6", 6.0, 10.0, 4.0, 5.2, [6.4, 12.0, 1.6]),
    ("M8", 8.0, 13.0, 5.3, 6.8, [8.4, 16.0, 1.6]),
    ("M10", 10.0, 16.0, 6.4, 8.4, [10.5, 20.0, 2.0]),
    ("M12", 12.0, 18.0, 7.5, 10.8, [13.0, 24.0, 2.5]),
    ("M16", 16.0, 24.0, 10.0, 14.8, [17.0, 30.0, 3.0]),
];

/// The bearings: each one's name, bore, outside diameter and width.
const BEARINGS: [(&str, f64, f64, f64); 8] = [
    ("608", 8.0, 22.0, 7.0),
    ("6000", 10.0, 26.0, 8.0),
    ("6001", 12.0, 28.0, 8.0),
    ("6200", 10.0, 30.0, 9.0),
    ("6201", 12.0, 32.0, 10.0),
    ("6202", 15.0, 35.0, 11.0),
    ("6204", 20.0, 47.0, 14.0),
    ("6205", 25.0, 52.0, 15.0),
];

impl StandardKind {
    /// Every kind.
    pub const ALL: [StandardKind; 5] = [
        StandardKind::HexScrew,
        StandardKind::SocketScrew,
        StandardKind::HexNut,
        StandardKind::Washer,
        StandardKind::Bearing,
    ];

    /// What it is called.
    pub fn label(self) -> &'static str {
        match self {
            StandardKind::HexScrew => "Hexagon head screw",
            StandardKind::SocketScrew => "Socket head cap screw",
            StandardKind::HexNut => "Hexagon nut",
            StandardKind::Washer => "Plain washer",
            StandardKind::Bearing => "Deep groove ball bearing",
        }
    }

    /// The standard that gives its sizes.
    pub fn norm(self) -> &'static str {
        match self {
            StandardKind::HexScrew => "ISO 4017",
            StandardKind::SocketScrew => "ISO 4762",
            StandardKind::HexNut => "ISO 4032",
            StandardKind::Washer => "ISO 7089",
            StandardKind::Bearing => "ISO 15",
        }
    }

    /// What a parts list calls one of size `size` (a screw `length`
    /// long): `Socket head cap screw ISO 4762 M5×16`, `Deep groove ball
    /// bearing 6204`.
    pub fn designation(self, size: &str, length: f64) -> String {
        match self {
            StandardKind::Bearing => format!("{} {size}", self.label()),
            _ if self.long() => format!("{} {} {size}×{length}", self.label(), self.norm()),
            _ => format!("{} {} {size}", self.label(), self.norm()),
        }
    }

    /// How a section treats it (ISO 128-50): a screw cut along its axis
    /// is drawn whole; a nut, a washer and a bearing's rings are cut.
    pub fn sectioned(self) -> workpiece_core::world::Sectioned {
        match self.family() {
            Family::Screws => workpiece_core::world::Sectioned::WholeAlong(Point3::Z),
            _ => workpiece_core::world::Sectioned::Cut,
        }
    }

    /// What it is, to look for it by.
    pub fn family(self) -> Family {
        match self {
            StandardKind::HexScrew | StandardKind::SocketScrew => Family::Screws,
            StandardKind::HexNut => Family::Nuts,
            StandardKind::Washer => Family::Washers,
            StandardKind::Bearing => Family::Bearings,
        }
    }

    /// Whether it has a length to choose: a screw's, under its head.
    pub fn long(self) -> bool {
        self.family() == Family::Screws
    }

    /// The sizes it comes in, by name.
    pub fn sizes(self) -> Vec<&'static str> {
        match self {
            StandardKind::Bearing => BEARINGS.iter().map(|b| b.0).collect(),
            _ => THREADS.iter().map(|t| t.0).collect(),
        }
    }
}

/// The solid an outline sweeps straight up z from `from` to `to`.
fn column(id: &str, outline: &(Sides, Weighted), from: f64, to: f64) -> anyhow::Result<Solid> {
    let path = Curve3::line(Point3::new(0.0, 0.0, from), Point3::new(0.0, 0.0, to))?;
    let at = steady(&outline.1);
    let outline = Outline {
        sides: outline.0.clone(),
        at: &at,
    };
    swept(
        id,
        &outline,
        &path,
        path.domain(),
        Point3::Y,
        (to - from).abs() * 4.0,
    )
}

/// A hexagon `flats` wide across its flats.
fn hexagon(flats: f64) -> (Sides, Weighted) {
    let radius = flats / 3.0_f64.sqrt();
    let corners: Vec<Point2> = (0..6)
        .map(|k| Point2::from_angle(TAU * f64::from(k) / 6.0) * radius)
        .collect();
    polygon(&corners)
}

/// The ring between the diameters `bore` and `outside`, `height` high.
fn ring(id: &str, bore: f64, outside: f64, height: f64) -> anyhow::Result<Solid> {
    let disk = column(id, &circle(Point2::ZERO, outside / 2.0), 0.0, height)?;
    let hole = column(
        &format!("{id}/bore"),
        &circle(Point2::ZERO, bore / 2.0),
        -height,
        height * 2.0,
    )?;
    Ok(combine(Op::Cut, &disk, &hole)?.solid)
}

/// The bodies of the part of `kind` and `size` (see [`standard`]), each
/// with its name: one, called `id`; of a bearing, its outer ring `id`
/// and its inner ring `id/inner` (cut, each hatched its own way).
pub fn bodies(
    id: &str,
    kind: StandardKind,
    size: &str,
    length: f64,
) -> anyhow::Result<Vec<(Solid, String)>> {
    if kind != StandardKind::Bearing {
        return Ok(vec![(standard(id, kind, size, length)?, id.to_owned())]);
    }
    let (bore, outside, width) = bearing(size)?;
    // (Each ring a third of the section high; the balls between them
    // are not drawn.)
    let wall = (outside - bore) / 2.0 * 0.36;
    let inner = format!("{id}/inner");
    Ok(vec![
        (
            ring(id, outside - 2.0 * wall, outside, width)?,
            id.to_owned(),
        ),
        (ring(&inner, bore, bore + 2.0 * wall, width)?, inner),
    ])
}

/// A bearing's bore, outside diameter and width.
fn bearing(size: &str) -> anyhow::Result<(f64, f64, f64)> {
    let &(_, bore, outside, width) = BEARINGS
        .iter()
        .find(|b| b.0 == size)
        .with_context(|| format!("there is no bearing {size:?}"))?;
    Ok((bore, outside, width))
}

/// The part of `kind` and `size` (a screw `length` long under its
/// head) as one solid on the xy plane, about z: a screw's head above
/// the plane and its shank below, anything else standing on it (a
/// bearing as one ring from its bore to its outside).
pub fn standard(id: &str, kind: StandardKind, size: &str, length: f64) -> anyhow::Result<Solid> {
    let thread = || {
        THREADS
            .iter()
            .find(|t| t.0 == size)
            .with_context(|| format!("there is no thread size {size:?}"))
    };
    match kind {
        StandardKind::HexScrew => {
            let &(_, d, flats, head, ..) = thread()?;
            anyhow::ensure!(length > 0.0, "a screw has some length");
            let head = column(&format!("{id}/head"), &hexagon(flats), 0.0, head)?;
            let shank = column(
                &format!("{id}/shank"),
                &circle(Point2::ZERO, d / 2.0),
                -length,
                0.0,
            )?;
            Ok(combine(Op::Union, &head, &shank)?.solid)
        }
        StandardKind::SocketScrew => {
            let &(_, d, ..) = thread()?;
            anyhow::ensure!(length > 0.0, "a screw has some length");
            let head = circle(Point2::ZERO, socket_head(d) / 2.0);
            let head = column(&format!("{id}/head"), &head, 0.0, d)?;
            let shank = column(
                &format!("{id}/shank"),
                &circle(Point2::ZERO, d / 2.0),
                -length,
                0.0,
            )?;
            Ok(combine(Op::Union, &head, &shank)?.solid)
        }
        StandardKind::HexNut => {
            let &(_, d, flats, _, height, _) = thread()?;
            let body = column(id, &hexagon(flats), 0.0, height)?;
            let hole = column(
                &format!("{id}/bore"),
                &circle(Point2::ZERO, d / 2.0),
                -height,
                height * 2.0,
            )?;
            Ok(combine(Op::Cut, &body, &hole)?.solid)
        }
        StandardKind::Washer => {
            let &(.., [bore, outside, height]) = thread()?;
            ring(id, bore, outside, height)
        }
        StandardKind::Bearing => {
            let (bore, outside, width) = bearing(size)?;
            ring(id, bore, outside, width)
        }
    }
}

/// The lengths screws come in (ISO 4017, ISO 4762), in millimetres.
const LENGTHS: [f64; 24] = [
    6.0, 8.0, 10.0, 12.0, 16.0, 20.0, 25.0, 30.0, 35.0, 40.0, 45.0, 50.0, 55.0, 60.0, 65.0, 70.0,
    80.0, 90.0, 100.0, 110.0, 120.0, 130.0, 140.0, 150.0,
];

/// The shortest and the longest screw of each thread size (ISO 4017;
/// socket head cap screws come in these too).
const LENGTH_RANGES: [(&str, f64, f64); 8] = [
    ("M3", 6.0, 30.0),
    ("M4", 8.0, 40.0),
    ("M5", 10.0, 50.0),
    ("M6", 12.0, 60.0),
    ("M8", 16.0, 80.0),
    ("M10", 20.0, 100.0),
    ("M12", 25.0, 120.0),
    ("M16", 30.0, 150.0),
];

/// The lengths a screw of thread `size` comes in: those of the
/// standard between its shortest and its longest.
pub fn lengths(size: &str) -> Vec<f64> {
    let Some(&(_, shortest, longest)) = LENGTH_RANGES.iter().find(|r| r.0 == size) else {
        return Vec::new();
    };
    LENGTHS
        .iter()
        .copied()
        .filter(|l| *l >= shortest && *l <= longest)
        .collect()
}

/// One part of the catalogue: its kind, its size and, for a screw, its
/// length.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    /// What kind of part.
    pub kind: StandardKind,
    /// Its size.
    pub size: &'static str,
    /// A screw's length; nothing else has one.
    pub length: Option<f64>,
}

impl Entry {
    /// What it is called in the catalogue: `Hexagon head screw M6 × 20
    /// · ISO 4017`.
    pub fn label(&self) -> String {
        let (kind, norm) = (self.kind.label(), self.kind.norm());
        match self.length {
            Some(length) => format!("{kind} {} × {length} · {norm}", self.size),
            None => format!("{kind} {} · {norm}", self.size),
        }
    }
}

/// Every part of the catalogue, kind by kind and size by size.
pub fn catalogue() -> Vec<Entry> {
    let mut all = Vec::new();
    for kind in StandardKind::ALL {
        for size in kind.sizes() {
            if kind.long() {
                all.extend(lengths(size).into_iter().map(|length| Entry {
                    kind,
                    size,
                    length: Some(length),
                }));
            } else {
                all.push(Entry {
                    kind,
                    size,
                    length: None,
                });
            }
        }
    }
    all
}

/// What of the catalogue shows while a standard part is chosen: only
/// one family of parts, only one size. Not kept with the part.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Filter {
    /// The only family that shows; all of them while `None`.
    pub family: Option<Family>,
    /// The only size that shows; every size while `None`.
    pub size: Option<String>,
}

impl Filter {
    /// Whether `entry` shows.
    pub fn takes(&self, entry: &Entry) -> bool {
        self.family.is_none_or(|f| entry.kind.family() == f)
            && self.size.as_deref().is_none_or(|s| entry.size == s)
    }

    /// The sizes to filter by: those of the family that shows, each
    /// once.
    fn sizes(&self) -> Vec<&'static str> {
        let mut sizes = Vec::new();
        for kind in StandardKind::ALL {
            if self.family.is_none_or(|f| kind.family() == f) {
                for size in kind.sizes() {
                    if !sizes.contains(&size) {
                        sizes.push(size);
                    }
                }
            }
        }
        sizes
    }
}

/// Which part it is, and how it is held where it is put.
#[derive(
    Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, workpiece_core::Arguments,
)]
#[serde(default)]
pub struct StandardPartArgs {
    /// What kind of part.
    #[arg(hidden)]
    pub kind: StandardKind,
    /// Its size, as the standard names it: `M6`, `608`.
    #[arg(hidden)]
    pub size: String,
    /// A screw's length under its head, in millimetres: one of those
    /// its size comes in.
    #[arg(hidden)]
    pub length: f64,
    /// How it is held: free, fixed, or by a joint between a reference
    /// on what is there already and one of its own.
    #[arg(hidden)]
    pub hold: Hold,
}

impl Default for StandardPartArgs {
    fn default() -> Self {
        Self {
            kind: StandardKind::HexScrew,
            size: "M6".into(),
            length: 20.0,
            hold: Hold::default(),
        }
    }
}

/// Places a part out of the standards — a screw, a nut, a washer, a
/// bearing, by its size — as a part of its own named after the
/// operation's id: dragged, fixed or held by a joint like any part
/// that is placed. Its numbers are millimetres.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct StandardPart {
    /// The operation's id; also the name of what it places.
    pub id: String,
    /// Which part it is, and how it is held.
    pub args: StandardPartArgs,
}

impl Operation for StandardPart {
    type Args = StandardPartArgs;
    type EditState = Filter;

    fn settles(&self) -> bool {
        true
    }

    fn id(&self) -> &str {
        &self.id
    }

    fn id_mut(&mut self) -> &mut String {
        &mut self.id
    }

    fn args(&self) -> &StandardPartArgs {
        &self.args
    }

    fn args_mut(&mut self) -> &mut StandardPartArgs {
        &mut self.args
    }

    fn make(&self, builder: &dyn Builder) -> anyhow::Result<workpiece_core::Made<'_>> {
        let args = &self.args;
        if args.kind.long() {
            anyhow::ensure!(
                lengths(&args.size).contains(&args.length),
                "a screw {} does not come {} long",
                args.size,
                args.length
            );
        }
        // A part of its own, about its own origin: a screw's head above
        // its xy plane and its shank below.
        // Of steel, as fasteners and bearings are; called in a parts
        // list by its designation.
        let mut world = workpiece_core::World {
            material: Some("Steel".into()),
            designation: Some(args.kind.designation(&args.size, args.length)),
            sectioned: args.kind.sectioned(),
            ..workpiece_core::World::new()
        };
        for (solid, name) in bodies(&self.id, args.kind, &args.size, args.length)? {
            workpiece_boolean::write(&mut world.topo, &solid, &name)?;
        }
        let mut made = workpiece_core::Made::default();
        workpiece_ops_instances::placing::place(
            &mut made,
            builder,
            &self.id,
            std::sync::Arc::new(world),
            &args.hold,
        )?;
        Ok(made)
    }

    fn visualize_edit(
        &self,
        context: &EditContext<'_>,
        filter: &Filter,
    ) -> EditVisualization<Edit<StandardPartArgs, Filter>> {
        // The catalogue, searched and filtered; then what it goes onto
        // and with what, and the constraint between the two; where it
        // is, and the gizmo, from the part's cache — as for any part
        // that is placed.
        let mut all: Vec<Widget<Edit<StandardPartArgs, Filter>>> = filter_fields(filter)
            .into_iter()
            .map(|field| field.map(Edit::state))
            .collect();
        all.push(catalogue_field(&self.args, filter).map(Edit::args));
        // (A screw or a nut is fastened where it is put, most likely.)
        let prefer = Some(workpiece_core::world::JointKind::Fastened);
        for field in placing::hold_fields(&self.args.hold, context.step, prefer) {
            let args = self.args.clone();
            all.push(field.map(move |hold| {
                Edit::args(StandardPartArgs {
                    hold,
                    ..args.clone()
                })
            }));
        }
        let (place, handles) = placing::pose_edit(&self.id, context, &self.args.hold);
        all.extend(place);
        EditVisualization::new(workpiece_core::widgets::VBox::new(all), handles)
    }

    fn summary(&self) -> String {
        let a = &self.args;
        let held = a.hold.constraint.said();
        if a.kind.long() {
            format!("{} {} × {}{held}", a.kind.label(), a.size, a.length)
        } else {
            format!("{} {}{held}", a.kind.label(), a.size)
        }
    }
}

/// The two filters of the catalogue, side by side: a family of parts,
/// and a size. Another family keeps the size only if it comes in it.
fn filter_fields(filter: &Filter) -> Vec<Widget<Filter>> {
    let mut families = vec!["All parts".to_owned()];
    families.extend(Family::ALL.iter().map(|f| f.label().to_owned()));
    let family = filter
        .family
        .and_then(|f| Family::ALL.iter().position(|o| *o == f))
        .map_or(0, |i| i + 1);
    let with_family = {
        let filter = filter.clone();
        move |i: usize| {
            let mut filter = Filter {
                family: i.checked_sub(1).map(|i| Family::ALL[i]),
                size: filter.size.clone(),
            };
            let sizes = filter.sizes();
            filter.size = filter.size.filter(|s| sizes.contains(&s.as_str()));
            filter
        }
    };
    let sizes = filter.sizes();
    let mut labels = vec!["Any size".to_owned()];
    labels.extend(sizes.iter().map(|s| (*s).to_owned()));
    let size = filter
        .size
        .as_deref()
        .and_then(|s| sizes.iter().position(|o| *o == s))
        .map_or(0, |i| i + 1);
    let with_size = {
        let filter = filter.clone();
        move |i: usize| Filter {
            family: filter.family,
            size: i.checked_sub(1).map(|i| sizes[i].to_owned()),
        }
    };
    vec![
        HBox::new(vec![
            Choice::new("Show", families, family, with_family)
                .drop_down()
                .into(),
            Choice::new("Size", labels, size, with_size)
                .drop_down()
                .into(),
        ])
        .into(),
    ]
}

/// The catalogue as a list that is searched by typing: the parts the
/// filter lets through, the one that is placed chosen.
fn catalogue_field(args: &StandardPartArgs, filter: &Filter) -> Widget<StandardPartArgs> {
    let entries: Vec<Entry> = catalogue()
        .into_iter()
        .filter(|e| filter.takes(e))
        .collect();
    let labels = entries.iter().map(Entry::label).collect();
    let now = |e: &Entry| {
        e.kind == args.kind && e.size == args.size && e.length.is_none_or(|l| l == args.length)
    };
    // (None of them, while the filter hides the one that is placed.)
    let chosen = entries.iter().position(now).unwrap_or(usize::MAX);
    let set = ui::set(args, move |a, i: usize| {
        let entry = &entries[i];
        a.kind = entry.kind;
        a.size = entry.size.to_owned();
        a.length = entry.length.unwrap_or(a.length);
    });
    Choice::new("Part", labels, chosen, set).searched().into()
}
