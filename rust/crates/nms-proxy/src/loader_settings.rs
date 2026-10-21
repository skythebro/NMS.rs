//! The loader's own settings, shown as the "NMS.rs" section of the in-game Mods page and stored in
//! `<game>\NMS.rs\loader.json`.
//!
//! - `hide_mod_warning` (default on): skip the boot "mods are installed" warning screen. The game
//!   decides once, after the splash logos, so the patch goes in at boot before anything else; a
//!   change on the Mods page is applied at once but only shows at the next game start.

use std::path::Path;
use std::sync::{Arc, Mutex};

use nms_core::log::Log;
use nms_core::process::ProcessImage;
use nms_core::{GameMod, Kind, SettingSpec, Settings};
use nms_sdk::sigs::loader::MOD_WARNING;
use serde_json::{json, Value};

pub const NAME: &str = "NMS.rs";

const SPECS: &[SettingSpec] = &[SettingSpec { key: "hide_mod_warning", label: "Hide mod warning", kind: Kind::Bool }];

fn load(loader: &Path) -> Settings {
    Settings::load(&loader.join("loader.json"), json!({ "hide_mod_warning": true }))
}

/// Apply or undo the mod-warning patch and log the outcome.
fn set_mod_warning(image: &ProcessImage, hide: bool, log: &Log) {
    let mut code = image.clone();
    let result = MOD_WARNING.set(&mut code, hide);
    log.info(&format!("loader: {}", result.message));
}

/// First thing after the game image is read: the warning check runs a few seconds later.
pub fn apply_at_boot(image: &ProcessImage, loader: &Path, log: &Log) {
    if load(loader).is_true("hide_mod_warning") {
        set_mod_warning(image, true, log);
    }
}

pub struct LoaderSettings {
    settings: Mutex<Settings>,
    image: ProcessImage,
    log: Arc<Log>,
}

impl LoaderSettings {
    pub fn new(loader: &Path, image: ProcessImage, log: Arc<Log>) -> Self {
        Self { settings: Mutex::new(load(loader)), image, log }
    }
}

impl GameMod for LoaderSettings {
    fn name(&self) -> &'static str {
        NAME
    }

    fn specs(&self) -> &'static [SettingSpec] {
        SPECS
    }

    fn get(&self, key: &str) -> Option<Value> {
        match key {
            "hide_mod_warning" => Some(json!(self.settings.lock().ok()?.is_true(key))),
            _ => None,
        }
    }

    fn default_value(&self, key: &str) -> Option<Value> {
        match key {
            "hide_mod_warning" => self.settings.lock().ok()?.default(key).cloned(),
            _ => None,
        }
    }

    fn set(&self, key: &str, value: &Value) -> Result<String, String> {
        let on = value.as_bool().ok_or_else(|| "expected true or false".to_string())?;
        match key {
            "hide_mod_warning" => {
                self.settings.lock().map_err(|_| "settings lock poisoned")?.set(key, json!(on))?;
                set_mod_warning(&self.image, on, &self.log);
                Ok(format!("Hide mod warning {} (shows at the next game start)", if on { "on" } else { "off" }))
            }
            _ => Err(format!("unknown setting {key:?}")),
        }
    }
}
