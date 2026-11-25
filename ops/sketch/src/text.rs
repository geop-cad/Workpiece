//! Text: the outlines of letters as curves of a sketch, laid out in the
//! box of a [`Shape::Text`](crate::Shape::Text) — lines broken where the
//! text breaks them (and, asked to, between words where the box ends),
//! aligned across the box and in its height.

use std::collections::BTreeMap;

use workpiece_core::math::Point2;
use workpiece_core::world::topo::Curve2;
use workpiece_core::world::topo::nurbs::Recognized;

/// Collects the outline of glyphs as lines and curves.
struct Outlines {
    /// Where the glyph's own origin is, and how big one of its units;
    /// the box's directions along and up the lines.
    origin: Point2,
    along: Point2,
    up: Point2,
    scale: f64,
    /// Where the outline being drawn started, and where it is.
    start: Point2,
    at: Point2,
    out: Vec<Recognized>,
}

impl Outlines {
    fn place(&self, x: f32, y: f32) -> Point2 {
        self.origin + (self.along * f64::from(x) + self.up * f64::from(y)) * self.scale
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

/// The font letters are written in.
fn font() -> anyhow::Result<ttf_parser::Face<'static>> {
    ttf_parser::Face::parse(epaint_default_fonts::UBUNTU_LIGHT, 0)
        .map_err(|e| anyhow::anyhow!("the font cannot be read: {e}"))
}

/// How far `line` reaches along, in the font's units.
fn advance(face: &ttf_parser::Face<'_>, line: &str) -> f64 {
    let em = f64::from(face.units_per_em());
    line.chars()
        .map(|c| {
            let glyph = face.glyph_index(c).unwrap_or_default();
            face.glyph_hor_advance(glyph).map_or(em * 0.5, f64::from)
        })
        .sum()
}

/// The lines `text` is written in: broken where it breaks, and, with
/// `wrap`, between words where a line would reach past `width` (font
/// units); a word longer than that stands on a line of its own.
fn lines(face: &ttf_parser::Face<'_>, text: &str, width: Option<f64>) -> Vec<String> {
    let mut out = Vec::new();
    for paragraph in text.split('\n') {
        let Some(width) = width else {
            out.push(paragraph.to_owned());
            continue;
        };
        let mut line = String::new();
        for word in paragraph.split(' ') {
            let longer = if line.is_empty() {
                word.to_owned()
            } else {
                format!("{line} {word}")
            };
            if !line.is_empty() && advance(face, &longer) > width {
                out.push(std::mem::take(&mut line));
                line = word.to_owned();
            } else {
                line = longer;
            }
        }
        out.push(line);
    }
    out
}

/// Where a text's box is: its corner where the first line's baseline
/// starts, its directions along the lines and up them (one long), and
/// its width and height.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    /// The lower left corner.
    pub origin: Point2,
    /// Along the lines.
    pub along: Point2,
    /// Up the lines.
    pub up: Point2,
    /// How wide the box is.
    pub width: f64,
    /// How high.
    pub height: f64,
}

impl Frame {
    /// The box of the corners `[lower left, lower right, upper right,
    /// upper left]`; `None` for one without width.
    pub fn of(corners: [Point2; 4]) -> Option<Self> {
        let along = corners[1] - corners[0];
        let width = along.length();
        let along = along.try_normalize()?;
        let rise = corners[3] - corners[0];
        let up = if along.perp_dot(rise) >= 0.0 {
            along.perp()
        } else {
            -along.perp()
        };
        Some(Self {
            origin: corners[0],
            along,
            up,
            width,
            height: rise.dot(up).abs(),
        })
    }

    /// Whether `p` is inside the box.
    pub fn contains(&self, p: Point2) -> bool {
        let d = p - self.origin;
        let (x, y) = (d.dot(self.along), d.dot(self.up));
        (0.0..=self.width).contains(&x) && (0.0..=self.height).contains(&y)
    }

    /// The box's outline, closed.
    pub fn outline(&self) -> Vec<Point2> {
        let (a, u) = (self.along * self.width, self.up * self.height);
        let o = self.origin;
        vec![o, o + a, o + a + u, o + u, o]
    }
}

/// The outlines of the letters of `text` in its box `frame`: lines and
/// curves, each letter's outlines closed.
pub fn layout(text: &crate::TextBox, frame: Frame) -> anyhow::Result<Vec<Recognized>> {
    anyhow::ensure!(text.size > 0.0, "letters have some height");
    let face = font()?;
    let em = f64::from(face.units_per_em());
    let capital = face.capital_height().map_or(em * 0.7, f64::from);
    let scale = text.size / capital;
    let width = text.wrap.then_some(frame.width / scale);
    let lines = lines(&face, &text.text, width);
    let (ascender, descender) = (f64::from(face.ascender()), f64::from(face.descender()));
    let step = f64::from(face.height()) * scale;
    // The block of lines: from the first's top to the last's foot.
    let block = (lines.len().saturating_sub(1)) as f64 * step + (ascender - descender) * scale;
    let top = match text.valign {
        crate::Align::Start => frame.height,
        crate::Align::Middle => (frame.height + block) / 2.0,
        crate::Align::End => block,
    };
    let mut builder = Outlines {
        origin: frame.origin,
        along: frame.along,
        up: frame.up,
        scale,
        start: frame.origin,
        at: frame.origin,
        out: Vec::new(),
    };
    for (row, line) in lines.iter().enumerate() {
        let wide = advance(&face, line) * scale;
        let x = match text.align {
            crate::Align::Start => 0.0,
            crate::Align::Middle => (frame.width - wide) / 2.0,
            crate::Align::End => frame.width - wide,
        };
        let y = top - ascender * scale - step * row as f64;
        builder.origin = frame.origin + frame.along * x + frame.up * y;
        for letter in line.chars() {
            let glyph = face.glyph_index(letter).unwrap_or_default();
            face.outline_glyph(glyph, &mut builder);
            let advance = face.glyph_hor_advance(glyph).map_or(em * 0.5, f64::from);
            builder.origin += frame.along * advance * scale;
        }
    }
    Ok(builder.out)
}

