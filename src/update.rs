//! Mise à jour automatique depuis les releases GitHub (Linux et Windows).
//!
//! Le binaire courant est remplacé par celui de la dernière release :
//! - Linux : renommage atomique par-dessus l'exécutable (autorisé même en cours d'exécution) ;
//! - Windows : l'exécutable en cours est renommé en `.old` (supprimé au lancement suivant), puis remplacé.
//!
//! Dépôt privé : le jeton est lu dans GITHUB_TOKEN / GH_TOKEN, le fichier `token` du dossier de données,
//! ou via `gh auth token`. Sans jeton ni dépôt public, la vérification échoue silencieusement (journalisée).
use crate::data;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::Duration;

pub const REPO: &str = "Paulem79/ecarte";

/// Étiquette de build fournie par le CI (`build-N-sha`) ; « dev » pour une compilation locale.
pub const CURRENT: &str = match option_env!("ECARTE_BUILD_TAG") {
    Some(t) => t,
    None => "dev",
};

#[cfg(windows)]
pub const ASSET: &str = "ecarte-windows-x86_64.exe";
#[cfg(not(windows))]
pub const ASSET: &str = "ecarte-linux-x86_64";

#[derive(Clone, Debug)]
pub struct Release {
    pub tag: String,
    pub notes: String,
    pub asset_url: String,
    /// URL publique de téléchargement direct (sans API ni jeton).
    pub download_url: String,
    pub asset_size: u64,
}

static EXE: OnceLock<PathBuf> = OnceLock::new();

/// Chemin de l'exécutable, figé au démarrage (sous Linux /proc/self/exe devient « (deleted) » après remplacement).
pub fn exe_path() -> PathBuf {
    EXE.get_or_init(|| std::env::current_exe().unwrap_or_else(|_| PathBuf::from("ecarte"))).clone()
}

fn old_path() -> PathBuf {
    let mut s = exe_path().into_os_string();
    s.push(".old");
    PathBuf::from(s)
}

/// À appeler au démarrage : mémorise le chemin et supprime l'ancien exécutable laissé par une mise à jour Windows.
pub fn init() {
    let _ = exe_path();
    let _ = std::fs::remove_file(old_path());
}

pub fn build_number(tag: &str) -> Option<u64> {
    tag.strip_prefix("build-")?.split('-').next()?.parse().ok()
}

fn token() -> Option<String> {
    for k in ["GITHUB_TOKEN", "GH_TOKEN"] {
        if let Ok(v) = std::env::var(k) {
            if !v.trim().is_empty() {
                return Some(v.trim().to_string());
            }
        }
    }
    let mut files = vec![data::cache_dir().join("token")];
    if let Some(h) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
        files.push(PathBuf::from(h).join(".config").join("ecarte").join("token"));
    }
    for f in files {
        if let Ok(t) = std::fs::read_to_string(f) {
            if !t.trim().is_empty() {
                return Some(t.trim().to_string());
            }
        }
    }
    let mut cmd = std::process::Command::new("gh");
    cmd.args(["auth", "token"]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let out = cmd.output().ok()?;
    let t = String::from_utf8(out.stdout).ok()?;
    if out.status.success() && !t.trim().is_empty() { Some(t.trim().to_string()) } else { None }
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(8)))
        .timeout_global(Some(Duration::from_secs(300)))
        .build()
        .into()
}

fn request_once(url: &str, accept: &str, token: &Option<String>) -> Result<ureq::http::Response<ureq::Body>, String> {
    let mut req = agent().get(url).header("Accept", accept).header("User-Agent", "ecarte-updater");
    if let Some(t) = token {
        req = req.header("Authorization", format!("Bearer {t}"));
    }
    req.call().map_err(|e| match e {
        ureq::Error::StatusCode(404) => "dépôt introuvable ou accès refusé".to_string(),
        ureq::Error::StatusCode(c) => format!("HTTP {c}"),
        e => e.to_string(),
    })
}

/// Requête sans jeton ; en cas d'échec, nouvel essai avec un jeton s'il en existe un.
fn request(url: &str, accept: &str) -> Result<ureq::http::Response<ureq::Body>, String> {
    match request_once(url, accept, &None) {
        Ok(r) => Ok(r),
        Err(first) => match token() {
            Some(t) => request_once(url, accept, &Some(t)).map_err(|_| first),
            None => Err(first),
        },
    }
}

