//! Aviso de versión nueva.
//!
//! Vive en la CLI y no en `yachay-core`: el core es puro, sin red ni I/O.
//! Como máximo una petición cada 24 h; el resto de las corridas solo leen el
//! cache de disco, así que un comando rápido sigue siendo rápido.

use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const REPO: &str = "wiracocha-labs/yachay";
const CHECK_EVERY_SECS: u64 = 24 * 60 * 60;
/// El TLS handshake con la API de GitHub tarda ~250 ms y la respuesta total
/// 0.5-1.5 s: con menos de 1 s el chequeo casi nunca pasaría. Solo se paga
/// una vez por día.
const TIMEOUT: Duration = Duration::from_millis(1000);
/// Versión compilada, no la del `Cargo.toml` que esté en disco.
const CURRENT: &str = env!("CARGO_PKG_VERSION");

/// Muestra el aviso si hay una versión más nueva, y refresca el cache si toca.
///
/// Falla en silencio a propósito: sin red, sin cache escribible o con una
/// respuesta rara, el comando del usuario sigue como si nada.
pub fn notify() {
    // Sin terminal no hay a quién avisar (scripts, CI, pipes a archivo).
    if std::env::var_os("YACHAY_NO_UPDATE_CHECK").is_some() || !std::io::stderr().is_terminal() {
        return;
    }
    let Some(path) = cache_path() else {
        return;
    };
    let mut cache = Cache::load(&path);

    if cache.is_stale() {
        if let Some(latest) = fetch_latest_tag() {
            cache.latest = Some(latest);
        }
        // Aunque la consulta falle, marcamos el intento: sin esto, estar
        // offline haría que cada comando espere el timeout.
        cache.checked_at = now();
        cache.save(&path);
    }

    let Some(latest) = cache.latest.clone() else {
        return;
    };
    // Una vez por versión: repetir el aviso en cada corrida sería peor que
    // no avisar.
    if !is_newer(&latest, CURRENT) || cache.notified.as_deref() == Some(latest.as_str()) {
        return;
    }

    print_notice(&latest);
    cache.notified = Some(latest);
    cache.save(&path);
}

fn print_notice(latest: &str) {
    let url = format!("https://github.com/{REPO}/releases/latest/download/yachay-cli-installer.sh");
    eprintln!();
    eprintln!("A newer yachay is available: {latest} (you have {CURRENT}).");
    eprintln!("Update with:");
    eprintln!("  curl --proto '=https' --tlsv1.2 -LsSf {url} | sh");
    eprintln!();
}

/// Pide el tag del último release publicado. `None` ante cualquier problema.
fn fetch_latest_tag() -> Option<String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(TIMEOUT))
        .build()
        .into();
    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let body = agent
        .get(&url)
        .header("User-Agent", "yachay-update-check")
        .call()
        .ok()?
        .body_mut()
        .read_to_string()
        .ok()?;
    let json: serde_json::Value = serde_json::from_str(&body).ok()?;
    let tag = json.get("tag_name")?.as_str()?;
    Some(tag.trim().trim_start_matches('v').to_string())
}

/// Estado del chequeo, guardado como tres líneas de texto plano:
/// `checked_at`, `latest`, `notified`. Vacío = desconocido.
struct Cache {
    checked_at: u64,
    latest: Option<String>,
    notified: Option<String>,
}

impl Cache {
    fn load(path: &Path) -> Cache {
        let text = std::fs::read_to_string(path).unwrap_or_default();
        Cache::parse(&text)
    }

    fn parse(text: &str) -> Cache {
        let mut lines = text.lines();
        let checked_at = lines
            .next()
            .and_then(|l| l.trim().parse().ok())
            .unwrap_or(0);
        let latest = lines.next().map(str::trim).filter(|s| !s.is_empty());
        let notified = lines.next().map(str::trim).filter(|s| !s.is_empty());
        Cache {
            checked_at,
            latest: latest.map(str::to_string),
            notified: notified.map(str::to_string),
        }
    }

    fn encode(&self) -> String {
        format!(
            "{}\n{}\n{}\n",
            self.checked_at,
            self.latest.as_deref().unwrap_or(""),
            self.notified.as_deref().unwrap_or(""),
        )
    }

    fn save(&self, path: &Path) {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(path, self.encode());
    }

    fn is_stale(&self) -> bool {
        now().saturating_sub(self.checked_at) >= CHECK_EVERY_SECS
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// `$XDG_CACHE_HOME/yachay/update-check.txt` en unix, `%LOCALAPPDATA%` en
/// Windows. No usamos `dirs` para no sumar otra dependencia por dos rutas.
fn cache_path() -> Option<PathBuf> {
    let base = if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
    } else {
        std::env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
    }?;
    Some(base.join("yachay").join("update-check.txt"))
}

/// `"v0.2.0"` → `[0, 2, 0]`. `None` si no es un número de tres partes.
fn version_triplet(version: &str) -> Option<[u64; 3]> {
    let version = version.trim().trim_start_matches('v');
    let mut parts = version.split('.');
    let mut out = [0u64; 3];
    for slot in &mut out {
        let part = parts.next()?;
        let digits: String = part.chars().take_while(|c| c.is_ascii_digit()).collect();
        *slot = digits.parse().ok()?;
    }
    Some(out)
}

fn is_newer(latest: &str, current: &str) -> bool {
    match (version_triplet(latest), version_triplet(current)) {
        (Some(latest), Some(current)) => latest > current,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_newer_versions() {
        assert!(is_newer("0.3.0", "0.2.0"));
        assert!(is_newer("v0.10.0", "0.9.9"));
        assert!(!is_newer("0.2.0", "0.2.0"));
        assert!(!is_newer("0.1.9", "0.2.0"));
        // Un tag con basura no debe anunciar una versión falsa.
        assert!(!is_newer("garbage", "0.2.0"));
        assert!(!is_newer("0.3", "0.2.0"));
    }

    #[test]
    fn cache_round_trips_through_text() {
        let cache = Cache {
            checked_at: 1_700_000_000,
            latest: Some("0.3.0".into()),
            notified: None,
        };
        let parsed = Cache::parse(&cache.encode());
        assert_eq!(parsed.checked_at, 1_700_000_000);
        assert_eq!(parsed.latest.as_deref(), Some("0.3.0"));
        assert_eq!(parsed.notified, None);
    }

    #[test]
    fn empty_cache_is_stale() {
        assert!(Cache::parse("").is_stale());
    }
}
