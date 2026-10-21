//! Hide the empty-slot Modify hover (ported from Python).

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use nms_core::game::dispatch::{self, StateChange, Tick};
use nms_core::hook::Hook;
use nms_core::log::Log;
use nms_core::mem::{self, readable, read_f32, read_ptr, read_u32, read_u8};
use nms_core::process::ProcessImage;
use nms_core::{paths, Code, GameMod, Kind, Pattern, SettingSpec, Settings};
use nms_sdk::sigs::hide_empty_hover;
use serde_json::{json, Value};

type GameFn = unsafe extern "system" fn(u64, u64, i32) -> u64;
type PopupFn = unsafe extern "system" fn(u64, u64, u64, u64, u64) -> u64;
type BasicsFn = unsafe extern "system" fn(
    u64, u64, u64, u64, u64, u64, u64, u64, u64, u64, u64,
) -> u64;

const LOOKUP_AT: u64 = 0x14890;
const UI_DATA: usize = 0x48;
const IS_HIDDEN: usize = 0x61;
const ROW_NAMES: &[&str] = &["TITLELAYER", "BUTTON1", "BUTTON2", "BUTTON3", "BUILDABLE"];

static POPUP_ORIG: AtomicUsize = AtomicUsize::new(0);
static BASICS_ORIG: AtomicUsize = AtomicUsize::new(0);
static IN_EMPTY: AtomicBool = AtomicBool::new(false);
static FILLED: AtomicBool = AtomicBool::new(false);
static RUNTIME: OnceLock<Arc<HoverRuntime>> = OnceLock::new();
/// Snapshot of the two hide switches, so the hover hook never takes the settings lock.
static HIDE_INV: AtomicBool = AtomicBool::new(true);
static HIDE_TECH: AtomicBool = AtomicBool::new(true);

/// A state change (load, death, galaxy map, quit to menu): forget any half-seen hover and log the
/// next "hidden" again, so each load logs once.
fn on_state_change(_: &StateChange) {
    STATE_CHANGES.fetch_add(1, Ordering::Relaxed);
    IN_EMPTY.store(false, Ordering::Release);
    FILLED.store(false, Ordering::Release);
    if let Some(rt) = RUNTIME.get() {
        rt.hiding.store(false, Ordering::Release);
        rt.bar_logged.store(false, Ordering::Release);
    }
}

/// Frame of the fully-booted callback (0 = not seen). Logged with the next "hidden" line, so the
/// callback itself does no file I/O on the main thread.
static BOOTED_AT: AtomicU64 = AtomicU64::new(0);
/// State changes seen, reported the same way.
static STATE_CHANGES: AtomicU64 = AtomicU64::new(0);

/// Once, at the main menu: proof that the lifecycle callbacks reach the mod.
fn on_fully_booted(t: &Tick) {
    BOOTED_AT.store(t.frame.max(1), Ordering::Relaxed);
}

struct HookAddrs {
    popup_rva: usize,
    basics_rva: usize,
    hash_rva: Option<usize>,
    lookup_rva: Option<usize>,
}

struct HoverRuntime {
    log: Arc<Log>,
    status: Arc<Mutex<String>>,
    hash_va: usize,
    lookup_va: usize,
    ready: AtomicBool,
    hiding: AtomicBool,
    bar_logged: AtomicBool,
}

const SPECS: &[SettingSpec] = &[
    SettingSpec {
        key: "status",
        label: "Status",
        kind: Kind::Status,
    },
    SettingSpec {
        key: "hide_empty_hover",
        label: "Hide inventory hover",
        kind: Kind::Bool,
    },
    SettingSpec {
        key: "hide_tech_hover",
        label: "Hide tech hover",
        kind: Kind::Bool,
    },
    SettingSpec {
        key: "write_logs",
        label: "Write log",
        kind: Kind::Bool,
    },
];

pub struct HideEmptyHover {
    settings: Arc<Mutex<Settings>>,
    status: Arc<Mutex<String>>,
    log: Arc<Log>,
    #[expect(dead_code)]
    popup_hook: Option<Hook>,
    #[expect(dead_code)]
    basics_hook: Option<Hook>,
}

