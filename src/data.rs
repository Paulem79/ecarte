//! Jeux de données cartographiques : intégrés (hors ligne) + téléchargeables avec cache local.
//! Tout fonctionne sans réseau ; la synchronisation est optionnelle et ne bloque jamais l'interface.
use crate::world::World;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DsKind {
    /// Fond de carte principal (pays, colorisable).
    Base,
    /// Couche additionnelle (lacs, rivières, subdivisions…).
    Layer,
}

pub struct Dataset {
    pub id: &'static str,
    pub label: &'static str,
    pub kind: DsKind,
    pub url: &'static str,
    pub file: &'static str,
    pub embedded: Option<&'static [u8]>,
    pub approx_kb: u32,
}

const NE: &str = "https://raw.githubusercontent.com/nvkelso/natural-earth-vector/master/geojson";

macro_rules! url {
    ($f:literal) => {
        concat!("https://raw.githubusercontent.com/nvkelso/natural-earth-vector/master/geojson/", $f)
    };
}

pub static CATALOG: &[Dataset] = &[
    Dataset {
        id: "countries_110m",
        label: "Pays 1:110m (intégré)",
        kind: DsKind::Base,
        url: url!("ne_110m_admin_0_countries.geojson"),
        file: "ne_110m_admin_0_countries.geojson",
        embedded: Some(include_bytes!("../data/ne_110m_admin_0_countries.geojson")),
        approx_kb: 820,
    },
    Dataset {
        id: "countries_50m",
        label: "Pays 1:50m (détaillé)",
        kind: DsKind::Base,
        url: url!("ne_50m_admin_0_countries.geojson"),
        file: "ne_50m_admin_0_countries.geojson",
        embedded: None,
        approx_kb: 3010,
    },
    Dataset {
        id: "lakes_110m",
        label: "Lacs 1:110m (intégré)",
        kind: DsKind::Layer,
        url: url!("ne_110m_lakes.geojson"),
        file: "ne_110m_lakes.geojson",
        embedded: Some(include_bytes!("../data/ne_110m_lakes.geojson")),
        approx_kb: 36,
    },
    Dataset {
        id: "rivers_110m",
        label: "Fleuves 1:110m (intégré)",
        kind: DsKind::Layer,
        url: url!("ne_110m_rivers_lake_centerlines.geojson"),
        file: "ne_110m_rivers_lake_centerlines.geojson",
        embedded: Some(include_bytes!("../data/ne_110m_rivers_lake_centerlines.geojson")),
        approx_kb: 38,
    },
    Dataset {
        id: "lakes_50m",
        label: "Lacs 1:50m",
        kind: DsKind::Layer,
        url: url!("ne_50m_lakes.geojson"),
        file: "ne_50m_lakes.geojson",
        embedded: None,
        approx_kb: 855,
    },
    Dataset {
        id: "rivers_50m",
        label: "Fleuves 1:50m",
        kind: DsKind::Layer,
        url: url!("ne_50m_rivers_lake_centerlines.geojson"),
        file: "ne_50m_rivers_lake_centerlines.geojson",
        embedded: None,
        approx_kb: 808,
    },
    Dataset {
        id: "admin1_50m",
        label: "Régions / États 1:50m",
        kind: DsKind::Layer,
        url: url!("ne_50m_admin_1_states_provinces.geojson"),
        file: "ne_50m_admin_1_states_provinces.geojson",
        embedded: None,
        approx_kb: 2270,
    },
];

#[allow(dead_code)]
pub const SOURCE_ROOT: &str = NE;

pub fn find(id: &str) -> Option<&'static Dataset> {
    CATALOG.iter().find(|d| d.id == id)
}

// ---------------------------------------------------------------- Cache local

pub fn cache_dir() -> PathBuf {
    if let Some(p) = std::env::var_os("ECARTE_DATA") {
        return PathBuf::from(p);
    }
    #[cfg(windows)]
    {
        if let Some(a) = std::env::var_os("APPDATA") {
            return PathBuf::from(a).join("ecarte");
        }
    }
    if let Some(x) = std::env::var_os("XDG_DATA_HOME") {
        return PathBuf::from(x).join("ecarte");
    }
    if let Some(h) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
        return PathBuf::from(h).join(".local").join("share").join("ecarte");
    }
    PathBuf::from("ecarte-data")
}

fn path_of(d: &Dataset) -> PathBuf {
    cache_dir().join(d.file)
}

