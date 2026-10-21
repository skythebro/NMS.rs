//! The mods ported from Python. Each one is a `GameMod` registered at game start.
//!
//! The release build aborts on panic, so code reached from a detour must never panic.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use nms_core::log::Log;
use nms_core::process::ProcessImage;
use nms_core::{paths, GameMod, Kind, Registry, SettingSpec};
use serde_json::{json, Value};

/// What every mod gets at start.
pub struct Ctx {
    pub root: PathBuf,
    pub image: ProcessImage,
    /// The core log (`NMS.rs\logs\core.log`). Mods write their own log next to their settings.
    pub log: Arc<Log>,
}

pub mod bigger_item_grids;
mod common;
mod craft_from_storage;
mod galaxy_map_filters;
mod hide_empty_hover;
mod picker_tweaks;
mod quick_recharge;
mod quick_transfer_all;
mod refiner_guard;
mod smaller_inventory_slots;
mod sphere_sized_mining;
mod tool_mode_binds;
mod trade_ui_tweaks;

type Make = fn(&Ctx) -> Box<dyn GameMod>;

/// (folder under GAMEDATA\MODS, Python file, constructor)
const MODS: &[(&str, &str, Make)] = &[
    ("SphereSizedMining", "sphere_sized_mining.py", sphere_sized_mining::register),
    ("HideEmptyHover", "hide_empty_hover.py", hide_empty_hover::register),
    ("BiggerItemGrids", "bigger_item_grids.py", bigger_item_grids::register),
    ("QuickRecharge", "quick_recharge.py", quick_recharge::register),
    ("TradeUITweaks", "trade_ui_tweaks.py", trade_ui_tweaks::register),
    ("CraftFromStorage", "craft_from_storage.py", craft_from_storage::register),
    ("PickerTweaks", "picker_tweaks.py", picker_tweaks::register),
    ("RefinerGuard", "refiner_guard.py", refiner_guard::register),
    ("ToolModeBinds", "tool_mode_binds.py", tool_mode_binds::register),
    ("QuickTransferAll", "quick_transfer_all.py", quick_transfer_all::register),
    ("SmallerInventorySlots", "smaller_inventory_slots.py", smaller_inventory_slots::register),
    ("GalaxyMapFilters", "galaxy_map_filters.py", galaxy_map_filters::register),
];

/// Add the in-game Mods page to the game's Options menu once every mod is registered.
pub fn install_options_menu(image: &ProcessImage, registry: Arc<Registry>, log: Arc<Log>) {
    common::options_menu::install(image, registry, log);
}

pub fn register(registry: &mut Registry, ctx: &Ctx) {
    for (name, py, make) in MODS {
        if python_copy(&ctx.root, name, py) {
            registry.add(Box::new(Disabled::new(name)));
        } else if !is_installed(&ctx.root, name) {
            registry.add(Box::new(Disabled::parked(name)));
        } else {
            registry.add(make(ctx));
        }
    }
}

/// The mod's folder in `GAMEDATA\MODS` is the on/off switch (the manager's Install and Park move
/// it). A parked or never installed mod must not hook anything, and must not write its settings:
/// saving them would recreate the folder and the manager would show the mod as on again.
pub fn is_installed(root: &Path, mod_name: &str) -> bool {
    paths::mod_dir(root, mod_name).is_dir()
}

/// True while the Python copy of a mod is installed: both loaders must never hook the same function.
pub fn python_copy(root: &Path, mod_name: &str, py_file: &str) -> bool {
    paths::mod_dir(root, mod_name).join(py_file).is_file()
}

/// Stands in for a ported mod whose Python copy is still installed.
pub struct Disabled {
    name: &'static str,
    reason: &'static str,
}

impl Disabled {
    pub fn new(name: &'static str) -> Self {
        Self { name, reason: "Off: Python copy present in GAMEDATA\\MODS" }
    }

    /// The mod's folder is not in `GAMEDATA\MODS` (parked or not installed).
    pub fn parked(name: &'static str) -> Self {
        Self { name, reason: "Off: parked (not in GAMEDATA\\MODS)" }
    }
}

const DISABLED_SPECS: &[SettingSpec] = &[SettingSpec {
    key: "status",
    label: "Status",
    kind: Kind::Status,
}];

impl GameMod for Disabled {
    fn name(&self) -> &'static str {
        self.name
    }

    fn specs(&self) -> &'static [SettingSpec] {
        DISABLED_SPECS
    }

    fn get(&self, key: &str) -> Option<Value> {
        (key == "status").then(|| json!(self.reason))
    }

    fn set(&self, key: &str, _value: &Value) -> Result<String, String> {
        Err(format!("{} is off ({key} not changed): {}", self.name, self.reason))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn python_copy_follows_the_file() {
        let root = std::env::temp_dir().join(format!("nms-mods-test-{}", std::process::id()));
        let dir = paths::mod_dir(&root, "SphereSizedMining");
        std::fs::create_dir_all(&dir).unwrap();
        assert!(!python_copy(
            &root,
            "SphereSizedMining",
            "sphere_sized_mining.py"
        ));
        std::fs::write(dir.join("sphere_sized_mining.py"), b"").unwrap();
        assert!(python_copy(
            &root,
            "SphereSizedMining",
            "sphere_sized_mining.py"
        ));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn disabled_is_read_only() {
        let d = Disabled::new("X");
        assert!(d
            .get("status")
            .unwrap()
            .as_str()
            .unwrap()
            .contains("Python copy"));
        assert!(d.set("write_logs", &json!(true)).is_err());
    }

    #[test]
    fn parked_mod_is_off_and_creates_no_folder() {
        let root = std::env::temp_dir().join(format!("nms-mods-parked-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        assert!(!is_installed(&root, "RefinerGuard"));
        let d = Disabled::parked("RefinerGuard");
        assert!(d.get("status").unwrap().as_str().unwrap().contains("parked"));
        assert!(d.set("write_logs", &json!(true)).is_err());
        assert!(!paths::mod_dir(&root, "RefinerGuard").exists());
        std::fs::create_dir_all(paths::mod_dir(&root, "RefinerGuard")).unwrap();
        assert!(is_installed(&root, "RefinerGuard"));
        std::fs::remove_dir_all(&root).unwrap();
    }
}
