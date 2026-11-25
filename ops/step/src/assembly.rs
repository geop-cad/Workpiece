//! Where the solids of a file are: a file that is an assembly holds
//! each part once, in its own coordinates, and says where every use of
//! it is placed in the part above, up to the whole.
//!
//! A solid is an item of a representation. A product's shape is one
//! (`SHAPE_DEFINITION_REPRESENTATION`), perhaps another that is declared
//! the same (`SHAPE_REPRESENTATION_RELATIONSHIP`). A use of a part in an
//! assembly (`NEXT_ASSEMBLY_USAGE_OCCURRENCE`) comes with a relationship
//! between the two representations that carries a transformation
//! (`CONTEXT_DEPENDENT_SHAPE_REPRESENTATION`): two placements, one to
//! be brought onto the other.

use std::collections::HashMap;

use workpiece_core::math::Point3;

use crate::geometry::{Frame, Reader};

/// A turn and a shift: where the axes go, and the origin.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Move {
    x: Point3,
    y: Point3,
    z: Point3,
    o: Point3,
}

impl Move {
    const NONE: Move = Move {
        x: Point3::X,
        y: Point3::Y,
        z: Point3::Z,
        o: Point3::ZERO,
    };

    /// Where `p` goes.
    pub(crate) fn of(&self, p: Point3) -> Point3 {
        self.o + self.x * p.x + self.y * p.y + self.z * p.z
    }

    /// Whether it moves nothing.
    pub(crate) fn is_none(&self) -> bool {
        *self == Move::NONE
    }

    /// What brings the frame `from` onto the frame `to`.
    fn between(from: &Frame, to: &Frame) -> Move {
        let at = |p: Point3| {
            let d = p - from.o;
            to.o + to.x * d.dot(from.x) + to.y * d.dot(from.y) + to.z * d.dot(from.z)
        };
        let o = at(Point3::ZERO);
        Move {
            x: at(Point3::X) - o,
            y: at(Point3::Y) - o,
            z: at(Point3::Z) - o,
            o,
        }
    }

    /// `self` after `first`.
    fn after(&self, first: &Move) -> Move {
        let o = self.of(first.o);
        Move {
            x: self.of(first.o + first.x) - o,
            y: self.of(first.o + first.y) - o,
            z: self.of(first.o + first.z) - o,
            o,
        }
    }
}

/// The placements of a file's representations.
pub(crate) struct Assembly {
    /// The representation each item is in.
    home: HashMap<u32, u32>,
    /// The representations that are one shape, by a name for the set.
    same: HashMap<u32, u32>,
    /// For a set of representations, each use of it: the set it is
    /// placed in, and how.
    uses: HashMap<u32, Vec<(u32, Move)>>,
}

impl Assembly {
    /// Reads the structure of the file of `r`.
    pub(crate) fn new(r: &Reader<'_>) -> Self {
        let file = r.file;
        // The items of every representation.
        let mut items: HashMap<u32, Vec<u32>> = HashMap::new();
        for i in file.instances() {
            let args = if i.parts().count() == 1 {
                if !i.name().ends_with("REPRESENTATION") {
                    continue;
                }
                i.own()
            } else {
                match i.arguments("REPRESENTATION") {
                    Some(a) => a,
                    None => continue,
                }
            };
            if let Some(list) = args.list(1) {
                items.insert(i.id, list.values().filter_map(|v| v.reference()).collect());
            }
        }
        let mut home = HashMap::new();
        for (rep, list) in &items {
            for item in list {
                home.insert(*item, *rep);
            }
        }
        // Representations declared the same shape.
        let mut same: HashMap<u32, u32> = items.keys().map(|k| (*k, *k)).collect();
        fn root(same: &mut HashMap<u32, u32>, mut k: u32) -> u32 {
            while let Some(&up) = same.get(&k) {
                if up == k {
                    break;
                }
                k = up;
            }
            k
        }
        for i in file.of_type("SHAPE_REPRESENTATION_RELATIONSHIP") {
            if i.parts().count() != 1 {
                continue;
            }
            if let (Some(a), Some(b)) = (i.own().reference(2), i.own().reference(3)) {
                let (a, b) = (root(&mut same, a), root(&mut same, b));
                same.insert(a, b);
            }
        }
        let keys: Vec<u32> = same.keys().copied().collect();
        for k in keys {
            let top = root(&mut same, k);
            same.insert(k, top);
        }
        // The shape of each product definition.
        let mut shape: HashMap<u32, u32> = HashMap::new();
        for i in file.of_type("SHAPE_DEFINITION_REPRESENTATION") {
            let (Some(property), Some(rep)) = (i.own().reference(0), i.own().reference(1)) else {
                continue;
            };
            if let Some(definition) = file.get(property).and_then(|p| p.own().reference(2)) {
                shape.insert(definition, rep);
            }
        }
        // Each use of a part in an assembly.
        let mut uses: HashMap<u32, Vec<(u32, Move)>> = HashMap::new();
        for i in file.of_type("CONTEXT_DEPENDENT_SHAPE_REPRESENTATION") {
            let found = (|| {
                let relation = file.get(i.own().reference(0)?)?;
                let pair = relation.arguments("REPRESENTATION_RELATIONSHIP")?;
                let (first, second) = (pair.reference(2)?, pair.reference(3)?);
                let operator = relation
                    .arguments("REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION")?
                    .reference(0)?;
                let operator = file.get(operator)?.own();
                let (one, two) = (operator.reference(2)?, operator.reference(3)?);
                // Which of the two is the part used: the shape of what
                // the occurrence relates, if the file says.
                let occurrence = file.get(i.own().reference(1)?)?.own().reference(2)?;
                let used = file
                    .get(occurrence)
                    .and_then(|o| o.own().reference(4))
                    .and_then(|definition| shape.get(&definition))
                    .map(|rep| *same.get(rep).unwrap_or(rep));
                let set = |rep: u32| *same.get(&rep).unwrap_or(&rep);
                let (child, parent) = if used == Some(set(second)) && used != Some(set(first)) {
                    (second, first)
                } else {
                    (first, second)
                };
                // The placement on the part's side is brought onto the
                // one on the assembly's: by which representation each is
                // an item of, or else in the order of the two.
                let within =
                    |item: u32, rep: u32| items.get(&rep).is_some_and(|l| l.contains(&item));
                let (from, to) = if within(two, child) && within(one, parent) && !within(one, child)
                {
                    (two, one)
                } else if child == first {
                    (one, two)
                } else {
                    (two, one)
                };
                let (from, to) = (r.frame(from).ok()?, r.frame(to).ok()?);
                Some((set(child), set(parent), Move::between(&from, &to)))
            })();
            if let Some((child, parent, by)) = found {
                uses.entry(child).or_default().push((parent, by));
            }
        }
        Self { home, same, uses }
    }

    /// Every place the item `item` (a solid) is at in the whole: one
    /// that moves nothing if the file places it nowhere.
    pub(crate) fn places(&self, item: u32) -> Vec<Move> {
        let Some(rep) = self.home.get(&item) else {
            return vec![Move::NONE];
        };
        let mut out = Vec::new();
        self.up(*self.same.get(rep).unwrap_or(rep), Move::NONE, 0, &mut out);
        out
    }

    fn up(&self, set: u32, so_far: Move, depth: usize, out: &mut Vec<Move>) {
        match self.uses.get(&set) {
            Some(uses) if depth < 64 => {
                for (parent, by) in uses {
                    self.up(*parent, by.after(&so_far), depth + 1, out);
                }
            }
            _ => out.push(so_far),
        }
    }
}