fn etag_path(d: &Dataset) -> PathBuf {
    cache_dir().join(format!("{}.etag", d.file))
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Availability {
    /// Fourni dans l'exécutable.
    Embedded,
    /// Téléchargé dans le cache local.
    Cached,
    /// Pas encore téléchargé.
    Missing,
}

pub fn availability(d: &Dataset) -> Availability {
    if d.embedded.is_some() {
        Availability::Embedded
    } else if path_of(d).is_file() {
        Availability::Cached
    } else {
        Availability::Missing
    }
}

/// Charge un jeu de données : le cache (plus récent) d'abord, sinon l'intégré. Jamais de réseau.
pub fn load(id: &str) -> Option<World> {
    let d = find(id)?;
    if let Ok(text) = std::fs::read_to_string(path_of(d)) {
        if let Some(w) = World::from_geojson(&text) {
            return Some(w);
        }
    }
    let bytes = d.embedded?;
    World::from_geojson(std::str::from_utf8(bytes).ok()?)
}

// ---------------------------------------------------------------- Synchronisation

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SyncOutcome {
    Downloaded,
    UpToDate,
    Offline(String),
    Failed(String),
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(6)))
        .timeout_global(Some(Duration::from_secs(90)))
        .build()
        .into()
}

/// Télécharge (ou met à jour, via ETag) un jeu de données. Bloquant : à appeler dans un thread.
pub fn sync_one(d: &Dataset) -> SyncOutcome {
    if std::fs::create_dir_all(cache_dir()).is_err() {
        return SyncOutcome::Failed("dossier de cache inaccessible".into());
    }
    let have = path_of(d).is_file();
    let mut req = agent().get(d.url);
    if have {
        if let Ok(etag) = std::fs::read_to_string(etag_path(d)) {
            if !etag.trim().is_empty() {
                req = req.header("If-None-Match", etag.trim());
            }
        }
    }
    let mut resp = match req.call() {
        Ok(r) => r,
        Err(ureq::Error::StatusCode(c)) => return SyncOutcome::Failed(format!("HTTP {c}")),
        Err(e) => return SyncOutcome::Offline(e.to_string()),
    };
    if resp.status().as_u16() == 304 {
        return SyncOutcome::UpToDate;
    }
    let etag = resp.headers().get("etag").and_then(|v| v.to_str().ok()).map(str::to_string);
    let body = match resp.body_mut().with_config().limit(64 * 1024 * 1024).read_to_vec() {
        Ok(b) => b,
        Err(e) => return SyncOutcome::Offline(e.to_string()),
    };
    // Valider avant d'écraser le cache existant.
    let ok = std::str::from_utf8(&body).ok().and_then(World::from_geojson).is_some_and(|w| !w.countries.is_empty());
    if !ok {
        return SyncOutcome::Failed("données invalides".into());
    }
    let tmp = cache_dir().join(format!("{}.part", d.file));
    if std::fs::write(&tmp, &body).and_then(|_| std::fs::rename(&tmp, path_of(d))).is_err() {
        return SyncOutcome::Failed("écriture du cache impossible".into());
    }
    match etag {
        Some(e) => {
            let _ = std::fs::write(etag_path(d), e);
        }
        None => {
            let _ = std::fs::remove_file(etag_path(d));
        }
    }
    SyncOutcome::Downloaded
}

// ---------------------------------------------------------------- Préférences

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Au démarrage, vérifier (si en ligne) les mises à jour des jeux de données déjà en cache.
    pub auto_sync: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { auto_sync: true }
    }
}

impl Settings {
    pub fn load() -> Settings {
        std::fs::read_to_string(cache_dir().join("settings.json"))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let _ = std::fs::create_dir_all(cache_dir());
        let _ = std::fs::write(cache_dir().join("settings.json"), serde_json::to_string_pretty(self).unwrap_or_default());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_datasets_load_offline() {
        for d in CATALOG.iter().filter(|d| d.embedded.is_some()) {
            let w = load(d.id).unwrap_or_else(|| panic!("{}", d.id));
            assert!(!w.countries.is_empty(), "{}", d.id);
        }
        assert!(find("rivers_110m").is_some());
        assert!(load("rivers_110m").unwrap().countries.iter().any(|c| !c.lines.is_empty()));
    }
}

#[cfg(test)]
mod live {
    use super::*;

    #[test]
    #[ignore]
    fn sync_live() {
        let d = find("lakes_50m").unwrap();
        let r1 = sync_one(d);
        assert_eq!(r1, SyncOutcome::Downloaded, "{r1:?}");
        assert_eq!(availability(d), Availability::Cached);
        let r2 = sync_one(d);
        assert_eq!(r2, SyncOutcome::UpToDate, "{r2:?}");
        assert!(load("lakes_50m").unwrap().countries.len() > 50);
    }
}
