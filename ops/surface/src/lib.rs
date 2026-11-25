//! Operations on surfaces and curves in space.

mod boundary;
mod curve;
mod surface;

pub use boundary::{Boundary, BoundaryArgs, BoundarySide, Continuity};
pub use curve::{
    CurveEnd, CurveIntersection, CurveIntersectionArgs, CurveProjection, CurveProjectionArgs,
    CurveThrough, CurveThroughArgs, EndHeld, EndWay, Helix, HelixArgs, crossing, through,
    write_wire,
};
pub use surface::{SplitBy, Surface, SurfaceArgs, SurfaceWay};

/// Tells `r` of this crate's operations: their kinds, the tools that
/// add them, and their icons.
pub fn register(r: &mut workpiece_core::Registry) {
    use workpiece_core::Tool;
    r.kind_by::<Surface>(workpiece_core::IconOf::By(|op| {
        match op.downcast_ref::<Surface>().map(|s| s.args.way) {
            Some(SurfaceWay::Offset) | None => "offsetsurface",
            Some(SurfaceWay::Delete) => "deletesurface",
            Some(SurfaceWay::Split) => "splitsurface",
            Some(SurfaceWay::Extend) => "extendsurface",
        }
    }));
    // Each way of Surface its tool: what it does, where it stands.
    let ways: [(Make, &str, &str, &str, char, &str); 4] = [
        (
            |cx| made(cx, "offsetsurface", SurfaceWay::Offset),
            "Surfaces",
            "Offset",
            "offsetsurface",
            'O',
            "A surface a distance off faces of a body, or off a whole surface: along the faces' normals, or flipped against them; by no distance, a copy of the faces",
        ),
        (
            |cx| made(cx, "extendsurface", SurfaceWay::Extend),
            "Surfaces",
            "Extend",
            "extendsurface",
            'e',
            "Run a surface on past its free edges, by a distance, as its faces go; or back to its faces' whole surfaces, what they were before anything was cut from them",
        ),
        (
            |cx| made(cx, "splitsurface", SurfaceWay::Split),
            "Edit",
            "Split",
            "splitsurface",
            'p',
            "Split faces along a sketch thrown onto them or along curves on them; or cut a surface in two by a plane",
        ),
        (
            |cx| made(cx, "deletesurface", SurfaceWay::Delete),
            "Edit",
            "Delete",
            "deletesurface",
            'D',
            "Delete faces or whole surfaces; faces of a solid — a hole, a pocket, a boss — are taken off and the faces around closed over where they were",
        ),
    ];
    for (make, group, label, icon, key, hint) in ways {
        r.tool(Tool {
            tab: "Surfacing",
            group,
            label,
            icon,
            key,
            hint,
            small: true,
            make,
        });
    }
    r.kind::<Boundary>("boundary");
    r.tool(Tool {
        tab: "Surfacing",
        group: "Create",
        label: "Boundary",
        icon: "boundary",
        key: 'B',
        hint: "A surface over a hole of any number of sides — flat where they lie in a plane: pick the edges around it, each free or running on smoothly (G1, G2) from the face beside it; it is sewn to the surfaces around, a solid where that closes them",
        small: false,
        make: |cx| Boundary { id: (cx.fresh_id)("boundary"), args: Default::default() }.into(),
    });
    r.kind::<CurveThrough>("curve-through");
    r.kind::<Helix>("helix");
    r.kind::<CurveIntersection>("curve-crossing");
    r.kind::<CurveProjection>("curve-projection");
    // The curves: each way its own step, offered in one menu.
    for tab in ["Modeling", "Surfacing"] {
        curve_tools(r, tab);
    }
    // In surfacing, the curve through points is a button of its own as
    // well: a curve to build surfaces on, held at its ends.
    r.tool(Tool {
        tab: "Surfacing",
        group: "Construct",
        label: THROUGH,
        icon: "curve-through",
        key: 'J',
        hint: "A smooth curve through points picked one after another, each end free or held: tangent (G1) or curvature (G2) to an edge or an axis, lying in or square to a face or a plane",
        small: false,
        make: |cx| CurveThrough { id: (cx.fresh_id)("curve"), args: Default::default() }.into(),
    });
    r.icon("curve-through", include_str!("../icons/curve-through.svg"));
    r.icon("helix", include_str!("../icons/helix.svg"));
    r.icon(
        "curve-crossing",
        include_str!("../icons/curve-crossing.svg"),
    );
    r.icon(
        "curve-projection",
        include_str!("../icons/curve-projection.svg"),
    );
    r.icon("boundary", include_str!("../icons/boundary.svg"));
    r.icon("curve", include_str!("../icons/curve.svg"));
    r.icon("deletesurface", include_str!("../icons/deletesurface.svg"));
    r.icon("extendsurface", include_str!("../icons/extendsurface.svg"));
    r.icon("offsetsurface", include_str!("../icons/offsetsurface.svg"));
    r.icon("splitsurface", include_str!("../icons/splitsurface.svg"));
    r.icon("split-plane", include_str!("../icons/split-plane.svg"));
    r.icon("untrim", include_str!("../icons/untrim.svg"));
    // How a curve, a section or a side runs on at an end (the sweep's
    // sections take them too).
    for (name, svg) in [
        ("end-free", include_str!("../icons/end-free.svg")),
        ("end-tangent", include_str!("../icons/end-tangent.svg")),
        ("end-curvature", include_str!("../icons/end-curvature.svg")),
        ("end-in-face", include_str!("../icons/end-in-face.svg")),
        ("end-normal", include_str!("../icons/end-normal.svg")),
    ] {
        r.icon(name, svg);
    }
}

