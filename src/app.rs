//! Interface egui : barre d'outils, canevas, panneaux de propriétés.
use crate::doc::*;
use crate::export::{self, Format};
use crate::geo::{haversine_km, Projection};
use crate::logs;
use crate::scene::{self, Prim, View, P};
use crate::data::{self, Availability, DsKind, Settings, SyncOutcome};
use crate::world::{Data, World};
use eframe::egui::{self, epaint, Align2, Color32, FontId, Pos2, Rect, Sense, Vec2};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{mpsc, Arc};

// ------------------------------------------------------------ Outils

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Tool {
    Select,
    Pan,
    Text,
    Point,
    Circle,
    Line,
    Arrow,
    Polyline,
    Rect,
    Polygon,
    Capital,
    Paint,
}

impl Tool {
    const ALL: [(Tool, &'static str, &'static str); 12] = [
        (Tool::Select, "⬉", "Sélection / déplacer (S)"),
        (Tool::Pan, "✋", "Déplacer la carte (molette = zoom) (H)"),
        (Tool::Text, "T", "Texte (T)"),
        (Tool::Point, "●", "Point (P)"),
        (Tool::Circle, "◯", "Cercle : glisser depuis le centre (C)"),
        (Tool::Line, "╱", "Ligne : glisser (L)"),
        (Tool::Arrow, "➚", "Flèche : glisser (A)"),
        (Tool::Polyline, "〰", "Polyligne : clics, double-clic pour finir"),
        (Tool::Rect, "▭", "Rectangle : glisser (R)"),
        (Tool::Polygon, "⬠", "Polygone : clics, double-clic pour finir (G)"),
        (Tool::Capital, "", "Capitale d'un pays : clic sur le pays (M)"),
        (Tool::Paint, "🖌", "Colorier un pays (clic droit = effacer) (K)"),
    ];
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Map,
    Page,
    Items,
    Legend,
    Data,
    Export,
}

enum SyncMsg {
    Item(String, SyncOutcome),
    Done,
}

enum Drag {
    None,
    Pan,
    Move { orig: Item, start: P },
    Create { id: u32, start: P },
}

// ------------------------------------------------------------ Cache d'affichage

enum U {
    Fill { verts: Vec<P>, idx: Vec<u32>, color: Color32, bounds: [f32; 4] },
    Stroke { pts: Vec<P>, closed: bool, color: Color32, width: f32, dashed: bool },
    Circle { c: P, r: f32, fill: Option<Color32>, stroke: Option<(Color32, f32, bool)> },
    Text { pos: P, text: String, size: f32, color: Color32, align: HAlign, rot: f32, halo: Option<Color32> },
    ClipBegin([f32; 4]),
    ClipEnd,
}

fn c32(c: Rgba) -> Color32 {
    Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3])
}

fn convert(prims: &[Prim]) -> Vec<U> {
    let mut out = Vec::with_capacity(prims.len() * 2);
    for p in prims {
        match p {
            Prim::ClipBegin(r) => out.push(U::ClipBegin(*r)),
            Prim::ClipEnd => out.push(U::ClipEnd),
            Prim::Path { rings, closed, fill, stroke } => {
                if let (Some(f), true) = (fill, *closed) {
                    let mut flat: Vec<f64> = Vec::new();
                    let mut verts: Vec<P> = Vec::new();
                    let mut holes: Vec<usize> = Vec::new();
                    for (i, r) in rings.iter().enumerate() {
                        if i > 0 {
                            holes.push(verts.len());
                        }
                        for q in r {
                            flat.push(q[0] as f64);
                            flat.push(q[1] as f64);
                            verts.push(*q);
                        }
                    }
                    if let Ok(idx) = earcutr::earcut(&flat, &holes, 2) {
                        if !idx.is_empty() {
                            let b = scene_bounds(&verts);
                            out.push(U::Fill { verts, idx: idx.into_iter().map(|i| i as u32).collect(), color: c32(*f), bounds: b });
                        }
                    }
                }
                if let Some(s) = stroke {
                    for r in rings {
                        if r.len() >= 2 {
                            out.push(U::Stroke { pts: r.clone(), closed: *closed, color: c32(s.color), width: s.width, dashed: s.dashed });
                        }
                    }
                }
            }
            Prim::Circle { c, r, fill, stroke } => out.push(U::Circle {
                c: *c,
                r: *r,
                fill: fill.map(c32),
                stroke: stroke.map(|s| (c32(s.color), s.width, s.dashed)),
            }),
            Prim::Text { pos, text, size, color, align, rot, halo, .. } => out.push(U::Text {
                pos: *pos,
                text: text.clone(),
                size: *size,
                color: c32(*color),
                align: *align,
                rot: *rot,
                halo: halo.map(c32),
            }),
        }
    }
    out
}

fn scene_bounds(v: &[P]) -> [f32; 4] {
    let mut b = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
    for p in v {
        b[0] = b[0].min(p[0]);
        b[1] = b[1].min(p[1]);
        b[2] = b[2].max(p[0]);
        b[3] = b[3].max(p[1]);
    }
    b
}

// ------------------------------------------------------------ Application

pub struct EcarteApp {
    doc: Document,
    snapshot: Document,
    undo: Vec<Document>,
    redo: Vec<Document>,
    pending_undo: Option<Document>,
    data: Arc<Data>,
    loaded: HashMap<String, Arc<World>>,
    data_key: String,
    settings: Settings,
    sync_rx: Option<mpsc::Receiver<SyncMsg>>,
    sync_log: Vec<String>,
    cache: Vec<U>,
    dirty: bool,

    tool: Tool,
    tab: Tab,
    selected: Option<u32>,
    drag: Drag,
    draft: Vec<Pos>,

    cur_stroke: Rgba,
    cur_fill: Rgba,
    cur_width: f32,
    paint_color: Rgba,
    cap_with_name: bool,

    path: Option<PathBuf>,
    status: String,

    fmt: Format,
    dpi: f32,
    jpg_quality: u8,
    export_rx: Option<mpsc::Receiver<Result<String, String>>>,
    quit_after_save: bool,
}

