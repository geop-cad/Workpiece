//! What is drawn on a sheet by hand — a sketch lying on the sheet — as
//! the lines and the lettering of the drawing.

use workpiece_core::handles::{CanvasShape, CanvasView, Tone};
use workpiece_core::math::Point2;

use super::notes::width;
use super::{Laid, Pen, Text, lettering};
use crate::Sheet;

/// How many millimetres of the sheet one pixel of what a sketch draws
/// is: its lettering comes out as high as a drawing's.
pub const PIXEL: f64 = 0.28;

/// Draws what is drawn on `sheet` by hand.
pub(crate) fn draw(laid: &mut Laid, sheet: &Sheet) {
    if sheet.drawn.is_none() {
        return;
    }
    let view = CanvasView {
        pixel: PIXEL,
        right: Point2::X,
        up: Point2::Y,
        middle: Point2::ZERO,
    };
    for shape in sheet.hand().drawn(&view) {
        match shape {
            CanvasShape::Polyline {
                points,
                tone,
                dashed,
                ..
            } => {
                let pen = match tone {
                    Tone::Construction | Tone::Preview | Tone::Badge | Tone::Hover => continue,
                    Tone::Dimension => Pen::Narrow,
                    _ if dashed => Pen::Dashed,
                    _ => Pen::Wide,
                };
                for pair in points.windows(2) {
                    laid.line(pair[0], pair[1], pen);
                }
            }
            CanvasShape::Arrowhead { tip, direction, .. } => laid.arrow(tip, direction, 3.5),
            CanvasShape::Text {
                at,
                along,
                text,
                upper,
                lower,
                badge: false,
                ..
            } => {
                let angle = along.y.atan2(along.x);
                let small = lettering() * 0.7;
                let over = width(&upper, small).max(width(&lower, small));
                let start = at - along * ((width(&text, lettering()) + over) / 2.0);
                laid.texts.push(Text {
                    at: start,
                    text: text.clone(),
                    height: lettering(),
                    anchor: [0.0, 0.5],
                    angle,
                    preview: false,
                });
                let after = start + along * (width(&text, lettering()) + 0.4);
                for (text, lift) in [
                    (lower, -lettering() / 2.0),
                    (upper, small * 1.35 - lettering() / 2.0),
                ] {
                    if !text.is_empty() {
                        laid.texts.push(Text {
                            at: after + along.perp() * lift,
                            text,
                            height: small,
                            anchor: [0.0, 0.0],
                            angle,
                            preview: false,
                        });
                    }
                }
            }
            CanvasShape::Text { .. } | CanvasShape::Dot { .. } => {}
        }
    }
}
