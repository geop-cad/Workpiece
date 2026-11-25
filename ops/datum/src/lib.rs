//! Operations that create datums, one per file: frames, axes and planes,
//! each constructed in the common ways. A construction has modes, and one
//! list of references; a mode takes what it needs from the list by type
//! ([`pick`]), whatever order they were picked in, and fails, saying why,
//! if they are not what it needs.

mod add_axis;
mod add_frame;
mod add_plane;

pub use add_axis::{AddAxis, AddAxisArgs, AxisMode};
pub use add_frame::{AddFrame, AddFrameArgs, FrameMode};
pub use add_plane::{AddPlane, AddPlaneArgs, PlaneMode};

use std::sync::Arc;

use anyhow::Context;
use workpiece_core::widgets::{Choice, ReferenceList};
use workpiece_core::{ReferenceKind, Resolve, Source, Widget, World};

/// The kinds of reference a construction takes.
const KINDS: [ReferenceKind; 4] = [
    ReferenceKind::Point,
    ReferenceKind::Axis,
    ReferenceKind::Plane,
    // (A face that is not flat: what a plane can touch.)
    ReferenceKind::Face,
];

/// How far the text of an angle a datum is set at stands off where its
/// sides meet: it has no size of its own to go by.
const ANGLE_REACH: f64 = 10.0;

/// A number a datum is constructed by, said of the model where it is
/// (see [`Annotation::held`](workpiece_core::world::Annotation::held)):
/// `value`, measured as `measured` says, called `{id}/{what}`.
fn said(
    id: &str,
    what: &str,
    measured: workpiece_core::world::Measured,
    value: f64,
) -> (String, workpiece_core::world::Annotation) {
    let reach = match measured {
        workpiece_core::world::Measured::Angle { .. } => ANGLE_REACH,
        _ => 0.0,
    };
    let measure = workpiece_core::Measure::new(value);
    (
        format!("{id}/{what}"),
        workpiece_core::world::Annotation::held(measured, value, &measure, reach),
    )
}

/// Fails if a datum named `name` already exists.
fn ensure_free(world: &World, name: &str) -> anyhow::Result<()> {
    anyhow::ensure!(!world.datums.contains_key(name), "datum {name:?} exists");
    Ok(())
}

/// The modes of a construction.
pub trait Mode: Copy + PartialEq + 'static {
    /// Every mode, in menu order.
    const ALL: &'static [Self];

    /// The mode's name, e.g. `"Midplane"`.
    fn label(self) -> &'static str;

    /// The mode's icon, by name.
    fn icon(self) -> &'static str;
}

/// What a construction's arguments have in common: a mode, and one list
/// of references the mode takes what it needs from.
trait Construction: Clone + 'static {
    /// The mode type.
    type Mode: Mode;
    /// The mode.
    fn mode(&self) -> Self::Mode;
    /// The references, in the order picked.
    fn references(&self) -> Vec<Source>;
    /// The arguments with `mode` and `references`.
    fn with(&self, mode: Self::Mode, references: Vec<Source>) -> Self;
    /// Fails, saying why, unless the references construct the datum in
    /// `world`.
    fn check(&self, world: &World) -> anyhow::Result<()>;
}

/// `result`, its error saying which mode failed.
fn in_mode<T>(mode: impl Mode, result: anyhow::Result<T>) -> anyhow::Result<T> {
    result.context(mode.label())
}

/// Whether `args` construct its datum in `world`.
fn constructs<A: Construction>(args: &A, world: &World) -> bool {
    args.check(world).is_ok()
}

/// `args` with `references`: the mode stays if it can construct from
/// them in `world`, else it becomes the first that can, if any.
fn with_references<A: Construction>(args: &A, world: &World, references: Vec<Source>) -> A {
    let same = args.with(args.mode(), references);
    if constructs(&same, world) {
        return same;
    }
    A::Mode::ALL
        .iter()
        .map(|&mode| same.with(mode, same.references()))
        .find(|other| constructs(other, world))
        .unwrap_or(same)
}

/// The references in `selection` a construction takes: the points, axes
/// and planes of `world`.
fn taken(world: &World, selection: &[Source]) -> Vec<Source> {
    let takes = |s: &&Source| KINDS.iter().any(|k| k.accepts(world, s));
    selection.iter().filter(takes).cloned().collect()
}

/// Whether `args`, made from `selection`, use all of it and construct
/// their datum in `world`.
fn fits<A: Construction>(args: &A, world: &World, selection: &[Source]) -> bool {
    args.references() == selection && constructs(args, world)
}

/// What a mode takes from its references: a tuple of points
/// ([`Position`]), axes and planes.
trait Pick: Sized {
    /// The kind of each value.
    fn kinds() -> Vec<ReferenceKind>;
    /// The values, from `sources` in this order.
    fn in_order(world: &World, sources: &[&Source]) -> Option<Self>;
}

impl Pick for () {
    fn kinds() -> Vec<ReferenceKind> {
        Vec::new()
    }

    fn in_order(_: &World, _: &[&Source]) -> Option<Self> {
        Some(())
    }
}