impl HideEmptyHover {
    pub fn new(ctx: &super::Ctx) -> Self {
        let dir = paths::mod_dir(&ctx.root, "HideEmptyHover");
        let settings = Arc::new(Mutex::new(Settings::load(
            &dir.join("hide_empty_hover.json"),
            json!({
                "write_logs": false,
                "hide_empty_hover": true,
                "hide_tech_hover": true,
            }),
        )));
        let write_logs = settings.lock().map(|s| s.is_not_false("write_logs")).unwrap_or(false);
        if let Ok(s) = settings.lock() {
            HIDE_INV.store(s.is_not_false("hide_empty_hover"), Ordering::Release);
            HIDE_TECH.store(s.is_not_false("hide_tech_hover"), Ordering::Release);
        }
        let log = Arc::new(Log::open(&dir.join("hide_empty_hover.log"), write_logs));
        let status = Arc::new(Mutex::new("Off, see log".to_string()));

        let (popup_hook, basics_hook) = match locate(&ctx.image) {
            Ok(addrs) => install_hooks(&ctx.image, addrs, log.clone(), status.clone()),
            Err(reason) => {
                log.info(&format!("Empty-slot hover not changed: {reason}"));
                if let Ok(mut s) = status.lock() {
                    *s = "Off, see log".to_string();
                }
                (None, None)
            }
        };

        // Lifecycle callbacks are optional: without them the mod works as before (the reset only
        // makes the "hidden" log line repeat once per load).
        let lifecycle = dispatch::on_state_change("HideEmptyHover", on_state_change)
            .and_then(|_| dispatch::on_fully_booted("HideEmptyHover", on_fully_booted));
        if let Err(e) = lifecycle {
            log.info(&format!("lifecycle callbacks not registered: {e}"));
        }

        let hooks_ready = popup_hook.is_some() && basics_hook.is_some();
        if hooks_ready {
            let hash_va = RUNTIME
                .get()
                .map(|r| r.hash_va)
                .unwrap_or(0);
            let lookup_va = RUNTIME.get().map(|r| r.lookup_va).unwrap_or(0);
            let inv_off = settings
                .lock()
                .map(|s| !s.is_not_false("hide_empty_hover"))
                .unwrap_or(false);
            let tech_off = settings
                .lock()
                .map(|s| !s.is_not_false("hide_tech_hover"))
                .unwrap_or(false);
            if inv_off && tech_off {
                if let Ok(mut s) = status.lock() {
                    *s = "Off".to_string();
                }
                log.info("HideEmptyHover is off: both hover switches are off");
            } else {
                if let Ok(mut s) = status.lock() {
                    *s = "On".to_string();
                }
                if hash_va == 0 || lookup_va == 0 {
                    log.info("Empty-slot hover: header hidden, row lookup missing (game update?)");
                } else {
                    log.info("Empty-slot hover: panel hidden after click and Quick Transfer run");
                }
            }
        }

        Self {
            settings,
            status,
            log,
            popup_hook,
            basics_hook,
        }
    }

    fn apply_hide(&self, key: &str, hide: bool) {
        let Ok(mut settings) = self.settings.lock() else {
            return;
        };
        if settings.set(key, json!(hide)).is_err() {
            return;
        }
        let flag = if key == "hide_tech_hover" { &HIDE_TECH } else { &HIDE_INV };
        flag.store(hide, Ordering::Release);
        let label = if key == "hide_tech_hover" {
            "tech"
        } else {
            "inventory"
        };
        let ready = RUNTIME
            .get()
            .map(|r| r.ready.load(Ordering::Acquire))
            .unwrap_or(false);
        if !ready {
            if let Ok(mut s) = self.status.lock() {
                *s = "Off, see log".to_string();
            }
            return;
        }
        let any_on =
            settings.is_not_false("hide_empty_hover") || settings.is_not_false("hide_tech_hover");
        if let Ok(mut s) = self.status.lock() {
            *s = if any_on { "On".to_string() } else { "Off".to_string() };
        }
        if hide {
            self.log.info(&format!(
                "Empty-slot hover: {label} hover hidden after click and Quick Transfer run"
            ));
            return;
        }
        self.log
            .info(&format!("Empty-slot hover: {label} hover is back (vanilla panel)"));
        if let Some(rt) = RUNTIME.get() {
            rt.hiding.store(false, Ordering::Release);
        }
    }
}

