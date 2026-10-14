//! Manager window settings, kept in `NMS.rs\manager.json`. Unknown keys are kept on save so a
//! newer build's settings survive an older one.

use nms_core::paths;
use serde_json::{json, Map, Value};
use std::fs;
use std::path::{Path, PathBuf};

pub const FILE: &str = "manager.json";
/// Text size is a multiple of the default look. 1.0 is shown as 100%.
pub const SCALE_MIN: f32 = 0.7;
pub const SCALE_MAX: f32 = 1.7;
pub const SCALE_DEFAULT: f32 = 1.0;
/// The window zoom that 100% maps to (the old 100% was too small to read).
pub const ZOOM_BASE: f32 = 1.3;
pub const LANGUAGE_DEFAULT: &str = "en";

// Files from the first builds, read once and then removed.
const OLD_LANGUAGE: &str = "manager_language.txt";
const OLD_SCALE: &str = "manager_scale.txt";

#[derive(Debug, Clone, PartialEq)]
pub struct ManagerSettings {
    pub language: String,
    pub text_size: f32,
}

impl Default for ManagerSettings {
    fn default() -> Self {
        Self {
            language: LANGUAGE_DEFAULT.into(),
            text_size: SCALE_DEFAULT,
        }
    }
}

pub fn clamp_scale(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(SCALE_MIN, SCALE_MAX)
    } else {
        SCALE_DEFAULT
    }
}

fn path(root: &Path) -> PathBuf {
    paths::loader_dir(root).join(FILE)
}

fn read_map(root: &Path) -> Map<String, Value> {
    let text = fs::read_to_string(path(root)).unwrap_or_default();
    match serde_json::from_str::<Value>(&text) {
        Ok(Value::Object(map)) => map,
        _ => Map::new(),
    }
}

fn from_map(map: &Map<String, Value>) -> ManagerSettings {
    let mut out = ManagerSettings::default();
    if let Some(code) = map.get("language").and_then(Value::as_str) {
        out.language = code.to_string();
    }
    if let Some(size) = map.get("text_size").and_then(Value::as_f64) {
        out.text_size = clamp_scale(size as f32);
    } else if let Some(zoom) = map.get("text_scale").and_then(Value::as_f64) {
        // Early builds stored the raw zoom as `text_scale`.
        out.text_size = clamp_scale(zoom as f32 / ZOOM_BASE);
    }
    out
}

fn migrate_old(root: &Path) -> Option<ManagerSettings> {
    let dir = paths::loader_dir(root);
    let language = fs::read_to_string(dir.join(OLD_LANGUAGE)).ok();
    let scale = fs::read_to_string(dir.join(OLD_SCALE)).ok();
    if language.is_none() && scale.is_none() {
        return None;
    }
    let mut out = ManagerSettings::default();
    if let Some(text) = language {
        out.language = text.trim().to_string();
    }
    if let Some(text) = scale {
        if let Ok(v) = text.trim().parse::<f32>() {
            out.text_size = clamp_scale(v / ZOOM_BASE);
        }
    }
    save(root, &out);
    let _ = fs::remove_file(dir.join(OLD_LANGUAGE));
    let _ = fs::remove_file(dir.join(OLD_SCALE));
    Some(out)
}

pub fn load(root: &Path) -> ManagerSettings {
    if !path(root).is_file() {
        if let Some(migrated) = migrate_old(root) {
            return migrated;
        }
        // Version 3 kept the language in `NMS.py\manager.json`: use it (read only) until NMS.rs
        // has its own file, so the first window and the migrate panel speak the user's language.
        let old = fs::read_to_string(root.join("NMS.py").join(FILE)).unwrap_or_default();
        if let Ok(Value::Object(map)) = serde_json::from_str::<Value>(&old) {
            if let Some(code) = map.get("language").and_then(Value::as_str) {
                return ManagerSettings { language: code.to_string(), ..ManagerSettings::default() };
            }
        }
    }
    from_map(&read_map(root))
}

pub fn save(root: &Path, settings: &ManagerSettings) {
    let mut map = read_map(root);
    map.insert("language".into(), json!(settings.language));
    map.remove("text_scale");
    map.insert("text_size".into(), json!((settings.text_size * 100.0).round() / 100.0));
    let dir = paths::loader_dir(root);
    let _ = fs::create_dir_all(&dir);
    if let Ok(text) = serde_json::to_string_pretty(&Value::Object(map)) {
        let _ = fs::write(path(root), text + "\n");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static N: AtomicU32 = AtomicU32::new(0);

    fn temp_root(tag: &str) -> PathBuf {
        let n = N.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("nms-rs-mset-{tag}-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(paths::loader_dir(&dir)).unwrap();
        dir
    }

    #[test]
    fn defaults_when_missing_or_broken() {
        let root = temp_root("def");
        assert_eq!(load(&root), ManagerSettings::default());
        fs::write(path(&root), "not json").unwrap();
        assert_eq!(load(&root), ManagerSettings::default());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn scale_is_clamped_and_round_trips() {
        let root = temp_root("rt");
        fs::write(path(&root), "{\"text_size\": 9, \"language\": \"de\"}").unwrap();
        let got = load(&root);
        assert_eq!(got.text_size, SCALE_MAX);
        assert_eq!(got.language, "de");
        save(&root, &ManagerSettings { language: "fr".into(), text_size: 1.6 });
        assert_eq!(load(&root), ManagerSettings { language: "fr".into(), text_size: 1.6 });
        assert_eq!(clamp_scale(f32::NAN), SCALE_DEFAULT);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn early_text_scale_key_becomes_text_size() {
        let root = temp_root("early");
        fs::write(path(&root), "{\"text_scale\": 1.3}").unwrap();
        assert_eq!(load(&root).text_size, 1.0);
        save(&root, &load(&root));
        let text = fs::read_to_string(path(&root)).unwrap();
        assert!(!text.contains("text_scale") && text.contains("text_size"), "{text}");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn unknown_keys_survive_a_save() {
        let root = temp_root("keep");
        fs::write(path(&root), "{\"future_option\": [1, 2]}").unwrap();
        save(&root, &ManagerSettings::default());
        let text = fs::read_to_string(path(&root)).unwrap();
        assert!(text.contains("future_option"), "{text}");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn nms_py_language_is_used_until_nms_rs_has_its_own() {
        let root = temp_root("nmspy");
        fs::create_dir_all(root.join("NMS.py")).unwrap();
        fs::write(root.join("NMS.py").join(FILE), "{\"language\": \"ko\"}").unwrap();
        assert_eq!(load(&root).language, "ko");
        assert!(!path(&root).exists(), "read only");
        save(&root, &ManagerSettings { language: "fr".into(), text_size: 1.0 });
        assert_eq!(load(&root).language, "fr");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn old_txt_files_migrate_once() {
        let root = temp_root("mig");
        let dir = paths::loader_dir(&root);
        fs::write(dir.join(OLD_LANGUAGE), "ja").unwrap();
        fs::write(dir.join(OLD_SCALE), "1.50").unwrap();
        let got = load(&root);
        assert_eq!(got.language, "ja");
        assert!((got.text_size - 1.5 / ZOOM_BASE).abs() < 0.001);
        assert!(path(&root).is_file());
        assert!(!dir.join(OLD_LANGUAGE).exists());
        assert!(!dir.join(OLD_SCALE).exists());
        let again = load(&root);
        assert_eq!(again.language, got.language);
        assert!((again.text_size - got.text_size).abs() < 0.01);
        let _ = fs::remove_dir_all(&root);
    }
}