impl EcarteApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut style = (*cc.egui_ctx.global_style()).clone();
        style.spacing.item_spacing = Vec2::new(6.0, 4.0);
        cc.egui_ctx.set_global_style(style);
        let doc = Document::default();
        EcarteApp {
            snapshot: doc.clone(),
            doc,
            undo: vec![],
            redo: vec![],
            pending_undo: None,
            data: Arc::new(Data::embedded_only()),
            loaded: HashMap::new(),
            data_key: String::new(),
            settings: Settings::load(),
            sync_rx: None,
            sync_log: vec![],
            cache: vec![],
            dirty: true,
            tool: Tool::Select,
            tab: Tab::Map,
            selected: None,
            drag: Drag::None,
            draft: vec![],
            cur_stroke: [200, 30, 30, 255],
            cur_fill: [200, 30, 30, 60],
            cur_width: 3.0,
            paint_color: [230, 160, 70, 255],
            cap_with_name: true,
            path: None,
            status: "Prêt. Molette = zoom carte, outil ✋ = déplacer.".into(),
            fmt: Format::Png,
            dpi: 150.0,
            jpg_quality: 90,
            export_rx: None,
            quit_after_save: false,
        }
    }

    fn rebuild(&mut self) {
        let prims = scene::build(&self.doc, &self.data);
        self.cache = convert(&prims);
        self.dirty = false;
    }

    fn set_doc(&mut self, d: Document) {
        self.doc = d.clone();
        self.snapshot = d;
        self.undo.clear();
        self.redo.clear();
        self.pending_undo = None;
        self.selected = None;
        self.draft.clear();
        self.dirty = true;
    }

    fn do_undo(&mut self) {
        if let Some(prev) = self.undo.pop() {
            self.redo.push(self.doc.clone());
            self.doc = prev.clone();
            self.snapshot = prev;
            self.dirty = true;
        }
    }

    fn do_redo(&mut self) {
        if let Some(next) = self.redo.pop() {
            self.undo.push(self.doc.clone());
            self.doc = next.clone();
            self.snapshot = next;
            self.dirty = true;
        }
    }

    fn save(&mut self, as_new: bool) -> bool {
        let path = if as_new || self.path.is_none() {
            rfd::FileDialog::new().add_filter("Carte E-cart-é", &["ecarte"]).set_file_name("carte.ecarte").save_file()
        } else {
            self.path.clone()
        };
        let Some(mut p) = path else {
            self.status = "Enregistrement annulé (ou boîte de dialogue indisponible : voir Aide > Ouvrir les logs)".into();
            logs::info("enregistrement : aucun chemin choisi (annulé ou dialogue indisponible)");
            return false;
        };
        if p.extension().is_none() {
            p.set_extension("ecarte");
        }
        match std::fs::write(&p, self.doc.to_json()) {
            Ok(()) => {
                logs::info(&format!("enregistré : {}", p.display()));
                self.status = format!("Enregistré : {}", p.display());
                self.path = Some(p);
                true
            }
            Err(e) => {
                logs::error(&format!("enregistrement {} : {e}", p.display()));
                self.status = format!("Erreur d'enregistrement : {e}");
                false
            }
        }
    }

    fn open(&mut self) {
        let Some(p) = rfd::FileDialog::new().add_filter("Carte E-cart-é", &["ecarte"]).pick_file() else {
            logs::info("ouverture : aucun fichier choisi (annulé ou dialogue indisponible)");
            return;
        };
        match std::fs::read_to_string(&p).map_err(|e| e.to_string()).and_then(|s| Document::from_json(&s)) {
            Ok(d) => {
                self.set_doc(d);
                self.status = format!("Ouvert : {}", p.display());
                self.path = Some(p);
            }
            Err(e) => self.status = format!("Erreur d'ouverture : {e}"),
        }
    }

    fn start_export(&mut self) {
        let fmt = self.fmt;
        let name = self.path.as_ref().and_then(|p| p.file_stem()).and_then(|s| s.to_str()).unwrap_or("carte").to_string();
        let Some(mut p) = rfd::FileDialog::new()
            .add_filter(fmt.ext(), &[fmt.ext()])
            .set_file_name(format!("{name}.{}", fmt.ext()))
            .save_file()
        else {
            logs::info("export : aucun chemin choisi (annulé ou dialogue indisponible)");
            self.status = "Export annulé (ou boîte de dialogue indisponible : voir Aide > Ouvrir les logs)".into();
            return;
        };
        if p.extension().is_none() {
            p.set_extension(fmt.ext());
        }
        let (doc, data, dpi, q) = (self.doc.clone(), self.data.clone(), self.dpi, self.jpg_quality);
        let (tx, rx) = mpsc::channel();
        self.export_rx = Some(rx);
        self.status = "Export en cours…".into();
        std::thread::spawn(move || {
            let r = export::export(&doc, &data, &p, fmt, dpi, q).map(|_| format!("Exporté : {}", p.display()));
            match &r {
                Ok(m) => logs::info(m),
                Err(e) => logs::error(&format!("export {} : {e}", p.display())),
            }
            let _ = tx.send(r);
        });
    }

    fn style_for_new(&self) -> (Rgba, Rgba, f32) {
        (self.cur_stroke, self.cur_fill, self.cur_width)
    }

    fn delete_selected(&mut self) {
        if let Some(id) = self.selected.take() {
            self.doc.items.retain(|i| i.id != id);
        }
    }

    fn duplicate_selected(&mut self) {
        let Some(id) = self.selected else { return };
        let Some(it) = self.doc.items.iter().find(|i| i.id == id).cloned() else { return };
        let view = View::new(&self.doc);
        let mut copy = scene::moved(&view, &it, 24.0, 24.0);
        copy.id = self.doc.next_id;
        copy.name = format!("{} (copie)", it.name);
        self.doc.next_id += 1;
        self.selected = Some(copy.id);
        self.doc.items.push(copy);
    }
}

// ------------------------------------------------------------ Panneaux

fn rgba_edit(ui: &mut egui::Ui, label: &str, c: &mut Rgba) {
    ui.horizontal(|ui| {
        ui.color_edit_button_srgba_unmultiplied(c);
        ui.label(label);
    });
}

fn pos_edit(ui: &mut egui::Ui, label: &str, pos: &mut Pos) {
    ui.horizontal(|ui| {
        ui.label(label);
        match pos {
            Pos::Geo { lon, lat } => {
                ui.add(egui::DragValue::new(lon).speed(0.1).range(-180.0..=180.0).suffix("° lon"));
                ui.add(egui::DragValue::new(lat).speed(0.1).range(-90.0..=90.0).suffix("° lat"));
            }
            Pos::Page { x, y } => {
                ui.add(egui::DragValue::new(x).speed(1.0).suffix(" x"));
                ui.add(egui::DragValue::new(y).speed(1.0).suffix(" y"));
            }
        }
    });
}

fn corner_combo(ui: &mut egui::Ui, id: &str, c: &mut Corner) {
    egui::ComboBox::from_id_salt(id).selected_text(c.label()).show_ui(ui, |ui| {
        for k in Corner::ALL {
            ui.selectable_value(c, k, k.label());
        }
    });
}