/// Interroge la dernière release. `Ok(Some)` si elle est plus récente que le build courant.
pub fn check() -> Result<Option<Release>, String> {
    let Some(cur) = build_number(CURRENT) else {
        return Ok(None); // build local (« dev ») : pas de mise à jour automatique
    };
    let mut resp = request(&format!("https://api.github.com/repos/{REPO}/releases/latest"), "application/vnd.github+json")?;
    let text = resp.body_mut().read_to_string().map_err(|e| e.to_string())?;
    let v: serde_json::Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    let tag = v.get("tag_name").and_then(|t| t.as_str()).ok_or("réponse inattendue")?.to_string();
    if build_number(&tag).is_none_or(|n| n <= cur) {
        return Ok(None);
    }
    let asset = v
        .get("assets")
        .and_then(|a| a.as_array())
        .and_then(|a| a.iter().find(|x| x.get("name").and_then(|n| n.as_str()) == Some(ASSET)));
    let Some(asset) = asset else {
        return Err(format!("la release {tag} ne contient pas le fichier {ASSET}"));
    };
    Ok(Some(Release {
        tag,
        notes: v.get("body").and_then(|b| b.as_str()).unwrap_or("").to_string(),
        asset_url: asset.get("url").and_then(|u| u.as_str()).unwrap_or("").to_string(),
        download_url: asset.get("browser_download_url").and_then(|u| u.as_str()).unwrap_or("").to_string(),
        asset_size: asset.get("size").and_then(|s| s.as_u64()).unwrap_or(0),
    }))
}

fn looks_like_executable(b: &[u8]) -> bool {
    #[cfg(windows)]
    {
        b.starts_with(b"MZ")
    }
    #[cfg(not(windows))]
    {
        b.starts_with(&[0x7f, b'E', b'L', b'F'])
    }
}

/// Télécharge et valide le binaire de la release (sans rien remplacer).
pub fn download(rel: &Release) -> Result<Vec<u8>, String> {
    let mut resp = match request_once(&rel.download_url, "application/octet-stream", &None) {
        Ok(r) => r,
        Err(_) => request(&rel.asset_url, "application/octet-stream")?,
    };
    let bytes = resp.body_mut().with_config().limit(300 * 1024 * 1024).read_to_vec().map_err(|e| e.to_string())?;
    if bytes.len() < 1_000_000 || (rel.asset_size > 0 && bytes.len() as u64 != rel.asset_size) {
        return Err(format!("téléchargement incomplet ({} octets, attendu {})", bytes.len(), rel.asset_size));
    }
    if !looks_like_executable(&bytes) {
        return Err("le fichier téléchargé n'est pas un exécutable valide".into());
    }
    Ok(bytes)
}

/// Télécharge la release et remplace l'exécutable courant. L'application doit ensuite être relancée.
pub fn install(rel: &Release) -> Result<(), String> {
    let bytes = download(rel)?;
    let exe = exe_path();
    let dir = exe.parent().ok_or("dossier de l'exécutable introuvable")?;
    let mut tmp = exe.clone().into_os_string();
    tmp.push(".new");
    let tmp = PathBuf::from(tmp);
    std::fs::write(&tmp, &bytes).map_err(|e| format!("écriture impossible dans {} : {e}", dir.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755));
        std::fs::rename(&tmp, &exe).map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            format!("remplacement impossible : {e}")
        })?;
    }
    #[cfg(windows)]
    {
        let old = old_path();
        let _ = std::fs::remove_file(&old);
        std::fs::rename(&exe, &old).map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            format!("remplacement impossible : {e}")
        })?;
        if let Err(e) = std::fs::rename(&tmp, &exe) {
            let _ = std::fs::rename(&old, &exe); // retour arrière
            let _ = std::fs::remove_file(&tmp);
            return Err(format!("remplacement impossible : {e}"));
        }
    }
    Ok(())
}

/// Relance l'application (mêmes arguments) puis quitte.
pub fn restart() -> ! {
    let _ = std::process::Command::new(exe_path()).args(std::env::args_os().skip(1)).spawn();
    std::process::exit(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_build_numbers() {
        assert_eq!(build_number("build-12-abc1234"), Some(12));
        assert_eq!(build_number("dev"), None);
        assert!(build_number("build-9-x") < build_number("build-10-y"));
    }

    #[test]
    fn dev_build_never_checks() {
        // Compilation locale : aucune requête réseau.
        if CURRENT == "dev" {
            assert!(check().unwrap().is_none());
        }
    }
}

#[cfg(test)]
mod live {
    use super::*;

    /// Nécessite un accès GitHub (jeton ou gh) et ECARTE_BUILD_TAG=build-1-test à la compilation.
    #[test]
    #[ignore]
    fn check_and_download_live() {
        assert_eq!(build_number(CURRENT), Some(1), "compiler avec ECARTE_BUILD_TAG=build-1-test");
        let rel = check().expect("check").expect("une release plus récente doit exister");
        println!("release {} asset {} ({} octets)", rel.tag, rel.asset_url, rel.asset_size);
        let bytes = download(&rel).expect("download");
        assert_eq!(bytes.len() as u64, rel.asset_size);
        println!("téléchargé {} octets, exécutable valide", bytes.len());
    }
}
