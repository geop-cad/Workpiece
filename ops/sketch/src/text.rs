//! Text: the outlines of letters as curves of a sketch.

use workpiece_core::math::Point2;
use workpiece_core::world::topo::Curve2;
use workpiece_core::world::topo::nurbs::Recognized;

/// Collects the outline of glyphs as lines and curves.
struct Outlines {
    /// Where the glyph's own origin is, and how big one of its units.
    origin: Point2,
    scale: f64,
    /// Where the outline being drawn started, and where it is.
    start: Point2,
    at: Point2,
    out: Vec<Recognized>,
}

impl Outlines {
    fn place(&self, x: f32, y: f32) -> Point2 {
        self.origin + Point2::new(f64::from(x), f64::from(y)) * self.scale
    }

    fn line(&mut self, to: Point2) {
        if to.distance(self.at) > self.scale * 1e-6 {
            self.out.push(Recognized::Line(self.at, to));
            self.at = to;
        }
    }

    fn curve(&mut self, control: Vec<Point2>) {
        let degree = control.len() - 1;
        let to = control[degree];
        let mut knots = vec![0.0; degree + 1];
        knots.extend(vec![1.0; degree + 1]);
        if let Ok(curve) = Curve2::new(degree, knots, control, None) {
            self.out.push(Recognized::Spline(curve));
            self.at = to;
        }
    }
}

impl ttf_parser::OutlineBuilder for Outlines {
    fn move_to(&mut self, x: f32, y: f32) {
        self.start = self.place(x, y);
        self.at = self.start;
    }

    fn line_to(&mut self, x: f32, y: f32) {
        let to = self.place(x, y);
        self.line(to);
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let control = vec![self.at, self.place(x1, y1), self.place(x, y)];
        self.curve(control);
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let control = vec![
            self.at,
            self.place(x1, y1),
            self.place(x2, y2),
            self.place(x, y),
        ];
        self.curve(control);
    }

    fn close(&mut self) {
        let start = self.start;
        self.line(start);
    }
}

/// The outlines of `text` written from `at` along x, its capital
/// letters `height` high: lines and curves, each letter's outlines
/// closed. A line break starts a new line below.
pub fn outlines(text: &str, height: f64, at: Point2) -> anyhow::Result<Vec<Recognized>> {
    anyhow::ensure!(height > 0.0, "letters have some height");
    let face = ttf_parser::Face::parse(epaint_default_fonts::UBUNTU_LIGHT, 0)
        .map_err(|e| anyhow::anyhow!("the font cannot be read: {e}"))?;
    let em = f64::from(face.units_per_em());
    let capital = face.capital_height().map_or(em * 0.7, f64::from);
    let scale = height / capital;
    let mut builder = Outlines {
        origin: at,
        scale,
        start: at,
        at,
        out: Vec::new(),
    };
    for (row, line) in text.lines().enumerate() {
        let down = f64::from(face.height()) * scale * row as f64;
        builder.origin = at - Point2::Y * down;
        for letter in line.chars() {
            let glyph = face.glyph_index(letter).unwrap_or_default();
            face.outline_glyph(glyph, &mut builder);
            let advance = face.glyph_hor_advance(glyph).map_or(em * 0.5, f64::from);
            builder.origin += Point2::X * advance * scale;
        }
    }
    Ok(builder.out)
}

/// Writes `text` into the sketch from `at`, its capital letters
/// `height` high: the outlines as curves of the sketch, held where they
/// are written.
pub fn write(
    args: &mut crate::model::SketchArgs,
    text: &str,
    height: f64,
    at: Point2,
) -> anyhow::Result<()> {
    // Where one piece ends and the next starts is one point of the
    // sketch: the outlines close, and the letters are areas.
    let mut at_place: std::collections::HashMap<[u64; 2], crate::Id> = Default::default();
    for shape in outlines(text, height, at)? {
        let found = crate::project::Found {
            source: workpiece_core::Source::Topology("text".into()),
            shape,
            tolerance: height * 1e-9,
            auto: false,
        };
        let id = crate::project::place(args, found);
        let mut gone = Vec::new();
        let mut one = |args: &crate::model::SketchArgs, p: &mut crate::Id| {
            let key = args.at(*p).to_array().map(|x| (x + 0.0).to_bits());
            let first = *at_place.entry(key).or_insert(*p);
            if first != *p {
                gone.push(*p);
                *p = first;
            }
        };
        let Some(mut entity) = args.entities.remove(&id) else {
            continue;
        };
        entity.projected = None;
        entity.construction = false;
        match &mut entity.shape {
            crate::Shape::Line(a, b) => {
                one(args, a);
                one(args, b);
            }
            crate::Shape::Spline { control, .. } => {
                let last = control.len() - 1;
                one(args, &mut control[0]);
                one(args, &mut control[last]);
            }
            _ => {}
        }
        args.entities.insert(id, entity);
        for p in gone {
            args.points.remove(&p);
        }
    }
    Ok(())
}
