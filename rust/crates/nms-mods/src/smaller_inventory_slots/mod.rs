//! Smaller inventory slots and extra filter buttons (ported from Python).

mod cave;
mod layout;
mod patches;
mod runtime;
mod ui;

use std::sync::{Arc, Mutex, OnceLock};

use nms_core::log::Log;
use nms_core::process::ProcessImage;
use nms_core::{paths, GameMod, Kind, Range, SettingSpec, Settings};
use nms_sdk::sigs::smaller_inventory_slots as sigs;
use serde_json::{json, Value};

use crate::common::custom_filter;
use crate::common::hooks::{do_inventory_slots, inventory_sort};
use layout::{
    preset_filters_from_file, preset_plan_from_file, REMEMBER_FREIGHTER_TAB, REMEMBER_TAB_FILTER, REMEMBER_TAB_SCROLL,
    SHRINK_CHEST_GRIDS, SHRINK_FREIGHTER_GRIDS, CATEGORIES, VANILLA_FILTERS,
};
use patches::{field_in_range, filter_field_offset, install_filters};
use runtime::SlotsRuntime;

static RUNTIME: OnceLock<Arc<Mutex<SlotsRuntime>>> = OnceLock::new();

const SPECS: &[SettingSpec] = &[
    SettingSpec {
        key: "status",
        label: "Status",
        kind: Kind::Status,
    },
    SettingSpec {
        key: "slots",
        label: "Slots",
        kind: Kind::Status,
    },
    SettingSpec {
        key: "filters",
        label: "Filter buttons",
        kind: Kind::Status,
    },
    SettingSpec {
        key: "remember_filter",
        label: "Keep filter per tab",
        kind: Kind::Bool,
    },
    SettingSpec {
        key: "remember_scroll",
        label: "Keep scroll per tab",
        kind: Kind::Bool,
    },
    SettingSpec {
        key: "remember_freighter",
        label: "Keep freighter tab",
        kind: Kind::Bool,
    },
    SettingSpec {
        key: "sort_storage",
        label: "Sort storage",
        kind: Kind::Bool,
    },
    SettingSpec {
        key: "filter_foods",
        label: "Foods filter",
        kind: Kind::Bool,
    },
    SettingSpec {
        key: "filter_custom",
        label: "Custom filter",
        kind: Kind::Bool,
    },
    SettingSpec {
        key: "custom_mark",
        label: "Custom marks",
        kind: Kind::Enum,
    },
    SettingSpec {
        key: "custom_remember",
        label: "Keep Custom marks",
        kind: Kind::Bool,
    },
    SettingSpec {
        key: "write_logs",
        label: "Write log",
        kind: Kind::Bool,
    },
];

/// Choices of `custom_mark`. Index 0 is the default: per slot, so a click lights and moves exactly
/// the stack that was clicked (in-game test 2026-10-13: one mark lighting three stacks surprised).
const MARK_MODES: &[(&str, &str)] = &[("slot", "Clicked stack"), ("item", "Every stack of the item")];

/// `custom_mark` value to slot mode: `Some(true)` per slot, `Some(false)` per item.
fn parse_mark_mode(value: &Value) -> Option<bool> {
    match value.as_str()?.trim().to_ascii_lowercase().as_str() {
        "slot" => Some(true),
        "item" => Some(false),
        _ => None,
    }
}

fn mark_mode_key(slot: bool) -> &'static str {
    if slot {
        "slot"
    } else {
        "item"
    }
}

pub struct SmallerInventorySlots {
    settings: Arc<Mutex<Settings>>,
    status: Arc<Mutex<String>>,
    slots: Arc<Mutex<String>>,
    filters: Arc<Mutex<String>>,
    log: Arc<Log>,
}