/// The outlines of the letters of `text` written from `at` along x,
/// its capital letters `height` high, on one line from the baseline at
/// `at` (a line break starts a new line below).
pub fn outlines(text: &str, height: f64, at: Point2) -> anyhow::Result<Vec<Recognized>> {
    let face = font()?;
    let em = f64::from(face.units_per_em());
    let capital = face.capital_height().map_or(em * 0.7, f64::from);
    let ascender = f64::from(face.ascender()) * height / capital;
    let boxed = crate::TextBox {
        corners: [0; 4],
        text: text.to_owned(),
        size: height,
        align: crate::Align::Start,
        valign: crate::Align::Start,
        wrap: false,
    };
    // (A box whose top is the first line's top: its baseline is `at`.)
    let frame = Frame {
        origin: at,
        along: Point2::X,
        up: Point2::Y,
        width: 1.0,
        height: ascender,
    };
    layout(&boxed, frame)
}

impl crate::SketchArgs {
    /// The box of text `id`, as its corners are now.
    pub fn text_frame(&self, id: crate::Id) -> Option<Frame> {
        match &self.entities.get(&id)?.shape {
            crate::Shape::Text(text) => Frame::of(text.corners.map(|p| self.at(p))),
            _ => None,
        }
    }

    /// The outlines of the letters of text `id`, as its box is now:
    /// nothing for what is no text, or a text that cannot be written.
    pub fn text_outlines(&self, id: crate::Id) -> Vec<Recognized> {
        let Some(crate::Shape::Text(text)) = self.entities.get(&id).map(|e| &e.shape) else {
            return Vec::new();
        };
        self.text_frame(id)
            .and_then(|frame| layout(text, frame).ok())
            .unwrap_or_default()
    }
}

/// What a written text's pieces are called, by their letter (`e` for an
/// entity, `p` for a point) and id: after the text (`e7/c3`, its third
/// curve; `e7/p5`, its fifth point), whatever ids they got.
pub(crate) type Names = BTreeMap<(char, crate::Id), String>;

/// `args` with each text written out: in place of the text, the lines
/// and curves of its letters' outlines (each piece starting where the one
/// before it ends, one point there), as the profile takes them; and what
/// the pieces made so are called.
pub(crate) fn written(args: &crate::SketchArgs) -> (crate::SketchArgs, Names) {
    let mut out = args.clone();
    let mut of = Names::new();
    let texts: Vec<(crate::Id, bool)> = args
        .entities
        .iter()
        .filter(|(_, e)| matches!(e.shape, crate::Shape::Text(_)))
        .map(|(&id, e)| (id, e.construction))
        .collect();
    for (id, construction) in texts {
        let shapes = args.text_outlines(id);
        if let Some(e) = out.entities.get_mut(&id) {
            // (The text itself stays, as nothing of the profile.)
            e.construction = true;
        }
        let tolerance = args.entities[&id].tolerance;
        let mut at_place: BTreeMap<[u64; 2], crate::Id> = BTreeMap::new();
        let mut points = 0;
        let mut point = |out: &mut crate::SketchArgs, of: &mut Names, at: Point2| {
            let key = at.to_array().map(|x| (x + 0.0).to_bits());
            *at_place.entry(key).or_insert_with(|| {
                let p = out.add_point(at);
                of.insert(('p', p), format!("e{id}/p{points}"));
                points += 1;
                p
            })
        };
        for (k, shape) in shapes.into_iter().enumerate() {
            let made = match shape {
                Recognized::Line(a, b) => {
                    let (a, b) = (point(&mut out, &mut of, a), point(&mut out, &mut of, b));
                    out.add(crate::Shape::Line(a, b))
                }
                Recognized::Spline(curve) => {
                    let n = curve.control_points().len();
                    let control: Vec<crate::Id> = curve
                        .control_points()
                        .iter()
                        .enumerate()
                        .map(|(j, &p)| {
                            if j == 0 || j + 1 == n {
                                point(&mut out, &mut of, p)
                            } else {
                                out.add_point(p)
                            }
                        })
                        .collect();
                    out.add(crate::Shape::Spline {
                        control,
                        degree: curve.degree(),
                        knots: curve.knots().to_vec(),
                        weights: Vec::new(),
                    })
                }
                _ => continue,
            };
            if let Some(e) = out.entities.get_mut(&made) {
                e.construction = construction;
                e.tolerance = tolerance;
            }
            of.insert(('e', made), format!("e{id}/c{k}"));
        }
    }
    (out, of)
}