impl EcarteApp {
    fn panel_map(&mut self, ui: &mut egui::Ui) {
        let m = &mut self.doc.map;
        ui.heading("Projection");
        egui::ComboBox::from_id_salt("proj").selected_text(m.proj.label()).width(260.0).show_ui(ui, |ui| {
            for p in Projection::ALL {
                ui.selectable_value(&mut m.proj, p, p.label());
            }
        });
        ui.horizontal(|ui| {
            ui.label("Centre");
            ui.add(egui::DragValue::new(&mut m.center_lon).speed(0.5).range(-180.0..=180.0).suffix("° lon"));
            ui.add(egui::DragValue::new(&mut m.center_lat).speed(0.5).range(-85.0..=85.0).suffix("° lat"));
        });
        ui.horizontal(|ui| {
            ui.label("Zoom");
            ui.add(egui::Slider::new(&mut m.zoom, 0.3..=200.0).logarithmic(true));
        });
        if m.proj == Projection::LambertConic {
            ui.horizontal(|ui| {
                ui.label("Parallèles standard");
                ui.add(egui::DragValue::new(&mut m.std1).range(-85.0..=85.0).suffix("°"));
                ui.add(egui::DragValue::new(&mut m.std2).range(-85.0..=85.0).suffix("°"));
            });
        }
        ui.horizontal_wrapped(|ui| {
            if ui.button("Monde entier").clicked() {
                m.zoom = 1.0;
                m.center_lon = 0.0;
                m.center_lat = 0.0;
            }
            if ui.button("Europe").clicked() {
                m.zoom = 4.5;
                m.center_lon = 12.0;
                m.center_lat = 52.0;
            }
            if ui.button("France").clicked() {
                m.zoom = 11.0;
                m.center_lon = 2.5;
                m.center_lat = 46.5;
            }
        });
        ui.separator();
        ui.heading("Fond");
        rgba_edit(ui, "Océans", &mut m.ocean);
        rgba_edit(ui, "Terres", &mut m.land);
        ui.checkbox(&mut m.show_land, "Remplir les terres");
        rgba_edit(ui, "Frontières", &mut m.border);
        ui.add(egui::Slider::new(&mut m.border_width, 0.0..=4.0).text("épaisseur"));
        ui.checkbox(&mut m.country_labels, "Noms des pays");
        if m.country_labels {
            ui.add(egui::Slider::new(&mut m.country_label_size, 6.0..=30.0).text("taille"));
            ui.add(egui::Slider::new(&mut m.country_label_min_area, 0.0..=400.0).text("aire min (°²)"));
        }
        ui.horizontal(|ui| {
            ui.label(format!("{} pays coloriés", m.country_colors.len()));
            if ui.button("Tout effacer").clicked() {
                m.country_colors.clear();
            }
        });
        ui.separator();
        ui.heading("Capitales");
        let cs = &mut m.capitals;
        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(&mut cs.mode, CapMode::Off, "Aucune");
            ui.selectable_value(&mut cs.mode, CapMode::Points, "Points");
            ui.selectable_value(&mut cs.mode, CapMode::PointsAndNames, "Points + noms");
        });
        if cs.mode != CapMode::Off {
            egui::ComboBox::from_id_salt("capshape").selected_text(cs.shape.label()).show_ui(ui, |ui| {
                for s in PointShape::ALL {
                    ui.selectable_value(&mut cs.shape, s, s.label());
                }
            });
            ui.add(egui::Slider::new(&mut cs.size, 2.0..=20.0).text("taille"));
            rgba_edit(ui, "Remplissage", &mut cs.fill);
            rgba_edit(ui, "Contour", &mut cs.outline);
            if cs.mode == CapMode::PointsAndNames {
                ui.add(egui::Slider::new(&mut cs.label_size, 6.0..=30.0).text("taille du nom"));
                rgba_edit(ui, "Couleur du nom", &mut cs.label_color);
            }
            ui.checkbox(&mut cs.only_colored, "Seulement les pays coloriés");
        }
        ui.separator();
        ui.heading("Graticule");
        let g = &mut m.graticule;
        ui.checkbox(&mut g.show, "Afficher le quadrillage");
        ui.add(egui::Slider::new(&mut g.step, 1.0..=90.0).text("pas (°)").step_by(1.0));
        rgba_edit(ui, "Couleur", &mut g.color);
        ui.add(egui::Slider::new(&mut g.width, 0.2..=4.0).text("épaisseur"));
        ui.checkbox(&mut g.dashed, "Pointillé");
        ui.checkbox(&mut g.labels, "Étiquettes de coordonnées");
        if g.labels {
            ui.add(egui::Slider::new(&mut g.label_size, 6.0..=30.0).text("taille"));
            rgba_edit(ui, "Couleur du texte", &mut g.label_color);
            ui.horizontal(|ui| {
                ui.checkbox(&mut g.sides[0], "Gauche");
                ui.checkbox(&mut g.sides[1], "Haut");
                ui.checkbox(&mut g.sides[2], "Droite");
                ui.checkbox(&mut g.sides[3], "Bas");
            });
        }
    }

    fn panel_page(&mut self, ui: &mut egui::Ui) {
        let d = &mut self.doc;
        ui.heading("Page");
        egui::ComboBox::from_id_salt("preset").selected_text("Formats prédéfinis").show_ui(ui, |ui| {
            for (n, w, h) in [
                ("A4 paysage", 1123.0, 794.0),
                ("A4 portrait", 794.0, 1123.0),
                ("A3 paysage", 1587.0, 1123.0),
                ("16:9 (1920×1080)", 1920.0, 1080.0),
                ("Carré (1200×1200)", 1200.0, 1200.0),
                ("Défaut (1600×1000)", 1600.0, 1000.0),
            ] {
                if ui.selectable_label(false, n).clicked() {
                    d.page.w = w;
                    d.page.h = h;
                }
            }
        });
        ui.horizontal(|ui| {
            ui.add(egui::DragValue::new(&mut d.page.w).range(200.0..=8000.0).suffix(" px"));
            ui.label("×");
            ui.add(egui::DragValue::new(&mut d.page.h).range(200.0..=8000.0).suffix(" px"));
        });
        ui.label("Marges du cadre : gauche, haut, droite, bas");
        ui.horizontal(|ui| {
            for m in &mut d.page.margins {
                ui.add(egui::DragValue::new(m).range(0.0..=600.0));
            }
        });
        rgba_edit(ui, "Fond de page", &mut d.page.bg);
        egui::ComboBox::from_id_salt("font").selected_text(format!("Police : {}", d.page.font)).show_ui(ui, |ui| {
            for f in ["sans-serif", "serif", "monospace", "Arial", "DejaVu Sans", "Liberation Serif"] {
                ui.selectable_value(&mut d.page.font, f.to_string(), f);
            }
        });
        ui.label("(l'aperçu utilise une police fixe ; la police choisie s'applique à l'export)");
        ui.separator();
        ui.heading("Titre");
        ui.checkbox(&mut d.title.visible, "Afficher");
        ui.text_edit_singleline(&mut d.title.text);
        ui.add(egui::Slider::new(&mut d.title.size, 8.0..=120.0).text("taille"));
        ui.checkbox(&mut d.title.bold, "Gras");
        rgba_edit(ui, "Couleur", &mut d.title.color);
        ui.heading("Sous-titre");
        ui.checkbox(&mut d.subtitle.visible, "Afficher");
        ui.text_edit_singleline(&mut d.subtitle.text);
        ui.add(egui::Slider::new(&mut d.subtitle.size, 8.0..=80.0).text("taille"));
        rgba_edit(ui, "Couleur", &mut d.subtitle.color);
        ui.separator();
        ui.heading("Cadre");
        egui::ComboBox::from_id_salt("framestyle").selected_text(d.frame.style.label()).show_ui(ui, |ui| {
            for s in FrameStyle::ALL {
                ui.selectable_value(&mut d.frame.style, s, s.label());
            }
        });
        rgba_edit(ui, "Couleur", &mut d.frame.color);
        ui.add(egui::Slider::new(&mut d.frame.width, 0.5..=10.0).text("épaisseur"));
        ui.horizontal(|ui| {
            ui.checkbox(&mut d.frame.scale_bar, "Échelle");
            corner_combo(ui, "scc", &mut d.frame.scale_corner);
        });
        ui.horizontal(|ui| {
            ui.checkbox(&mut d.frame.north_arrow, "Nord");
            corner_combo(ui, "nac", &mut d.frame.north_corner);
        });
    }

    fn panel_items(&mut self, ui: &mut egui::Ui) {
        ui.heading("Éléments");
        let mut sel = self.selected;
        let mut del: Option<usize> = None;
        let mut swap: Option<(usize, usize)> = None;
        let n = self.doc.items.len();
        egui::ScrollArea::vertical().id_salt("items").max_height(170.0).show(ui, |ui| {
            for i in (0..n).rev() {
                let it = &mut self.doc.items[i];
                ui.horizontal(|ui| {
                    ui.checkbox(&mut it.visible, "");
                    if ui.selectable_label(sel == Some(it.id), &it.name).clicked() {
                        sel = Some(it.id);
                    }
                    if ui.small_button("^").clicked() && i + 1 < n {
                        swap = Some((i, i + 1));
                    }
                    if ui.small_button("v").clicked() && i > 0 {
                        swap = Some((i, i - 1));
                    }
                    if ui.small_button("x").clicked() {
                        del = Some(i);
                    }
                });
            }
        });
        self.selected = sel;
        if let Some((a, b)) = swap {
            self.doc.items.swap(a, b);
        }
        if let Some(i) = del {
            let id = self.doc.items[i].id;
            self.doc.items.remove(i);
            if self.selected == Some(id) {
                self.selected = None;
            }
        }
        if ui.button("Dupliquer").clicked() {
            self.duplicate_selected();
        }
        ui.separator();
        let Some(id) = self.selected else {
            ui.label("Aucun élément sélectionné.");
            return;
        };
        let Some(it) = self.doc.item_mut(id) else { return };
        ui.heading("Propriétés");
        ui.text_edit_singleline(&mut it.name);
        let is_text = matches!(it.kind, Kind::Text { .. });
        rgba_edit(ui, if is_text { "Couleur du texte" } else { "Contour" }, if is_text { &mut it.fill } else { &mut it.stroke });
        if !is_text && !matches!(it.kind, Kind::Line { .. }) {
            rgba_edit(ui, "Remplissage", &mut it.fill);
        }
        if !is_text {
            ui.add(egui::Slider::new(&mut it.width, 0.0..=20.0).text("épaisseur"));
            ui.checkbox(&mut it.dashed, "Pointillé");
        }
        match &mut it.kind {
            Kind::Text { pos, text, size, bold, italic, rot, halo, align } => {
                ui.add(egui::TextEdit::multiline(text).desired_rows(2));
                ui.add(egui::Slider::new(size, 6.0..=200.0).text("taille"));
                ui.add(egui::Slider::new(rot, -180.0..=180.0).text("rotation °"));
                ui.horizontal(|ui| {
                    ui.checkbox(bold, "Gras");
                    ui.checkbox(italic, "Italique");
                    ui.checkbox(halo, "Halo blanc");
                });
                ui.horizontal(|ui| {
                    ui.selectable_value(align, HAlign::Left, "Gauche");
                    ui.selectable_value(align, HAlign::Center, "Centre");
                    ui.selectable_value(align, HAlign::Right, "Droite");
                });
                pos_edit(ui, "Position", pos);
            }
            Kind::Point { pos, radius, shape, label, label_size } => {
                egui::ComboBox::from_id_salt("shape").selected_text(shape.label()).show_ui(ui, |ui| {
                    for s in PointShape::ALL {
                        ui.selectable_value(shape, s, s.label());
                    }
                });
                ui.add(egui::Slider::new(radius, 2.0..=60.0).text("rayon"));
                ui.text_edit_singleline(label);
                ui.add(egui::Slider::new(label_size, 6.0..=60.0).text("taille étiquette"));
                pos_edit(ui, "Position", pos);
            }
            Kind::Circle { center, radius, unit } => {
                ui.horizontal(|ui| {
                    ui.add(egui::DragValue::new(radius).speed(1.0).range(0.0..=40000.0));
                    ui.selectable_value(unit, RadiusUnit::Km, "km (géographique)");
                    ui.selectable_value(unit, RadiusUnit::Px, "px");
                });
                pos_edit(ui, "Centre", center);
            }
            Kind::Line { pts, arrow_start, arrow_end, geodesic } => {
                ui.horizontal(|ui| {
                    ui.checkbox(arrow_start, "Flèche début");
                    ui.checkbox(arrow_end, "Flèche fin");
                });
                ui.checkbox(geodesic, "Grand cercle (géodésique)");
                for (i, p) in pts.iter_mut().enumerate() {
                    pos_edit(ui, &format!("Point {}", i + 1), p);
                }
            }
            Kind::Rect { a, b } => {
                pos_edit(ui, "Coin A", a);
                pos_edit(ui, "Coin B", b);
            }
            Kind::Polygon { pts } => {
                for (i, p) in pts.iter_mut().enumerate() {
                    pos_edit(ui, &format!("Sommet {}", i + 1), p);
                }
            }
        }
    }

    fn panel_legend(&mut self, ui: &mut egui::Ui) {
        let doc = &mut self.doc;
        ui.heading("Légende");
        let lg = &mut doc.legend;
        ui.checkbox(&mut lg.visible, "Afficher");
        ui.text_edit_singleline(&mut lg.title);
        corner_combo(ui, "lgc", &mut lg.corner);
        ui.add(egui::Slider::new(&mut lg.size, 8.0..=40.0).text("taille"));
        let mut del = None;
        for (i, (c, t)) in lg.entries.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                ui.color_edit_button_srgba_unmultiplied(c);
                ui.text_edit_singleline(t);
                if ui.small_button("x").clicked() {
                    del = Some(i);
                }
            });
        }
        if let Some(i) = del {
            lg.entries.remove(i);
        }
        ui.horizontal(|ui| {
            if ui.button("+ Entrée").clicked() {
                lg.entries.push(([200, 60, 60, 255], "Nouvelle entrée".into()));
            }
            if ui.button("Depuis les pays coloriés").clicked() {
                let mut seen: Vec<Rgba> = vec![];
                for c in doc.map.country_colors.values() {
                    if !seen.contains(c) {
                        seen.push(*c);
                    }
                }
                for c in seen {
                    if !lg.entries.iter().any(|(k, _)| *k == c) {
                        lg.entries.push((c, "À nommer".into()));
                    }
                }
                lg.visible = true;
            }
        });
    }

    fn panel_export(&mut self, ui: &mut egui::Ui) {
        ui.heading("Exporter");
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.fmt, Format::Png, "PNG");
            ui.selectable_value(&mut self.fmt, Format::Jpg, "JPG");
            ui.selectable_value(&mut self.fmt, Format::Svg, "SVG");
        });
        if self.fmt != Format::Svg {
            ui.add(egui::Slider::new(&mut self.dpi, 72.0..=600.0).text("dpi"));
            let k = self.dpi / 96.0;
            ui.label(format!("Image : {} × {} px", (self.doc.page.w * k) as u32, (self.doc.page.h * k) as u32));
        }
        if self.fmt == Format::Jpg {
            ui.add(egui::Slider::new(&mut self.jpg_quality, 40..=100).text("qualité"));
        }
        let busy = self.export_rx.is_some();
        if ui.add_enabled(!busy, egui::Button::new("Exporter…")).clicked() {
            self.start_export();
        }
        if busy {
            ui.label("Export en cours…");
        }
        ui.separator();
        ui.label("Le SVG est vectoriel et éditable (Inkscape, LibreOffice Draw). PNG/JPG sont rendus à partir du même SVG : aperçu et export sont identiques.");
    }
}

