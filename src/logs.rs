//! Journal simple dans le dossier de données (ecarte.log), sans dépendance externe.
use crate::data;
use std::io::Write;
use std::path::PathBuf;

pub fn path() -> PathBuf {
    data::cache_dir().join("ecarte.log")
}

fn stamp() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) as i64;
    let (days, rem) = (secs.div_euclid(86400), secs.rem_euclid(86400));
    // jours depuis 1970 -> date civile (algorithme de H. Hinnant)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02} UTC", rem / 3600, rem % 3600 / 60, rem % 60)
}

fn write(level: &str, msg: &str) {
    let p = path();
    let _ = std::fs::create_dir_all(data::cache_dir());
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(p) {
        let _ = writeln!(f, "{} [{level}] {msg}", stamp());
    }
}

pub fn info(msg: &str) {
    write("INFO", msg);
}

pub fn error(msg: &str) {
    write("ERREUR", msg);
}

/// À appeler au démarrage : limite la taille du fichier et journalise les plantages.
pub fn init() {
    let p = path();
    if std::fs::metadata(&p).map(|m| m.len() > 1_000_000).unwrap_or(false) {
        let _ = std::fs::rename(&p, p.with_extension("log.old"));
    }
    std::panic::set_hook(Box::new(|info| {
        write("PANIQUE", &info.to_string());
    }));
    info(&format!("démarrage E-cart-é {} ({})", env!("CARGO_PKG_VERSION"), std::env::consts::OS));
}

/// Ouvre un fichier avec l'application par défaut du système. Renvoie une erreur lisible si aucun lanceur n'existe.
pub fn open_in_viewer(path: &std::path::Path) -> Result<(), String> {
    use std::process::Command;
    #[cfg(windows)]
    {
        return Command::new("notepad").arg(path).spawn().map(|_| ()).map_err(|e| e.to_string());
    }
    #[cfg(not(windows))]
    {
        let candidates: [(&str, &[&str]); 7] = [
            ("xdg-open", &[]),
            ("geany", &[]),
            ("featherpad", &[]),
            ("leafpad", &[]),
            ("mousepad", &[]),
            ("gedit", &[]),
            ("x-terminal-emulator", &["-e", "less"]),
        ];
        for (cmd, args) in candidates {
            if Command::new(cmd).args(args).arg(path).spawn().is_ok() {
                return Ok(());
            }
        }
        Err(format!("aucun lanceur trouvé ; fichier : {}", path.display()))
    }
}
