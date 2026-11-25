//! What is said of the model on it, for those who make and measure it:
//! which faces it is measured from ([`DatumFeature`], ISO 5459), how
//! large it is and how exactly ([`Dimension`], ISO 129-1), and how true
//! its faces are ([`GeometricTolerance`], ISO 1101). Nothing of the
//! geometry follows from them: a drawing of the part shows them.

mod datum;
mod dimension;
mod geometric;
mod mass;

pub use datum::{DatumFeature, DatumFeatureArgs};
pub use dimension::{Bend, Dimension, DimensionArgs, DimensionKind};
pub use geometric::{GeometricTolerance, GeometricToleranceArgs, Of};
pub use mass::{MassProperties, MassPropertiesArgs};

/// The fields of `args`, for a note that a click on nothing places:
/// with what it is about known (`about`: the middle of it), the click
/// puts its text where the pointer is — as far from the eye as what it
/// is about — and the note is done.
pub(crate) fn placed<A: workpiece_core::Arguments + Clone + 'static>(
    args: &A,
    context: &workpiece_core::EditContext<'_>,
    about: Option<workpiece_core::math::Point3>,
    at: fn(&A, [f64; 3]) -> A,
) -> workpiece_core::EditVisualization<workpiece_core::Edit<A, ()>> {
    let fields = workpiece_core::Arguments::fields(args, context);
    let args = args.clone();
    workpiece_core::EditVisualization::new(
        workpiece_core::widgets::VBox::new(fields),
        Default::default(),
    )
    .placed_by(move |ray| {
        let about = about?;
        let along = (about - ray.start()).dot(ray.direction());
        Some(at(&args, ray.at(along).to_array()))
    })
    .map(workpiece_core::Edit::args)
}

/// Tells `r` of this crate's operations: their kinds, the tools that
/// add them, and their icons.
pub fn register(r: &mut workpiece_core::Registry) {
    use workpiece_core::Tool;
    r.kind::<DatumFeature>("datumfeature");
    r.tool(Tool {
        tab: "Annotate",
        group: "Define",
        label: "Datum",
        icon: "datumfeature",
        key: 'D',
        hint: "Name a face a datum — A, B, C: what the part is measured from, and what its geometric tolerances refer to",
        small: true,
        make: |cx| {
            DatumFeature {
                id: (cx.fresh_id)("datum"),
                args: DatumFeatureArgs::next(cx.world),
            }
            .into()
        },
    });
    r.kind::<Dimension>("dimension3");
    r.tool(Tool {
        tab: "Annotate",
        group: "Define",
        label: "Dimension",
        icon: "dimension3",
        key: 'M',
        hint: "A dimension of the model itself, with its tolerance: the distance between two faces, edges or points, the length of an edge, a diameter or a radius, an angle",
        small: false,
        make: |cx| Dimension { id: (cx.fresh_id)("dimension"), args: Default::default() }.into(),
    });
    r.kind::<GeometricTolerance>("tolerance");
    r.tool(Tool {
        tab: "Annotate",
        group: "Define",
        label: "Tolerance",
        icon: "tolerance",
        key: 'T',
        hint: "A geometric tolerance of a face: its form (flatness, cylindricity…), or its orientation, location or run-out from datums",
        small: true,
        make: |cx| {
            GeometricTolerance {
                id: (cx.fresh_id)("tolerance"),
                args: Default::default(),
            }
            .into()
        },
    });
    r.kind::<MassProperties>("mass");
    r.tool(Tool {
        tab: "Properties",
        group: "Part",
        label: "Mass properties",
        icon: "mass",
        key: '\0',
        hint: "How bodies weigh and turn: their mass, centre of gravity and moments of inertia, each body by what it is made of — shown in the view as the ellipsoid that weighs and turns alike",
        small: false,
        make: |cx| {
            MassProperties {
                id: (cx.fresh_id)("mass"),
                args: MassPropertiesArgs::default(),
            }
            .into()
        },
    });
    r.icon("mass", include_str!("../icons/mass.svg"));
    r.icon("datumfeature", include_str!("../icons/datumfeature.svg"));
    r.icon("dimension3", include_str!("../icons/dimension3.svg"));
    r.icon("tolerance", include_str!("../icons/tolerance.svg"));
}