impl SmallerInventorySlots {
    pub fn new(ctx: &super::Ctx) -> Self {
        let dir = paths::mod_dir(&ctx.root, "SmallerInventorySlots");
        let defaults = json!({
            "write_logs": false,
            "remember_filter": true,
            "remember_scroll": true,
            "remember_freighter": true,
            "sort_storage": true,
            "filter_foods": false,
            "filter_custom": false,
            "custom_mark": "slot",
            "custom_remember": false,
        });
        let settings = Arc::new(Mutex::new(Settings::load(
            &dir.join("smaller_inventory_slots.json"),
            defaults,
        )));
        let write_logs = settings
            .lock()
            .map(|s| s.is_not_false("write_logs"))
            .unwrap_or(false);
        let log = Arc::new(Log::open(&dir.join("smaller_inventory_slots.log"), write_logs));
        let probe = dir.join("probe.flag").is_file();
        if probe && !write_logs {
            log.set_enabled(true);
        }
        if probe {
            match filter_field_offset(&ctx.image) {
                Some(field) => log.info(&format!("Probe: on, filter_field {field:+#x}")),
                None => log.info("Probe: on, filter_field unknown"),
            }
        } else {
            log.info("Probe: off");
        }

        let mut runtime = SlotsRuntime::new(log.clone());
        runtime.probe = probe;
        runtime.filter_field = filter_field_offset(&ctx.image);
        let on = |key: &str| {
            settings
                .lock()
                .map(|s| s.is_not_false(key))
                .unwrap_or(true)
        };
        runtime.remember_filter = REMEMBER_TAB_FILTER && on("remember_filter");
        runtime.remember_scroll = REMEMBER_TAB_SCROLL && on("remember_scroll");
        runtime.remember_freighter = REMEMBER_FREIGHTER_TAB && on("remember_freighter");
        runtime.sort_storage = on("sort_storage");
        let is_true = |key: &str| settings.lock().map(|s| s.is_true(key)).unwrap_or(false);
        runtime.filter_foods = is_true("filter_foods");
        runtime.filter_custom = is_true("filter_custom");
        runtime.custom_remember = is_true("custom_remember");
        let slot_mode = settings
            .lock()
            .ok()
            .and_then(|s| s.get("custom_mark").and_then(parse_mark_mode))
            .unwrap_or(true);
        custom_filter::set_slot_mode(slot_mode);
        log.info(&format!("Custom marks: {}", MARK_MODES[usize::from(!slot_mode)].1));
        runtime.shrink_chest = SHRINK_CHEST_GRIDS;
        runtime.shrink_freighter = SHRINK_FREIGHTER_GRIDS;

        match preset_plan_from_file(&dir) {
            Ok(plan) => {
                runtime.status = "On".into();
                runtime.plan = plan;
                if let Some(plan) = plan {
                    runtime.slots = format!(
                        "{:.0}%, {}+{} rows",
                        plan.scale * 100.0,
                        plan.tech_rows,
                        plan.cargo_rows
                    );
                    log.info(&format!("Inventory slots: {}", plan.describe()));
                } else {
                    runtime.slots = "100% (vanilla)".into();
                    log.info("Inventory slots: vanilla size, filter buttons only");
                }
            }
            Err(error) => {
                log.info(&format!("Inventory slots left vanilla: {error}"));
            }
        }

        match preset_filters_from_file(&dir) {
            Ok(true) => {
                let mut live = ctx.image.clone();
                match install_filters(&mut live, &log) {
                    Ok(installed) => {
                        runtime.set_filter_hover(installed.hover);
                        runtime.set_extra_caves(installed.foods_on, installed.custom, installed.hide_on);
                        runtime.filters = "On".into();
                        log.info(&format!(
                            "Filters: side buttons FILTER6-FILTER{} are live",
                            VANILLA_FILTERS as u32 + CATEGORIES as u32
                        ));
                    }
                    Err(error) => {
                        log.info(&format!(
                            "Filters are off, the extra side buttons do nothing: {error}"
                        ));
                    }
                }
            }
            Ok(false) => {
                runtime.filters = "Off".into();
                log.info("Filters: extra buttons were left out of these UI files");
            }
            Err(error) => {
                log.info(&format!(
                    "Filters are off, the extra side buttons do nothing: {error}"
                ));
            }
        }

        if runtime.remember_filter {
            match runtime.filter_field {
                Some(field) if field_in_range(field) => {}
                None => {
                    runtime.remember_filter = false;
                    log.info("Filter memory: filter_field unknown, leaving the game's filter alone");
                }
                Some(field) => {
                    runtime.remember_filter = false;
                    log.info(&format!(
                        "Filter memory: filter_field {field:+#x} is outside the range this mod will write"
                    ));
                }
            }
        }
        log.info(&format!(
            "Storage sort: {}",
            if runtime.sort_storage { "on" } else { "off" }
        ));
        runtime.apply_extra_switches();
        log.info(&extra_line("Foods filter", runtime.filter_foods, runtime.foods_ready()));
        log.info(&extra_line("Custom filter", runtime.filter_custom, runtime.custom_ready()));
        log.info(&format!(
            "Features: filter memory {}, scroll memory {}, chest grid {}, freighter grid {}, storage sort {}",
            on_off(runtime.remember_filter),
            on_off(runtime.remember_scroll),
            on_off(runtime.shrink_chest),
            on_off(runtime.shrink_freighter),
            on_off(runtime.sort_storage),
        ));

        let status = Arc::new(Mutex::new(runtime.status.clone()));
        let slots = Arc::new(Mutex::new(runtime.slots.clone()));
        let filters = Arc::new(Mutex::new(runtime.filters.clone()));
        let rt = Arc::new(Mutex::new(runtime));
        let _ = RUNTIME.set(rt);

        install_hooks(&ctx.image, &log);
        Self {
            settings,
            status,
            slots,
            filters,
            log,
        }
    }
}

