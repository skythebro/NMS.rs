//! Core self-test for the in-game gate before any mod is ported.
//!
//! Two switches, both off by default:
//! - "Refiner: storage tabs": the CraftFromStorage byte patch, so the live patch and undo path can
//!   be seen in game (the Refiner transfer popup gains the Storage tab).
//! - "Log terrain hook": a log-only detour on ApplyTerrainEditStroke (the SphereSizedMining hook),
//!   proving hooks with the original called through the trampoline.

use std::ffi::c_void;
use std::path::Path;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use nms_core::hook::Hook;
use nms_core::log::Log;
use nms_core::process::ProcessImage;
use nms_core::{Code, GameMod, Kind, Pattern, Range, SettingSpec, Settings};
use nms_sdk::sigs::{craft_from_storage, sphere_sized_mining};
use serde_json::{json, Value};

type StrokeFn = unsafe extern "system" fn(*mut c_void, u64, *mut c_void) -> i64;

static STROKE_ORIG: AtomicUsize = AtomicUsize::new(0);
static STROKE_CALLS: AtomicU64 = AtomicU64::new(0);
static STROKE_LOG: OnceLock<Arc<Log>> = OnceLock::new();

/// cGcTerrainEditorBeam::ApplyTerrainEditStroke(this, sTerrainEditData (1 byte), impact) -> i64.
unsafe extern "system" fn stroke_detour(this: *mut c_void, edit: u64, impact: *mut c_void) -> i64 {
    if STROKE_CALLS.fetch_add(1, Ordering::Relaxed) == 0 {
        if let Some(log) = STROKE_LOG.get() {
            log.info(&format!("terrain hook: first stroke seen (edit byte {:#04x})", edit & 0xFF));
        }
    }
    // SAFETY: set from the hook's trampoline before the hook is enabled; same signature.
    let original: StrokeFn = std::mem::transmute(STROKE_ORIG.load(Ordering::Acquire));
    original(this, edit, impact)
}

const SPECS: &[SettingSpec] = &[
    SettingSpec { key: "status", label: "Status", kind: Kind::Status },
    SettingSpec { key: "refiner_tab", label: "Refiner: storage tabs", kind: Kind::Bool },
    SettingSpec { key: "stroke_log", label: "Log terrain hook", kind: Kind::Bool },
    SettingSpec { key: "test_int", label: "Test: whole number", kind: Kind::Int },
    SettingSpec { key: "test_float", label: "Test: decimal number", kind: Kind::Float },
    SettingSpec { key: "test_choice", label: "Test: choice", kind: Kind::Enum },
];

const CHOICES: &[(&str, &str)] = &[("low", "Low"), ("medium", "Medium"), ("high", "High")];

pub struct SelfTest {
    settings: Mutex<Settings>,
    image: ProcessImage,
    log: Arc<Log>,
    stroke: Option<Hook>,
    status: String,
}

impl SelfTest {
    pub fn new(dir: &Path, image: ProcessImage, log: Arc<Log>) -> Self {
        let settings = Settings::load(&dir.join("selftest.json"), json!({"refiner_tab": false, "stroke_log": false, "test_int": 5, "test_float": 0.5, "test_choice": "medium"}));
        let _ = STROKE_LOG.set(log.clone());
        let stroke = Self::prepare_stroke(&image, &log);
        let status = if stroke.is_some() { "On" } else { "Off, see log" }.to_string();
        let me = Self { settings: Mutex::new(settings), image, log, stroke, status };
        for key in ["refiner_tab", "stroke_log"] {
            let on = me.settings.lock().map(|s| s.is_true(key)).unwrap_or(false);
            if on {
                let line = me.apply(key, true);
                me.log.info(&line);
            }
        }
        me
    }

    fn prepare_stroke(image: &ProcessImage, log: &Log) -> Option<Hook> {
        let pattern = Pattern::parse(sphere_sized_mining::STROKE).ok()?;
        let Some(rva) = image.find_unique(&pattern).unique() else {
            log.info("terrain hook not set: game code not found or found twice (game update?)");
            return None;
        };
        // SAFETY: the signature is the function start; stroke_detour declares all three arguments.
        match unsafe { Hook::new("ApplyTerrainEditStroke", image.va(rva), stroke_detour as *const ()) } {
            Ok(hook) => {
                STROKE_ORIG.store(hook.original() as usize, Ordering::Release);
                Some(hook)
            }
            Err(e) => {
                log.info(&format!("terrain hook not set: {e}"));
                None
            }
        }
    }

    fn apply(&self, key: &str, on: bool) -> String {
        match key {
            "refiner_tab" => craft_from_storage::REFINER_TAB.set(&mut self.image.clone(), on).message,
            "stroke_log" => match &self.stroke {
                None => "Log terrain hook not changed: hook not available, see log".into(),
                Some(hook) => {
                    let done = if on { hook.enable() } else { hook.disable() };
                    match done {
                        Ok(()) if on => {
                            STROKE_CALLS.store(0, Ordering::Relaxed);
                            "Log terrain hook applied".into()
                        }
                        Ok(()) => format!("Log terrain hook turned off after {} strokes", STROKE_CALLS.load(Ordering::Relaxed)),
                        Err(e) => format!("Log terrain hook not changed: {e}"),
                    }
                }
            },
            _ => format!("unknown setting {key}"),
        }
    }
}

impl GameMod for SelfTest {
    fn name(&self) -> &'static str {
        "NMS.rs self-test"
    }

    fn specs(&self) -> &'static [SettingSpec] {
        SPECS
    }

    fn get(&self, key: &str) -> Option<Value> {
        if key == "status" {
            return Some(json!(self.status));
        }
        self.settings.lock().ok()?.get(key).cloned()
    }

    fn default_value(&self, key: &str) -> Option<Value> {
        if key == "status" {
            return None;
        }
        self.settings.lock().ok()?.default(key).cloned()
    }

    fn range(&self, key: &str) -> Option<Range> {
        match key {
            "test_int" => Some(Range::Int { min: 1, max: 10, step: 1, default: 5 }),
            "test_float" => Some(Range::Float { min: 0.0, max: 1.0, step: 0.1, default: 0.5 }),
            "test_choice" => Some(Range::Enum { choices: CHOICES, default: 1 }),
            _ => None,
        }
    }

    fn set(&self, key: &str, value: &Value) -> Result<String, String> {
        if matches!(key, "test_int" | "test_float" | "test_choice") {
            self.settings.lock().map_err(|_| "settings lock poisoned")?.set(key, value.clone())?;
            let line = format!("{key} set to {value}");
            self.log.info(&line);
            return Ok(line);
        }
        let on = value.as_bool().ok_or("expected true or false")?;
        self.settings.lock().map_err(|_| "settings lock poisoned")?.set(key, json!(on))?;
        let line = self.apply(key, on);
        self.log.info(&line);
        Ok(line)
    }
}
