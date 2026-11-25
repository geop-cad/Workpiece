//! Materials: what parts are made of. A material has a density (what a
//! body weighs) and a look: a colour, a texture that is drawn over it,
//! and how it takes light. The materials there are to choose from are
//! a [`Library`]: a file beside the parts, [`LIBRARY_FILE`].

/// What is drawn over a material's colour: patterns worked out where
/// the face is, not pictures.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
pub enum Texture {
    /// Nothing: the colour alone.
    #[default]
    Plain,
    /// Fine lines one way, as of brushed metal.
    Brushed,
    /// The rings and the grain of wood.
    Wood,
    /// The weave of a cloth of carbon fibre.
    Weave,
    /// Squares, light and dark.
    Checker,
    /// Specks, as of cast metal, stone or concrete.
    Speckle,
    /// Ribs across, as of knurling or a ribbed rubber.
    Ribbed,
}

impl Texture {
    /// Every texture.
    pub const ALL: [Texture; 7] = [
        Texture::Plain,
        Texture::Brushed,
        Texture::Wood,
        Texture::Weave,
        Texture::Checker,
        Texture::Speckle,
        Texture::Ribbed,
    ];

    /// Its name.
    pub fn label(self) -> &'static str {
        match self {
            Texture::Plain => "Plain",
            Texture::Brushed => "Brushed",
            Texture::Wood => "Wood grain",
            Texture::Weave => "Weave",
            Texture::Checker => "Checker",
            Texture::Speckle => "Speckle",
            Texture::Ribbed => "Ribbed",
        }
    }

    /// The number the renderer knows it by.
    pub fn code(self) -> u8 {
        Texture::ALL.iter().position(|t| *t == self).unwrap_or(0) as u8
    }
}

/// A material.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Material {
    /// What it is called.
    pub name: String,
    /// Its density, in kilograms per cubic metre.
    pub density: f64,
    /// Its colour: red, green and blue, as a screen has them.
    pub colour: [u8; 3],
    /// What is drawn over the colour.
    #[serde(default)]
    pub texture: Texture,
    /// How large the texture's pattern is: the length of one repeat of
    /// it, in the part's unit.
    #[serde(default = "one")]
    pub scale: f64,
    /// How rough it is, from 0 (a mirror) to 1 (matte): how wide what
    /// it reflects is spread.
    #[serde(default = "half")]
    pub roughness: f64,
    /// How much of a metal it is, from 0 to 1: a metal reflects in its
    /// own colour, and nothing of the light goes into it.
    #[serde(default)]
    pub metallic: f64,
    /// How much light goes through it, from 0 to 1 (clear as glass).
    #[serde(default)]
    pub transparency: f64,
}

fn one() -> f64 {
    1.0
}

fn half() -> f64 {
    0.5
}

impl Material {
    /// A material called `name`, for its numbers to be set: grey and
    /// as dense as water.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            density: 1000.0,
            colour: [180, 180, 186],
            texture: Texture::Plain,
            scale: 1.0,
            roughness: 0.5,
            metallic: 0.0,
            transparency: 0.0,
        }
    }

    /// How it shows on the screen.
    pub fn shade(&self) -> shapecraft_core::world::Shade {
        shapecraft_core::world::Shade {
            colour: self.colour,
            texture: self.texture.code(),
            scale: (self.scale * 1000.0).round().clamp(1.0, 4e9) as u32,
        }
    }
}

/// The name of the file the materials are kept in, in the directory
/// the parts are in.
pub const LIBRARY_FILE: &str = "materials.json";

/// The materials there are to choose from: one of a name.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Library {
    /// The materials, in the order they are listed.
    pub materials: Vec<Material>,
}

