//! A sheet laid out, as a DXF and as an SVG file: its lines by what
//! they are drawn with, its lettering as text. (What is only shown as
//! what it would be is left out.)

use std::fmt::Write;

use workpiece_core::math::Point2;

use super::{Laid, PREVIEW, Pen};

/// The layer the lines of `pen` are on, and how they are broken.
fn layer(pen: Pen) -> (&'static str, &'static str) {
    match pen {
        Pen::Frame => ("FRAME", "CONTINUOUS"),
        Pen::Wide => ("VISIBLE", "CONTINUOUS"),
        Pen::Narrow => ("THIN", "CONTINUOUS"),
        Pen::Dashed => ("HIDDEN", "HIDDEN"),
        Pen::Chain => ("CENTRE", "CENTER"),
        Pen::ChainWide => ("CUTTING", "CENTER"),
        Pen::Phantom => ("ADJACENT", "PHANTOM"),
    }
}

impl Laid {
    /// The sheet as a DXF file, in millimetres: each kind of line on a
    /// layer of its own, as wide and as broken as it is drawn.
    pub fn dxf(&self) -> String {
        let mut out = String::from("0\nSECTION\n2\nHEADER\n9\n$INSUNITS\n70\n4\n0\nENDSEC\n");
        out.push_str("0\nSECTION\n2\nTABLES\n0\nTABLE\n2\nLTYPE\n70\n4\n");
        out.push_str("0\nLTYPE\n2\nCONTINUOUS\n70\n0\n3\nSolid\n72\n65\n73\n0\n40\n0.0\n");
        // (The line types as ISO 128-2 has them for the narrow lines.)
        for (name, said, pen) in [
            ("HIDDEN", "Dashed", Pen::Dashed),
            ("CENTER", "Long-dashed dotted", Pen::Chain),
            ("PHANTOM", "Long-dashed double-dotted", Pen::Phantom),
        ] {
            let dashes = pen.dashes(self.wide);
            let _ = write!(
                out,
                "0\nLTYPE\n2\n{name}\n70\n0\n3\n{said}\n72\n65\n73\n{}\n40\n{}\n",
                dashes.len(),
                dashes.iter().sum::<f64>()
            );
            for (k, x) in dashes.iter().enumerate() {
                let x = if k % 2 == 0 { *x } else { -x };
                let _ = write!(out, "49\n{x}\n");
            }
        }
        out.push_str("0\nENDTAB\n0\nTABLE\n2\nLAYER\n70\n8\n");
        let pens = [
            Pen::Frame,
            Pen::Wide,
            Pen::Narrow,
            Pen::Dashed,
            Pen::Chain,
            Pen::ChainWide,
            Pen::Phantom,
        ];
        for pen in pens {
            let (name, kind) = layer(pen);
            // (Line weights are in hundredths of a millimetre.)
            let _ = write!(
                out,
                "0\nLAYER\n2\n{name}\n70\n0\n62\n7\n6\n{kind}\n370\n{}\n",
                (pen.width(self.wide) * 100.0).round()
            );
        }
        out.push_str("0\nLAYER\n2\nTEXT\n70\n0\n62\n7\n6\nCONTINUOUS\n");
        out.push_str("0\nENDTAB\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n");
        for fill in self.fills.iter().filter(|f| f.colour == [0, 0, 0]) {
            let [a, b, c] = fill.corners;
            let _ = write!(
                out,
                "0\nSOLID\n8\nTHIN\n10\n{:.6}\n20\n{:.6}\n11\n{:.6}\n21\n{:.6}\n12\n{:.6}\n22\n{:.6}\n13\n{:.6}\n23\n{:.6}\n",
                a.x, a.y, b.x, b.y, c.x, c.y, c.x, c.y
            );
        }
        for stroke in self.strokes.iter().filter(|s| !s.preview) {
            let (name, _) = layer(stroke.pen);
            let _ = write!(
                out,
                "0\nLINE\n8\n{name}\n10\n{:.6}\n20\n{:.6}\n11\n{:.6}\n21\n{:.6}\n",
                stroke.a.x, stroke.a.y, stroke.b.x, stroke.b.y
            );
        }
        for text in self.texts.iter().filter(|t| !t.preview) {
            // (Aligned by the point it is set by: left, middle or
            // right; on its line, or by its middle.)
            let across = (text.anchor[0] * 2.0).round() as i32;
            let up = if text.anchor[1] > 0.75 {
                3
            } else if text.anchor[1] > 0.25 {
                2
            } else {
                0
            };
            let _ = write!(
                out,
                "0\nTEXT\n8\nTEXT\n10\n{x:.6}\n20\n{y:.6}\n40\n{:.4}\n1\n{}\n50\n{:.4}\n72\n{across}\n11\n{x:.6}\n21\n{y:.6}\n73\n{up}\n",
                text.height,
                text.text
                    .replace(['⌀', 'Ø'], "%%c")
                    .replace('±', "%%p")
                    .replace('°', "%%d"),
                text.angle.to_degrees(),
                x = text.at.x,
                y = text.at.y,
            );
        }
        out.push_str("0\nENDSEC\n0\nEOF\n");
        out
    }

