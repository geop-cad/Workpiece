//! The picture as it adds up, and how it becomes pixels of a screen.

use std::sync::OnceLock;

/// What one pixel has gathered: the light of the model (three numbers),
/// how many samples met the model, and the light of the backdrop and
/// the floor (three numbers).
pub(crate) type Sum = [f32; 7];

/// The picture as it adds up, pass by pass. Every pass adds one more
/// sample to each pixel, and the picture is the average of them.
pub struct Film {
    pub(crate) width: usize,
    pub(crate) height: usize,
    pub(crate) sums: Vec<Sum>,
    pub(crate) passes: u32,
}

impl Film {
    /// An empty film of the given size in pixels.
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            sums: vec![[0.0; 7]; width * height],
            passes: 0,
        }
    }

    /// The width of the picture in pixels.
    pub fn width(&self) -> usize {
        self.width
    }

    /// The height of the picture in pixels.
    pub fn height(&self) -> usize {
        self.height
    }

    /// How many passes the film has gathered.
    pub fn passes(&self) -> u32 {
        self.passes
    }

    /// The picture as 8-bit sRGB pixels (red, green, blue, alpha), row
    /// by row from the top. The light of the model goes through a
    /// filmic curve, so that bright spots roll off softly; the backdrop
    /// and the floor show in the colour that was asked for. Alpha is
    /// always 255.
    pub fn rgba(&self) -> Vec<u8> {
        let table = encoding();
        let last = (table.len() - 1) as f32;
        let encode = |v: f32| table[(v.clamp(0.0, 1.0) * last + 0.5) as usize];
        let per_pass = 1.0 / self.passes.max(1) as f32;
        let mut out = Vec::with_capacity(self.sums.len() * 4);
        for sum in &self.sums {
            let met = sum[3];
            let (cover, per_met) = if met > 0.0 {
                (met * per_pass, 1.0 / met)
            } else {
                (0.0, 0.0)
            };
            for c in 0..3 {
                let model = filmic(sum[c] * per_met) * cover;
                out.push(encode(model + sum[4 + c] * per_pass));
            }
            out.push(255);
        }
        out
    }
}

/// The filmic curve (the ACES fit of Narkowicz): light of any strength
/// in, a value from 0 to 1 out.
fn filmic(x: f32) -> f32 {
    let x = x.max(0.0);
    ((x * (2.51 * x + 0.03)) / (x * (2.43 * x + 0.59) + 0.14)).clamp(0.0, 1.0)
}

/// The sRGB byte of every linear value, as a table.
fn encoding() -> &'static [u8] {
    static TABLE: OnceLock<Vec<u8>> = OnceLock::new();
    TABLE.get_or_init(|| {
        const SIZE: usize = 1 << 14;
        (0..SIZE)
            .map(|i| {
                let v = i as f32 / (SIZE - 1) as f32;
                let s = if v <= 0.003_130_8 {
                    12.92 * v
                } else {
                    1.055 * v.powf(1.0 / 2.4) - 0.055
                };
                (s * 255.0 + 0.5) as u8
            })
            .collect()
    })
}
