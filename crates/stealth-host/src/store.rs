//! The player's files: `config/<name>` in the user's config directory and `data/<name>` in the
//! data directory (each under `stealth-mission-89/`), or the browser's localStorage in the
//! browser build. Keys use `/` as separator, e.g. `data/campaigns/ce-1.json`.

use anyhow::Result;

/// The contents of `key`, None if there is none.
pub fn read(key: &str) -> Result<Option<String>> {
    imp::read(key)
}

/// Stores `text` under `key` (a crash mid-write keeps the previous contents).
pub fn write(key: &str, text: &str) -> Result<()> {
    imp::write(key, text)
}

/// Removes `key`; one that is already gone is no error.
pub fn remove(key: &str) -> Result<()> {
    imp::remove(key)
}

/// The names in directory `dir` (`data/pilots`), each with when it was last written (seconds
/// since the Unix epoch, 0 if unknown).
pub fn list(dir: &str) -> Vec<(String, f64)> {
    imp::list(dir)
}

#[cfg(not(target_arch = "wasm32"))]
pub use imp::path;

#[cfg(not(target_arch = "wasm32"))]
mod imp {
    use std::path::PathBuf;

    use anyhow::{Context, Result};

    /// Where `key` is on disk; None without a config or data directory.
    pub fn path(key: &str) -> Option<PathBuf> {
        let (base, rest) = key.split_once('/')?;
        let dir = match base {
            "config" => dirs::config_dir(),
            "data" => dirs::data_dir(),
            _ => None,
        }?;
        Some(rest.split('/').fold(dir.join("stealth-mission-89"), |p, s| p.join(s)))
    }

    fn need_path(key: &str) -> Result<PathBuf> {
        path(key).with_context(|| format!("{key}: no user config or data directory"))
    }

    pub fn read(key: &str) -> Result<Option<String>> {
        let path = need_path(key)?;
        match std::fs::read_to_string(&path) {
            Ok(t) => Ok(Some(t)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e).with_context(|| path.display().to_string()),
        }
    }

    pub fn write(key: &str, text: &str) -> Result<()> {
        let path = need_path(key)?;
        if let Some(d) = path.parent() {
            std::fs::create_dir_all(d)?;
        }
        let mut tmp = path.as_os_str().to_owned();
        tmp.push(".tmp");
        std::fs::write(&tmp, text).with_context(|| path.display().to_string())?;
        std::fs::rename(&tmp, &path).with_context(|| path.display().to_string())
    }

    pub fn remove(key: &str) -> Result<()> {
        let path = need_path(key)?;
        match std::fs::remove_file(&path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                Err(e).with_context(|| path.display().to_string())
            }
            _ => Ok(()),
        }
    }

    pub fn list(dir: &str) -> Vec<(String, f64)> {
        let Some(Ok(entries)) = path(dir).map(std::fs::read_dir) else { return Vec::new() };
        entries
            .flatten()
            .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
            .filter_map(|e| {
                let name = e.file_name().into_string().ok()?;
                let modified = e
                    .metadata()
                    .and_then(|m| m.modified())
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map_or(0.0, |d| d.as_secs_f64());
                Some((name, modified))
            })
            .collect()
    }
}

#[cfg(target_arch = "wasm32")]
mod imp {
    use anyhow::{Result, anyhow};
    use web_sys::Storage;

    /// The contents are under `sm89/<key>`, when they were written under `sm89-time/<key>`.
    const DATA: &str = "sm89/";
    const TIME: &str = "sm89-time/";

    fn storage() -> Result<Storage> {
        web_sys::window()
            .and_then(|w| w.local_storage().ok().flatten())
            .ok_or_else(|| anyhow!("the browser keeps no local storage"))
    }

    fn js(e: web_sys::wasm_bindgen::JsValue) -> anyhow::Error {
        anyhow!("local storage: {e:?}")
    }

    pub fn read(key: &str) -> Result<Option<String>> {
        storage()?.get_item(&format!("{DATA}{key}")).map_err(js)
    }

    pub fn write(key: &str, text: &str) -> Result<()> {
        let s = storage()?;
        // Full (the quota is a few MB): the previous contents stay.
        s.set_item(&format!("{DATA}{key}"), text).map_err(js)?;
        let _ = s.set_item(&format!("{TIME}{key}"), &(js_sys::Date::now() / 1000.0).to_string());
        Ok(())
    }

    pub fn remove(key: &str) -> Result<()> {
        let s = storage()?;
        s.remove_item(&format!("{DATA}{key}")).map_err(js)?;
        s.remove_item(&format!("{TIME}{key}")).map_err(js)
    }

    pub fn list(dir: &str) -> Vec<(String, f64)> {
        let Ok(s) = storage() else { return Vec::new() };
        let prefix = format!("{DATA}{}/", dir.trim_end_matches('/'));
        let n = s.length().unwrap_or(0);
        (0..n)
            .filter_map(|i| s.key(i).ok().flatten())
            .filter_map(|k| {
                let name = k.strip_prefix(&prefix)?;
                if name.contains('/') {
                    return None;
                }
                let key = &k[DATA.len()..];
                let time = s
                    .get_item(&format!("{TIME}{key}"))
                    .ok()
                    .flatten()
                    .and_then(|t| t.parse().ok())
                    .unwrap_or(0.0);
                Some((name.to_string(), time))
            })
            .collect()
    }
}
