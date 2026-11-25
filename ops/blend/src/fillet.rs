//! Fillet and chamfer: rounding and cutting off the edges of bodies.

use workpiece_core::EdgesOf;
use workpiece_core::{Builder, Measure, Operation};

use workpiece_shape::blend::{self, By, Section, Shape};

fn names(edges: &[workpiece_core::Reference<EdgesOf>]) -> Vec<String> {
    edges.iter().map(|s| s.name().to_owned()).collect()
}

/// The corner at the middle of the first edge `picked` stands for: an
/// edge picked, or the first edge of a face or of a body picked; `None`
/// if none is an edge between two faces of a solid.
fn first_corner(
    topo: &workpiece_core::world::topo::Topo,
    picked: &[String],
) -> Option<workpiece_shape::on::Corner> {
    let name = picked.first()?;
    if let Some(corner) = workpiece_shape::on::corner(topo, name) {
        return Some(corner);
    }
    let (body, _) = workpiece_shape::bodies::bodies_with(topo, &picked[..1])
        .into_iter()
        .next()?;
    let solid = workpiece_boolean::read(topo, body).ok()?;
    let named = |n: &Option<String>, also: &[String]| {
        n.as_deref() == Some(name.as_str()) || also.contains(name)
    };
    let edge = match solid.faces.iter().find(|f| named(&f.name, &f.also)) {
        Some(face) => face.loops.iter().flatten().find_map(|u| match u.walk {
            workpiece_boolean::model::Walk::Edge { edge, .. } => Some(edge),
            _ => None,
        })?,
        // (A body: its first edge.)
        None => 0,
    };
    workpiece_shape::on::corners_of(&solid, edge)
        .into_iter()
        .next()
}

/// The serde default of the `tangent` arguments: on.
fn yes() -> bool {
    true
}

/// The edges a blend of `edges` takes along, with `tangent`, in the
/// world of `context`: shown with those picked while it is edited.
fn implied(
    context: &workpiece_core::EditContext<'_>,
    edges: &[workpiece_core::Reference<EdgesOf>],
    tangent: bool,
) -> Vec<String> {
    if !tangent || edges.is_empty() {
        return Vec::new();
    }
    blend::chained(&context.world.topo, &names(edges))
}

/// A size as typed after a drag: to a thousandth, and more than nothing.
fn dragged(x: f64) -> Measure {
    Measure::new(((x * 1000.0).round() / 1000.0).max(0.001))
}

/// An arrow at the middle of the first edge, across face `face` of the
/// two there, its tip where the blend starts on that face when the size
/// is `size`: the blend starts `per` times its size from the edge.
fn arrow<A: Clone + 'static>(
    corner: &workpiece_shape::on::Corner,
    face: usize,
    per: f64,
    size: f64,
    args: &A,
    put: fn(&mut A, Measure),
) -> Option<workpiece_core::handles::Handle<A>> {
    (per.is_finite() && per > 1e-6).then_some(())?;
    let set = workpiece_shape::ui::set(args, move |a: &mut A, x: f64| put(a, dragged(x / per)));
    let arrow =
        workpiece_core::handles::LinearHandle::new(corner.at, corner.into[face], size * per, set)?;
    Some(arrow.into())
}

/// What a fillet rounds, and how.
#[derive(
    Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, workpiece_core::Arguments,
)]
pub struct FilletArgs {
    /// The edges rounded; a face or a body stands for all its edges.
    #[arg(required)]
    pub edges: Vec<workpiece_core::Reference<workpiece_core::EdgesOf>>,
    /// Whether the edges that run on smoothly from those picked (their
    /// tangent chain, through every corner where the edge goes on
    /// without a bend) are rounded with them: worked out from the model
    /// as it is built, so the chain follows the model when it changes.
    #[serde(default = "yes")]
    #[arg(
        label = "Tangent edges",
        options = "Excluded|Included",
        icons = "tangent-picked|tangent-chain",
        icon = "tangent-chain"
    )]
    pub tangent: bool,
    /// What the size is the size of.
    #[arg(choice, label = "Measured by")]
    pub by: By,
    /// The size: a radius, a width or a distance.
    #[arg(label_by = "self.by.label()")]
    pub size: Measure,
    /// How the round runs from one face to the other.
    #[arg(choice)]
    pub section: Section,
    /// A conic's fullness, between 0 and 1.
    #[arg(when = "self.section == Section::Conic")]
    pub rho: Measure,
    /// Whether corners are left as the rounds cut each other off,
    /// rather than closed by a ball.
    #[arg(
        label = "Corners",
        options = "Rounded|Mitered",
        icons = "corner-round|corner-miter"
    )]
    pub mitered: bool,
}

