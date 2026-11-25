//! Fillet and chamfer: rounding and cutting off the edges of bodies.

use workpiece_core::EdgesOf;
use workpiece_core::{Builder, Measure, Operation};

use workpiece_shape::blend::{self, By, Section, Shape};

fn names(edges: &[workpiece_core::Reference<EdgesOf>]) -> Vec<String> {
    edges.iter().map(|s| s.name().to_owned()).collect()
}

/// What a fillet rounds, and how.
#[derive(
    Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, workpiece_core::Arguments,
)]
pub struct FilletArgs {
    /// The edges rounded; a face or a body stands for all its edges.
    #[arg(required)]
    pub edges: Vec<workpiece_core::Reference<workpiece_core::EdgesOf>>,
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
    #[arg(label = "Corners", options = "Rounded|Mitered")]
    pub mitered: bool,
}

impl Default for FilletArgs {
    fn default() -> Self {
        Self {
            edges: Vec::new(),
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
    /// Whether it starts at another distance across the second face.
    #[arg(label = "Distances", options = "Equal|Two")]
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
    #[arg(label = "Corners", options = "Flat|Mitered")]
    pub mitered: bool,
}

impl Default for ChamferArgs {
    fn default() -> Self {
        Self {
            edges: Vec::new(),
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
