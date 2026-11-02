//! Modèle de document (sérialisé en JSON, extension .ecarte).
use crate::geo::Projection;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub type Rgba = [u8; 4];

#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub enum Pos {
    Geo { lon: f64, lat: f64 },
    Page { x: f32, y: f32 },
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum HAlign {
    Left,
    Center,
    Right,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum PointShape {
    Circle,
    Square,
    Triangle,
    Diamond,
    Cross,
    Star,
}

impl PointShape {
    pub const ALL: [PointShape; 6] = [
        PointShape::Circle,
        PointShape::Square,
        PointShape::Triangle,
        PointShape::Diamond,
        PointShape::Cross,
        PointShape::Star,
    ];
    pub fn label(self) -> &'static str {
        match self {
            PointShape::Circle => "Disque",
            PointShape::Square => "Carré",
            PointShape::Triangle => "Triangle",
            PointShape::Diamond => "Losange",
            PointShape::Cross => "Croix",
            PointShape::Star => "Étoile",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum RadiusUnit {
    Km,
    Px,
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub enum Kind {
    Text { pos: Pos, text: String, size: f32, bold: bool, italic: bool, rot: f32, halo: bool, align: HAlign },
    Point { pos: Pos, radius: f32, shape: PointShape, label: String, label_size: f32 },
    Circle { center: Pos, radius: f32, unit: RadiusUnit },
    Line { pts: Vec<Pos>, arrow_start: bool, arrow_end: bool, geodesic: bool },
    Rect { a: Pos, b: Pos },
    Polygon { pts: Vec<Pos> },
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Item {
    pub id: u32,
    pub name: String,
    pub visible: bool,
    /// Contour (ou, pour un Texte, inutilisé).
    pub stroke: Rgba,
    /// Remplissage (couleur du texte pour un Texte).
    pub fill: Rgba,
    pub width: f32,
    pub dashed: bool,
    pub kind: Kind,
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Page {
    pub w: f32,
    pub h: f32,
    pub bg: Rgba,
    /// Marges autour du cadre : gauche, haut, droite, bas.
    pub margins: [f32; 4],
    pub font: String,
}

impl Default for Page {
    fn default() -> Self {
        Page { w: 1600.0, h: 1000.0, bg: [255, 255, 255, 255], margins: [70.0, 110.0, 40.0, 60.0], font: "sans-serif".into() }
    }
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct TextBlock {
    pub text: String,
    pub size: f32,
    pub color: Rgba,
    pub visible: bool,
    pub bold: bool,
}

impl TextBlock {
    fn new(text: &str, size: f32, visible: bool, bold: bool) -> Self {
        TextBlock { text: text.into(), size, color: [20, 20, 20, 255], visible, bold }
    }
}
impl Default for TextBlock {
    fn default() -> Self {
        TextBlock::new("", 24.0, true, false)
    }
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Graticule {
    pub show: bool,
    pub step: f64,
    pub color: Rgba,
    pub width: f32,
    pub dashed: bool,
    pub labels: bool,
    pub label_size: f32,
    pub label_color: Rgba,
    /// Étiquettes sur les côtés gauche, haut, droite, bas.
    pub sides: [bool; 4],
}
impl Default for Graticule {
    fn default() -> Self {
        Graticule {
            show: true,
            step: 30.0,
            color: [90, 110, 140, 160],
            width: 0.8,
            dashed: false,
            labels: true,
            label_size: 13.0,
            label_color: [40, 40, 40, 255],
            sides: [true, false, false, true],
        }
    }
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct MapSettings {
    pub base: String,
    pub layers: Vec<LayerStyle>,
    pub proj: Projection,
    pub center_lon: f64,
    pub center_lat: f64,
    pub std1: f64,
    pub std2: f64,
    pub zoom: f64,
    pub ocean: Rgba,
    pub land: Rgba,
    pub border: Rgba,
    pub border_width: f32,
    pub show_land: bool,
    pub country_colors: BTreeMap<String, Rgba>,
    pub country_labels: bool,
    pub country_label_size: f32,
    pub country_label_min_area: f64,
    pub graticule: Graticule,
    pub capitals: CapitalsStyle,
}
impl Default for MapSettings {
    fn default() -> Self {
        MapSettings {
            base: "countries_110m".into(),
            layers: LayerStyle::defaults(),
            proj: Projection::Mercator,
            center_lon: 10.0,
            center_lat: 30.0,
            std1: 30.0,
            std2: 60.0,
            zoom: 1.0,
            ocean: [200, 224, 242, 255],
            land: [238, 232, 213, 255],
            border: [110, 100, 85, 255],
            border_width: 0.7,
            show_land: true,
            country_colors: BTreeMap::new(),
            country_labels: false,
            country_label_size: 11.0,
            country_label_min_area: 30.0,
            graticule: Graticule::default(),
            capitals: CapitalsStyle::default(),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum FrameStyle {
    None,
    Simple,
    Double,
    Ticks,
}
impl FrameStyle {
    pub const ALL: [FrameStyle; 4] = [FrameStyle::None, FrameStyle::Simple, FrameStyle::Double, FrameStyle::Ticks];
    pub fn label(self) -> &'static str {
        match self {
            FrameStyle::None => "Aucun",
            FrameStyle::Simple => "Simple",
            FrameStyle::Double => "Double",
            FrameStyle::Ticks => "Bandes alternées",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}
impl Corner {
    pub const ALL: [Corner; 4] = [Corner::TopLeft, Corner::TopRight, Corner::BottomLeft, Corner::BottomRight];
    pub fn label(self) -> &'static str {
        match self {
            Corner::TopLeft => "Haut gauche",
            Corner::TopRight => "Haut droite",
            Corner::BottomLeft => "Bas gauche",
            Corner::BottomRight => "Bas droite",
        }
    }
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Frame {
    pub style: FrameStyle,
    pub color: Rgba,
    pub width: f32,
    pub scale_bar: bool,
    pub scale_corner: Corner,
    pub north_arrow: bool,
    pub north_corner: Corner,
}
impl Default for Frame {
    fn default() -> Self {
        Frame {
            style: FrameStyle::Double,
            color: [30, 30, 30, 255],
            width: 2.0,
            scale_bar: true,
            scale_corner: Corner::BottomLeft,
            north_arrow: true,
            north_corner: Corner::TopRight,
        }
    }
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Legend {
    pub visible: bool,
    pub title: String,
    pub corner: Corner,
    pub size: f32,
    pub entries: Vec<(Rgba, String)>,
}
impl Default for Legend {
    fn default() -> Self {
        Legend { visible: false, title: "Légende".into(), corner: Corner::BottomRight, size: 15.0, entries: vec![] }
    }
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Document {
    pub page: Page,
    pub title: TextBlock,
    pub subtitle: TextBlock,
    pub map: MapSettings,
    pub frame: Frame,
    pub legend: Legend,
    pub items: Vec<Item>,
    pub next_id: u32,
}

impl Default for Document {
    fn default() -> Self {
        Document {
            page: Page::default(),
            title: TextBlock::new("Titre de la carte", 40.0, true, true),
            subtitle: TextBlock::new("", 20.0, false, false),
            map: MapSettings::default(),
            frame: Frame::default(),
            legend: Legend::default(),
            items: vec![],
            next_id: 1,
        }
    }
}

impl Document {
    pub fn add_item(&mut self, name: &str, stroke: Rgba, fill: Rgba, width: f32, kind: Kind) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        self.items.push(Item { id, name: format!("{name} {id}"), visible: true, stroke, fill, width, dashed: false, kind });
        id
    }

    pub fn item_mut(&mut self, id: u32) -> Option<&mut Item> {
        self.items.iter_mut().find(|i| i.id == id)
    }

    pub fn frame_rect(&self) -> [f32; 4] {
        let m = self.page.margins;
        [m[0], m[1], (self.page.w - m[0] - m[2]).max(10.0), (self.page.h - m[1] - m[3]).max(10.0)]
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default()
    }

    pub fn from_json(s: &str) -> Result<Document, String> {
        serde_json::from_str(s).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_roundtrip() {
        let mut d = Document::default();
        d.add_item(
            "Flèche",
            [255, 0, 0, 255],
            [0, 0, 0, 0],
            3.0,
            Kind::Line {
                pts: vec![Pos::Geo { lon: 1.0, lat: 2.0 }, Pos::Page { x: 5.0, y: 6.0 }],
                arrow_start: false,
                arrow_end: true,
                geodesic: true,
            },
        );
        let back = Document::from_json(&d.to_json()).unwrap();
        assert_eq!(d, back);
        assert!(Document::from_json("{}").is_ok());
    }
}

/// Couche additionnelle (lacs, fleuves, régions…) tirée du catalogue de données.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct LayerStyle {
    pub id: String,
    pub enabled: bool,
    /// Remplissage des polygones (alpha 0 = aucun).
    pub fill: Rgba,
    pub stroke: Rgba,
    pub width: f32,
}

impl LayerStyle {
    pub fn defaults() -> Vec<LayerStyle> {
        let l = |id: &str, enabled, fill, stroke, width| LayerStyle { id: id.into(), enabled, fill, stroke, width };
        vec![
            l("lakes_110m", true, [200, 224, 242, 255], [110, 150, 180, 255], 0.5),
            l("rivers_110m", true, [0, 0, 0, 0], [110, 160, 200, 255], 0.8),
            l("lakes_50m", false, [200, 224, 242, 255], [110, 150, 180, 255], 0.5),
            l("rivers_50m", false, [0, 0, 0, 0], [110, 160, 200, 255], 0.7),
            l("admin1_50m", false, [0, 0, 0, 0], [150, 140, 125, 255], 0.5),
        ]
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum CapMode {
    Off,
    Points,
    PointsAndNames,
}

/// Capitales des pays affichées directement sur la carte.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct CapitalsStyle {
    pub mode: CapMode,
    pub shape: PointShape,
    pub size: f32,
    pub fill: Rgba,
    pub outline: Rgba,
    pub label_size: f32,
    pub label_color: Rgba,
    /// Seulement les capitales des pays coloriés.
    pub only_colored: bool,
}

impl Default for CapitalsStyle {
    fn default() -> Self {
        CapitalsStyle {
            mode: CapMode::Off,
            shape: PointShape::Circle,
            size: 4.5,
            fill: [200, 30, 30, 255],
            outline: [255, 255, 255, 255],
            label_size: 12.0,
            label_color: [20, 20, 20, 255],
            only_colored: false,
        }
    }
}