// ------------------------------------------------------------ Canevas

struct Xf {
    origin: Pos2,
    s: f32,
}
impl Xf {
    fn to_screen(&self, p: P) -> Pos2 {
        Pos2::new(self.origin.x + p[0] * self.s, self.origin.y + p[1] * self.s)
    }
    fn to_page(&self, p: Pos2) -> P {
        [(p.x - self.origin.x) / self.s, (p.y - self.origin.y) / self.s]
    }
}

fn paint_cache(painter: &egui::Painter, canvas: Rect, xf: &Xf, cache: &[U]) {
    let mut stack: Vec<egui::Painter> = Vec::new();
    let mut cur = painter.clone();
    for u in cache {
        match u {
            U::ClipBegin(r) => {
                stack.push(cur.clone());
                let rect = Rect::from_min_max(xf.to_screen([r[0], r[1]]), xf.to_screen([r[0] + r[2], r[1] + r[3]]));
                cur = painter.with_clip_rect(rect.intersect(canvas));
            }
            U::ClipEnd => {
                if let Some(p) = stack.pop() {
                    cur = p;
                }
            }
            U::Fill { verts, idx, color, bounds } => {
                let b = Rect::from_min_max(xf.to_screen([bounds[0], bounds[1]]), xf.to_screen([bounds[2], bounds[3]]));
                if !b.intersects(cur.clip_rect()) {
                    continue;
                }
                let mut mesh = epaint::Mesh::default();
                mesh.vertices.reserve(verts.len());
                for v in verts {
                    mesh.colored_vertex(xf.to_screen(*v), *color);
                }
                mesh.indices = idx.clone();
                cur.add(epaint::Shape::mesh(mesh));
            }
            U::Stroke { pts, closed, color, width, dashed } => {
                let mut v: Vec<Pos2> = pts.iter().map(|p| xf.to_screen(*p)).collect();
                let st = epaint::Stroke::new((width * xf.s).max(0.4), *color);
                if *dashed {
                    if *closed {
                        v.push(v[0]);
                    }
                    cur.extend(epaint::Shape::dashed_line(&v, st, (width * 4.0 + 2.0) * xf.s, (width * 3.0 + 2.0) * xf.s));
                } else if *closed {
                    cur.add(epaint::Shape::closed_line(v, st));
                } else {
                    cur.add(epaint::Shape::line(v, st));
                }
            }
            U::Circle { c, r, fill, stroke } => {
                let (cp, rr) = (xf.to_screen(*c), r * xf.s);
                if let Some(f) = fill {
                    cur.circle_filled(cp, rr, *f);
                }
                if let Some((col, w, _)) = stroke {
                    cur.circle_stroke(cp, rr, epaint::Stroke::new((w * xf.s).max(0.4), *col));
                }
            }
            U::Text { pos, text, size, color, align, rot, halo } => {
                let galley = cur.layout_no_wrap(text.clone(), FontId::proportional((size * xf.s).max(3.0)), *color);
                let sz = galley.size();
                let ox = match align {
                    HAlign::Left => 0.0,
                    HAlign::Center => -sz.x / 2.0,
                    HAlign::Right => -sz.x,
                };
                let oy = -sz.y / 2.0;
                let a = rot.to_radians();
                let (sn, cs) = a.sin_cos();
                let p0 = xf.to_screen(*pos) + Vec2::new(ox * cs - oy * sn, ox * sn + oy * cs);
                let draw = |off: Vec2, col: Color32, g: &Arc<egui::Galley>| {
                    let mut ts = epaint::TextShape::new(p0 + off, g.clone(), col);
                    ts.angle = a;
                    cur.add(ts);
                };
                if let Some(h) = halo {
                    let hg = cur.layout_no_wrap(text.clone(), FontId::proportional((size * xf.s).max(3.0)), *h);
                    let d = (size * xf.s * 0.09).max(0.8);
                    for off in [Vec2::new(d, 0.0), Vec2::new(-d, 0.0), Vec2::new(0.0, d), Vec2::new(0.0, -d)] {
                        draw(off, *h, &hg);
                    }
                }
                draw(Vec2::ZERO, *color, &galley);
            }
        }
    }
}

