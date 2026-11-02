//! Construction de la « liste d'affichage » (primitives en pixels de page).
//! Une seule source de vérité : l'écran (egui), le SVG, le PNG et le JPG en dérivent.
use crate::doc::*;
use crate::geo::*;
use crate::world::{Capital, Data, Poly, Ring};

pub type P = [f32; 2];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stroke {
    pub color: Rgba,
    pub width: f32,
    pub dashed: bool,
}

#[derive(Clone, Debug)]
pub enum Prim {
    /// `rings` : contours (le 1er extérieur, les autres trous, règle pair-impair si rempli).
    Path { rings: Vec<Vec<P>>, closed: bool, fill: Option<Rgba>, stroke: Option<Stroke> },
    Circle { c: P, r: f32, fill: Option<Rgba>, stroke: Option<Stroke> },
    /// `pos` = point d'ancrage ; centré verticalement sur `pos.y`.
    Text { pos: P, text: String, size: f32, color: Rgba, align: HAlign, rot: f32, bold: bool, italic: bool, halo: Option<Rgba> },
    ClipBegin([f32; 4]),
    ClipEnd,
}

// ---------------------------------------------------------------- Vue

pub struct View {
    pub params: Params,
    pub proj: Proj,
    pub scale: f64,
    cx: f64,
    cy: f64,
    pub frame: [f32; 4],
}

impl View {
    pub fn new(doc: &Document) -> View {
        let m = &doc.map;
        let params = Params { proj: m.proj, lon0: m.center_lon, lat0: m.center_lat, std1: m.std1, std2: m.std2 };
        let proj = Proj::new(params);
        let (cx, cy) = proj.forward(m.center_lon, m.center_lat, true).unwrap_or((0.0, 0.0));
        let (ew, eh) = m.proj.fit_extent();
        let fr = doc.frame_rect();
        let base = (fr[2] as f64 / ew).min(fr[3] as f64 / eh) * 0.98;
        View { params, proj, scale: base * m.zoom.max(0.01), cx, cy, frame: fr }
    }

    fn xy_to_page(&self, x: f64, y: f64) -> P {
        let f = self.frame;
        [
            (f[0] as f64 + f[2] as f64 / 2.0 + (x - self.cx) * self.scale) as f32,
            (f[1] as f64 + f[3] as f64 / 2.0 - (y - self.cy) * self.scale) as f32,
        ]
    }

    pub fn project(&self, lon: f64, lat: f64, clamp: bool) -> Option<P> {
        let (x, y) = self.proj.forward(lon, lat, clamp)?;
        let p = self.xy_to_page(x, y);
        if p[0].is_finite() && p[1].is_finite() { Some(p) } else { None }
    }

    pub fn unproject(&self, p: P) -> Option<(f64, f64)> {
        let f = self.frame;
        let x = self.cx + (p[0] as f64 - f[0] as f64 - f[2] as f64 / 2.0) / self.scale;
        let y = self.cy - (p[1] as f64 - f[1] as f64 - f[3] as f64 / 2.0) / self.scale;
        self.proj.inverse(x, y)
    }

    pub fn km_per_px(&self) -> Option<f64> {
        let c = [self.frame[0] + self.frame[2] / 2.0, self.frame[1] + self.frame[3] / 2.0];
        let a = self.unproject(c)?;
        let b = self.unproject([c[0] + 100.0, c[1]])?;
        let d = haversine_km(a, b) / 100.0;
        if d.is_finite() && d > 0.0 { Some(d) } else { None }
    }

    pub fn pos_to_page(&self, pos: &Pos) -> Option<P> {
        match *pos {
            Pos::Page { x, y } => Some([x, y]),
            Pos::Geo { lon, lat } => self.project(lon, lat, false),
        }
    }

    /// Position issue d'un clic : géographique si possible, sinon page.
    pub fn pos_from_page(&self, p: P) -> Pos {
        let f = self.frame;
        let inside = p[0] >= f[0] && p[0] <= f[0] + f[2] && p[1] >= f[1] && p[1] <= f[1] + f[3];
        if inside {
            if let Some((lon, lat)) = self.unproject(p) {
                return Pos::Geo { lon, lat };
            }
        }
        Pos::Page { x: p[0], y: p[1] }
    }
}

// ---------------------------------------------------------------- Utilitaires

fn rect_ring(r: [f32; 4]) -> Vec<P> {
    vec![[r[0], r[1]], [r[0] + r[2], r[1]], [r[0] + r[2], r[1] + r[3]], [r[0], r[1] + r[3]]]
}

fn rect_prim(r: [f32; 4], fill: Option<Rgba>, stroke: Option<Stroke>) -> Prim {
    Prim::Path { rings: vec![rect_ring(r)], closed: true, fill, stroke }
}

fn text(pos: P, t: &str, size: f32, color: Rgba, align: HAlign) -> Prim {
    Prim::Text { pos, text: t.to_string(), size, color, align, rot: 0.0, bold: false, italic: false, halo: None }
}

pub fn text_width(t: &str, size: f32) -> f32 {
    t.chars().count() as f32 * size * 0.56
}

fn decimate(pts: Vec<P>, min_d: f32) -> Vec<P> {
    let mut out: Vec<P> = Vec::with_capacity(pts.len());
    for p in pts {
        match out.last() {
            Some(l) if (l[0] - p[0]).abs() < min_d && (l[1] - p[1]).abs() < min_d => {}
            _ => out.push(p),
        }
    }
    out
}

fn bbox(pts: &[P]) -> [f32; 4] {
    let mut b = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
    for p in pts {
        b[0] = b[0].min(p[0]);
        b[1] = b[1].min(p[1]);
        b[2] = b[2].max(p[0]);
        b[3] = b[3].max(p[1]);
    }
    b
}

fn bbox_hits_rect(b: [f32; 4], r: [f32; 4], pad: f32) -> bool {
    !(b[2] < r[0] - pad || b[0] > r[0] + r[2] + pad || b[3] < r[1] - pad || b[1] > r[1] + r[3] + pad)
}

fn fmt_num(v: f64) -> String {
    if (v - v.round()).abs() < 1e-9 { format!("{}", v.round() as i64) } else { format!("{:.1}", v) }
}

fn fmt_lat(v: f64) -> String {
    if v.abs() < 1e-9 { "0°".into() } else { format!("{}° {}", fmt_num(v.abs()), if v > 0.0 { "N" } else { "S" }) }
}

fn fmt_lon(v: f64) -> String {
    let v = wrap_lon(v);
    if v.abs() < 1e-9 || (v.abs() - 180.0).abs() < 1e-9 {
        format!("{}°", fmt_num(v.abs()))
    } else {
        format!("{}° {}", fmt_num(v.abs()), if v > 0.0 { "E" } else { "O" })
    }
}

fn nice(v: f64) -> f64 {
    let e = 10f64.powf(v.log10().floor());
    let m = v / e;
    let n = if m < 1.5 { 1.0 } else if m < 3.5 { 2.0 } else if m < 7.5 { 5.0 } else { 10.0 };
    n * e
}

// ---------------------------------------------------------------- Anneaux géographiques