fn locate(image: &ProcessImage) -> Result<HookAddrs, String> {
    let popup_pat = Pattern::parse(hide_empty_hover::POPUP).map_err(|e| e.to_string())?;
    let basics_pat = Pattern::parse(hide_empty_hover::BASICS).map_err(|e| e.to_string())?;
    let Some(popup_rva) = image.find_unique(&popup_pat).unique() else {
        return Err("game code not found (game update?)".into());
    };
    let Some(basics_rva) = image.find_unique(&basics_pat).unique() else {
        return Err("game code not found (game update?)".into());
    };
    let hash_pat = Pattern::parse(hide_empty_hover::HASH).map_err(|e| e.to_string())?;
    let lookup_pat = Pattern::parse(hide_empty_hover::LOOKUP).map_err(|e| e.to_string())?;
    let hash_rva = image.find_unique(&hash_pat).unique();
    let lookup_rva = image.find_unique(&lookup_pat).unique();
    Ok(HookAddrs {
        popup_rva,
        basics_rva,
        hash_rva,
        lookup_rva,
    })
}

fn install_hooks(
    image: &ProcessImage,
    addrs: HookAddrs,
    log: Arc<Log>,
    status: Arc<Mutex<String>>,
) -> (Option<Hook>, Option<Hook>) {
    let hash_va = addrs.hash_rva.map(|r| image.va(r)).unwrap_or(0);
    let lookup_va = addrs.lookup_rva.map(|r| image.va(r)).unwrap_or(0);
    let runtime = Arc::new(HoverRuntime {
        log: log.clone(),
        status: status.clone(),
        hash_va,
        lookup_va,
        ready: AtomicBool::new(true),
        hiding: AtomicBool::new(false),
        bar_logged: AtomicBool::new(false),
    });
    let _ = RUNTIME.set(runtime);

    let install = |name, rva, detour: *const (), original: &AtomicUsize| {
        // SAFETY: both patterns are function starts; each detour declares every argument.
        unsafe { Hook::install(name, image.va(rva), detour, original) }
            .map_err(|e| log.info(&format!("Empty-slot hover not changed: {e}")))
            .ok()
    };
    let popup_hook = install("DoEmptySlotPopup", addrs.popup_rva, popup_detour as *const (), &POPUP_ORIG);
    let basics_hook = install("SetPopupBasics", addrs.basics_rva, basics_detour as *const (), &BASICS_ORIG);
    (popup_hook, basics_hook)
}

impl HoverRuntime {
    fn fail(&self, reason: &str) {
        self.ready.store(false, Ordering::Release);
        if let Ok(mut s) = self.status.lock() {
            *s = "Off, see log".to_string();
        }
        self.log
            .info(&format!("Empty-slot hover not changed: {reason}"));
    }

    fn after_empty_slot(&self, page: u64, current_inventory: u64) {
        let filled = FILLED.load(Ordering::Acquire);
        IN_EMPTY.store(false, Ordering::Release);
        FILLED.store(false, Ordering::Release);
        if !filled || !self.ready.load(Ordering::Acquire) {
            return;
        }
        if let Err(reason) = self.try_after_empty(page, current_inventory) {
            self.fail(&reason);
        }
    }