fn on_off(v: bool) -> &'static str {
    if v {
        "on"
    } else {
        "off"
    }
}

fn extra_line(name: &str, setting: bool, ready: bool) -> String {
    match (setting, ready) {
        (true, true) => format!("{name}: on"),
        (true, false) => format!("{name}: off, its filter test is not installed (see above)"),
        (false, _) => format!("{name}: off"),
    }
}

fn install_hooks(image: &ProcessImage, log: &Log) {
    if let Err(e) = do_inventory_slots::subscribe(
        image,
        sigs::NMSPY_DO_INVENTORY_SLOTS,
        Some(before_slots),
        Some(after_slots),
    ) {
        log.info(&format!("Inventory slots: {e}"));
    }
    if let Err(e) = inventory_sort::subscribe(image, sigs::SORT, Some(before_sort), None) {
        log.info(&format!("Storage sort: {e}"));
    }
}

fn before_slots(args: &mut do_inventory_slots::Args) {
    let Some(rt) = RUNTIME.get() else { return };
    let Ok(mut state) = rt.try_lock() else { return };
    let out = state.before_slots(
        args.page,
        args.layer,
        args.inventory,
        args.slot_width,
        args.slot_height,
        args.render_height,
        args.scroll_offset,
    );
    args.slot_width = out.slot_width;
    args.slot_height = out.slot_height;
    args.render_height = out.render_height;
    args.scroll_offset = out.scroll;
}

fn after_slots(args: &do_inventory_slots::Args, _ret: u64) {
    if let Some(Ok(mut state)) = RUNTIME.get().map(|rt| rt.try_lock()) {
        state.after_slots(args.page);
    }
}

fn before_sort(args: &mut inventory_sort::Args) {
    if let Some(Ok(mut state)) = RUNTIME.get().map(|rt| rt.try_lock()) {
        args.inventory = state.before_sort(args.inventory, args.order);
    }
}

