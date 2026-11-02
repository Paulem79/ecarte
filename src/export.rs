//! Export : SVG (vectoriel), PNG et JPG (rastérisés via resvg depuis le SVG).
use crate::doc::Document;
use crate::scene;
use crate::svg;
use crate::world::Data;
use std::path::Path;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Format {
    Svg,
    Png,
    Jpg,
}

impl Format {
    pub fn ext(self) -> &'static str {
        match self {
            Format::Svg => "svg",
            Format::Png => "png",
            Format::Jpg => "jpg",
        }
    }
}

pub fn svg_string(doc: &Document, data: &Data) -> String {
    let prims = scene::build(doc, data);
    svg::to_svg(&prims, doc.page.w, doc.page.h, &doc.page.font)
}

/// `dpi` : 96 = taille de la page en pixels ; 300 = qualité impression.
pub fn render_rgba(svg_text: &str, doc: &Document, dpi: f32) -> Result<(u32, u32, Vec<u8>), String> {
    let mut opt = resvg::usvg::Options::default();
    opt.fontdb_mut().load_system_fonts();
    let tree = resvg::usvg::Tree::from_str(svg_text, &opt).map_err(|e| e.to_string())?;
    let k = (dpi / 96.0).clamp(0.1, 8.0);
    let (w, h) = ((doc.page.w * k).round() as u32, (doc.page.h * k).round() as u32);
    if (w as u64) * (h as u64) > 120_000_000 {
        return Err("image trop grande (limite 120 Mpx)".into());
    }
    let mut pix = resvg::tiny_skia::Pixmap::new(w.max(1), h.max(1)).ok_or("allocation de l'image impossible")?;
    resvg::render(&tree, resvg::tiny_skia::Transform::from_scale(k, k), &mut pix.as_mut());
    // dé-prémultiplier
    let mut data = pix.take();
    for px in data.chunks_exact_mut(4) {
        let a = px[3] as u32;
        if a != 0 && a != 255 {
            for c in &mut px[..3] {
                *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }
    Ok((w, h, data))
}

pub fn export(doc: &Document, data: &Data, path: &Path, fmt: Format, dpi: f32, jpg_quality: u8) -> Result<(), String> {
    let svg_text = svg_string(doc, data);
    match fmt {
        Format::Svg => std::fs::write(path, svg_text).map_err(|e| e.to_string()),
        Format::Png => {
            let (w, h, data) = render_rgba(&svg_text, doc, dpi)?;
            image::save_buffer(path, &data, w, h, image::ExtendedColorType::Rgba8).map_err(|e| e.to_string())
        }
        Format::Jpg => {
            let (w, h, data) = render_rgba(&svg_text, doc, dpi)?;
            let rgb: Vec<u8> = data
                .chunks_exact(4)
                .flat_map(|p| {
                    let a = p[3] as u32;
                    // composition sur blanc
                    [0, 1, 2].map(|i| ((p[i] as u32 * a + 255 * (255 - a)) / 255) as u8)
                })
                .collect();
            let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
            let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(std::io::BufWriter::new(file), jpg_quality.clamp(1, 100));
            enc.encode(&rgb, w, h, image::ExtendedColorType::Rgb8).map_err(|e| e.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn svg_parses_and_renders() {
        let world = Data::embedded_only();
        let mut d = Document::default();
        d.title.text = "Test <&> é".into();
        let svg = svg_string(&d, &world);
        assert!(svg.starts_with("<?xml"));
        let (w, h, data) = render_rgba(&svg, &d, 48.0).expect("render");
        assert_eq!(data.len(), (w * h * 4) as usize);
        assert!(data.chunks_exact(4).any(|p| p[3] == 255));
    }
}

#[cfg(test)]
mod manual {
    use super::*;
    use crate::doc::*;

    #[test]
    #[ignore]
    fn sample_png() {
        let out = std::env::var("ECARTE_OUT").expect("ECARTE_OUT");
        let data = Data::embedded_only();
        let mut d = Document::default();
        d.title.text = "Europe — test".into();
        d.map.zoom = 4.5;
        d.map.center_lon = 12.0;
        d.map.center_lat = 52.0;
        d.map.country_colors.insert("FRA".into(), [220, 120, 90, 255]);
        d.map.capitals.mode = CapMode::PointsAndNames;
        d.map.country_labels = true;
        d.legend.visible = true;
        d.legend.entries.push(([220, 120, 90, 255], "France".into()));
        d.add_item("Flèche", [200, 30, 30, 255], [0, 0, 0, 0], 4.0, Kind::Line { pts: vec![Pos::Geo { lon: 2.0, lat: 48.0 }, Pos::Geo { lon: 13.0, lat: 52.0 }], arrow_start: false, arrow_end: true, geodesic: true });
        d.add_item("Cercle", [30, 30, 200, 255], [30, 30, 200, 50], 2.0, Kind::Circle { center: Pos::Geo { lon: 2.3, lat: 48.8 }, radius: 500.0, unit: RadiusUnit::Km });
        export(&d, &data, std::path::Path::new(&format!("{out}/a.png")), Format::Png, 96.0, 90).unwrap();
        d.map.proj = crate::geo::Projection::Orthographic;
        d.map.zoom = 1.0;
        d.map.country_labels = false;
        export(&d, &data, std::path::Path::new(&format!("{out}/b.png")), Format::Png, 96.0, 90).unwrap();
        d.map.proj = crate::geo::Projection::Mollweide;
        d.map.center_lon = 0.0;
        d.map.center_lat = 0.0;
        export(&d, &data, std::path::Path::new(&format!("{out}/c.jpg")), Format::Jpg, 96.0, 85).unwrap();
        export(&d, &data, std::path::Path::new(&format!("{out}/c.svg")), Format::Svg, 96.0, 85).unwrap();
    }
}

#[cfg(test)]
mod icon {
    #[test]
    #[ignore]
    fn make_icon_png() {
        let svg = std::fs::read_to_string("assets/ecarte.svg").unwrap();
        let tree = resvg::usvg::Tree::from_str(&svg, &resvg::usvg::Options::default()).unwrap();
        let mut pix = resvg::tiny_skia::Pixmap::new(48, 48).unwrap();
        resvg::render(&tree, resvg::tiny_skia::Transform::from_scale(48.0 / 64.0, 48.0 / 64.0), &mut pix.as_mut());
        pix.save_png("assets/ecarte-48.png").unwrap();
    }
}

#[cfg(test)]
mod view_dump {
    use super::*;

    #[test]
    #[ignore]
    fn dump_view() {
        let out = std::env::var("ECARTE_OUT").unwrap();
        let g = |k: &str, d: f64| std::env::var(k).ok().and_then(|v| v.parse().ok()).unwrap_or(d);
        let data = Data::embedded_only();
        let mut d = Document::default();
        d.map.center_lon = g("CLON", 20.0);
        d.map.center_lat = g("CLAT", -15.0);
        d.map.zoom = g("ZOOM", 3.0);
        d.title.visible = false;
        export(&d, &data, std::path::Path::new(&format!("{out}/v.png")), Format::Png, 72.0, 90).unwrap();
    }
}
