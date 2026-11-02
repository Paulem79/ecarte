//! Sérialisation de la liste d'affichage en SVG.
use crate::doc::{HAlign, Rgba};
use crate::scene::{Prim, Stroke};
use std::fmt::Write;

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn hex(c: Rgba) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

fn fill_attr(f: Option<Rgba>) -> String {
    match f {
        None => " fill=\"none\"".into(),
        Some(c) if c[3] == 255 => format!(" fill=\"{}\"", hex(c)),
        Some(c) => format!(" fill=\"{}\" fill-opacity=\"{:.3}\"", hex(c), c[3] as f32 / 255.0),
    }
}

fn stroke_attr(s: Option<Stroke>) -> String {
    match s {
        None => String::new(),
        Some(s) => {
            let mut a = format!(" stroke=\"{}\" stroke-width=\"{:.2}\" stroke-linejoin=\"round\" stroke-linecap=\"round\"", hex(s.color), s.width);
            if s.color[3] != 255 {
                let _ = write!(a, " stroke-opacity=\"{:.3}\"", s.color[3] as f32 / 255.0);
            }
            if s.dashed {
                let _ = write!(a, " stroke-dasharray=\"{:.1} {:.1}\"", s.width * 4.0 + 2.0, s.width * 3.0 + 2.0);
            }
            a
        }
    }
}

fn path_d(rings: &[Vec<[f32; 2]>], closed: bool) -> String {
    let mut d = String::new();
    for r in rings {
        for (i, p) in r.iter().enumerate() {
            let _ = write!(d, "{}{:.2} {:.2}", if i == 0 { "M" } else { "L" }, p[0], p[1]);
        }
        if closed {
            d.push('Z');
        }
    }
    d
}

pub fn to_svg(prims: &[Prim], w: f32, h: f32, font: &str) -> String {
    let mut s = String::with_capacity(1 << 20);
    let _ = writeln!(
        s,
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}\" height=\"{h}\" viewBox=\"0 0 {w} {h}\" font-family=\"{}\">",
        esc(font)
    );
    let mut clip_id = 0;
    let mut open_groups = 0;
    for p in prims {
        match p {
            Prim::ClipBegin(r) => {
                clip_id += 1;
                let _ = writeln!(s, "<clipPath id=\"c{clip_id}\"><rect x=\"{:.2}\" y=\"{:.2}\" width=\"{:.2}\" height=\"{:.2}\"/></clipPath>", r[0], r[1], r[2], r[3]);
                let _ = writeln!(s, "<g clip-path=\"url(#c{clip_id})\">");
                open_groups += 1;
            }
            Prim::ClipEnd => {
                if open_groups > 0 {
                    s.push_str("</g>\n");
                    open_groups -= 1;
                }
            }
            Prim::Path { rings, closed, fill, stroke } => {
                if rings.iter().all(|r| r.len() < 2) {
                    continue;
                }
                let _ = writeln!(
                    s,
                    "<path d=\"{}\"{} fill-rule=\"evenodd\"{}/>",
                    path_d(rings, *closed),
                    fill_attr(if *closed { *fill } else { None }),
                    stroke_attr(*stroke)
                );
            }
            Prim::Circle { c, r, fill, stroke } => {
                let _ = writeln!(s, "<circle cx=\"{:.2}\" cy=\"{:.2}\" r=\"{:.2}\"{}{}/>", c[0], c[1], r, fill_attr(*fill), stroke_attr(*stroke));
            }
            Prim::Text { pos, text, size, color, align, rot, bold, italic, halo } => {
                let anchor = match align {
                    HAlign::Left => "start",
                    HAlign::Center => "middle",
                    HAlign::Right => "end",
                };
                let lines: Vec<&str> = text.lines().collect();
                let n = lines.len().max(1) as f32;
                let mut attrs = format!(" font-size=\"{size:.1}\" text-anchor=\"{anchor}\"{}", fill_attr(Some(*color)));
                if *bold {
                    attrs.push_str(" font-weight=\"bold\"");
                }
                if *italic {
                    attrs.push_str(" font-style=\"italic\"");
                }
                if let Some(hc) = halo {
                    let _ = write!(attrs, " stroke=\"{}\" stroke-opacity=\"{:.2}\" stroke-width=\"{:.1}\" stroke-linejoin=\"round\" paint-order=\"stroke\"", hex(*hc), hc[3] as f32 / 255.0, size * 0.25);
                }
                let tr = if *rot != 0.0 { format!(" transform=\"rotate({:.2} {:.2} {:.2})\"", rot, pos[0], pos[1]) } else { String::new() };
                let _ = write!(s, "<text{attrs}{tr}>");
                for (i, l) in lines.iter().enumerate() {
                    // ligne médiane centrée sur pos.y ; 0.35*size ≈ demi-hauteur des capitales
                    let y = pos[1] + 0.35 * size + (i as f32 - (n - 1.0) / 2.0) * size * 1.15;
                    let _ = write!(s, "<tspan x=\"{:.2}\" y=\"{:.2}\">{}</tspan>", pos[0], y, esc(l));
                }
                s.push_str("</text>\n");
            }
        }
    }
    for _ in 0..open_groups {
        s.push_str("</g>\n");
    }
    s.push_str("</svg>\n");
    s
}
