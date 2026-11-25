//! How bodies and faces look and what they are made of: a material by
//! its name, and a colour over the material's.

use std::collections::BTreeMap;

use super::World;
use super::topo::FaceId;

/// How something looks: what it is made of, and its colour. What is
/// not said is as what it is part of has it — a face as its body, a
/// body as the part.
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Default, serde::Serialize, serde::Deserialize,
)]
pub struct Look {
    /// The material it is made of, by its name in the library.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material: Option<String>,
    /// Its colour, over its material's: red, green and blue, as a
    /// screen has them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub colour: Option<[u8; 3]>,
}

impl Look {
    /// This look, and for what it does not say, what `under` says.
    #[must_use]
    pub fn over(&self, under: &Look) -> Look {
        Look {
            material: self.material.clone().or_else(|| under.material.clone()),
            // (A material given over a colour shows as the material.)
            colour: match (&self.material, self.colour) {
                (_, Some(colour)) => Some(colour),
                (Some(_), None) => None,
                (None, None) => under.colour,
            },
        }
    }
}

/// How a material shows on the screen: what the one that draws the
/// model knows of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shade {
    /// Its colour.
    pub colour: [u8; 3],
    /// The texture drawn over the colour, by its number (0: none).
    pub texture: u8,
    /// The length of one repeat of the texture's pattern, in
    /// thousandths of the world's unit.
    pub scale: u32,
}

/// How materials show, by name.
pub type Shades = BTreeMap<String, Shade>;

impl World {
    /// How the body called `body` looks: as it was given, over the
    /// part's.
    pub fn body_look(&self, body: &str) -> Look {
        let part = Look {
            material: self.material.clone(),
            colour: None,
        };
        match self.looks.get(body) {
            Some(own) => own.over(&part),
            None => part,
        }
    }

    /// How `face` looks: as it was given, over its body's (`body`, by
    /// name), over the part's.
    pub fn face_look(&self, face: FaceId, body: Option<&str>) -> Look {
        let under = match body {
            Some(body) => self.body_look(body),
            None => Look {
                material: self.material.clone(),
                colour: None,
            },
        };
        match self.topo.name_of(face).and_then(|n| self.looks.get(n)) {
            Some(own) => own.over(&under),
            None => under,
        }
    }
}
