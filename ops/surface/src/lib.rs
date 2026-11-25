//! Operations on surfaces and curves in space.

mod curve;
mod surface;

pub use curve::{Curve, CurveArgs, CurveWay, crossing, through, write_wire};
pub use surface::{Surface, SurfaceArgs, SurfaceWay};

/// Tells `r` of this crate's operations: their kinds, the tools that
/// add them, and their icons.
pub fn register(r: &mut shapecraft_core::Registry) {
    use shapecraft_core::Tool;
    r.kind_by::<Surface>(shapecraft_core::IconOf::By(|op| {
        match op.downcast_ref::<Surface>().map(|s| s.args.way) {
            Some(SurfaceWay::Copy) => "copyfaces",
            Some(SurfaceWay::Stitch) => "stitch",
            Some(SurfaceWay::Unstitch) => "unstitch",
            Some(SurfaceWay::Reverse) => "reverse",
            Some(SurfaceWay::Delete) => "deletesurface",
            Some(SurfaceWay::Offset) => "offsetsurface",
            Some(SurfaceWay::Fill) => "fill",
            Some(SurfaceWay::Ruled) => "ruled",
            Some(SurfaceWay::Trim) => "trimsurface",
            Some(SurfaceWay::Split) => "splitsurface",
            Some(SurfaceWay::Fillet) => "fillet",
            Some(SurfaceWay::Boundary) => "boundary",
            Some(SurfaceWay::Untrim) => "untrim",
            Some(SurfaceWay::Extend) => "extendsurface",
            None => "copyfaces",
        }
    }));
    r.tool(Tool {
        tab: "Surfacing",
        group: "Surfaces",
        label: "Copy faces",
        icon: "copyfaces",
        key: 'C',
        hint: "Copy faces of a body (or all of it) into a surface of their own",
        small: true,
        make: |cx| {
            Surface {
                id: (cx.fresh_id)("copyfaces"),
                args: SurfaceArgs {
                    way: SurfaceWay::Copy,
                    ..Default::default()
                },
            }
            .into()
        },
    });
    r.tool(Tool {
        tab: "Surfacing",
        group: "Sew",
        label: "Stitch",
        icon: "stitch",
        key: 'S',
        hint: "Sew surfaces into one where their edges lie on each other; closed all around, they become a solid",
        small: true,
        make: |cx| Surface { id: (cx.fresh_id)("stitch"), args: SurfaceArgs { way: SurfaceWay::Stitch, ..Default::default() } }.into(),
    });
    r.tool(Tool {
        tab: "Surfacing",
        group: "Sew",
        label: "Unstitch",
        icon: "unstitch",
        key: 'U',
        hint: "Take a surface apart: every face a surface of its own",
        small: true,
        make: |cx| {
            Surface {
                id: (cx.fresh_id)("unstitch"),
                args: SurfaceArgs {
                    way: SurfaceWay::Unstitch,
                    ..Default::default()
                },
            }
            .into()
        },
    });
    r.tool(Tool {
        tab: "Surfacing",
        group: "Sew",
        label: "Reverse",
        icon: "reverse",
        key: 'V',
        hint: "Turn a surface around: its back becomes its front",
        small: true,
        make: |cx| {
            Surface {
                id: (cx.fresh_id)("reverse"),
                args: SurfaceArgs {
                    way: SurfaceWay::Reverse,
                    ..Default::default()
                },
            }
            .into()
        },
    });
    r.tool(Tool {
        tab: "Surfacing",
        group: "Edit",
        label: "Delete",
        icon: "deletesurface",
        key: 'D',
        hint: "Remove faces from a surface, or a surface altogether",
        small: true,
        make: |cx| {
            Surface {
                id: (cx.fresh_id)("deletesurface"),
                args: SurfaceArgs {
                    way: SurfaceWay::Delete,
                    ..Default::default()
                },
            }
            .into()
        },
    });
    r.tool(Tool {
        tab: "Surfacing",
        group: "Surfaces",
        label: "Offset",
        icon: "offsetsurface",
        key: 'O',
        hint: "A surface a distance off faces of a body, or off a whole surface",
        small: true,
        make: |cx| {
            Surface {
                id: (cx.fresh_id)("offsetsurface"),
                args: SurfaceArgs {
                    way: SurfaceWay::Offset,
                    ..Default::default()
                },
            }
            .into()
        },
    });
    r.tool(Tool {
        tab: "Surfacing",
        group: "Surfaces",
        label: "Fill",
        icon: "fill",
        key: 'L',
        hint: "Fill the flat holes of a surface with faces; closed all around by that, it becomes a solid",
        small: true,
        make: |cx| Surface { id: (cx.fresh_id)("fill"), args: SurfaceArgs { way: SurfaceWay::Fill, ..Default::default() } }.into(),
    });
    r.tool(Tool {
        tab: "Surfacing",
        group: "Surfaces",
        label: "Ruled",
        icon: "ruled",
        key: 'r',
        hint: "A surface from edges straight along an axis, a distance",
        small: true,
        make: |cx| {
            Surface {
                id: (cx.fresh_id)("ruled"),
                args: SurfaceArgs {
                    way: SurfaceWay::Ruled,
                    ..Default::default()
                },
            }
            .into()
        },
    });
    r.tool(Tool {
        tab: "Surfacing",
        group: "Edit",
        label: "Trim",
        icon: "trimsurface",
        key: 't',
        hint: "Cut a surface back to a plane: what is in front of the plane goes",
        small: true,
        make: |cx| {
            Surface {
                id: (cx.fresh_id)("trimsurface"),
                args: SurfaceArgs {
                    way: SurfaceWay::Trim,
                    ..Default::default()
                },
            }
            .into()
        },
    });
    r.tool(Tool {
        tab: "Surfacing",
        group: "Edit",
        label: "Split",
        icon: "splitsurface",
        key: 'p',
        hint: "Cut a surface in two by a plane",
        small: true,
        make: |cx| {
            Surface {
                id: (cx.fresh_id)("splitsurface"),
                args: SurfaceArgs {
                    way: SurfaceWay::Split,
                    ..Default::default()
                },
            }
            .into()
        },
    });
    r.tool(Tool {
        tab: "Surfacing",
        group: "Edit",
        label: "Surface fillet",
        icon: "fillet",
        key: 'f',
        hint: "Round edges between two faces of a surface, to the distance as a radius",
        small: true,
        make: |cx| {
            Surface {
                id: (cx.fresh_id)("fillet"),
                args: SurfaceArgs {
                    way: SurfaceWay::Fillet,
                    ..Default::default()
                },
            }
            .into()
        },
    });
    r.tool(Tool {
        tab: "Surfacing",
        group: "Surfaces",
        label: "Boundary",
        icon: "boundary",
        key: 'B',
        hint: "A surface between four edges that meet end to end around it, running along each",
        small: true,
        make: |cx| {
            Surface {
                id: (cx.fresh_id)("boundary"),
                args: SurfaceArgs {
                    way: SurfaceWay::Boundary,
                    ..Default::default()
                },
            }
            .into()
        },
    });
    r.tool(Tool {
        tab: "Surfacing",
        group: "Edit",
        label: "Untrim",
        icon: "untrim",
        key: 'u',
        hint: "Every face of a surface back to all of its own surface: what it was before anything was cut from it",
        small: true,
        make: |cx| Surface { id: (cx.fresh_id)("untrim"), args: SurfaceArgs { way: SurfaceWay::Untrim, ..Default::default() } }.into(),
    });
    r.tool(Tool {
        tab: "Surfacing",
        group: "Surfaces",
        label: "Extend",
        icon: "extendsurface",
        key: 'e',
        hint: "Run a surface on past its free edges, by a distance, as its faces go",
        small: true,
        make: |cx| {
            Surface {
                id: (cx.fresh_id)("extendsurface"),
                args: SurfaceArgs {
                    way: SurfaceWay::Extend,
                    ..Default::default()
                },
            }
            .into()
        },
    });
    r.kind::<Curve>("curve");
    r.tool(Tool {
        tab: "Modeling",
        group: "Construct",
        label: "Curve",
        icon: "curve",
        key: 'j',
        hint: "A curve in space as a body: through points, a helix about an axis, or where two faces cross; a sweep can follow it",
        small: true,
        make: |cx| Curve { id: (cx.fresh_id)("curve"), args: Default::default() }.into(),
    });
    r.tool(Tool {
        tab: "Surfacing",
        group: "Construct",
        label: "Curve",
        icon: "curve",
        key: 'j',
        hint: "A curve in space as a body: through points, a helix about an axis, or where two faces cross; a sweep can follow it",
        small: true,
        make: |cx| Curve { id: (cx.fresh_id)("curve"), args: Default::default() }.into(),
    });
    r.icon("boundary", include_str!("../icons/boundary.svg"));
    r.icon("copyfaces", include_str!("../icons/copyfaces.svg"));
    r.icon("curve", include_str!("../icons/curve.svg"));
    r.icon("deletesurface", include_str!("../icons/deletesurface.svg"));
    r.icon("extendsurface", include_str!("../icons/extendsurface.svg"));
    r.icon("fill", include_str!("../icons/fill.svg"));
    r.icon("offsetsurface", include_str!("../icons/offsetsurface.svg"));
    r.icon("reverse", include_str!("../icons/reverse.svg"));
    r.icon("ruled", include_str!("../icons/ruled.svg"));
    r.icon("splitsurface", include_str!("../icons/splitsurface.svg"));
    r.icon("stitch", include_str!("../icons/stitch.svg"));
    r.icon("trim", include_str!("../icons/trim.svg"));
    r.icon("trimsurface", include_str!("../icons/trimsurface.svg"));
    r.icon("unstitch", include_str!("../icons/unstitch.svg"));
    r.icon("untrim", include_str!("../icons/untrim.svg"));
}