fn unwrap_ring(ring: &Ring, lon0: f64) -> Vec<(f64, f64)> {
    let mut out = Vec::with_capacity(ring.len());
    let mut shift = 0.0;
    let mut prev: Option<f64> = None;
    for p in ring {
        let raw = p[0] as f64;
        let lon = match prev {
            None => {
                let base = lon0 + wrap_lon(raw - lon0);
                shift = base - raw;
                base
            }
            Some(pl) => {
                let mut l = raw + shift;
                while l - pl > 180.0 {
                    l -= 360.0;
                    shift -= 360.0;
                }
                while l - pl < -180.0 {
                    l += 360.0;
                    shift += 360.0;
                }
                l
            }
        };
        prev = Some(lon);
        out.push((lon, p[1] as f64));
    }
    out
}

fn clip_side(inp: &[(f64, f64)], edge: f64, keep_ge: bool) -> Vec<(f64, f64)> {
    let inside = |p: &(f64, f64)| if keep_ge { p.0 >= edge } else { p.0 <= edge };
    let n = inp.len();
    let mut out = Vec::with_capacity(n + 4);
    for i in 0..n {
        let a = inp[i];
        let b = inp[(i + 1) % n];
        let (ia, ib) = (inside(&a), inside(&b));
        if ia {
            out.push(a);
        }
        if ia != ib {
            let t = (edge - a.0) / (b.0 - a.0);
            out.push((edge, a.1 + t * (b.1 - a.1)));
        }
    }
    out
}

fn clip_band(r: &[(f64, f64)], lo: f64, hi: f64) -> Vec<(f64, f64)> {
    let a = clip_side(r, lo, true);
    if a.is_empty() { a } else { clip_side(&a, hi, false) }
}

fn project_ring(view: &View, r: &[(f64, f64)]) -> Vec<P> {
    let pts: Vec<P> = r.iter().filter_map(|&(lo, la)| view.project(lo, la, true)).collect();
    decimate(pts, 0.35)
}

fn poly_prims(view: &View, poly: &Poly, fill: Option<Rgba>, stroke: Option<Stroke>, out: &mut Vec<Prim>) {
    if poly.rings.is_empty() {
        return;
    }
    let banded = view.params.proj.is_banded();
    let lon0 = view.params.lon0;
    let unwrapped: Vec<Vec<(f64, f64)>> = poly.rings.iter().map(|r| unwrap_ring(r, lon0)).collect();
    let shifts: &[f64] = if banded { &[-360.0, 0.0, 360.0] } else { &[0.0] };
    for &s in shifts {
        let mut rings: Vec<Vec<P>> = Vec::new();
        for (i, r) in unwrapped.iter().enumerate() {
            let shifted: Vec<(f64, f64)> = r.iter().map(|p| (p.0 + s, p.1)).collect();
            let ring = if banded {
                let (lo, hi) = (lon0 - 180.0, lon0 + 180.0);
                let mn = shifted.iter().fold(f64::MAX, |a, p| a.min(p.0));
                let mx = shifted.iter().fold(f64::MIN, |a, p| a.max(p.0));
                if mx < lo || mn > hi { Vec::new() } else { clip_band(&shifted, lo, hi) }
            } else {
                shifted
            };
            let pts = if ring.len() >= 3 { project_ring(view, &ring) } else { Vec::new() };
            if pts.len() < 3 {
                if i == 0 {
                    break;
                }
                continue;
            }
            if i == 0 && !bbox_hits_rect(bbox(&pts), view.frame, 40.0) {
                break;
            }
            rings.push(pts);
        }
        if !rings.is_empty() {
            out.push(Prim::Path { rings, closed: true, fill, stroke });
        }
    }
}

/// Ligne ouverte (fleuve…) : copies ±360° pour les projections bornées, coupée hors de la bande.
fn line_prims(view: &View, line: &Ring, stroke: Stroke, out: &mut Vec<Prim>) {
    if line.len() < 2 {
        return;
    }
    let banded = view.params.proj.is_banded();
    let lon0 = view.params.lon0;
    let un = unwrap_ring(line, lon0);
    let shifts: &[f64] = if banded { &[-360.0, 0.0, 360.0] } else { &[0.0] };
    for &s in shifts {
        let mut runs: Vec<Vec<P>> = Vec::new();
        let mut cur: Vec<P> = Vec::new();
        for &(lo, la) in &un {
            let lo = lo + s;
            let in_band = !banded || (lo >= lon0 - 180.0 && lo <= lon0 + 180.0);
            match if in_band { view.project(lo, la, false) } else { None } {
                Some(p) => cur.push(p),
                None => {
                    if cur.len() >= 2 {
                        runs.push(std::mem::take(&mut cur));
                    }
                    cur.clear();
                }
            }
        }
        if cur.len() >= 2 {
            runs.push(cur);
        }
        for r in runs {
            let r = decimate(r, 0.35);
            if r.len() >= 2 && bbox_hits_rect(bbox(&r), view.frame, 40.0) {
                out.push(Prim::Path { rings: vec![r], closed: false, fill: None, stroke: Some(stroke) });
            }
        }
    }
}

// ---------------------------------------------------------------- Sphère & graticule

fn sphere_outline(view: &View) -> Option<Vec<P>> {
    let proj = view.params.proj;
    if proj.is_banded() {
        let (lo, hi) = (view.params.lon0 - 180.0, view.params.lon0 + 180.0);
        let ml = proj.max_lat();
        let mut pts = Vec::new();
        let mut lat = -ml;
        while lat < ml {
            pts.extend(view.project(lo, lat, true));
            lat += 2.0;
        }
        pts.extend(view.project(lo, ml, true));
        let mut lat = ml;
        while lat > -ml {
            pts.extend(view.project(hi, lat, true));
            lat -= 2.0;
        }
        pts.extend(view.project(hi, -ml, true));
        Some(pts)
    } else if proj == Projection::Orthographic || proj == Projection::AzimuthalEqualArea {
        let r = if proj == Projection::Orthographic { 1.0 } else { 2.0 };
        Some(
            (0..180)
                .map(|i| {
                    let a = i as f64 * std::f64::consts::PI / 90.0;
                    view.xy_to_page(r * a.cos(), r * a.sin())
                })
                .collect(),
        )
    } else {
        None
    }
}

struct GRun {
    pts: Vec<P>,
    meridian: bool,
    value: f64,
}

fn flush(runs: &mut Vec<GRun>, cur: &mut Vec<P>, meridian: bool, value: f64, frame: [f32; 4]) {
    let pts = std::mem::take(cur);
    if pts.len() >= 2 && bbox_hits_rect(bbox(&pts), frame, 30.0) {
        runs.push(GRun { pts, meridian, value });
    }
}

fn graticule_runs(view: &View, step: f64) -> Vec<GRun> {
    let proj = view.params.proj;
    let ml = proj.max_lat();
    let lon0 = view.params.lon0;
    let step = step.clamp(1.0, 90.0);
    let (lo, hi) = (lon0 - 180.0, lon0 + 180.0);
    let mut runs = Vec::new();
    let mut cur: Vec<P> = Vec::new();

    let mut m = (lo / step).ceil() * step;
    while m <= hi + 1e-9 {
        let mut lat = -ml;
        loop {
            match view.project(m, lat, false) {
                Some(p) => cur.push(p),
                None => flush(&mut runs, &mut cur, true, m, view.frame),
            }
            if lat >= ml {
                break;
            }
            lat = (lat + 1.5).min(ml);
        }
        flush(&mut runs, &mut cur, true, m, view.frame);
        m += step;
    }

    let mut l = (-ml / step).ceil() * step;
    while l <= ml + 1e-9 {
        if l.abs() < 89.99 {
            let mut lon = lo;
            loop {
                match view.project(lon, l, false) {
                    Some(p) => cur.push(p),
                    None => flush(&mut runs, &mut cur, false, l, view.frame),
                }
                if lon >= hi {
                    break;
                }
                lon = (lon + 2.0).min(hi);
            }
            flush(&mut runs, &mut cur, false, l, view.frame);
        }
        l += step;
    }
    runs
}