    fn try_after_empty(&self, page: u64, current_inventory: u64) -> Result<(), String> {
        let tech = is_tech_store(current_inventory);
        let on = if tech { HIDE_TECH.load(Ordering::Acquire) } else { HIDE_INV.load(Ordering::Acquire) };
        if !on {
            return Ok(());
        }
        if hide_layers(self, page)? && !self.hiding.swap(true, Ordering::AcqRel) {
            self.log.info(&format!(
                "Empty-slot hover hidden (lifecycle: fully booted at frame {}, {} state changes so far)",
                BOOTED_AT.load(Ordering::Relaxed),
                STATE_CHANGES.load(Ordering::Relaxed)
            ));
        }
        Ok(())
    }
}

fn pad_name(name: &str) -> [u8; 16] {
    let mut buf = [0u8; 16];
    let bytes = name.as_bytes();
    let n = bytes.len().min(16);
    if n > 0 {
        buf[..n].copy_from_slice(&bytes[..n]);
    }
    buf
}

fn lookup_named(rt: &HoverRuntime, page: u64, name: &str) -> u64 {
    if page < mem::MIN_ADDR as u64 || rt.hash_va == 0 || rt.lookup_va == 0 {
        return 0;
    }
    let mut tkid = pad_name(name);
    let mut hashed = [0u8; 24];
    let kind: i32 = 0;
    // SAFETY: addresses come from unique patterns in the game binary.
    let hash: GameFn = unsafe { std::mem::transmute(rt.hash_va) };
    let lookup: GameFn = unsafe { std::mem::transmute(rt.lookup_va) };
    unsafe {
        hash(
            hashed.as_mut_ptr() as u64,
            tkid.as_mut_ptr() as u64,
            kind,
        );
    }
    let found = unsafe {
        lookup(
            page.wrapping_add(LOOKUP_AT),
            hashed.as_ptr() as u64,
            kind,
        )
    };
    if found < mem::MIN_ADDR as u64 || found == page || found == page.wrapping_add(LOOKUP_AT) {
        return 0;
    }
    found
}

fn set_hidden(element: u64, hidden: bool) -> bool {
    let element = element as usize;
    if element < mem::MIN_ADDR {
        return false;
    }
    let Some(data) = read_ptr(element.wrapping_add(UI_DATA)) else {
        return false;
    };
    if data < mem::MIN_ADDR {
        return false;
    }
    let value = if hidden { 1u8 } else { 0u8 };
    // SAFETY: `data + IS_HIDDEN` is the UI hidden flag field.
    unsafe { mem::write(data.wrapping_add(IS_HIDDEN), value) }
}

fn bar_candidates(root: u64) -> Vec<u64> {
    let mut found = Vec::new();
    let root = root as usize;
    if !readable(root.wrapping_add(0x58), 0x10) {
        return found;
    }
    let Some(count) = read_u32(root.wrapping_add(0x5C)) else {
        return found;
    };
    if count == 0 || count > 4000 {
        return found;
    }
    let Some(array) = read_ptr(root.wrapping_add(0x60)) else {
        return found;
    };
    let byte_len = match (count as usize).checked_mul(8) {
        Some(n) => n,
        None => return found,
    };
    if !readable(array, byte_len) {
        return found;
    }
    let mut i = 0u32;
    while i < count {
        let off = match (i as usize).checked_mul(8) {
            Some(o) => o,
            None => break,
        };
        let Some(element) = read_ptr(array.wrapping_add(off)) else {
            i = i.wrapping_add(1);
            continue;
        };
        if element < mem::MIN_ADDR || !readable(element.wrapping_add(UI_DATA), 8) {
            i = i.wrapping_add(1);
            continue;
        }
        let Some(data) = read_ptr(element.wrapping_add(UI_DATA)) else {
            i = i.wrapping_add(1);
            continue;
        };
        if data < mem::MIN_ADDR || !readable(data, 0x68) {
            i = i.wrapping_add(1);
            continue;
        }
        if read_u8(data.wrapping_add(0x48)) != Some(0) {
            i = i.wrapping_add(1);
            continue;
        }
        if read_f32(data.wrapping_add(0x24)) != Some(12.0) {
            i = i.wrapping_add(1);
            continue;
        }
        if read_u8(data.wrapping_add(0x41)) != Some(1) {
            i = i.wrapping_add(1);
            continue;
        }
        found.push(element as u64);
        i = i.wrapping_add(1);
    }
    found
}