impl EcarteApp {
    fn canvas(&mut self, ui: &mut egui::Ui) {
        let avail = ui.available_rect_before_wrap();
        let resp = ui.allocate_rect(avail, Sense::click_and_drag());
        let ctx = ui.ctx().clone();
        let painter = ui.painter_at(avail);
        painter.rect_filled(avail, 0.0, Color32::from_gray(90));

        let (pw, ph) = (self.doc.page.w, self.doc.page.h);
        let s = ((avail.width() - 20.0) / pw).min((avail.height() - 20.0) / ph).max(0.05);
        let size = Vec2::new(pw * s, ph * s);
        let origin = avail.center() - size / 2.0;
        let xf = Xf { origin, s };
        let page_rect = Rect::from_min_size(origin, size);
        painter.rect_filled(page_rect.translate(Vec2::new(4.0, 4.0)), 0.0, Color32::from_black_alpha(80));

        if self.dirty {
            self.rebuild();
        }
        paint_cache(&painter, avail, &xf, &self.cache);

        let view = View::new(&self.doc);
        let hover = resp.hover_pos().map(|p| xf.to_page(p));
        if let Some(h) = hover {
            let geo = view.unproject(h).filter(|_| {
                let f = view.frame;
                h[0] >= f[0] && h[0] <= f[0] + f[2] && h[1] >= f[1] && h[1] <= f[1] + f[3]
            });
            let kmpp = view.km_per_px().map(|k| format!(" · {:.2} km/px", k)).unwrap_or_default();
            self.status = match geo {
                Some((lo, la)) => format!("{:.3}°{} {:.3}°{}{}", la.abs(), if la >= 0.0 { "N" } else { "S" }, lo.abs(), if lo >= 0.0 { "E" } else { "O" }, kmpp),
                None => format!("Zoom ×{:.2}{}", self.doc.map.zoom, kmpp),
            };
        }

        // zoom à la molette
        if resp.hovered() {
            let dy = ctx.input(|i| i.smooth_scroll_delta().y);
            if dy.abs() > 0.1 {
                self.doc.map.zoom = (self.doc.map.zoom * (dy as f64 * 0.0015).exp()).clamp(0.3, 200.0);
            }
        }

        let tol = 7.0 / s;
        let pos_at = |p: Option<Pos2>| p.map(|p| xf.to_page(p));
        let pointer = pos_at(resp.interact_pointer_pos());
        let press = ctx.input(|i| i.pointer.press_origin()).map(|p| xf.to_page(p));
        let (sel_style, tool) = (self.style_for_new(), self.tool);

        // ------- début de glissement
        if resp.drag_started() {
            if resp.dragged_by(egui::PointerButton::Middle) || tool == Tool::Pan {
                self.drag = Drag::Pan;
            } else if let (Some(start), true) = (press, resp.dragged_by(egui::PointerButton::Primary)) {
                match tool {
                    Tool::Select => {
                        let found = self.doc.items.iter().rev().filter(|i| i.visible).find(|it| {
                            scene::item_shape(&view, it).is_some_and(|sh| scene::hit(&sh, start, tol + it.width / 2.0, it.fill[3] > 0))
                        });
                        match found {
                            Some(it) => {
                                self.selected = Some(it.id);
                                self.drag = Drag::Move { orig: it.clone(), start };
                            }
                            None => {
                                self.selected = None;
                                self.drag = Drag::Pan;
                            }
                        }
                    }
                    Tool::Circle | Tool::Line | Tool::Arrow | Tool::Rect => {
                        let p0 = view.pos_from_page(start);
                        let (stroke, fill, w) = sel_style;
                        let (name, kind) = match tool {
                            Tool::Circle => ("Cercle", Kind::Circle { center: p0, radius: 0.0, unit: if matches!(p0, Pos::Geo { .. }) { RadiusUnit::Km } else { RadiusUnit::Px } }),
                            Tool::Line => ("Ligne", Kind::Line { pts: vec![p0, p0], arrow_start: false, arrow_end: false, geodesic: false }),
                            Tool::Arrow => ("Flèche", Kind::Line { pts: vec![p0, p0], arrow_start: false, arrow_end: true, geodesic: false }),
                            _ => ("Rectangle", Kind::Rect { a: p0, b: p0 }),
                        };
                        let fill = if matches!(tool, Tool::Line | Tool::Arrow) { [0, 0, 0, 0] } else { fill };
                        let id = self.doc.add_item(name, stroke, fill, w, kind);
                        self.selected = Some(id);
                        self.drag = Drag::Create { id, start };
                    }
                    _ => {}
                }
            }
        }

        // ------- glissement en cours
        if resp.dragged() {
            match &self.drag {
                Drag::Pan => {
                    let d = resp.drag_delta() / s;
                    let f = view.frame;
                    let c = [f[0] + f[2] / 2.0 - d.x, f[1] + f[3] / 2.0 - d.y];
                    if let Some((lo, la)) = view.unproject(c) {
                        self.doc.map.center_lon = lo;
                        self.doc.map.center_lat = la.clamp(-85.0, 85.0);
                    }
                }
                Drag::Move { orig, start } => {
                    if let Some(cur) = pointer {
                        let moved = scene::moved(&view, orig, cur[0] - start[0], cur[1] - start[1]);
                        if let Some(it) = self.doc.item_mut(orig.id) {
                            it.kind = moved.kind;
                        }
                    }
                }
                Drag::Create { id, .. } => {
                    if let Some(cur) = pointer {
                        let end = view.pos_from_page(cur);
                        let geo_cur = view.unproject(cur);
                        if let Some(it) = self.doc.item_mut(*id) {
                            match &mut it.kind {
                                Kind::Circle { center, radius, unit } => match (center, unit) {
                                    (Pos::Geo { lon, lat }, RadiusUnit::Km) => {
                                        if let Some(g) = geo_cur {
                                            *radius = haversine_km((*lon, *lat), g) as f32;
                                        }
                                    }
                                    (c, _) => {
                                        if let Some(cp) = view.pos_to_page(c) {
                                            *radius = ((cur[0] - cp[0]).powi(2) + (cur[1] - cp[1]).powi(2)).sqrt();
                                        }
                                    }
                                },
                                Kind::Line { pts, .. } => pts[1] = end,
                                Kind::Rect { b, .. } => *b = end,
                                _ => {}
                            }
                        }
                    }
                }
                Drag::None => {}
            }
        }

        if resp.drag_stopped() {
            if let Drag::Create { id, start } = self.drag {
                let moved_enough = pointer.map(|c| (c[0] - start[0]).hypot(c[1] - start[1]) > 3.0 / s * 1.0).unwrap_or(false);
                if !moved_enough {
                    self.doc.items.retain(|i| i.id != id);
                    self.selected = None;
                } else {
                    self.tab = Tab::Items;
                }
            }
            self.drag = Drag::None;
        }

        // ------- clics
        if resp.clicked() || resp.secondary_clicked() || resp.double_clicked() {
            if let Some(p) = pointer {
                let pos = view.pos_from_page(p);
                let (stroke, fill, w) = sel_style;
                match tool {
                    Tool::Select => {
                        if resp.clicked() {
                            let found = self.doc.items.iter().rev().filter(|i| i.visible).find(|it| {
                                scene::item_shape(&view, it).is_some_and(|sh| scene::hit(&sh, p, tol + it.width / 2.0, it.fill[3] > 0))
                            });
                            self.selected = found.map(|i| i.id);
                            if self.selected.is_some() {
                                self.tab = Tab::Items;
                            }
                        }
                    }
                    Tool::Text if resp.clicked() => {
                        let id = self.doc.add_item("Texte", [0, 0, 0, 0], [20, 20, 20, 255], 0.0, Kind::Text { pos, text: "Texte".into(), size: 28.0, bold: false, italic: false, rot: 0.0, halo: true, align: HAlign::Center });
                        self.selected = Some(id);
                        self.tab = Tab::Items;
                        self.tool = Tool::Select;
                    }
                    Tool::Point if resp.clicked() => {
                        let id = self.doc.add_item("Point", [255, 255, 255, 255], stroke, 1.5, Kind::Point { pos, radius: 7.0, shape: PointShape::Circle, label: String::new(), label_size: 16.0 });
                        self.selected = Some(id);
                        self.tab = Tab::Items;
                    }
                    Tool::Polyline | Tool::Polygon => {
                        if resp.secondary_clicked() || resp.double_clicked() {
                            self.finish_draft(tool, stroke, fill, w);
                        } else if resp.clicked() {
                            let dup = self.draft.last().and_then(|l| view.pos_to_page(l)).is_some_and(|l| (l[0] - p[0]).hypot(l[1] - p[1]) < 2.0 / s);
                            if !dup {
                                self.draft.push(pos);
                            }
                        }
                    }
                    Tool::Capital if resp.clicked() => {
                        self.place_capital(&view, p);
                    }
                    Tool::Paint => {
                        if let Some((lo, la)) = view.unproject(p) {
                            if let Some(c) = self.data.base.country_at(lo, la) {
                                let iso = c.iso.clone();
                                let name = c.name_fr.clone();
                                if resp.secondary_clicked() {
                                    self.doc.map.country_colors.remove(&iso);
                                } else {
                                    self.doc.map.country_colors.insert(iso, self.paint_color);
                                }
                                self.status = name;
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        // ------- clavier
        let (del, esc, enter, undo, redo, save, open) = ctx.input(|i| {
            (
                i.key_pressed(egui::Key::Delete),
                i.key_pressed(egui::Key::Escape),
                i.key_pressed(egui::Key::Enter),
                i.modifiers.command && !i.modifiers.shift && i.key_pressed(egui::Key::Z),
                i.modifiers.command && (i.key_pressed(egui::Key::Y) || (i.modifiers.shift && i.key_pressed(egui::Key::Z))),
                i.modifiers.command && i.key_pressed(egui::Key::S),
                i.modifiers.command && i.key_pressed(egui::Key::O),
            )
        });
        let typing = ctx.egui_wants_keyboard_input();
        if !typing {
            if del {
                self.delete_selected();
            }
            if undo {
                self.do_undo();
            }
            if redo {
                self.do_redo();
            }
            if enter && matches!(tool, Tool::Polyline | Tool::Polygon) {
                let (stroke, fill, w) = sel_style;
                self.finish_draft(tool, stroke, fill, w);
            }
            let keys = [
                (egui::Key::S, Tool::Select),
                (egui::Key::H, Tool::Pan),
                (egui::Key::T, Tool::Text),
                (egui::Key::P, Tool::Point),
                (egui::Key::C, Tool::Circle),
                (egui::Key::L, Tool::Line),
                (egui::Key::A, Tool::Arrow),
                (egui::Key::R, Tool::Rect),
                (egui::Key::G, Tool::Polygon),
                (egui::Key::M, Tool::Capital),
                (egui::Key::K, Tool::Paint),
            ];
            ctx.input(|i| {
                if !i.modifiers.command {
                    for (k, t) in keys {
                        if i.key_pressed(k) {
                            self.tool = t;
                        }
                    }
                }
            });
        }
        if esc {
            self.draft.clear();
            self.tool = Tool::Select;
        }
        if save {
            self.save(false);
        }
        if open {
            self.open();
        }

        // ------- surcouches : sélection et brouillon
        if let Some(it) = self.selected.and_then(|id| self.doc.items.iter().find(|i| i.id == id)) {
            if let Some(sh) = scene::item_shape(&view, it) {
                let b = scene::shape_bounds(&sh);
                let r = Rect::from_min_max(xf.to_screen([b[0], b[1]]), xf.to_screen([b[2], b[3]])).expand(5.0);
                let st = epaint::Stroke::new(1.5, Color32::from_rgb(40, 120, 255));
                painter.extend(epaint::Shape::dashed_line(&[r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()], st, 5.0, 4.0));
                for c in [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom()] {
                    painter.rect_filled(Rect::from_center_size(c, Vec2::splat(6.0)), 1.0, Color32::from_rgb(40, 120, 255));
                }
            }
        }
        if !self.draft.is_empty() {
            let mut pts: Vec<Pos2> = self.draft.iter().filter_map(|p| view.pos_to_page(p)).map(|p| xf.to_screen(p)).collect();
            if let Some(h) = resp.hover_pos() {
                pts.push(h);
            }
            painter.add(epaint::Shape::line(pts.clone(), epaint::Stroke::new(2.0, Color32::from_rgb(40, 120, 255))));
            for p in &pts {
                painter.circle_filled(*p, 3.0, Color32::from_rgb(40, 120, 255));
            }
        }
        if tool == Tool::Paint {
            if let Some(h) = resp.hover_pos() {
                painter.circle_filled(h + Vec2::new(14.0, 14.0), 7.0, c32(self.paint_color));
            }
        }
        if matches!(tool, Tool::Pan) {
            ctx.set_cursor_icon(egui::CursorIcon::Grab);
        } else if tool != Tool::Select {
            ctx.set_cursor_icon(egui::CursorIcon::Crosshair);
        }
        painter.text(avail.right_bottom() - Vec2::new(8.0, 6.0), Align2::RIGHT_BOTTOM, "", FontId::default(), Color32::WHITE);
    }

    fn finish_draft(&mut self, tool: Tool, stroke: Rgba, fill: Rgba, w: f32) {
        let pts = std::mem::take(&mut self.draft);
        let need = if tool == Tool::Polygon { 3 } else { 2 };
        if pts.len() < need {
            return;
        }
        let id = if tool == Tool::Polygon {
            self.doc.add_item("Polygone", stroke, fill, w, Kind::Polygon { pts })
        } else {
            self.doc.add_item("Polyligne", stroke, [0, 0, 0, 0], w, Kind::Line { pts, arrow_start: false, arrow_end: false, geodesic: false })
        };
        self.selected = Some(id);
        self.tab = Tab::Items;
    }
}

// ------------------------------------------------------------ eframe

impl eframe::App for EcarteApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        if let Some(rx) = &self.export_rx {
            if let Ok(r) = rx.try_recv() {
                self.status = match r {
                    Ok(m) => m,
                    Err(e) => format!("Échec de l'export : {e}"),
                };
                self.export_rx = None;
            } else {
                ctx.request_repaint_after(std::time::Duration::from_millis(200));
            }
        }

        self.poll_data(&ctx);

        egui::Panel::top("menu").show_inside(ui, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button("Fichier", |ui| {
                    if ui.button("Nouveau").clicked() {
                        self.set_doc(Document::default());
                        self.path = None;
                    }
                    if ui.button("Ouvrir…  Ctrl+O").clicked() {
                        self.open();
                    }
                    if ui.button("Enregistrer  Ctrl+S").clicked() {
                        self.save(false);
                    }
                    if ui.button("Enregistrer sous…").clicked() {
                        self.save(true);
                    }
                    ui.separator();
                    for (f, n) in [(Format::Png, "Exporter en PNG…"), (Format::Jpg, "Exporter en JPG…"), (Format::Svg, "Exporter en SVG…")] {
                        if ui.button(n).clicked() {
                            self.fmt = f;
                            self.start_export();
                        }
                    }
                    ui.separator();
                    if ui.button("Quitter").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
                ui.menu_button("Édition", |ui| {
                    if ui.add_enabled(!self.undo.is_empty(), egui::Button::new("Annuler  Ctrl+Z")).clicked() {
                        self.do_undo();
                    }
                    if ui.add_enabled(!self.redo.is_empty(), egui::Button::new("Rétablir  Ctrl+Y")).clicked() {
                        self.do_redo();
                    }
                    ui.separator();
                    if ui.button("Dupliquer").clicked() {
                        self.duplicate_selected();
                    }
                    if ui.button("Supprimer  Suppr").clicked() {
                        self.delete_selected();
                    }
                });
                ui.menu_button("Aide", |ui| {
                    ui.label("E-cart-é — cartes thématiques");
                    ui.label("Molette : zoom · outil ✋ ou clic milieu : déplacer");
                    ui.label("Polygone/polyligne : double-clic ou Entrée pour finir");
                    ui.label("Données : Natural Earth (domaine public)");
                    ui.separator();
                    if ui.button("Ouvrir les logs").clicked() {
                        let p = logs::path();
                        logs::info("ouverture du journal demandée");
                        self.status = match logs::open_in_viewer(&p) {
                            Ok(()) => format!("Logs : {}", p.display()),
                            Err(e) => format!("Logs : {e}"),
                        };
                        ui.close();
                    }
                    ui.label(format!("{}", logs::path().display()));
                });
            });
        });

        egui::Panel::bottom("status").show_inside(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(&self.status);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let name = self.path.as_ref().and_then(|p| p.file_name()).and_then(|s| s.to_str()).unwrap_or("sans titre");
                    ui.label(name);
                });
            });
        });

        egui::Panel::left("tools").resizable(true).default_size(112.0).size_range(64.0..=300.0).show_inside(ui, |ui| {
            egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = Vec2::splat(4.0);
                    for (t, _, tip) in Tool::ALL.iter() {
                        let (rect, resp) = ui.allocate_exact_size(Vec2::splat(32.0), Sense::click());
                        let sel = self.tool == *t;
                        let bg = if sel {
                            Color32::from_rgb(40, 100, 200)
                        } else if resp.hovered() {
                            Color32::from_gray(75)
                        } else {
                            Color32::from_gray(52)
                        };
                        ui.painter().rect_filled(rect, 4.0, bg);
                        draw_tool_icon(ui.painter(), rect.shrink(7.0), *t, Color32::WHITE);
                        if resp.on_hover_text(*tip).clicked() {
                            self.tool = *t;
                            self.draft.clear();
                        }
                    }
                });
                ui.separator();
                ui.label("Style");
                ui.horizontal_wrapped(|ui| {
                    ui.color_edit_button_srgba_unmultiplied(&mut self.cur_stroke).on_hover_text("Contour");
                    ui.color_edit_button_srgba_unmultiplied(&mut self.cur_fill).on_hover_text("Remplissage");
                });
                ui.add(egui::DragValue::new(&mut self.cur_width).range(0.5..=20.0).speed(0.1).prefix("trait "));
                if self.tool == Tool::Capital {
                    ui.checkbox(&mut self.cap_with_name, "Avec le nom");
                }
                if self.tool == Tool::Paint {
                    ui.label("Couleur du pays");
                    ui.color_edit_button_srgba_unmultiplied(&mut self.paint_color);
                }
            });
        });

        egui::Panel::right("props").resizable(true).default_size(310.0).size_range(200.0..=700.0).show_inside(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                for (t, n) in [(Tab::Map, "Carte"), (Tab::Page, "Page"), (Tab::Items, "Éléments"), (Tab::Legend, "Légende"), (Tab::Data, "Données"), (Tab::Export, "Export")] {
                    ui.selectable_value(&mut self.tab, t, n);
                }
            });
            ui.separator();
            egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| match self.tab {
                Tab::Map => self.panel_map(ui),
                Tab::Page => self.panel_page(ui),
                Tab::Items => self.panel_items(ui),
                Tab::Legend => self.panel_legend(ui),
                Tab::Data => self.panel_data(ui),
                Tab::Export => self.panel_export(ui),
            });
        });

        egui::CentralPanel::default().show_inside(ui, |ui| self.canvas(ui));

        // suivi des modifications -> annuler/rétablir groupé + reconstruction du cache
        if self.doc != self.snapshot {
            if self.pending_undo.is_none() {
                self.pending_undo = Some(self.snapshot.clone());
            }
            self.snapshot = self.doc.clone();
            self.dirty = true;
        }
        if self.pending_undo.is_some() && !ctx.input(|i| i.pointer.any_down()) {
            if let Some(p) = self.pending_undo.take() {
                self.undo.push(p);
                if self.undo.len() > 100 {
                    self.undo.remove(0);
                }
                self.redo.clear();
            }
        }
        if self.dirty {
            ctx.request_repaint();
        }
    }
}