macro_rules! pick_tuple {
    ($($value:ident $i:tt),+) => {
        impl<$($value: Resolve),+> Pick for ($($value,)+) {
            fn kinds() -> Vec<ReferenceKind> {
                vec![$($value::KIND),+]
            }

            fn in_order(world: &World, sources: &[&Source]) -> Option<Self> {
                Some(($($value::resolve(world, sources.get($i)?).ok()?,)+))
            }
        }
    };
}
pick_tuple!(A 0);
pick_tuple!(A 0, B 1);
pick_tuple!(A 0, B 1, C 2);

/// Every order of `0..n`, the given one first.
fn orders(n: usize) -> Vec<Vec<usize>> {
    fn go(n: usize, order: &mut Vec<usize>, out: &mut Vec<Vec<usize>>) {
        if order.len() == n {
            out.push(order.clone());
            return;
        }
        for i in 0..n {
            if !order.contains(&i) {
                order.push(i);
                go(n, order, out);
                order.pop();
            }
        }
    }
    let mut out = Vec::new();
    go(n, &mut Vec::new(), &mut out);
    out
}

/// What a mode takes, from all of `sources`, whatever order they were
/// picked in: the first order (the picked one first) in which each
/// resolves as what is asked for. A frame axis is an axis and a plane, so
/// which reference plays which role is only known by trying. Fails,
/// saying what is needed and what was picked instead.
fn pick<P: Pick>(world: &World, sources: &[impl std::borrow::Borrow<Source>]) -> anyhow::Result<P> {
    let sources: Vec<Source> = sources.iter().map(|s| s.borrow().clone()).collect();
    let sources = sources.as_slice();
    let kinds = P::kinds();
    let found = (sources.len() == kinds.len())
        .then(|| {
            orders(sources.len()).into_iter().find_map(|order| {
                let ordered: Vec<&Source> = order.iter().map(|&i| &sources[i]).collect();
                P::in_order(world, &ordered)
            })
        })
        .flatten();
    found.with_context(|| {
        let needs: Vec<String> = kinds.iter().map(|k| k.name().to_owned()).collect();
        let picked: Vec<String> = sources.iter().map(|s| described(world, s)).collect();
        format!("needs {}. Picked: {}.", counted(&needs), counted(&picked))
    })
}

/// What `source` is in `world`, of what a construction takes: `"axis or
/// plane"`.
fn described(world: &World, source: &Source) -> String {
    let names: Vec<&str> = KINDS
        .iter()
        .filter(|k| k.accepts(world, source))
        .map(|k| k.name())
        .collect();
    if names.is_empty() {
        "unknown reference".to_owned()
    } else {
        names.join(" or ")
    }
}

/// A construction's reference box and its modes, the ones that cannot
/// construct from the references greyed out, saying why.
fn construction_fields<A: Construction>(args: &A, world: &Arc<World>) -> Vec<Widget<A>> {
    let set_references = {
        let (args, world) = (args.clone(), world.clone());
        move |references| with_references(&args, &world, references)
    };
    let modes = A::Mode::ALL;
    let labels = modes.iter().map(|m| m.label().to_owned()).collect();
    let checks: Vec<anyhow::Result<()>> = modes
        .iter()
        .map(|&m| in_mode(m, args.with(m, args.references()).check(world)))
        .collect();
    let enabled = checks.iter().map(Result::is_ok).collect();
    let reasons = checks
        .iter()
        .map(|check| match check {
            Ok(()) => String::new(),
            Err(error) => format!("{error:#}"),
        })
        .collect();
    let selected = modes.iter().position(|&m| m == args.mode()).unwrap_or(0);
    let set_mode = {
        let args = args.clone();
        move |i: usize| args.with(modes[i], args.references())
    };
    let mut references = ReferenceList::new(
        "References",
        KINDS.to_vec(),
        args.references(),
        set_references,
    );
    // Empty, it is what is picked first: a construction that needs
    // nothing (a frame at a pose) starts from what is picked as well.
    if args.references().is_empty() {
        references = references.required();
    }
    vec![
        references.into(),
        // (Many: a list, each with its picture.)
        Choice::new("Construction", labels, selected, set_mode)
            .with_icons(modes.iter().map(|m| Some(m.icon())).collect())
            .drop_down()
            .with_enabled(enabled)
            .with_reasons(reasons)
            .into(),
    ]
}

/// `names`, counted and joined: `"2 planes and a point"`.
fn counted(names: &[String]) -> String {
    let mut counts: Vec<(&str, usize)> = Vec::new();
    for name in names {
        match counts.iter_mut().find(|(n, _)| n == name) {
            Some((_, count)) => *count += 1,
            None => counts.push((name, 1)),
        }
    }
    let parts: Vec<String> = counts
        .into_iter()
        .map(|(name, count)| match count {
            1 if name.starts_with(['a', 'e', 'i', 'o', 'u']) => format!("an {name}"),
            1 => format!("a {name}"),
            n => format!(
                "{n} {}",
                if name == "axis" {
                    "axes".into()
                } else {
                    format!("{name}s")
                }
            ),
        })
        .collect();
    match parts.split_last() {
        None => "nothing".to_owned(),
        Some((last, [])) => last.clone(),
        Some((last, rest)) => format!("{} and {last}", rest.join(", ")),
    }
}

