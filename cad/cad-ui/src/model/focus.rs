//! Following step paths through placed parts.

use shapecraft_cad_part::Workspace;
use shapecraft_core::Pose;

/// A step of a part: `steps` operations of the part at `part` came before
/// it.
#[derive(Debug, Clone, PartialEq)]
pub struct Context {
    /// The part's path in the workspace.
    pub part: String,
    /// How many of its operations come before.
    pub steps: usize,
    /// Where the part sits in the active part.
    pub pose: Pose,
}

/// Where a path of [`AddPart`](shapecraft_ops_instances::AddPart) steps leads.
#[derive(Debug, Clone, PartialEq)]
pub struct Located {
    /// The placed part's path in the workspace.
    pub part: String,
    /// Where it sits in the active part.
    pub pose: Pose,
    /// The variant of it that is placed; `None` for the part as it is by
    /// default (and for the active part itself).
    pub variant: Option<String>,
    /// Each part on the way, at the step that places the next one.
    pub contexts: Vec<Context>,
}

/// Follows `path`, the ids of `AddPart` steps, from the part at `active`
/// down to the part the last one places.
pub fn locate(workspace: &Workspace, active: &str, path: &[String]) -> anyhow::Result<Located> {
    let mut located = Located {
        part: active.to_owned(),
        pose: Pose::IDENTITY,
        variant: None,
        contexts: Vec::new(),
    };
    for id in path {
        let part = workspace.resolve(&located.part)?;
        let steps = part
            .program
            .iter()
            .position(|op| op.id() == id)
            .ok_or_else(|| anyhow::anyhow!("no step {id:?} in part {:?}", located.part))?;
        let Some((placed_part, placed_variant)) = part.program[steps].placed() else {
            anyhow::bail!("step {id:?} does not place a part");
        };
        located.contexts.push(Context {
            part: located.part.clone(),
            steps,
            pose: located.pose,
        });
        // Where the part is: where its joints or a drag left it.
        let placed = shapecraft_ops_instances::assembly::placed(&part.cache, id);
        located.pose = located.pose * placed;
        located.part = placed_part.to_owned();
        located.variant = placed_variant.map(str::to_owned);
    }
    Ok(located)
}