impl Default for FilletArgs {
    fn default() -> Self {
        Self {
            edges: Vec::new(),
            tangent: true,
            by: By::Radius,
            size: Measure::new(1.0),
            section: Section::Circular,
            rho: Measure::new(0.5),
            mitered: false,
        }
    }
}

/// Rounds edges of bodies.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Fillet {
    /// The operation's id.
    pub id: String,
    /// What is rounded, and how.
    pub args: FilletArgs,
}

impl Operation for Fillet {
    type Args = FilletArgs;
    type EditState = ();

    fn id(&self) -> &str {
        &self.id
    }

    fn id_mut(&mut self) -> &mut String {
        &mut self.id
    }

    fn args(&self) -> &FilletArgs {
        &self.args
    }

    fn args_mut(&mut self) -> &mut FilletArgs {
        &mut self.args
    }

    fn make(&self, builder: &dyn Builder) -> anyhow::Result<workpiece_core::Made<'_>> {
        // (Nothing is changed here: the bodies it makes are written by
        // the builder.)
        let mut out = workpiece_shape::bodies::Out::default();
        let args = &self.args;
        let size = (*builder).measure(&args.size, "the fillet's size")?;
        let rho = match args.section {
            Section::Conic => (*builder).measure(&args.rho, "the conic's rho")?,
            _ => 0.5,
        };
        let shape = Shape::Fillet {
            by: args.by,
            size,
            until: None,
            section: args.section,
            rho,
        };
        let topo = &builder.tracked().topo;
        let blended = blend::blend(
            topo,
            &mut out,
            &self.id,
            &names(&args.edges),
            &shape,
            !args.mitered,
            args.tangent,
        )?;
        // Its size is a size of the model, whichever edges it rounds
        // (picked, or of a face or a body picked): said at the first
        // that is one edge between two faces — and also at the other
        // places of its edges where a view sees it true. A radius as a
        // radius, whatever its section; a width as the straight line
        // from rail to rail; a distance across the first face.
        use workpiece_core::world::{Annotation, Measured, Note};
        let corners = said_at(&blended);
        let measured = |c: &workpiece_shape::on::Corner| match args.by {
            By::Radius => Some(Measured::Radius {
                centre: c.ball(size),
                axis: c.along,
                radius: size,
            }),
            By::Width => {
                let across = (c.into[0] - c.into[1]).length();
                (across > 1e-9).then(|| {
                    let by = size / across;
                    Measured::Length {
                        from: c.at + c.into[0] * by,
                        to: c.at + c.into[1] * by,
                        along: None,
                    }
                })
            }
            By::Distance => Some(Measured::Length {
                from: c.at,
                to: c.at + c.into[0] * size,
                along: None,
            }),
        };
        let mut places = corners.iter().filter_map(measured);
        if let Some(first) = places.next() {
            let mut held = Annotation::held(first, size, &args.size, 0.0);
            if let Note::Dimension { elsewhere, .. } = &mut held.note {
                *elsewhere = places.collect();
            }
            let what = match args.by {
                By::Radius => "radius",
                By::Width => "width",
                By::Distance => "distance",
            };
            out.made
                .annotations
                .push((format!("{}/{what}", self.id), held));
        }
        Ok(out.made)
    }

    fn visualize_edit(
        &self,
        context: &workpiece_core::EditContext<'_>,
        (): &(),
    ) -> workpiece_core::EditVisualization<workpiece_core::Edit<FilletArgs, ()>> {
        let args = &self.args;
        let fields = workpiece_core::Arguments::fields(args, context);
        let mut handles = std::collections::BTreeMap::new();
        // An arrow across the first face at the first edge, its tip
        // where the fillet starts there: dragged, the size follows.
        let size = args
            .size
            .resolve(context.parameters)
            .unwrap_or(args.size.value);
        if let Some(c) = first_corner(&context.world.topo, &names(&args.edges)) {
            // (How far from the edge it starts on each face, for a
            // radius of one.)
            let ball = c.ball(1.0) - c.at;
            let setback = [ball.dot(c.into[0]), ball.dot(c.into[1])];
            let per = match args.by {
                By::Radius => setback[0],
                By::Distance => 1.0,
                By::Width => {
                    let chord = (c.into[0] * setback[0] - c.into[1] * setback[1]).length();
                    setback[0] / chord
                }
            };
            let put: fn(&mut FilletArgs, Measure) = |a, m| a.size = m;
            if let Some(h) = arrow(&c, 0, per, size, args, put) {
                handles.insert("size".to_owned(), h);
            }
        }
        workpiece_core::EditVisualization::new(workpiece_core::widgets::VBox::new(fields), handles)
            .implying(implied(context, &args.edges, args.tangent))
            .map(workpiece_core::Edit::args)
    }

    fn summary(&self) -> String {
        let op = self;
        let a = &op.args;
        let section = match a.section {
            workpiece_shape::Section::Circular => String::new(),
            other => format!(", {}", other.label().to_lowercase()),
        };
        format!(
            "{} {} on {}{section}",
            a.by.label().to_lowercase(),
            a.size,
            crate::fillet::picks(a.edges.len())
        )
    }
}