fn cross_at_x(run: &[P], x: f32) -> Vec<f32> {
    run.windows(2)
        .filter(|w| (w[0][0] - x) * (w[1][0] - x) < 0.0)
        .map(|w| w[0][1] + (x - w[0][0]) / (w[1][0] - w[0][0]) * (w[1][1] - w[0][1]))
        .collect()
}

fn cross_at_y(run: &[P], y: f32) -> Vec<f32> {
    run.windows(2)
        .filter(|w| (w[0][1] - y) * (w[1][1] - y) < 0.0)
        .map(|w| w[0][0] + (y - w[0][1]) / (w[1][1] - w[0][1]) * (w[1][0] - w[0][0]))
        .collect()
}

// ---------------------------------------------------------------- Éléments

pub enum Shape {
    Pt(P, f32),
    Poly(Vec<P>, bool),
    TextBox([f32; 4]),
}

pub fn positions(kind: &Kind) -> Vec<&Pos> {
    match kind {
        Kind::Text { pos, .. } | Kind::Point { pos, .. } => vec![pos],
        Kind::Circle { center, .. } => vec![center],
        Kind::Line { pts, .. } | Kind::Polygon { pts } => pts.iter().collect(),
        Kind::Rect { a, b } => vec![a, b],
    }
}

pub fn positions_mut(kind: &mut Kind) -> Vec<&mut Pos> {
    match kind {
        Kind::Text { pos, .. } | Kind::Point { pos, .. } => vec![pos],
        Kind::Circle { center, .. } => vec![center],
        Kind::Line { pts, .. } | Kind::Polygon { pts } => pts.iter_mut().collect(),
        Kind::Rect { a, b } => vec![a, b],
    }
}

fn has_geo(kind: &Kind) -> bool {
    positions(kind).iter().any(|p| matches!(p, Pos::Geo { .. }))
}

/// Déplace une copie de l'élément de (dx,dy) pixels de page.
pub fn moved(view: &View, item: &Item, dx: f32, dy: f32) -> Item {
    let mut it = item.clone();
    for pos in positions_mut(&mut it.kind) {
        match *pos {
            Pos::Page { x, y } => *pos = Pos::Page { x: x + dx, y: y + dy },
            Pos::Geo { lon, lat } => {
                if let Some(p) = view.project(lon, lat, false) {
                    if let Some((lo, la)) = view.unproject([p[0] + dx, p[1] + dy]) {
                        *pos = Pos::Geo { lon: lo, lat: la };
                    }
                }
            }
        }
    }
    it
}

fn route(view: &View, pts: &[Pos], geodesic: bool) -> Vec<P> {
    let mut out = Vec::new();
    for (i, p) in pts.iter().enumerate() {
        if geodesic && i > 0 {
            if let (Pos::Geo { lon: a, lat: b }, Pos::Geo { lon: c, lat: d }) = (&pts[i - 1], p) {
                let gc = great_circle((*a, *b), (*c, *d), 2.0);
                out.extend(gc.iter().skip(1).filter_map(|&(lo, la)| view.project(lo, la, false)));
                continue;
            }
        }
        out.extend(view.pos_to_page(p));
    }
    out
}

fn km_circle(view: &View, lon: f64, lat: f64, km: f64) -> Vec<P> {
    let mut pts = Vec::new();
    let mut prev = lon;
    for i in 0..72 {
        let (mut lo, la) = destination(lon, lat, i as f64 * 5.0, km);
        while lo - prev > 180.0 {
            lo -= 360.0;
        }
        while lo - prev < -180.0 {
            lo += 360.0;
        }
        prev = lo;
        pts.extend(view.project(lo, la, true));
    }
    pts
}

pub fn item_shape(view: &View, item: &Item) -> Option<Shape> {
    Some(match &item.kind {
        Kind::Text { pos, text, size, align, .. } => {
            let p = view.pos_to_page(pos)?;
            let w = text.lines().map(|l| text_width(l, *size)).fold(0.0, f32::max);
            let h = *size * text.lines().count().max(1) as f32;
            let x = match align {
                HAlign::Left => p[0],
                HAlign::Center => p[0] - w / 2.0,
                HAlign::Right => p[0] - w,
            };
            Shape::TextBox([x, p[1] - h / 2.0, w, h])
        }
        Kind::Point { pos, radius, .. } => Shape::Pt(view.pos_to_page(pos)?, *radius),
        Kind::Circle { center, radius, unit } => match (unit, center) {
            (RadiusUnit::Km, Pos::Geo { lon, lat }) => Shape::Poly(km_circle(view, *lon, *lat, *radius as f64), true),
            _ => Shape::Pt(view.pos_to_page(center)?, *radius),
        },
        Kind::Line { pts, geodesic, .. } => Shape::Poly(route(view, pts, *geodesic), false),
        Kind::Rect { a, b } => {
            let (a, b) = (view.pos_to_page(a)?, view.pos_to_page(b)?);
            Shape::Poly(vec![a, [b[0], a[1]], b, [a[0], b[1]]], true)
        }
        Kind::Polygon { pts } => Shape::Poly(route(view, pts, false), true),
    })
}

fn dist_seg(p: P, a: P, b: P) -> f32 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let l2 = dx * dx + dy * dy;
    let t = if l2 < 1e-9 { 0.0 } else { (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / l2).clamp(0.0, 1.0) };
    ((p[0] - a[0] - t * dx).powi(2) + (p[1] - a[1] - t * dy).powi(2)).sqrt()
}

fn point_in_poly(p: P, pts: &[P]) -> bool {
    let n = pts.len();
    let mut inside = false;
    if n < 3 {
        return false;
    }
    let mut j = n - 1;
    for i in 0..n {
        if (pts[i][1] > p[1]) != (pts[j][1] > p[1])
            && p[0] < (pts[j][0] - pts[i][0]) * (p[1] - pts[i][1]) / (pts[j][1] - pts[i][1]) + pts[i][0]
        {
            inside = !inside;
        }
        j = i;
    }
    inside
}

pub fn hit(shape: &Shape, p: P, tol: f32, filled: bool) -> bool {
    match shape {
        Shape::Pt(c, r) => ((c[0] - p[0]).powi(2) + (c[1] - p[1]).powi(2)).sqrt() <= r + tol,
        Shape::TextBox(b) => p[0] >= b[0] - tol && p[0] <= b[0] + b[2] + tol && p[1] >= b[1] - tol && p[1] <= b[1] + b[3] + tol,
        Shape::Poly(pts, closed) => {
            if *closed && filled && point_in_poly(p, pts) {
                return true;
            }
            let n = pts.len();
            let segs = if *closed { n } else { n.saturating_sub(1) };
            (0..segs).any(|i| dist_seg(p, pts[i], pts[(i + 1) % n]) <= tol)
        }
    }
}

