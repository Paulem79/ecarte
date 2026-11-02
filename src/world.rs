//! Données vectorielles du fond de carte (Natural Earth 110m, domaine public).
use serde_json::Value;

pub type Ring = Vec<[f32; 2]>; // (lon, lat)

#[derive(Clone, Debug, Default)]
pub struct Poly {
    /// rings[0] = contour extérieur, le reste = trous.
    pub rings: Vec<Ring>,
}

#[derive(Clone, Debug, Default)]
pub struct Country {
    pub name: String,
    pub name_fr: String,
    pub iso: String,
    pub polys: Vec<Poly>,
    /// Lignes ouvertes (rivières, etc.).
    pub lines: Vec<Ring>,
    pub label: (f64, f64),
    /// Aire (degrés²) du plus grand polygone, pour filtrer les étiquettes.
    pub area: f64,
}

#[derive(Default)]
pub struct World {
    pub countries: Vec<Country>,
}

static EMBEDDED: &[u8] = include_bytes!("../data/ne_110m_admin_0_countries.geojson");

fn ring_from(v: &Value) -> Ring {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|p| {
                    let p = p.as_array()?;
                    Some([p.first()?.as_f64()? as f32, p.get(1)?.as_f64()? as f32])
                })
                .collect()
        })
        .unwrap_or_default()
}

fn poly_from(v: &Value) -> Poly {
    Poly { rings: v.as_array().map(|a| a.iter().map(ring_from).collect()).unwrap_or_default() }
}

fn centroid(r: &Ring) -> (f64, f64, f64) {
    let (mut a, mut cx, mut cy) = (0.0f64, 0.0f64, 0.0f64);
    for w in r.windows(2) {
        let (x0, y0, x1, y1) = (w[0][0] as f64, w[0][1] as f64, w[1][0] as f64, w[1][1] as f64);
        let c = x0 * y1 - x1 * y0;
        a += c;
        cx += (x0 + x1) * c;
        cy += (y0 + y1) * c;
    }
    if a.abs() < 1e-9 {
        let n = r.len().max(1) as f64;
        let sx: f64 = r.iter().map(|p| p[0] as f64).sum();
        let sy: f64 = r.iter().map(|p| p[1] as f64).sum();
        return (sx / n, sy / n, 0.0);
    }
    (cx / (3.0 * a), cy / (3.0 * a), (a / 2.0).abs())
}

impl World {
    pub fn embedded() -> World {
        World::from_geojson(std::str::from_utf8(EMBEDDED).unwrap_or("{}")).unwrap_or_default()
    }

    pub fn from_geojson(text: &str) -> Option<World> {
        let root: Value = serde_json::from_str(text).ok()?;
        let feats = root.get("features")?.as_array()?;
        let mut countries = Vec::new();
        for f in feats {
            let props = f.get("properties");
            let s = |k: &str| {
                props
                    .and_then(|p| p.get(k))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string()
            };
            let geom = f.get("geometry")?;
            let coords = geom.get("coordinates")?;
            let mut lines: Vec<Ring> = Vec::new();
            let polys: Vec<Poly> = match geom.get("type")?.as_str()? {
                "Polygon" => vec![poly_from(coords)],
                "MultiPolygon" => coords.as_array()?.iter().map(poly_from).collect(),
                "LineString" => {
                    lines.push(ring_from(coords));
                    vec![]
                }
                "MultiLineString" => {
                    lines.extend(coords.as_array()?.iter().map(ring_from));
                    vec![]
                }
                _ => continue,
            };
            let mut best = (0.0, 0.0, -1.0);
            for p in &polys {
                if let Some(o) = p.rings.first() {
                    let c = centroid(o);
                    if c.2 > best.2 {
                        best = c;
                    }
                }
            }
            let mut name = s("NAME");
            if name.is_empty() {
                name = s("ADMIN");
            }
            if name.is_empty() {
                name = s("name");
            }
            let mut name_fr = s("NAME_FR");
            if name_fr.is_empty() {
                name_fr = name.clone();
            }
            let mut iso = s("ADM0_A3");
            if iso.is_empty() {
                iso = name.clone();
            }
            countries.push(Country { name, name_fr, iso, polys, lines, label: (best.0, best.1), area: best.2.max(0.0) });
        }
        Some(World { countries })
    }
}

/// Point dans polygone (lon/lat, lancer de rayon, trous gérés par parité).
pub fn poly_contains(p: &Poly, lon: f64, lat: f64) -> bool {
    let mut inside = false;
    for ring in &p.rings {
        let n = ring.len();
        if n < 3 {
            continue;
        }
        let mut j = n - 1;
        for i in 0..n {
            let (xi, yi) = (ring[i][0] as f64, ring[i][1] as f64);
            let (xj, yj) = (ring[j][0] as f64, ring[j][1] as f64);
            if (yi > lat) != (yj > lat) && lon < (xj - xi) * (lat - yi) / (yj - yi) + xi {
                inside = !inside;
            }
            j = i;
        }
    }
    inside
}

impl World {
    pub fn country_at(&self, lon: f64, lat: f64) -> Option<&Country> {
        self.countries
            .iter()
            .find(|c| c.polys.iter().any(|p| poly_contains(p, lon, lat)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_loads_and_finds_france() {
        let w = World::embedded();
        assert!(w.countries.len() > 150);
        let c = w.country_at(2.35, 47.0).expect("France");
        assert_eq!(c.iso, "FRA");
    }
}

/// Ensemble des données chargées : fond (pays) + couches additionnelles par identifiant.
pub struct Data {
    pub base: std::sync::Arc<World>,
    pub layers: std::collections::HashMap<String, std::sync::Arc<World>>,
}

impl Data {
    pub fn embedded_only() -> Data {
        Data { base: std::sync::Arc::new(World::embedded()), layers: Default::default() }
    }
}