/// What a chamfer cuts off, and how far.
#[derive(
    Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, workpiece_core::Arguments,
)]
pub struct ChamferArgs {
    /// The edges cut off; a face or a body stands for all its edges.
    #[arg(required)]
    pub edges: Vec<workpiece_core::Reference<workpiece_core::EdgesOf>>,
    /// Whether the edges that run on smoothly from those picked (their
    /// tangent chain, through every corner where the edge goes on
    /// without a bend) are cut off with them: worked out from the model
    /// as it is built, so the chain follows the model when it changes.
    #[serde(default = "yes")]
    #[arg(
        label = "Tangent edges",
        options = "Excluded|Included",
        icons = "tangent-picked|tangent-chain",
        icon = "tangent-chain"
    )]
    pub tangent: bool,
    /// Whether it starts at another distance across the second face.
    #[arg(
        label = "Distances",
        options = "Equal|Two",
        icons = "chamfer-equal|chamfer-two"
    )]
    pub two: bool,
    /// How far from the edge the cut starts, across each face.
    pub distance: Measure,
    /// The distance across the second face, with `two`.
    #[arg(when = "self.two")]
    pub second: Measure,
    /// Whether the two distances change faces.
    #[arg(flip, row, when = "self.two")]
    pub flip: bool,
    /// Whether corners are left as the chamfers cut each other off,
    /// rather than closed by a flat.
    #[arg(
        label = "Corners",
        options = "Flat|Mitered",
        icons = "corner-flat|corner-miter"
    )]
    pub mitered: bool,
}

impl Default for ChamferArgs {
    fn default() -> Self {
        Self {
            edges: Vec::new(),
            tangent: true,
            distance: Measure::new(1.0),
            two: false,
            second: Measure::new(1.0),
            flip: false,
            mitered: false,
        }
    }
}

/// Of how many of its edges a blend's own size is said: at the first
/// that is one edge between two faces, and where a view sees the size
/// true at it or the others.
const EDGES_SAID_AT: usize = 12;

/// Where a blend's own size is said: the corners of the edges it
/// blended (the first [`EDGES_SAID_AT`] of them) where a view sees a
/// size at them true.
fn said_at(blended: &[(workpiece_boolean::Solid, Vec<usize>)]) -> Vec<workpiece_shape::on::Corner> {
    blended
        .iter()
        .flat_map(|(solid, edges)| edges.iter().map(move |&e| (solid, e)))
        .take(EDGES_SAID_AT)
        .flat_map(|(solid, e)| workpiece_shape::on::corners_of(solid, e))
        .collect()
}

/// Cuts edges of bodies off flat.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Chamfer {
    /// The operation's id.
    pub id: String,
    /// What is cut off, and how far.
    pub args: ChamferArgs,
}

impl Operation for Chamfer {
    type Args = ChamferArgs;
    type EditState = ();