pub fn shape_bounds(s: &Shape) -> [f32; 4] {
    match s {
        Shape::Pt(c, r) => [c[0] - r, c[1] - r, c[0] + r, c[1] + r],
        Shape::TextBox(b) => [b[0], b[1], b[0] + b[2], b[1] + b[3]],
        Shape::Poly(p, _) => bbox(p),
    }
}

fn arrow_head(tip: P, from: P, len: f32) -> Option<(Vec<P>, P)> {
    let (dx, dy) = (tip[0] - from[0], tip[1] - from[1]);
    let l = (dx * dx + dy * dy).sqrt();
    if l < 1e-3 {
        return None;
    }
    let (ux, uy) = (dx / l, dy / l);
    let base = [tip[0] - ux * len, tip[1] - uy * len];
    let hw = len * 0.38;
    let head = vec![tip, [base[0] - uy * hw, base[1] + ux * hw], [base[0] + uy * hw, base[1] - ux * hw]];
    let shorten = (len * 0.7).min(l * 0.9);
    Some((head, [tip[0] - ux * shorten, tip[1] - uy * shorten]))
}

fn marker(shape: PointShape, c: P, r: f32) -> Vec<P> {
    match shape {
        PointShape::Square => vec![[c[0] - r, c[1] - r], [c[0] + r, c[1] - r], [c[0] + r, c[1] + r], [c[0] - r, c[1] + r]],
        PointShape::Triangle => vec![[c[0], c[1] - r * 1.15], [c[0] + r, c[1] + r * 0.8], [c[0] - r, c[1] + r * 0.8]],
        PointShape::Diamond => vec![[c[0], c[1] - r * 1.3], [c[0] + r, c[1]], [c[0], c[1] + r * 1.3], [c[0] - r, c[1]]],
        PointShape::Star => (0..10)
            .map(|i| {
                let a = -std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::PI / 5.0;
                let rr = if i % 2 == 0 { r * 1.3 } else { r * 0.55 };
                [c[0] + rr * a.cos(), c[1] + rr * a.sin()]
            })
            .collect(),
        _ => vec![],
    }
}

/// Marqueur ponctuel (disque, carré, triangle, losange, croix, étoile).
fn marker_prims(shape: PointShape, c: P, r: f32, fill: Option<Rgba>, stroke: Option<Stroke>, out: &mut Vec<Prim>) {
    match shape {
        PointShape::Circle => out.push(Prim::Circle { c, r, fill, stroke }),
        PointShape::Cross => {
            let s = stroke.map(|s| Stroke { width: s.width.max(1.5), ..s });
            out.push(Prim::Path { rings: vec![vec![[c[0] - r, c[1] - r], [c[0] + r, c[1] + r]]], closed: false, fill: None, stroke: s });
            out.push(Prim::Path { rings: vec![vec![[c[0] - r, c[1] + r], [c[0] + r, c[1] - r]]], closed: false, fill: None, stroke: s });
        }
        s => out.push(Prim::Path { rings: vec![marker(s, c, r)], closed: true, fill, stroke }),
    }
}

fn item_prims(view: &View, it: &Item, out: &mut Vec<Prim>) {
    let stroke = Some(Stroke { color: it.stroke, width: it.width, dashed: it.dashed });
    let fill = if it.fill[3] > 0 { Some(it.fill) } else { None };
    let halo = Some([255, 255, 255, 210]);
    match &it.kind {
        Kind::Text { pos, text, size, bold, italic, rot, halo: h, align } => {
            if let Some(p) = view.pos_to_page(pos) {
                out.push(Prim::Text {
                    pos: p,
                    text: text.clone(),
                    size: *size,
                    color: it.fill,
                    align: *align,
                    rot: *rot,
                    bold: *bold,
                    italic: *italic,
                    halo: if *h { halo } else { None },
                });
            }
        }
        Kind::Point { pos, radius, shape, label, label_size } => {
            let Some(c) = view.pos_to_page(pos) else { return };
            match shape {
                PointShape::Circle => out.push(Prim::Circle { c, r: *radius, fill, stroke }),
                PointShape::Cross => {
                    let r = *radius;
                    let s = Some(Stroke { color: it.stroke, width: it.width.max(1.5), dashed: false });
                    out.push(Prim::Path { rings: vec![vec![[c[0] - r, c[1] - r], [c[0] + r, c[1] + r]]], closed: false, fill: None, stroke: s });
                    out.push(Prim::Path { rings: vec![vec![[c[0] - r, c[1] + r], [c[0] + r, c[1] - r]]], closed: false, fill: None, stroke: s });
                }
                s => out.push(Prim::Path { rings: vec![marker(*s, c, *radius)], closed: true, fill, stroke }),
            }
            if !label.is_empty() {
                out.push(Prim::Text {
                    pos: [c[0] + radius + 5.0, c[1]],
                    text: label.clone(),
                    size: *label_size,
                    color: [20, 20, 20, 255],
                    align: HAlign::Left,
                    rot: 0.0,
                    bold: false,
                    italic: false,
                    halo,
                });
            }
        }
        Kind::Circle { center, radius, unit } => match (unit, center) {
            (RadiusUnit::Km, Pos::Geo { lon, lat }) => {
                let ring = km_circle(view, *lon, *lat, *radius as f64);
                if ring.len() >= 3 {
                    out.push(Prim::Path { rings: vec![ring], closed: true, fill, stroke });
                }
            }
            _ => {
                if let Some(c) = view.pos_to_page(center) {
                    out.push(Prim::Circle { c, r: *radius, fill, stroke });
                }
            }
        },
        Kind::Line { pts, arrow_start, arrow_end, geodesic } => {
            let mut line = route(view, pts, *geodesic);
            if line.len() < 2 {
                return;
            }
            let hl = (it.width * 4.5).max(12.0);
            let mut heads = Vec::new();
            if *arrow_end {
                let n = line.len();
                if let Some((h, short)) = arrow_head(line[n - 1], line[n - 2], hl) {
                    heads.push(h);
                    line[n - 1] = short;
                }
            }
            if *arrow_start {
                let orig0 = line[0];
                if let Some((h, short)) = arrow_head(orig0, line[1], hl) {
                    heads.push(h);
                    line[0] = short;
                }
            }
            out.push(Prim::Path { rings: vec![line], closed: false, fill: None, stroke });
            for h in heads {
                out.push(Prim::Path {
                    rings: vec![h],
                    closed: true,
                    fill: Some(it.stroke),
                    stroke: Some(Stroke { color: it.stroke, width: 1.0, dashed: false }),
                });
            }
        }
        Kind::Rect { .. } | Kind::Polygon { .. } => {
            if let Some(Shape::Poly(pts, _)) = item_shape(view, it) {
                if pts.len() >= 3 {
                    out.push(Prim::Path { rings: vec![pts], closed: true, fill, stroke });
                }
            }
        }
    }
}

// ---------------------------------------------------------------- Habillage

fn corner_origin(c: Corner, f: [f32; 4], w: f32, h: f32, pad: f32) -> P {
    let x = match c {
        Corner::TopLeft | Corner::BottomLeft => f[0] + pad,
        _ => f[0] + f[2] - pad - w,
    };
    let y = match c {
        Corner::TopLeft | Corner::TopRight => f[1] + pad,
        _ => f[1] + f[3] - pad - h,
    };
    [x, y]
}