impl GameMod for SmallerInventorySlots {
    fn name(&self) -> &'static str {
        "SmallerInventorySlots"
    }

    fn specs(&self) -> &'static [SettingSpec] {
        SPECS
    }

    fn get(&self, key: &str) -> Option<Value> {
        match key {
            "status" => Some(json!(self.status.lock().ok()?.clone())),
            "slots" => Some(json!(self.slots.lock().ok()?.clone())),
            "filters" => Some(json!(self.filters.lock().ok()?.clone())),
            "write_logs" => Some(json!(self.settings.lock().ok()?.is_not_false("write_logs"))),
            "remember_filter" => Some(json!(RUNTIME.get()?.lock().ok()?.remember_filter)),
            "remember_scroll" => Some(json!(RUNTIME.get()?.lock().ok()?.remember_scroll)),
            "remember_freighter" => Some(json!(RUNTIME.get()?.lock().ok()?.remember_freighter)),
            "sort_storage" => Some(json!(RUNTIME.get()?.lock().ok()?.sort_storage)),
            "filter_foods" => Some(json!(RUNTIME.get()?.lock().ok()?.filter_foods)),
            "filter_custom" => Some(json!(RUNTIME.get()?.lock().ok()?.filter_custom)),
            "custom_remember" => Some(json!(RUNTIME.get()?.lock().ok()?.custom_remember)),
            "custom_mark" => Some(json!(mark_mode_key(custom_filter::slot_mode()))),
            _ => None,
        }
    }

    fn range(&self, key: &str) -> Option<Range> {
        (key == "custom_mark").then_some(Range::Enum { choices: MARK_MODES, default: 0 })
    }

    fn default_value(&self, key: &str) -> Option<Value> {
        match key {
            "status" | "slots" | "filters" => None,
            _ => self.settings.lock().ok()?.default(key).cloned(),
        }
    }

    fn set(&self, key: &str, value: &Value) -> Result<String, String> {
        if key == "custom_mark" {
            let slot = parse_mark_mode(value).ok_or_else(|| "custom_mark must be slot or item".to_string())?;
            self.settings
                .lock()
                .map_err(|_| "settings lock poisoned")?
                .set(key, json!(mark_mode_key(slot)))?;
            let dropped = custom_filter::set_slot_mode(slot);
            let mut line = format!("Custom marks: {}", MARK_MODES[usize::from(!slot)].1);
            if dropped > 0 {
                line.push_str(&format!(", {dropped} mark(s) cleared"));
            }
            self.log.info(&line);
            return Ok(line);
        }
        let on = value
            .as_bool()
            .ok_or_else(|| "expected true or false".to_string())?;
        match key {
            "write_logs" => {
                self.settings
                    .lock()
                    .map_err(|_| "settings lock poisoned")?
                    .set(key, json!(on))?;
                self.log.set_enabled(on);
                Ok(format!("Write log {}", on_off(on)))
            }
            "remember_filter" | "remember_scroll" | "remember_freighter" | "sort_storage" => {
                let mut apply = on;
                if key == "remember_filter" {
                    apply = on
                        && RUNTIME
                            .get()
                            .and_then(|rt| rt.lock().ok())
                            .map(|s| s.filter_field.is_some())
                            .unwrap_or(false);
                }
                self.settings
                    .lock()
                    .map_err(|_| "settings lock poisoned")?
                    .set(key, json!(apply))?;
                if let Some(rt) = RUNTIME.get() {
                    if let Ok(mut state) = rt.lock() {
                        match key {
                            "remember_filter" => state.remember_filter = apply,
                            "remember_scroll" => state.remember_scroll = apply,
                            "remember_freighter" => state.remember_freighter = apply,
                            "sort_storage" => state.sort_storage = apply,
                            _ => {}
                        }
                    }
                }
                self.log.info(&format!("{key}: {}", on_off(apply)));
                Ok(format!("{key} {}", on_off(apply)))
            }
            "filter_foods" | "filter_custom" => {
                self.settings
                    .lock()
                    .map_err(|_| "settings lock poisoned")?
                    .set(key, json!(on))?;
                let mut line = format!("{key}: {}", on_off(on));
                if let Some(rt) = RUNTIME.get() {
                    if let Ok(mut state) = rt.lock() {
                        if key == "filter_foods" {
                            state.filter_foods = on;
                            line = extra_line("Foods filter", on, state.foods_ready());
                        } else {
                            state.filter_custom = on;
                            line = extra_line("Custom filter", on, state.custom_ready());
                            if !on {
                                state.custom_switched_off();
                            }
                        }
                        state.apply_extra_switches();
                    }
                }
                self.log.info(&line);
                Ok(line)
            }
            "custom_remember" => {
                self.settings
                    .lock()
                    .map_err(|_| "settings lock poisoned")?
                    .set(key, json!(on))?;
                if let Some(rt) = RUNTIME.get() {
                    if let Ok(mut state) = rt.lock() {
                        state.custom_remember = on;
                    }
                }
                let line = format!("Keep Custom marks: {}", on_off(on));
                self.log.info(&line);
                Ok(line)
            }
            _ => Err(format!("unknown setting {key:?}")),
        }
    }
}

pub fn register(ctx: &super::Ctx) -> Box<dyn GameMod> {
    Box::new(SmallerInventorySlots::new(ctx))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mark_mode_setting() {
        assert_eq!(MARK_MODES[0].0, "slot", "per slot is the default (index 0)");
        assert_eq!(parse_mark_mode(&json!("slot")), Some(true));
        assert_eq!(parse_mark_mode(&json!(" Item ")), Some(false));
        assert_eq!(parse_mark_mode(&json!("stack")), None);
        assert_eq!(parse_mark_mode(&json!(true)), None);
        for slot in [true, false] {
            assert_eq!(parse_mark_mode(&json!(mark_mode_key(slot))), Some(slot));
            assert_eq!(MARK_MODES[usize::from(!slot)].0, mark_mode_key(slot));
        }
        assert!(SPECS.iter().any(|s| s.key == "custom_mark" && s.kind == Kind::Enum));
    }
}