    fn id(&self) -> &str {
        &self.id
    }

    fn id_mut(&mut self) -> &mut String {
        &mut self.id
    }

    fn args(&self) -> &ChamferArgs {
        &self.args
    }

    fn args_mut(&mut self) -> &mut ChamferArgs {
        &mut self.args
    }

    fn make(&self, builder: &dyn Builder) -> anyhow::Result<workpiece_core::Made<'_>> {
        // (Nothing is changed here: the bodies it makes are written by
        // the builder.)
        let mut out = workpiece_shape::bodies::Out::default();
        let args = &self.args;
        let first = (*builder).measure(&args.distance, "the chamfer's distance")?;
        let second = if args.two {
            (*builder).measure(&args.second, "the chamfer's second distance")?
        } else {
            first
        };
        let (first, second) = if args.flip {
            (second, first)
        } else {
            (first, second)
        };
        let shape = Shape::Chamfer { first, second };
        let topo = &builder.tracked().topo;
        let blended = blend::blend(
            topo,
            &mut out,
            &self.id,
            &names(&args.edges),
            &shape,
            !args.mitered,
            args.tangent,
        )?;
        // How far it starts from the edge is a size of the model: said
        // at the first edge that is one edge between two faces, across
        // each face that has a distance of its own — and also at the
        // other places of its edges where a view sees it true.
        let corners = said_at(&blended);
        if !corners.is_empty() {
            use workpiece_core::world::{Annotation, Measured, Note};
            let length = |c: &workpiece_shape::on::Corner, k: usize, by: f64| Measured::Length {
                from: c.at,
                to: c.at + c.into[k] * by,
                along: None,
            };
            let mut say = |what: &str, k: usize, by: f64, measure: &Measure| {
                let mut held = Annotation::held(length(&corners[0], k, by), by, measure, 0.0);
                if let Note::Dimension { elsewhere, .. } = &mut held.note {
                    *elsewhere = corners[1..].iter().map(|c| length(c, k, by)).collect();
                }
                out.made
                    .annotations
                    .push((format!("{}/{what}", self.id), held));
            };
            say("distance", 0, first, &args.distance);
            if args.two {
                say("second", 1, second, &args.second);
            }
        }
        Ok(out.made)
    }

    fn visualize_edit(
        &self,
        context: &workpiece_core::EditContext<'_>,
        (): &(),
    ) -> workpiece_core::EditVisualization<workpiece_core::Edit<ChamferArgs, ()>> {
        let args = &self.args;
        let fields = workpiece_core::Arguments::fields(args, context);
        let mut handles = std::collections::BTreeMap::new();
        // An arrow across each face at the first edge that has a
        // distance of its own, its tip where the chamfer starts there.
        if let Some(c) = first_corner(&context.world.topo, &names(&args.edges)) {
            let value = |m: &Measure| m.resolve(context.parameters).unwrap_or(m.value);
            let first = if args.two && args.flip { 1 } else { 0 };
            let put: fn(&mut ChamferArgs, Measure) = |a, m| a.distance = m;
            if let Some(h) = arrow(&c, first, 1.0, value(&args.distance), args, put) {
                handles.insert("distance".to_owned(), h);
            }
            if args.two {
                let put: fn(&mut ChamferArgs, Measure) = |a, m| a.second = m;
                if let Some(h) = arrow(&c, 1 - first, 1.0, value(&args.second), args, put) {
                    handles.insert("second".to_owned(), h);
                }
            }
        }
        workpiece_core::EditVisualization::new(workpiece_core::widgets::VBox::new(fields), handles)
            .implying(implied(context, &args.edges, args.tangent))
            .map(workpiece_core::Edit::args)
    }

    fn summary(&self) -> String {
        let op = self;
        let a = &op.args;
        let how = if a.two {
            format!("{} × {}", a.distance, a.second)
        } else {
            format!("{}", a.distance)
        };
        format!("{how} on {}", crate::fillet::picks(a.edges.len()))
    }
}

/// How many edges, faces or bodies a blend is of, in words.
pub(crate) fn picks(n: usize) -> String {
    match n {
        0 => "(nothing picked)".to_owned(),
        1 => "1 pick".to_owned(),
        n => format!("{n} picks"),
    }
}