fn scale_bar(view: &View, doc: &Document, out: &mut Vec<Prim>) {
    let Some(kmpp) = view.km_per_px() else { return };
    let km = nice(140.0 * kmpp);
    let len = (km / kmpp) as f32;
    if !len.is_finite() || len < 10.0 {
        return;
    }
    let (w, h) = (len + 40.0, 40.0);
    let o = corner_origin(doc.frame.scale_corner, view.frame, w, h, 14.0);
    out.push(rect_prim([o[0], o[1], w, h], Some([255, 255, 255, 190]), None));
    let seg = len / 4.0;
    let by = o[1] + 22.0;
    let black = Some(Stroke { color: [20, 20, 20, 255], width: 1.0, dashed: false });
    for i in 0..4 {
        let fill = if i % 2 == 0 { [20, 20, 20, 255] } else { [255, 255, 255, 255] };
        out.push(rect_prim([o[0] + 20.0 + i as f32 * seg, by, seg, 8.0], Some(fill), black));
    }
    out.push(text([o[0] + 20.0, o[1] + 11.0], "0", 12.0, [20, 20, 20, 255], HAlign::Center));
    out.push(text([o[0] + 20.0 + len, o[1] + 11.0], &format!("{} km", fmt_num(km)), 12.0, [20, 20, 20, 255], HAlign::Center));
}

fn north_arrow(view: &View, doc: &Document, out: &mut Vec<Prim>) {
    let (w, h) = (44.0, 72.0);
    let o = corner_origin(doc.frame.north_corner, view.frame, w, h, 14.0);
    let cx = o[0] + w / 2.0;
    let st = Some(Stroke { color: [20, 20, 20, 255], width: 1.2, dashed: false });
    out.push(text([cx, o[1] + 9.0], "N", 18.0, [20, 20, 20, 255], HAlign::Center));
    let (tip, base_y, notch_y) = (o[1] + 22.0, o[1] + h, o[1] + h - 14.0);
    out.push(Prim::Path { rings: vec![vec![[cx, tip], [cx - 14.0, base_y], [cx, notch_y]]], closed: true, fill: Some([20, 20, 20, 255]), stroke: st });
    out.push(Prim::Path { rings: vec![vec![[cx, tip], [cx + 14.0, base_y], [cx, notch_y]]], closed: true, fill: Some([255, 255, 255, 255]), stroke: st });
}

fn legend(view: &View, doc: &Document, out: &mut Vec<Prim>) {
    let lg = &doc.legend;
    if !lg.visible || (lg.entries.is_empty() && lg.title.is_empty()) {
        return;
    }
    let s = lg.size;
    let row = s + 8.0;
    let mut tw = text_width(&lg.title, s * 1.1);
    for (_, t) in &lg.entries {
        tw = tw.max(text_width(t, s) + s * 2.0);
    }
    let rows = lg.entries.len() as f32 + if lg.title.is_empty() { 0.0 } else { 1.0 };
    let (w, h) = (tw + 24.0, rows * row + 14.0);
    let o = corner_origin(lg.corner, view.frame, w, h, 14.0);
    out.push(rect_prim(
        [o[0], o[1], w, h],
        Some([255, 255, 255, 225]),
        Some(Stroke { color: [60, 60, 60, 255], width: 1.0, dashed: false }),
    ));
    let mut y = o[1] + 7.0 + row / 2.0;
    if !lg.title.is_empty() {
        out.push(Prim::Text { pos: [o[0] + 12.0, y], text: lg.title.clone(), size: s * 1.1, color: [20, 20, 20, 255], align: HAlign::Left, rot: 0.0, bold: true, italic: false, halo: None });
        y += row;
    }
    for (c, t) in &lg.entries {
        out.push(rect_prim([o[0] + 12.0, y - s / 2.0, s * 1.4, s], Some(*c), Some(Stroke { color: [60, 60, 60, 255], width: 0.8, dashed: false })));
        out.push(text([o[0] + 12.0 + s * 1.4 + 8.0, y], t, s, [20, 20, 20, 255], HAlign::Left));
        y += row;
    }
}

fn frame_prims(view: &View, doc: &Document, runs: &[GRun], out: &mut Vec<Prim>) {
    let f = view.frame;
    let fs = &doc.frame;
    let st = Some(Stroke { color: fs.color, width: fs.width, dashed: false });
    match fs.style {
        FrameStyle::None => {}
        FrameStyle::Simple => out.push(rect_prim(f, None, st)),
        FrameStyle::Double => {
            out.push(rect_prim(f, None, st));
            let g = 6.0;
            out.push(rect_prim([f[0] - g, f[1] - g, f[2] + 2.0 * g, f[3] + 2.0 * g], None, Some(Stroke { color: fs.color, width: (fs.width * 0.5).max(0.8), dashed: false })));
        }
        FrameStyle::Ticks => {
            let t = 8.0;
            let thin = Some(Stroke { color: fs.color, width: 0.8, dashed: false });
            // bornes sur chaque côté : croisements du graticule, sinon pas régulier
            let mut xs_top = vec![f[0], f[0] + f[2]];
            let mut xs_bot = xs_top.clone();
            let mut ys_l = vec![f[1], f[1] + f[3]];
            let mut ys_r = ys_l.clone();
            for r in runs {
                let (a, b, c, d) = (
                    cross_at_y(&r.pts, f[1]),
                    cross_at_y(&r.pts, f[1] + f[3]),
                    cross_at_x(&r.pts, f[0]),
                    cross_at_x(&r.pts, f[0] + f[2]),
                );
                if r.meridian {
                    xs_top.extend(a.into_iter().filter(|x| *x > f[0] && *x < f[0] + f[2]));
                    xs_bot.extend(b.into_iter().filter(|x| *x > f[0] && *x < f[0] + f[2]));
                } else {
                    ys_l.extend(c.into_iter().filter(|y| *y > f[1] && *y < f[1] + f[3]));
                    ys_r.extend(d.into_iter().filter(|y| *y > f[1] && *y < f[1] + f[3]));
                }
            }
            let mut band = |mut v: Vec<f32>, mk: &dyn Fn(f32, f32) -> [f32; 4]| {
                v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                v.dedup_by(|a, b| (*a - *b).abs() < 1.0);
                for (i, w) in v.windows(2).enumerate() {
                    let fill = if i % 2 == 0 { fs.color } else { [255, 255, 255, 255] };
                    out.push(rect_prim(mk(w[0], w[1]), Some(fill), thin));
                }
            };
            band(xs_top, &|a, b| [a, f[1] - t, b - a, t]);
            band(xs_bot, &|a, b| [a, f[1] + f[3], b - a, t]);
            band(ys_l, &|a, b| [f[0] - t, a, t, b - a]);
            band(ys_r, &|a, b| [f[0] + f[2], a, t, b - a]);
            out.push(rect_prim(f, None, st));
        }
    }
}

// ---------------------------------------------------------------- Assemblage