/// What a tool calls to make its step.
type Make = fn(&workpiece_core::ToolContext<'_>) -> workpiece_core::AnyOperation;

/// A new surface step of `way`, with an id from `stem`.
fn made(
    cx: &workpiece_core::ToolContext<'_>,
    stem: &str,
    way: SurfaceWay,
) -> workpiece_core::AnyOperation {
    Surface {
        id: (cx.fresh_id)(stem),
        args: SurfaceArgs {
            way,
            ..Default::default()
        },
    }
    .into()
}

/// The label of the curve through points.
const THROUGH: &str = "Curve through points";

/// The tools that make curves in space, on `tab`, and the menu that
/// offers them.
fn curve_tools(r: &mut workpiece_core::Registry, tab: &'static str) {
    use workpiece_core::Tool;
    const HELIX: &str = "Helix or spiral";
    const CROSSING: &str = "Where faces cross";
    const PROJECTION: &str = "Curve onto faces";
    r.tool(Tool {
        tab,
        group: "Construct",
        label: THROUGH,
        icon: "curve-through",
        key: 'j',
        hint: "A smooth curve through points picked one after another — vertices, frames, places on faces — open or closed; a sweep can follow it",
        small: true,
        make: |cx| CurveThrough { id: (cx.fresh_id)("curve"), args: Default::default() }.into(),
    });
    r.tool(Tool {
        tab,
        group: "Construct",
        label: HELIX,
        icon: "helix",
        key: 'y',
        hint: "A helix about an axis, or a spiral that grows as it turns: its radius, height, turns and start dragged in the view",
        small: true,
        make: |cx| Helix { id: (cx.fresh_id)("helix"), args: Default::default() }.into(),
    });
    r.tool(Tool {
        tab,
        group: "Construct",
        label: CROSSING,
        icon: "curve-crossing",
        key: 'q',
        hint: "The curve where two faces cross",
        small: true,
        make: |cx| {
            CurveIntersection {
                id: (cx.fresh_id)("crossing"),
                args: Default::default(),
            }
            .into()
        },
    });
    r.tool(Tool {
        tab,
        group: "Construct",
        label: PROJECTION,
        icon: "curve-projection",
        key: 'o',
        hint: "Edges, curves or whole sketches thrown onto faces along a direction (a sketch along its normal unless one is picked)",
        small: true,
        make: |cx| CurveProjection { id: (cx.fresh_id)("projection"), args: Default::default() }.into(),
    });
    r.menu(workpiece_core::Menu {
        tab,
        group: "Construct",
        label: "Curve",
        icon: "curve",
        hint: "A curve in space as a body of its own, to sweep along or build on: through points, a helix, where faces cross, or thrown onto faces",
        small: true,
        tools: &[THROUGH, HELIX, CROSSING, PROJECTION],
    });
}
