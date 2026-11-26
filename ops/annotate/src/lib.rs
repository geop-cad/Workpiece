//! What is said of the model on it, for those who make and measure it:
//! which faces it is measured from ([`DatumFeature`], ISO 5459), how
//! large it is and how exactly ([`Dimension`], ISO 129-1), how true
//! its faces are ([`GeometricTolerance`], ISO 1101) and how rough
//! ([`SurfaceTexture`], ISO 21920-1). Nothing of the
//! geometry follows from them: a drawing of the part shows them.

mod datum;
mod dimension;
mod geometric;
mod mass;
mod texture;

pub use datum::{DatumFeature, DatumFeatureArgs};
pub use dimension::{Bend, Dimension, DimensionArgs, DimensionKind};
pub use geometric::{GeometricTolerance, GeometricToleranceArgs, Of};
pub use mass::{MassProperties, MassPropertiesArgs};
pub use texture::{Process, SurfaceTexture, SurfaceTextureArgs};

/// The axis of the face `face` of `solid` where it is round (part of a
/// cylinder: a bore, a shaft); nothing for any other face.
pub(crate) fn axis_of(
    solid: &workpiece_boolean::Solid,
    face: usize,
) -> Option<workpiece_core::world::FeatureAxis> {
    let round = workpiece_shape::thread::cylinder(solid, face).ok()?;
    let middle = (round.reach.0 + round.reach.1) / 2.0;
    Some(workpiece_core::world::FeatureAxis {
        centre: round.origin + round.axis * middle,
        axis: round.axis,
        radius: round.radius.abs(),
        length: (round.reach.1 - round.reach.0).abs(),
    })
}

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
    r.kind::<SurfaceTexture>("surfacetexture");
    r.tool(Tool {
        tab: "Annotate",
        group: "Define",
        label: "Surface",
        icon: "surfacetexture",
        key: '\0',
        hint: "How rough a face may be (ISO 21920-1): its parameter and limit, Ra 1,6, and whether it is machined — the surface texture symbol on the drawing",
        small: true,
        make: |cx| {
            SurfaceTexture {
                id: (cx.fresh_id)("surface"),
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
    r.icon(
        "surfacetexture",
        include_str!("../icons/surfacetexture.svg"),
    );
    for (name, svg) in OPTION_ICONS {
        r.icon(name, svg);
    }
}

macro_rules! icons {
    ($($name:literal),* $(,)?) => {
        &[$(($name, include_str!(concat!("../icons/", $name, ".svg")))),*]
    };
}

/// The icons of the options of these operations' choices.
const OPTION_ICONS: &[(&str, &str)] = icons!(
    "dim-auto",
    "dim-distance",
    "gdt-angularity",
    "gdt-concentricity",
    "gdt-cylindricity",
    "gdt-flatness",
    "gdt-parallelism",
    "gdt-perpendicularity",
    "gdt-position",
    "gdt-profile-line",
    "gdt-profile-surface",
    "gdt-roundness",
    "gdt-runout",
    "gdt-straightness",
    "gdt-symmetry",
    "gdt-total-runout",
    "tol-deviations",
    "tol-exact",
    "tol-fit",
    "tol-general",
    "tol-limits",
    "tol-reference",
    "texture-any",
    "texture-machined",
    "texture-unmachined",
    "tol-symmetric",
    "zone-planes",
    "zone-round",
);