#[allow(dead_code)]
fn _unused(_: &mut bool) {}

// ------------------------------------------------------------ Données hors ligne / synchronisation

impl EcarteApp {
    /// Identifiants nécessaires au document courant.
    fn needed_ids(&self) -> Vec<String> {
        let mut v = vec![self.doc.map.base.clone()];
        v.extend(self.doc.map.layers.iter().filter(|l| l.enabled).map(|l| l.id.clone()));
        v
    }

    /// (Re)construit `self.data` depuis les données intégrées / en cache. Ne touche jamais au réseau.
    fn refresh_data(&mut self) {
        let ids = self.needed_ids();
        let mut layers = HashMap::new();
        let mut base: Option<Arc<World>> = None;
        for id in &ids {
            let w = match self.loaded.get(id) {
                Some(w) => Some(w.clone()),
                None => data::load(id).map(|w| {
                    let w = Arc::new(w);
                    self.loaded.insert(id.clone(), w.clone());
                    w
                }),
            };
            match (w, data::find(id).map(|d| d.kind)) {
                (Some(w), Some(DsKind::Base)) => base = Some(w),
                (Some(w), _) => {
                    layers.insert(id.clone(), w);
                }
                (None, Some(DsKind::Base)) => self.status = format!("« {id} » indisponible hors ligne : fond intégré utilisé"),
                _ => {}
            }
        }
        let base = base.unwrap_or_else(|| {
            self.loaded
                .entry("countries_110m".into())
                .or_insert_with(|| Arc::new(data::load("countries_110m").unwrap_or_default()))
                .clone()
        });
        self.data = Arc::new(Data { base, layers, capitals: self.data.capitals.clone() });
        self.data_key = ids.join("|");
        self.dirty = true;
    }