pub fn build(doc: &Document, data: &Data) -> Vec<Prim> {
    let world = &*data.base;
    let view = View::new(doc);
    let m = &doc.map;
    let pg = &doc.page;
    let f = view.frame;
    let mut out: Vec<Prim> = Vec::new();

    out.push(rect_prim([0.0, 0.0, pg.w, pg.h], Some(pg.bg), None));

    // titres
    let mt = pg.margins[1];
    let (ty, sy) = if doc.subtitle.visible && !doc.subtitle.text.is_empty() { (mt * 0.38, mt * 0.74) } else { (mt * 0.5, 0.0) };
    if doc.title.visible && !doc.title.text.is_empty() {
        out.push(Prim::Text { pos: [pg.w / 2.0, ty], text: doc.title.text.clone(), size: doc.title.size, color: doc.title.color, align: HAlign::Center, rot: 0.0, bold: doc.title.bold, italic: false, halo: None });
    }
    if doc.subtitle.visible && !doc.subtitle.text.is_empty() {
        out.push(Prim::Text { pos: [pg.w / 2.0, sy], text: doc.subtitle.text.clone(), size: doc.subtitle.size, color: doc.subtitle.color, align: HAlign::Center, rot: 0.0, bold: doc.subtitle.bold, italic: false, halo: None });
    }

    let g = &m.graticule;
    let need_runs = g.show || g.labels || doc.frame.style == FrameStyle::Ticks;
    let runs = if need_runs { graticule_runs(&view, g.step) } else { Vec::new() };

    out.push(Prim::ClipBegin(f));
    match sphere_outline(&view) {
        Some(ring) if ring.len() >= 3 => out.push(Prim::Path { rings: vec![ring], closed: true, fill: Some(m.ocean), stroke: None }),
        _ => out.push(rect_prim(f, Some(m.ocean), None)),
    }
    let border = if m.border_width > 0.0 { Some(Stroke { color: m.border, width: m.border_width, dashed: false }) } else { None };
    for c in &world.countries {
        let fill = m.country_colors.get(&c.iso).copied().or(if m.show_land { Some(m.land) } else { None });
        for p in &c.polys {
            poly_prims(&view, p, fill, border, &mut out);
        }
    }
    for ls in m.layers.iter().filter(|l| l.enabled) {
        let Some(w) = data.layers.get(&ls.id) else { continue };
        let fill = if ls.fill[3] > 0 { Some(ls.fill) } else { None };
        let stroke = if ls.width > 0.0 { Some(Stroke { color: ls.stroke, width: ls.width, dashed: false }) } else { None };
        for c in &w.countries {
            for p in &c.polys {
                poly_prims(&view, p, fill, stroke, &mut out);
            }
            if let Some(st) = stroke {
                for l in &c.lines {
                    line_prims(&view, l, st, &mut out);
                }
            }
        }
    }
    if g.show {
        let st = Some(Stroke { color: g.color, width: g.width, dashed: g.dashed });
        for r in &runs {
            out.push(Prim::Path { rings: vec![r.pts.clone()], closed: false, fill: None, stroke: st });
        }
    }
    if m.capitals.mode != CapMode::Off && !data.capitals.is_empty() {
        let cs = &m.capitals;
        let banded = view.params.proj.is_banded();
        let lon0 = view.params.lon0;
        let mut caps: Vec<&Capital> = data.capitals.iter().filter(|c| !cs.only_colored || m.country_colors.contains_key(&c.iso)).collect();
        caps.sort_by(|a, b| b.pop.partial_cmp(&a.pop).unwrap_or(std::cmp::Ordering::Equal));
        let mut pts: Vec<(P, &Capital)> = Vec::new();
        for c in caps {
            let lon = if banded { lon0 + wrap_lon(c.lon - lon0) } else { c.lon };
            if let Some(p) = view.project(lon, c.lat, false) {
                if p[0] > f[0] && p[0] < f[0] + f[2] && p[1] > f[1] && p[1] < f[1] + f[3] {
                    pts.push((p, c));
                }
            }
        }
        let r = cs.size;
        let st = Some(Stroke { color: cs.outline, width: 1.2, dashed: false });
        let mut placed: Vec<[f32; 4]> = pts.iter().map(|(p, _)| [p[0] - r - 1.0, p[1] - r - 1.0, 2.0 * r + 2.0, 2.0 * r + 2.0]).collect();
        for (p, _) in &pts {
            marker_prims(cs.shape, *p, r, Some(cs.fill), st, &mut out);
        }
        if cs.mode == CapMode::PointsAndNames {
            let ls = cs.label_size;
            for (i, (p, c)) in pts.iter().enumerate() {
                let b = [p[0] + r + 3.0, p[1] - ls * 0.6, text_width(&c.name_fr, ls), ls * 1.2];
                let hit = placed.iter().enumerate().any(|(j, o)| j != i && !(b[0] + b[2] < o[0] || o[0] + o[2] < b[0] || b[1] + b[3] < o[1] || o[1] + o[3] < b[1]));
                if hit {
                    continue;
                }
                placed.push(b);
                out.push(Prim::Text { pos: [p[0] + r + 3.0, p[1]], text: c.name_fr.clone(), size: ls, color: cs.label_color, align: HAlign::Left, rot: 0.0, bold: false, italic: false, halo: Some([255, 255, 255, 210]) });
            }
        }
    }
    if m.country_labels {
        for c in &world.countries {
            if c.area < m.country_label_min_area {
                continue;
            }
            let lon = view.params.lon0 + wrap_lon(c.label.0 - view.params.lon0);
            if let Some(p) = view.project(lon, c.label.1, false) {
                if p[0] > f[0] && p[0] < f[0] + f[2] && p[1] > f[1] && p[1] < f[1] + f[3] {
                    out.push(Prim::Text { pos: p, text: c.name_fr.clone(), size: m.country_label_size, color: [50, 45, 40, 255], align: HAlign::Center, rot: 0.0, bold: false, italic: false, halo: Some([255, 255, 255, 190]) });
                }
            }
        }
    }
    out.push(Prim::ClipEnd);

    // éléments
    for it in doc.items.iter().filter(|i| i.visible) {
        let clip = has_geo(&it.kind);
        if clip {
            out.push(Prim::ClipBegin(f));
        }
        item_prims(&view, it, &mut out);
        if clip {
            out.push(Prim::ClipEnd);
        }
    }

    frame_prims(&view, doc, &runs, &mut out);

    // étiquettes du graticule
    if g.labels {
        let off = match doc.frame.style {
            FrameStyle::Double | FrameStyle::Ticks => 14.0,
            _ => 6.0,
        };
        let s = g.label_size;
        let [l, t, r, b] = g.sides;
        for run in &runs {
            let label = if run.meridian { fmt_lon(run.value) } else { fmt_lat(run.value) };
            let mut put = |pos: P, al: HAlign| out.push(text(pos, &label, s, g.label_color, al));
            if run.meridian {
                if t {
                    for x in cross_at_y(&run.pts, f[1]).into_iter().filter(|x| *x >= f[0] && *x <= f[0] + f[2]) {
                        put([x, f[1] - off - s * 0.5], HAlign::Center);
                    }
                }
                if b {
                    for x in cross_at_y(&run.pts, f[1] + f[3]).into_iter().filter(|x| *x >= f[0] && *x <= f[0] + f[2]) {
                        put([x, f[1] + f[3] + off + s * 0.5], HAlign::Center);
                    }
                }
            } else {
                if l {
                    for y in cross_at_x(&run.pts, f[0]).into_iter().filter(|y| *y >= f[1] && *y <= f[1] + f[3]) {
                        put([f[0] - off, y], HAlign::Right);
                    }
                }
                if r {
                    for y in cross_at_x(&run.pts, f[0] + f[2]).into_iter().filter(|y| *y >= f[1] && *y <= f[1] + f[3]) {
                        put([f[0] + f[2] + off, y], HAlign::Left);
                    }
                }
            }
        }
    }

    if doc.frame.scale_bar {
        scale_bar(&view, doc, &mut out);
    }
    if doc.frame.north_arrow {
        north_arrow(&view, doc, &mut out);
    }
    legend(&view, doc, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_for_every_projection() {
        let world = Data::embedded_only();
        for proj in Projection::ALL {
            let mut d = Document::default();
            d.map.proj = proj;
            d.map.center_lon = 170.0; // force le passage de l'antiméridien
            let prims = build(&d, &world);
            assert!(prims.len() > 100, "{proj:?}: {}", prims.len());
            for p in &prims {
                if let Prim::Path { rings, .. } = p {
                    assert!(rings.iter().flatten().all(|q| q[0].is_finite() && q[1].is_finite()));
                }
            }
        }
    }

    #[test]
    fn view_roundtrip() {
        let d = Document::default();
        let v = View::new(&d);
        let p = v.project(5.0, 45.0, false).unwrap();
        let (lo, la) = v.unproject(p).unwrap();
        assert!((lo - 5.0).abs() < 1e-3 && (la - 45.0).abs() < 1e-3);
    }
}

// ---------------------------------------------------------------- Poignées de redimensionnement

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum HandleKind {
    /// Sommet d'une ligne / flèche / polygone.
    Vertex(usize),
    /// Coin d'un rectangle (0 = A, 1 = (Bx,Ay), 2 = B, 3 = (Ax,By)).
    Corner(usize),
    /// Rayon d'un cercle ou d'un point.
    Radius,
    /// Taille d'un texte.
    TextSize,
}

pub fn handles(view: &View, item: &Item) -> Vec<(P, HandleKind)> {
    match &item.kind {
        Kind::Line { pts, .. } | Kind::Polygon { pts } => pts
            .iter()
            .enumerate()
            .filter_map(|(i, p)| view.pos_to_page(p).map(|q| (q, HandleKind::Vertex(i))))
            .collect(),
        Kind::Rect { a, b } => {
            let (Some(a), Some(b)) = (view.pos_to_page(a), view.pos_to_page(b)) else { return vec![] };
            vec![(a, HandleKind::Corner(0)), ([b[0], a[1]], HandleKind::Corner(1)), (b, HandleKind::Corner(2)), ([a[0], b[1]], HandleKind::Corner(3))]
        }
        Kind::Circle { center, radius, unit } => match (unit, center) {
            (RadiusUnit::Km, Pos::Geo { lon, lat }) => {
                let ring = km_circle(view, *lon, *lat, *radius as f64);
                if ring.len() == 72 { vec![(ring[18], HandleKind::Radius)] } else { vec![] }
            }
            _ => view.pos_to_page(center).map(|c| vec![([c[0] + radius, c[1]], HandleKind::Radius)]).unwrap_or_default(),
        },
        Kind::Point { pos, radius, .. } => view.pos_to_page(pos).map(|c| vec![([c[0] + radius, c[1]], HandleKind::Radius)]).unwrap_or_default(),
        Kind::Text { .. } => match item_shape(view, item) {
            Some(Shape::TextBox(b)) => vec![([b[0] + b[2], b[1] + b[3]], HandleKind::TextSize)],
            _ => vec![],
        },
    }
}

fn rebuild_pos(view: &View, orig: &Pos, p: P) -> Pos {
    match orig {
        Pos::Page { .. } => Pos::Page { x: p[0], y: p[1] },
        Pos::Geo { .. } => view.unproject(p).map(|(lon, lat)| Pos::Geo { lon, lat }).unwrap_or(*orig),
    }
}

/// Copie de `orig` modifiée comme si la poignée `kind` était amenée en `cur` (pixels de page).
pub fn apply_handle(view: &View, orig: &Item, kind: HandleKind, cur: P) -> Item {
    let mut it = orig.clone();
    let orig_box = match item_shape(view, orig) {
        Some(Shape::TextBox(b)) => Some(b),
        _ => None,
    };
    match (&mut it.kind, kind) {
        (Kind::Line { pts, .. } | Kind::Polygon { pts }, HandleKind::Vertex(i)) => {
            if let Some(p) = pts.get(i) {
                let np = rebuild_pos(view, p, cur);
                pts[i] = np;
            }
        }
        (Kind::Rect { a, b }, HandleKind::Corner(c)) => {
            let (Some(ap), Some(bp)) = (view.pos_to_page(a), view.pos_to_page(b)) else { return it };
            match c {
                0 => *a = rebuild_pos(view, a, cur),
                2 => *b = rebuild_pos(view, b, cur),
                1 => {
                    let (na, nb) = (rebuild_pos(view, a, [ap[0], cur[1]]), rebuild_pos(view, b, [cur[0], bp[1]]));
                    *a = na;
                    *b = nb;
                }
                _ => {
                    let (na, nb) = (rebuild_pos(view, a, [cur[0], ap[1]]), rebuild_pos(view, b, [bp[0], cur[1]]));
                    *a = na;
                    *b = nb;
                }
            }
        }
        (Kind::Circle { center, radius, unit }, HandleKind::Radius) => match (&*unit, &*center) {
            (RadiusUnit::Km, Pos::Geo { lon, lat }) => {
                if let Some(g) = view.unproject(cur) {
                    *radius = haversine_km((*lon, *lat), g) as f32;
                }
            }
            _ => {
                if let Some(c) = view.pos_to_page(center) {
                    *radius = (cur[0] - c[0]).hypot(cur[1] - c[1]);
                }
            }
        },
        (Kind::Point { pos, radius, .. }, HandleKind::Radius) => {
            if let Some(c) = view.pos_to_page(pos) {
                *radius = (cur[0] - c[0]).hypot(cur[1] - c[1]).clamp(2.0, 80.0);
            }
        }
        (Kind::Text { size, .. }, HandleKind::TextSize) => {
            if let Some(b) = orig_box {
                let d0 = b[2].hypot(b[3]).max(1.0);
                let d1 = (cur[0] - b[0]).hypot(cur[1] - b[1]);
                *size = (*size * d1 / d0).clamp(6.0, 300.0);
            }
        }
        _ => {}
    }
    it
}

#[cfg(test)]
mod handle_tests {
    use super::*;

    fn pg(x: f32, y: f32) -> Pos {
        Pos::Page { x, y }
    }

    #[test]
    fn resizing_does_not_move_the_item() {
        let d = Document::default();
        let v = View::new(&d);
        let line = Item { id: 1, name: "f".into(), visible: true, stroke: [0; 4], fill: [0; 4], width: 3.0, dashed: false, kind: Kind::Line { pts: vec![pg(100.0, 100.0), pg(300.0, 200.0)], arrow_start: false, arrow_end: true, geodesic: false } };
        let hs = handles(&v, &line);
        assert_eq!(hs.len(), 2);
        let out = apply_handle(&v, &line, HandleKind::Vertex(1), [400.0, 250.0]);
        match out.kind {
            Kind::Line { pts, .. } => {
                assert_eq!(pts[0], pg(100.0, 100.0)); // l'autre extrémité ne bouge pas
                assert_eq!(pts[1], pg(400.0, 250.0));
            }
            _ => panic!(),
        }
        let rect = Item { kind: Kind::Rect { a: pg(10.0, 10.0), b: pg(110.0, 60.0) }, ..line.clone() };
        let r = apply_handle(&v, &rect, HandleKind::Corner(2), [200.0, 100.0]);
        assert_eq!(r.kind, Kind::Rect { a: pg(10.0, 10.0), b: pg(200.0, 100.0) });
        let r = apply_handle(&v, &rect, HandleKind::Corner(1), [150.0, 0.0]);
        assert_eq!(r.kind, Kind::Rect { a: pg(10.0, 0.0), b: pg(150.0, 60.0) });
        let circ = Item { kind: Kind::Circle { center: pg(100.0, 100.0), radius: 20.0, unit: RadiusUnit::Px }, ..line.clone() };
        let c = apply_handle(&v, &circ, HandleKind::Radius, [160.0, 100.0]);
        assert_eq!(c.kind, Kind::Circle { center: pg(100.0, 100.0), radius: 60.0, unit: RadiusUnit::Px });
        let txt = Item { kind: Kind::Text { pos: pg(300.0, 300.0), text: "abc".into(), size: 20.0, bold: false, italic: false, rot: 0.0, halo: false, align: HAlign::Left }, ..line };
        let hs = handles(&v, &txt);
        let t = apply_handle(&v, &txt, HandleKind::TextSize, [hs[0].0[0] + 30.0, hs[0].0[1] + 20.0]);
        match t.kind {
            Kind::Text { size, pos, .. } => {
                assert!(size > 20.0);
                assert_eq!(pos, pg(300.0, 300.0));
            }
            _ => panic!(),
        }
    }
}

#[cfg(test)]
mod debug_edges {
    use super::*;
    use crate::world::World;

    #[test]
    #[ignore]
    fn find_long_edges() {
        let w = World::embedded();
        for proj in [Projection::Mercator, Projection::Equirectangular] {
            for clon in [-170.0, -60.0, -10.0, 0.0, 10.0, 15.0, 20.0, 25.0, 30.0, 40.0, 100.0, 170.0, 179.0] {
                for clat in [-40.0, -35.0, -30.0, -10.0, 0.0, 20.0, 50.0] {
                    for zoom in [1.0, 2.0, 3.0, 4.0, 6.0, 8.0] {
                        let mut d = Document::default();
                        d.map.proj = proj;
                        d.map.center_lon = clon;
                        d.map.center_lat = clat;
                        d.map.zoom = zoom;
                        let v = View::new(&d);
                        let f = v.frame;
                        let inside = |p: P| p[0] > f[0] && p[0] < f[0] + f[2] && p[1] > f[1] && p[1] < f[1] + f[3];
                        for c in &w.countries {
                            for poly in &c.polys {
                                let mut out = Vec::new();
                                poly_prims(&v, poly, None, Some(Stroke { color: [0; 4], width: 1.0, dashed: false }), &mut out);
                                for p in out {
                                    if let Prim::Path { rings, .. } = p {
                                        for r in rings {
                                            let n = r.len();
                                            for i in 0..n {
                                                let (a, b) = (r[i], r[(i + 1) % n]);
                                                let len = (a[0] - b[0]).hypot(a[1] - b[1]);
                                                let x_span = (a[0] - b[0]).abs();
                                                // arête longue, quasi verticale, qui touche le cadre
                                                if len > 120.0 && x_span < len * 0.35 && (inside(a) || inside(b)) {
                                                    println!("{:?} lon0={clon} lat0={clat} z={zoom} {} edge {:?}->{:?} len={len:.0}", proj, c.name, a, b);
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod debug_layers {
    use super::*;

    #[test]
    #[ignore]
    fn find_long_edges_layers() {
        for id in ["countries_50m", "rivers_110m", "lakes_110m", "lakes_50m", "rivers_50m", "admin1_50m"] {
            let Some(w) = crate::data::load(id) else { continue };
            let mut hits = 0;
            for clon in [0.0, 10.0, 20.0, 25.0, 30.0, 40.0] {
                for clat in [-40.0, -30.0, -10.0] {
                    for zoom in [2.0, 3.0, 4.0, 6.0, 9.0] {
                        let mut d = Document::default();
                        d.map.center_lon = clon;
                        d.map.center_lat = clat;
                        d.map.zoom = zoom;
                        let v = View::new(&d);
                        let f = v.frame;
                        let inside = |p: P| p[0] > f[0] && p[0] < f[0] + f[2] && p[1] > f[1] && p[1] < f[1] + f[3];
                        let st = Stroke { color: [0; 4], width: 1.0, dashed: false };
                        for c in &w.countries {
                            let mut out = Vec::new();
                            for poly in &c.polys {
                                poly_prims(&v, poly, None, Some(st), &mut out);
                            }
                            for l in &c.lines {
                                line_prims(&v, l, st, &mut out);
                            }
                            for p in out {
                                if let Prim::Path { rings, closed, .. } = p {
                                    for r in rings {
                                        let n = r.len();
                                        let segs = if closed { n } else { n - 1 };
                                        for i in 0..segs {
                                            let (a, b) = (r[i], r[(i + 1) % n]);
                                            let len = (a[0] - b[0]).hypot(a[1] - b[1]);
                                            if len > 200.0 && (inside(a) || inside(b)) && hits < 4 {
                                                hits += 1;
                                                println!("{id} lon0={clon} lat0={clat} z={zoom} {} {:?}->{:?} len={len:.0}", c.name, a, b);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod debug_build {
    use super::*;
    use crate::world::Data;
    use std::collections::HashMap;
    use std::sync::Arc;

    #[test]
    #[ignore]
    fn scan_build_output() {
        for base in ["countries_110m", "countries_50m"] {
            let mut layers = HashMap::new();
            for id in ["lakes_110m", "rivers_110m"] {
                layers.insert(id.to_string(), Arc::new(crate::data::load(id).unwrap()));
            }
            let data = Data { base: Arc::new(crate::data::load(base).unwrap()), capitals: Arc::new(crate::data::capitals()), layers };
            let mut found = 0;
            let mut lon = -10.0;
            while lon <= 70.0 {
                // balayage dense
                let mut lat = -50.0;
                while lat <= 15.0 {
                    for zoom in [4.0, 6.0, 8.0] {
                        let mut d = Document::default();
                        d.map.center_lon = lon;
                        d.map.center_lat = lat;
                        d.map.zoom = zoom;
                        let prims = build(&d, &data);
                        let f = View::new(&d).frame;
                        let inside = |p: P| p[0] > f[0] && p[0] < f[0] + f[2] && p[1] > f[1] && p[1] < f[1] + f[3];
                        for p in &prims {
                            if let Prim::Path { rings, closed, stroke: Some(_), .. } = p {
                                for r in rings {
                                    let n = r.len();
                                    if n < 3 || n == 4 && *closed { continue; }
                                    let segs = if *closed { n } else { n - 1 };
                                    for i in 0..segs {
                                        let (a, b) = (r[i], r[(i + 1) % n]);
                                        let len = (a[0] - b[0]).hypot(a[1] - b[1]);
                                        if len > 150.0 && (inside(a) || inside(b)) && found < 6 {
                                            found += 1;
                                            println!("{base} lon={lon} lat={lat} z={zoom} closed={closed} n={n} {:?}->{:?} len={len:.0}", a, b);
                                        }
                                    }
                                }
                            }
                        }
                    }
                    lat += 2.5;
                }
                lon += 1.5;
            }
        }
    }
}