/// The materials a library starts with.
impl Default for Library {
    fn default() -> Self {
        use Texture::{Brushed, Plain, Ribbed, Speckle, Weave, Wood};
        // Name, density, colour, texture and the length of its repeat,
        // roughness, how much a metal, how clear.
        let of =
            |name: &str, density, colour, texture, scale, roughness, metallic, clear| Material {
                name: name.to_owned(),
                density,
                colour,
                texture,
                scale,
                roughness,
                metallic,
                transparency: clear,
            };
        let materials = vec![
            of("Steel", 7850.0, [150, 154, 160], Plain, 1.0, 0.35, 1.0, 0.0),
            of(
                "Stainless steel",
                8000.0,
                [188, 190, 194],
                Brushed,
                1.0,
                0.3,
                1.0,
                0.0,
            ),
            of(
                "Polished steel",
                7850.0,
                [200, 202, 206],
                Plain,
                1.0,
                0.06,
                1.0,
                0.0,
            ),
            of(
                "Cast iron",
                7200.0,
                [92, 94, 98],
                Speckle,
                0.5,
                0.7,
                1.0,
                0.0,
            ),
            of(
                "Aluminium",
                2700.0,
                [202, 205, 210],
                Brushed,
                1.0,
                0.3,
                1.0,
                0.0,
            ),
            of("Brass", 8500.0, [201, 170, 92], Plain, 1.0, 0.25, 1.0, 0.0),
            of("Copper", 8960.0, [196, 118, 82], Plain, 1.0, 0.25, 1.0, 0.0),
            of("Gold", 19300.0, [230, 190, 95], Plain, 1.0, 0.12, 1.0, 0.0),
            of(
                "Titanium",
                4500.0,
                [140, 138, 134],
                Brushed,
                1.0,
                0.4,
                1.0,
                0.0,
            ),
            of("ABS", 1050.0, [228, 226, 218], Plain, 1.0, 0.45, 0.0, 0.0),
            of(
                "ABS, black",
                1050.0,
                [28, 28, 30],
                Plain,
                1.0,
                0.4,
                0.0,
                0.0,
            ),
            of("ABS, red", 1050.0, [190, 32, 30], Plain, 1.0, 0.3, 0.0, 0.0),
            of(
                "Nylon (PA6)",
                1140.0,
                [236, 232, 220],
                Plain,
                1.0,
                0.6,
                0.0,
                0.0,
            ),
            of(
                "Polycarbonate",
                1200.0,
                [214, 228, 236],
                Plain,
                1.0,
                0.05,
                0.0,
                0.85,
            ),
            of("Glass", 2500.0, [236, 244, 240], Plain, 1.0, 0.0, 0.0, 0.95),
            of("Rubber", 1100.0, [38, 38, 40], Ribbed, 2.0, 0.9, 0.0, 0.0),
            of("Oak", 750.0, [170, 126, 76], Wood, 20.0, 0.6, 0.0, 0.0),
            of(
                "Carbon fibre",
                1600.0,
                [36, 38, 42],
                Weave,
                4.0,
                0.25,
                0.0,
                0.0,
            ),
            of(
                "Concrete",
                2400.0,
                [158, 156, 150],
                Speckle,
                2.0,
                0.95,
                0.0,
                0.0,
            ),
        ];
        Self { materials }
    }
}

impl Library {
    /// The material called `name`.
    pub fn get(&self, name: &str) -> Option<&Material> {
        self.materials.iter().find(|m| m.name == name)
    }

    /// The names of the materials, in order.
    pub fn names(&self) -> Vec<String> {
        self.materials.iter().map(|m| m.name.clone()).collect()
    }

    /// How each material shows on the screen, by name.
    pub fn shades(&self) -> shapecraft_core::world::Shades {
        self.materials
            .iter()
            .map(|m| (m.name.clone(), m.shade()))
            .collect()
    }

    /// A name no material has yet: `Material 1`, or with the next
    /// number.
    pub fn fresh_name(&self) -> String {
        (1..)
            .map(|n| format!("Material {n}"))
            .find(|name| self.get(name).is_none())
            .unwrap_or_default()
    }

    /// The library as the text of its file.
    pub fn to_text(&self) -> anyhow::Result<String> {
        Ok(serde_json::to_string_pretty(self)? + "\n")
    }

    /// The library the text of a file holds.
    pub fn from_text(text: &str) -> anyhow::Result<Self> {
        Ok(serde_json::from_str(text)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_library_is_what_its_file_holds() -> anyhow::Result<()> {
        let mut library = Library::default();
        library.materials.push(Material::new(library.fresh_name()));
        let text = library.to_text()?;
        assert_eq!(Library::from_text(&text)?, library);
        // What a material's file does not say is as usual.
        let short =
            r#"{"materials": [{"name": "Wax", "density": 900, "colour": [250, 240, 200]}]}"#;
        let wax = Library::from_text(short)?;
        assert_eq!(wax.get("Wax").map(|m| m.roughness), Some(0.5));
        assert_eq!(library.fresh_name(), "Material 2");
        Ok(())
    }
}