    fn start_sync(&mut self, ids: Vec<&'static str>) {
        if self.sync_rx.is_some() || ids.is_empty() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        self.sync_rx = Some(rx);
        self.status = "Synchronisation des cartes…".into();
        std::thread::spawn(move || {
            for id in ids {
                if let Some(d) = data::find(id) {
                    let _ = tx.send(SyncMsg::Item(id.to_string(), data::sync_one(d)));
                }
            }
            let _ = tx.send(SyncMsg::Done);
        });
    }

    fn cached_ids() -> Vec<&'static str> {
        data::CATALOG
            .iter()
            .filter(|d| d.embedded.is_none() && data::availability(d) == Availability::Cached)
            .map(|d| d.id)
            .collect()
    }

    fn poll_data(&mut self, ctx: &egui::Context) {
        if self.data_key.is_empty() {
            self.refresh_data();
            if self.settings.auto_sync {
                let ids = Self::cached_ids();
                self.start_sync(ids);
            }
        } else if self.needed_ids().join("|") != self.data_key {
            self.refresh_data();
        }
        let mut done = false;
        let mut reload = false;
        let mut msgs = Vec::new();
        if let Some(rx) = &self.sync_rx {
            while let Ok(m) = rx.try_recv() {
                msgs.push(m);
            }
            ctx.request_repaint_after(std::time::Duration::from_millis(300));
        }
        for m in msgs {
            match m {
                SyncMsg::Item(id, out) => {
                    let txt = match &out {
                        SyncOutcome::Downloaded => "mis à jour".to_string(),
                        SyncOutcome::UpToDate => "déjà à jour".to_string(),
                        SyncOutcome::Offline(_) => "hors ligne (cache conservé)".to_string(),
                        SyncOutcome::Failed(e) => format!("échec : {e}"),
                    };
                    if out == SyncOutcome::Downloaded {
                        self.loaded.remove(&id);
                        reload = true;
                    }
                    logs::info(&format!("sync {id} : {txt}"));
                    self.sync_log.push(format!("{id} : {txt}"));
                }
                SyncMsg::Done => done = true,
            }
        }
        if reload {
            self.refresh_data();
        }
        if done {
            self.sync_rx = None;
            self.status = self.sync_log.last().cloned().unwrap_or_else(|| "Synchronisation terminée".into());
            self.sync_log.truncate(40);
        }
    }

    fn panel_data(&mut self, ui: &mut egui::Ui) {
        ui.heading("Cartes et données");
        ui.label("Les cartes « intégré » fonctionnent toujours sans connexion. Les autres sont téléchargées une fois, puis utilisables hors ligne.");
        ui.separator();
        let busy = self.sync_rx.is_some();
        ui.label("Fond de carte :");
        let base_now = self.doc.map.base.clone();
        let mut want: Option<&'static str> = None;
        let mut dl: Vec<&'static str> = vec![];
        for d in data::CATALOG {
            let av = data::availability(d);
            if d.kind == DsKind::Base {
                ui.horizontal(|ui| {
                    if ui.add_enabled(av != Availability::Missing, egui::RadioButton::new(base_now == d.id, d.label)).clicked() {
                        want = Some(d.id);
                    }
                    status_and_button(ui, d, av, busy, &mut dl);
                });
            }
        }
        if let Some(id) = want {
            self.doc.map.base = id.into();
        }
        ui.separator();
        ui.label("Couches :");
        for ls in &mut self.doc.map.layers {
            let Some(d) = data::find(&ls.id) else { continue };
            let av = data::availability(d);
            ui.horizontal(|ui| {
                ui.add_enabled(av != Availability::Missing, egui::Checkbox::new(&mut ls.enabled, d.label));
                status_and_button(ui, d, av, busy, &mut dl);
            });
            if ls.enabled {
                ui.horizontal(|ui| {
                    ui.add_space(22.0);
                    ui.color_edit_button_srgba_unmultiplied(&mut ls.stroke);
                    ui.color_edit_button_srgba_unmultiplied(&mut ls.fill);
                    ui.add(egui::Slider::new(&mut ls.width, 0.0..=4.0).text("trait"));
                });
            }
        }
        ui.separator();
        ui.horizontal(|ui| {
            if ui.add_enabled(!busy, egui::Button::new("Synchroniser (mises à jour)")).clicked() {
                dl.extend(Self::cached_ids());
            }
            if busy {
                ui.spinner();
            }
        });
        if ui.checkbox(&mut self.settings.auto_sync, "Vérifier les mises à jour au démarrage (si en ligne)").changed() {
            self.settings.save();
        }
        ui.label(format!("Cache local : {}", data::cache_dir().display()));
        for l in self.sync_log.iter().rev().take(6) {
            ui.small(l);
        }
        dl.dedup();
        if !dl.is_empty() {
            self.start_sync(dl);
        }
    }
}