fn hide_layers(rt: &HoverRuntime, page: u64) -> Result<bool, String> {
    let mut hidden = false;
    for name in ROW_NAMES {
        let found = lookup_named(rt, page, name);
        if found != 0 && set_hidden(found, true) {
            hidden = true;
        }
    }
    if hidden {
        let bars = bar_candidates(page.wrapping_add(LOOKUP_AT));
        if bars.len() == 1
            && set_hidden(bars[0], true)
            && !rt.bar_logged.swap(true, Ordering::AcqRel)
        {
            rt.log.info("Empty-slot hover: divider bar hidden");
        }
    }
    Ok(hidden)
}

fn is_tech_store(store: u64) -> bool {
    let store = store as usize;
    if !readable(store, 0x110) {
        return false;
    }
    let Some(count) = read_u32(store.wrapping_add(0xC4)) else {
        return false;
    };
    count > 0 && count <= 32
}

unsafe extern "system" fn popup_detour(
    page: u64,
    slot: u64,
    empty_slot_actions: u64,
    index: u64,
    current_inventory: u64,
) -> u64 {
    IN_EMPTY.store(true, Ordering::Release);
    FILLED.store(false, Ordering::Release);
    let original: PopupFn = std::mem::transmute(POPUP_ORIG.load(Ordering::Acquire));
    let ret = original(page, slot, empty_slot_actions, index, current_inventory);
    if let Some(rt) = RUNTIME.get() {
        rt.after_empty_slot(page, current_inventory);
    }
    ret
}

unsafe extern "system" fn basics_detour(
    parent_layer: u64,
    title: u64,
    colour: u64,
    description: u64,
    subtitle: u64,
    ty: u64,
    tech_tree_cost: u64,
    tech_tree_cost_alt: u64,
    special_subtitle: u64,
    amount_to_show: u64,
    layer_id_override: u64,
) -> u64 {
    let original: BasicsFn = std::mem::transmute(BASICS_ORIG.load(Ordering::Acquire));
    let ret = original(
        parent_layer,
        title,
        colour,
        description,
        subtitle,
        ty,
        tech_tree_cost,
        tech_tree_cost_alt,
        special_subtitle,
        amount_to_show,
        layer_id_override,
    );
    if IN_EMPTY.load(Ordering::Acquire) {
        FILLED.store(true, Ordering::Release);
    }
    ret
}

impl GameMod for HideEmptyHover {
    fn name(&self) -> &'static str {
        "HideEmptyHover"
    }

    fn specs(&self) -> &'static [SettingSpec] {
        SPECS
    }

    fn get(&self, key: &str) -> Option<Value> {
        match key {
            "status" => Some(json!(self.status.lock().ok()?.clone())),
            "write_logs" => Some(json!(self.settings.lock().ok()?.is_not_false("write_logs"))),
            "hide_empty_hover" => Some(json!(self.settings.lock().ok()?.is_not_false("hide_empty_hover"))),
            "hide_tech_hover" => Some(json!(self.settings.lock().ok()?.is_not_false("hide_tech_hover"))),
            _ => None,
        }
    }

    fn default_value(&self, key: &str) -> Option<Value> {
        match key {
            "write_logs" | "hide_empty_hover" | "hide_tech_hover" => self.settings.lock().ok()?.default(key).cloned(),
            _ => None,
        }
    }

    fn set(&self, key: &str, value: &Value) -> Result<String, String> {
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
                Ok(format!("Write log {}", if on { "on" } else { "off" }))
            }
            "hide_empty_hover" | "hide_tech_hover" => {
                self.apply_hide(key, on);
                Ok(format!("{key} {}", if on { "on" } else { "off" }))
            }
            _ => Err(format!("unknown setting {key:?}")),
        }
    }
}

pub fn register(ctx: &super::Ctx) -> Box<dyn GameMod> {
    Box::new(HideEmptyHover::new(ctx))
}