/// Tells `r` of this crate's operations: their kinds, the tools that
/// add them, and their icons.
pub fn register(r: &mut workpiece_core::Registry) {
    use workpiece_core::Tool;
    r.kind::<AddPlane>("plane");
    r.tool(Tool {
        tab: "Modeling",
        group: "Construct",
        label: "Plane",
        icon: "plane",
        key: 'P',
        hint: "A datum plane, from what is selected or picked next",
        small: false,
        make: |cx| AddPlane::from_selection((cx.fresh_id)("plane"), cx.world, cx.selection).into(),
    });
    r.tool(Tool {
        tab: "Surfacing",
        group: "Construct",
        label: "Plane",
        icon: "plane",
        key: 'P',
        hint: "A datum plane, from what is selected or picked next",
        small: true,
        make: |cx| AddPlane::from_selection((cx.fresh_id)("plane"), cx.world, cx.selection).into(),
    });
    r.tool(Tool {
        tab: "Sheet metal",
        group: "Construct",
        label: "Plane",
        icon: "plane",
        key: 'P',
        hint: "A datum plane, from what is selected or picked next",
        small: true,
        make: |cx| AddPlane::from_selection((cx.fresh_id)("plane"), cx.world, cx.selection).into(),
    });
    r.kind::<AddAxis>("axis");
    r.tool(Tool {
        tab: "Modeling",
        group: "Construct",
        label: "Axis",
        icon: "axis",
        key: 'A',
        hint: "A datum axis, from what is selected or picked next",
        small: true,
        make: |cx| AddAxis::from_selection((cx.fresh_id)("axis"), cx.world, cx.selection).into(),
    });
    r.tool(Tool {
        tab: "Surfacing",
        group: "Construct",
        label: "Axis",
        icon: "axis",
        key: 'A',
        hint: "A datum axis, from what is selected or picked next",
        small: true,
        make: |cx| AddAxis::from_selection((cx.fresh_id)("axis"), cx.world, cx.selection).into(),
    });
    r.tool(Tool {
        tab: "Sheet metal",
        group: "Construct",
        label: "Axis",
        icon: "axis",
        key: 'A',
        hint: "A datum axis, from what is selected or picked next",
        small: true,
        make: |cx| AddAxis::from_selection((cx.fresh_id)("axis"), cx.world, cx.selection).into(),
    });
    r.kind::<AddFrame>("frame");
    r.tool(Tool {
        tab: "Modeling",
        group: "Construct",
        label: "Frame",
        icon: "frame",
        key: 'F',
        hint: "A datum frame, from what is selected or picked next",
        small: true,
        make: |cx| AddFrame::from_selection((cx.fresh_id)("frame"), cx.world, cx.selection).into(),
    });
    r.tool(Tool {
        tab: "Surfacing",
        group: "Construct",
        label: "Frame",
        icon: "frame",
        key: 'F',
        hint: "A datum frame, from what is selected or picked next",
        small: true,
        make: |cx| AddFrame::from_selection((cx.fresh_id)("frame"), cx.world, cx.selection).into(),
    });
    r.tool(Tool {
        tab: "Sheet metal",
        group: "Construct",
        label: "Frame",
        icon: "frame",
        key: 'F',
        hint: "A datum frame, from what is selected or picked next",
        small: true,
        make: |cx| AddFrame::from_selection((cx.fresh_id)("frame"), cx.world, cx.selection).into(),
    });
    r.icon("axis", include_str!("../icons/axis.svg"));
    r.icon("frame", include_str!("../icons/frame.svg"));
    r.icon("parallel", include_str!("../icons/parallel.svg"));
    r.icon("plane", include_str!("../icons/plane.svg"));
    for (name, svg) in OPTION_ICONS {
        r.icon(name, svg);
    }
}

/// What a construction is, in a few words: its mode, and what of.
pub(crate) fn constructed(mode: &str, references: &[Source]) -> String {
    if references.is_empty() {
        return format!("{} (nothing picked)", mode.to_lowercase());
    }
    let from: Vec<String> = references.iter().map(Source::described).collect();
    format!("{} of {}", mode.to_lowercase(), from.join(", "))
}

macro_rules! icons {
    ($($name:literal),* $(,)?) => {
        &[$(($name, include_str!(concat!("../icons/", $name, ".svg")))),*]
    };
}

/// The icons of the options of these operations' choices.
const OPTION_ICONS: &[(&str, &str)] = icons!(
    "axis-2points",
    "axis-edge",
    "axis-normal",
    "axis-parallel-point",
    "axis-planes",
    "frame-3points",
    "frame-axis-point",
    "frame-plane",
    "frame-plane-point",
    "frame-point",
    "frame-pose",
    "plane-3points",
    "plane-angle",
    "plane-axis-point",
    "plane-midplane",
    "plane-normal-axis",
    "plane-offset",
    "plane-parallel-point",
    "plane-tangent",
);