fn status_and_button(ui: &mut egui::Ui, d: &'static data::Dataset, av: Availability, busy: bool, dl: &mut Vec<&'static str>) {
    match av {
        Availability::Embedded => {
            ui.weak("intégré");
        }
        Availability::Cached => {
            ui.weak("en cache");
        }
        Availability::Missing => {
            let mo = (d.approx_kb as f32 / 1024.0 * 10.0).ceil() / 10.0;
            if ui.add_enabled(!busy, egui::Button::new(format!("Télécharger ({mo} Mo)"))).clicked() {
                dl.push(d.id);
            }
        }
    }
}

/// Icônes d'outils dessinées en vectoriel (aucune dépendance aux glyphes de la police).
fn draw_tool_icon(p: &egui::Painter, r: Rect, tool: Tool, c: Color32) {
    let st = epaint::Stroke::new(1.8, c);
    let add = |s: epaint::Shape| {
        p.add(s);
    };
    let pt = |x: f32, y: f32| Pos2::new(r.left() + x * r.width(), r.top() + y * r.height());
    match tool {
        Tool::Select => add(epaint::Shape::convex_polygon(
            vec![pt(0.1, 0.0), pt(0.1, 0.9), pt(0.35, 0.68), pt(0.55, 1.0), pt(0.7, 0.9), pt(0.5, 0.6), pt(0.85, 0.55)],
            c,
            epaint::Stroke::NONE,
        )),
        Tool::Pan => {
            p.line_segment([pt(0.5, 0.0), pt(0.5, 1.0)], st);
            p.line_segment([pt(0.0, 0.5), pt(1.0, 0.5)], st);
            for (a, b, d) in [
                (pt(0.5, 0.0), Vec2::new(-0.2, 0.2), Vec2::new(0.2, 0.2)),
                (pt(0.5, 1.0), Vec2::new(-0.2, -0.2), Vec2::new(0.2, -0.2)),
                (pt(0.0, 0.5), Vec2::new(0.2, -0.2), Vec2::new(0.2, 0.2)),
                (pt(1.0, 0.5), Vec2::new(-0.2, -0.2), Vec2::new(-0.2, 0.2)),
            ] {
                let s = r.width();
                add(epaint::Shape::convex_polygon(vec![a, a + b * s, a + d * s], c, epaint::Stroke::NONE));
            }
        }
        Tool::Text => {
            p.line_segment([pt(0.05, 0.05), pt(0.95, 0.05)], st);
            p.line_segment([pt(0.5, 0.05), pt(0.5, 0.95)], st);
            p.line_segment([pt(0.3, 0.95), pt(0.7, 0.95)], st);
        }
        Tool::Point => {
            p.circle_filled(r.center(), r.width() * 0.32, c);
        }
        Tool::Circle => {
            p.circle_stroke(r.center(), r.width() * 0.48, st);
        }
        Tool::Line => {
            p.line_segment([pt(0.05, 0.95), pt(0.95, 0.05)], st);
        }
        Tool::Arrow => {
            p.line_segment([pt(0.0, 1.0), pt(0.8, 0.2)], st);
            add(epaint::Shape::convex_polygon(vec![pt(1.0, 0.0), pt(0.62, 0.12), pt(0.88, 0.38)], c, epaint::Stroke::NONE));
        }
        Tool::Polyline => add(epaint::Shape::line(vec![pt(0.0, 0.9), pt(0.3, 0.2), pt(0.6, 0.8), pt(1.0, 0.1)], st)),
        Tool::Rect => {
            p.rect_stroke(r.shrink(1.0), 1.0, st, epaint::StrokeKind::Inside);
        }
        Tool::Polygon => add(epaint::Shape::closed_line(vec![pt(0.5, 0.0), pt(1.0, 0.38), pt(0.82, 1.0), pt(0.18, 1.0), pt(0.0, 0.38)], st)),
        Tool::Capital => {
            let star: Vec<Pos2> = (0..10)
                .map(|i| {
                    let a = -std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::PI / 5.0;
                    let rr = if i % 2 == 0 { 0.5 } else { 0.22 };
                    pt(0.5 + rr * a.cos(), 0.5 + rr * a.sin())
                })
                .collect();
            add(epaint::Shape::closed_line(star, st));
            p.circle_filled(r.center(), r.width() * 0.1, c);
        }
        Tool::Paint => {
            p.line_segment([pt(0.95, 0.05), pt(0.45, 0.55)], epaint::Stroke::new(3.0, c));
            add(epaint::Shape::convex_polygon(vec![pt(0.5, 0.45), pt(0.6, 0.6), pt(0.1, 0.98), pt(0.02, 0.88)], c, epaint::Stroke::NONE));
        }
    };
}

impl EcarteApp {
    /// Outil « Capitale » : clic sur un pays -> point (et nom) à sa capitale, modifiable ensuite.
    fn place_capital(&mut self, view: &View, p: P) {
        let Some((lo, la)) = view.unproject(p) else { return };
        let Some(country) = self.data.base.country_at(lo, la) else {
            self.status = "Cliquez à l'intérieur d'un pays.".into();
            return;
        };
        let (iso, cname) = (country.iso.clone(), country.name_fr.clone());
        let cap = self
            .data
            .capitals
            .iter()
            .filter(|c| c.iso == iso)
            .min_by_key(|c| c.alt as u8)
            .cloned();
        let Some(cap) = cap else {
            self.status = format!("Aucune capitale connue pour {cname}.");
            return;
        };
        let label = if self.cap_with_name { cap.name_fr.clone() } else { String::new() };
        let id = self.doc.add_item(
            "Capitale",
            [255, 255, 255, 255],
            self.cur_stroke,
            1.5,
            Kind::Point { pos: Pos::Geo { lon: cap.lon, lat: cap.lat }, radius: 6.0, shape: PointShape::Circle, label, label_size: 16.0 },
        );
        if let Some(it) = self.doc.item_mut(id) {
            it.name = format!("Capitale : {}", cap.name_fr);
        }
        self.selected = Some(id);
        self.status = format!("{} (capitale de {cname})", cap.name_fr);
    }
}