    /// The sheet as an SVG file, true to size.
    pub fn svg(&self) -> String {
        let [w, h] = self.size;
        let at = |p: Point2| (p.x, h - p.y);
        let mut out = format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}mm\" height=\"{h}mm\" viewBox=\"0 0 {w} {h}\">\n<rect width=\"{w}\" height=\"{h}\" fill=\"white\"/>\n"
        );
        for fill in self.fills.iter().filter(|f| f.colour != PREVIEW) {
            let [a, b, c] = fill.corners.map(at);
            let [r, g, bl] = fill.colour;
            let _ = writeln!(
                out,
                "<path d=\"M{:.3} {:.3}L{:.3} {:.3}L{:.3} {:.3}Z\" fill=\"rgb({r},{g},{bl})\" stroke=\"rgb({r},{g},{bl})\" stroke-width=\"0.05\"/>",
                a.0, a.1, b.0, b.1, c.0, c.1
            );
        }
        let pens = [
            Pen::Narrow,
            Pen::Dashed,
            Pen::Chain,
            Pen::ChainWide,
            Pen::Phantom,
            Pen::Wide,
            Pen::Frame,
        ];
        for pen in pens {
            let mut path = String::new();
            // (Joined where they meet: a dashed curve dashed along.)
            let strokes = self.strokes.iter().filter(|s| s.pen == pen && !s.preview);
            let chain = matches!(pen, Pen::Chain | Pen::ChainWide | Pen::Phantom);
            for run in super::chains(strokes) {
                let mut own = String::new();
                for (k, p) in run.iter().enumerate() {
                    let (x, y) = at(*p);
                    let op = if k == 0 { 'M' } else { 'L' };
                    let _ = write!(own, "{op}{x:.3} {y:.3}");
                }
                if chain {
                    // (A chain line's long dashes fitted to its length:
                    // a path of its own.)
                    let length: f64 = run.windows(2).map(|w| w[0].distance(w[1])).sum();
                    let dashes: Vec<String> = pen
                        .rounded(self.wide, pen.fitted_dashes(self.wide, length))
                        .iter()
                        .map(f64::to_string)
                        .collect();
                    let _ = writeln!(
                        out,
                        "<path d=\"{own}\" fill=\"none\" stroke=\"black\" stroke-width=\"{}\" stroke-linecap=\"round\" stroke-linejoin=\"round\" stroke-dasharray=\"{}\"/>",
                        pen.width(self.wide),
                        dashes.join(" ")
                    );
                } else {
                    path.push_str(&own);
                }
            }
            if path.is_empty() {
                continue;
            }
            let dashes: Vec<String> = pen
                .round_dashes(self.wide)
                .iter()
                .map(f64::to_string)
                .collect();
            let dashes = match dashes.is_empty() {
                true => String::new(),
                false => format!(" stroke-dasharray=\"{}\"", dashes.join(" ")),
            };
            let _ = writeln!(
                out,
                "<path d=\"{path}\" fill=\"none\" stroke=\"black\" stroke-width=\"{}\" stroke-linecap=\"round\" stroke-linejoin=\"round\"{dashes}/>",
                pen.width(self.wide)
            );
        }
        for text in self.texts.iter().filter(|t| !t.preview) {
            let (x, y) = at(text.at);
            let anchor = ["start", "middle", "end"][(text.anchor[0] * 2.0).round() as usize % 3];
            // (Set by its line; lifted or lowered by what of its
            // capitals' height the point it is set by is up it.)
            let down = text.anchor[1] * text.height;
            let said = text
                .text
                .replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;");
            let _ = writeln!(
                out,
                "<text transform=\"translate({x:.3} {y:.3}) rotate({:.3}) translate(0 {down:.3})\" font-family=\"Helvetica, Arial, sans-serif\" font-size=\"{:.3}\" text-anchor=\"{anchor}\">{said}</text>",
                -text.angle.to_degrees(),
                text.height / 0.718,
            );
        }
        out.push_str("</svg>\n");
        out
    }
}
