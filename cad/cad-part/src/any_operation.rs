//! Any operation a part's program can hold: those the crates of
//! operations this program is built with tell of.

use std::sync::OnceLock;

pub use workpiece_core::{AnyEdit, AnyEditState, AnyOperation};

/// The operations there are — their kinds, tools and icons — as every
/// crate of operations tells of its own. Asked for before the first
/// part is read: a part's steps are read by it.
pub fn registry() -> &'static workpiece_core::Registry {
    static BUILT: OnceLock<&'static workpiece_core::Registry> = OnceLock::new();
    BUILT.get_or_init(|| {
        let mut all = workpiece_core::Registry::default();
        workpiece_ops_sketch::register(&mut all);
        workpiece_ops_datum::register(&mut all);
        workpiece_ops_basic_shapes::register(&mut all);
        workpiece_ops_sweep::register(&mut all);
        workpiece_ops_features::register(&mut all);
        workpiece_ops_blend::register(&mut all);
        workpiece_ops_faces::register(&mut all);
        workpiece_ops_bodies::register(&mut all);
        workpiece_ops_surface::register(&mut all);
        workpiece_ops_sheet_metal::register(&mut all);
        workpiece_ops_annotate::register(&mut all);
        workpiece_ops_drawing::register(&mut all);
        workpiece_ops_appearance::register(&mut all);
        workpiece_ops_mesh::register(&mut all);
        workpiece_ops_step::register(&mut all);
        workpiece_ops_instances::register(&mut all);
        workpiece_ops_standard::register(&mut all);
        workpiece_core::registry::install(all)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use workpiece_core::world::World;

    #[test]
    fn every_tool_makes_a_step_that_reads_back_and_has_its_icon_and_own_key() -> anyhow::Result<()>
    {
        let all = registry();
        let world = World::default();
        let fresh = |stem: &str| format!("{stem}1");
        let context = workpiece_core::ToolContext {
            fresh_id: &fresh,
            world: &world,
            unit: workpiece_core::Unit::default(),
            selection: &[],
        };
        let mut keys = std::collections::BTreeSet::new();
        for tool in all.tools() {
            assert!(all.svg(tool.icon).is_some(), "no icon {}", tool.icon);
            assert!(
                keys.insert((tool.tab, tool.key)),
                "{} shares its key {:?} on {}",
                tool.label,
                tool.key,
                tool.tab
            );
            // What it makes is written as its kind and read back the same.
            let op = (tool.make)(&context);
            assert!(all.svg(op.icon()).is_some(), "no icon of {}", op.kind());
            let text = serde_json::to_string(&op)?;
            assert!(text.starts_with(&format!("{{\"{}\":", op.kind())), "{text}");
            assert_eq!(serde_json::from_str::<AnyOperation>(&text)?, op);
        }
        // Every kind there is has a tool, but the import (a file opened)
        // and the primitives, which older parts still read.
        let made: std::collections::BTreeSet<_> = all
            .tools()
            .iter()
            .map(|t| (t.make)(&context).kind())
            .collect();
        let without: Vec<_> = all
            .kinds()
            .map(|k| k.name)
            .filter(|k| !made.contains(k))
            .collect();
        assert_eq!(
            without,
            [
                "AddBox",
                "AddCone",
                "AddCylinder",
                "AddSphere",
                "AddTorus",
                "ImportStep",
                "Mate"
            ]
        );
        assert!(serde_json::from_str::<AnyOperation>("{\"Nothing\":{}}").is_err());
        Ok(())
    }
}
