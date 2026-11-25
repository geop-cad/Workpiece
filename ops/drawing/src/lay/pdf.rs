//! A sheet laid out, as a PDF file: true to size, its lines as wide
//! and broken as they are drawn.

use std::fmt::Write;

use shapecraft_core::math::Point2;

use super::{Laid, Pen};

/// Points (72nds of an inch) to a millimetre.
const POINTS: f64 = 72.0 / 25.4;

/// How wide `c` is in Helvetica, in thousandths of the font's size:
/// what the PDF and the SVG set text in, and what text is laid out by.
pub(crate) fn advance(c: char) -> f64 {
    match c {
        ' ' | ',' | '.' | ':' | ';' | '/' | 'i' | 'j' | 'l' | 'I' | '!' | '\'' => 278.0,
        'f' | 't' | 'r' | '(' | ')' | '-' | '−' => 333.0,
        '0'..='9' | '+' | '±' | '=' | '–' => 556.0,
        'm' | 'M' | 'W' => 833.0,
        'w' => 722.0,
        'Ø' | '⌀' | 'O' | 'Q' | 'G' => 778.0,
        '°' => 400.0,
        '×' => 584.0,
        '"' => 355.0,
        '*' => 389.0,
        '#' | '$' | '_' => 556.0,
        '%' => 889.0,
        '&' => 667.0,
        'J' | 'c' | 'k' | 's' | 'v' | 'x' | 'y' | 'z' => 500.0,
        'F' | 'T' | 'Z' => 611.0,
        'C' | 'D' | 'H' | 'N' | 'R' | 'U' => 722.0,
        'L' => 556.0,
        c if c.is_uppercase() => 667.0,
        _ => 556.0,
    }
}

/// `text` as a PDF string of the standard Latin encoding: what that
/// has no sign for becomes the nearest it has.
fn latin(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        let code = match c {
            '(' | ')' | '\\' => {
                out.push('\\');
                out.push(c);
                continue;
            }
            '⌀' | 'Ø' => 0o330,
            '±' => 0o261,
            '°' => 0o260,
            '–' => 0o226,
            '−' => u32::from('-'),
            '×' => 0o327,
            '³' => 0o263,
            c if c.is_ascii() && !c.is_ascii_control() => {
                out.push(c);
                continue;
            }
            c if (0xA0..=0xFF).contains(&u32::from(c)) => u32::from(c),
            _ => u32::from('?'),
        };
        let _ = write!(out, "\\{code:03o}");
    }
    out
}

impl Laid {
    /// The sheet as a PDF file.
    pub fn pdf(&self) -> Vec<u8> {
        let at = |p: Point2| (p.x * POINTS, p.y * POINTS);
        let mut page = String::from("1 j 1 J 0.15 w\n");
        // (The colour set only where it changes; corners to a hundredth
        // of a point: a shaded view is many triangles.)
        let mut colour = None;
        for fill in self.fills.iter().filter(|f| f.colour != super::PREVIEW) {
            if colour != Some(fill.colour) {
                let [r, g, b] = fill.colour.map(|c| f64::from(c) / 255.0);
                let _ = writeln!(page, "{r:.3} {g:.3} {b:.3} rg {r:.3} {g:.3} {b:.3} RG");
                colour = Some(fill.colour);
            }
            let [a, b2, c] = fill.corners.map(at);
            // (Shading stroked round in its own colour as well: else the
            // edges of its triangles show as hairlines where a viewer
            // smooths them.)
            let shade = fill.colour != [0, 0, 0];
            let _ = writeln!(
                page,
                "{:.2} {:.2} m {:.2} {:.2} l {:.2} {:.2} l {}",
                a.0,
                a.1,
                b2.0,
                b2.1,
                c.0,
                c.1,
                if shade { "b" } else { "f" }
            );
        }
        page.push_str("0 G\n");
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
            let dashes: Vec<String> = pen
                .round_dashes(self.wide)
                .iter()
                .map(|d| format!("{:.3}", d * POINTS))
                .collect();
            let _ = writeln!(
                page,
                "{:.3} w [{}] 0 d",
                pen.width(self.wide) * POINTS,
                dashes.join(" ")
            );
            let mut any = false;
            // (Joined where they meet: a dashed curve dashed along.)
            let strokes = self.strokes.iter().filter(|s| s.pen == pen && !s.preview);
            for run in super::chains(strokes) {
                // (A chain line's long dashes fitted to its length.)
                if matches!(pen, Pen::Chain | Pen::ChainWide | Pen::Phantom) {
                    let length: f64 = run.windows(2).map(|w| w[0].distance(w[1])).sum();
                    let dashes: Vec<String> = pen
                        .rounded(self.wide, pen.fitted_dashes(self.wide, length))
                        .iter()
                        .map(|d| format!("{:.3}", d * POINTS))
                        .collect();
                    if any {
                        page.push_str("S\n");
                    }
                    let _ = writeln!(page, "[{}] 0 d", dashes.join(" "));
                }
                for (k, p) in run.iter().enumerate() {
                    let (x, y) = at(*p);
                    let op = if k == 0 { "m" } else { "l" };
                    let _ = write!(page, "{x:.3} {y:.3} {op} ");
                }
                page.push('\n');
                any = true;
            }
            if any {
                page.push_str("S\n");
            }
        }
        page.push_str("0 g\n");
        for text in self.texts.iter().filter(|t| !t.preview) {
            // (Helvetica's capitals are 0.718 of its size high.)
            let size = text.height / 0.718 * POINTS;
            let wide: f64 = text.text.chars().map(advance).sum::<f64>() / 1000.0 * size;
            let (sin, cos) = text.angle.sin_cos();
            let (x, y) = at(text.at);
            let (dx, dy) = (
                -text.anchor[0] * wide,
                -text.anchor[1] * text.height * POINTS,
            );
            let (x, y) = (x + dx * cos - dy * sin, y + dx * sin + dy * cos);
            let _ = writeln!(
                page,
                "BT /F1 {size:.3} Tf {cos:.5} {sin:.5} {:.5} {cos:.5} {x:.3} {y:.3} Tm ({}) Tj ET",
                -sin,
                latin(&text.text)
            );
        }
        let [w, h] = self.size.map(|s| s * POINTS);
        let objects = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {w:.3} {h:.3}] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>"
            ),
            format!("<< /Length {} >>\nstream\n{page}endstream", page.len()),
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>"
                .to_owned(),
        ];
        let mut out = String::from("%PDF-1.4\n");
        let mut starts = Vec::new();
        for (k, object) in objects.iter().enumerate() {
            starts.push(out.len());
            let _ = write!(out, "{} 0 obj\n{object}\nendobj\n", k + 1);
        }
        let table = out.len();
        let _ = write!(out, "xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1);
        for start in starts {
            let _ = writeln!(out, "{start:010} 00000 n ");
        }
        let _ = write!(
            out,
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{table}\n%%EOF\n",
            objects.len() + 1
        );
        out.into_bytes()
    }
}
