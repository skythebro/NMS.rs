//! Craft from storage: crafting, building and charging use Storage Containers.

mod game;
mod hooks;
mod lists;
mod patches;
mod quick;

use std::cell::Cell;
use std::collections::HashSet;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use nms_core::log::Log;
use nms_core::mem::{read_i32, MIN_ADDR};
use nms_core::process::ProcessImage;
use nms_core::{paths, GameMod, Kind, SettingSpec, Settings};
use serde_json::{json, Value};

use game::Game;
use hooks::{all_named_enabled, close_craft_gate, install, set_build_gate, HookHandles, QUICK_ROW_ORIG};
use lists::{
    chest_kinds, chests_usable_text, kinds, picker_reach_text, CHESTS, CHARGE_LINGER_SECONDS, FEED_MODE,
    HOOKS_WAIT_SECONDS, PANTRY_KIND, POPUP_MODE_AT, POPUP_MODES, POPUP_SOURCE_TAB, WATCH_SECONDS,
};
use patches::{
    patch_cooker_substances, patch_cooker_tabs, patch_list_order, patch_quick_freighter_row, patch_quick_text,
    patch_refiner_tab, quick_set_all_containers, SHOW_ALL,
};
use quick::QuickState;

pub(crate) static RUNTIME: OnceLock<Arc<Mutex<CraftRuntime>>> = OnceLock::new();

thread_local! {
    pub(crate) static BUILD_DEPTH: Cell<u32> = const { Cell::new(0) };
    pub(crate) static POPUP_MODE: Cell<Option<i32>> = const { Cell::new(None) };
}

const SPECS: &[SettingSpec] = &[
    SettingSpec { key: "status", label: "Status", kind: Kind::Status },
    SettingSpec { key: "anywhere", label: "Anywhere (cheat)", kind: Kind::Bool },
    SettingSpec { key: "fossil_storage", label: "Fossil bones from storage", kind: Kind::Bool },
    SettingSpec { key: "list_order", label: "All tab: inventory order", kind: Kind::Bool },
    SettingSpec { key: "refiner_tab", label: "Refiner: storage tabs", kind: Kind::Bool },
    SettingSpec { key: "log_feed", label: "Log frigate feed", kind: Kind::Bool },
    SettingSpec { key: "quick_rows", label: "Quick Transfer: each box", kind: Kind::Bool },
    SettingSpec { key: "show_all_containers", label: "QT: All Containers row", kind: Kind::Bool },
    SettingSpec { key: "cooker_tabs", label: "Cooker: inventory tabs", kind: Kind::Bool },
    SettingSpec { key: "cooker_elements", label: "Cooker: elements on Raw only", kind: Kind::Bool },
    SettingSpec { key: "write_logs", label: "Write log", kind: Kind::Bool },
];

fn defaults() -> Value {
    json!({
        "anywhere": false,
        "write_logs": false,
        "fossil_storage": true,
        "refiner_tab": true,
        "cooker_elements": false,
        "list_order": true,
        "cooker_tabs": true,
        "log_feed": false,
        "quick_rows": true,
        "show_all_containers": true,
    })
}

pub(crate) struct CraftRuntime {
    log: Arc<Log>,
    settings: Arc<Mutex<Settings>>,
    image: ProcessImage,
    status: Arc<Mutex<String>>,
    game: Option<Game>,
    stopped: bool,
    anywhere: bool,
    fossil: bool,
    logged: HashSet<(String, Vec<i32>)>,
    build_until: Option<Instant>,
    build_hooks_on: bool,
    charge_until: Option<Instant>,
    last_mode: Option<i32>,
    picker_state: Option<String>,
    feed_state: Option<(i32, i32, u32)>,
    /// Log frigate feed: popup builds already described (mode, source tab, tier, inventories collected).
    collect_seen: std::collections::HashSet<(i32, i32, i32, u32)>,
    inactive_logged: bool,
    hooks: Arc<HookHandles>,
    pub quick: QuickState,
}

impl CraftRuntime {
    fn active(&self) -> bool {
        !self.stopped && self.game.is_some()
    }

    fn setting_not_false(&self, key: &str) -> bool {
        self.settings.lock().map(|s| s.is_not_false(key)).unwrap_or(false)
    }

    fn setting_true(&self, key: &str) -> bool {
        self.settings.lock().map(|s| s.is_true(key)).unwrap_or(false)
    }

    fn chests(&mut self) -> Vec<i32> {
        if self.anywhere {
            CHESTS.collect()
        } else if let Some(game) = self.game.as_mut() {
            game.reachable_chests()
        } else {
            Vec::new()
        }
    }

    fn log_usable(&mut self, context: &str, chests: Vec<i32>) {
        let mut sorted = chests;
        sorted.sort_unstable();
        let key = (context.to_string(), sorted);
        if self.logged.contains(&key) {
            return;
        }
        let text = chests_usable_text(&key.1);
        self.logged.insert(key);
        self.log.info(&format!("{context}: Storage Containers usable {text}"));
    }

    fn stop(&mut self) {
        self.stopped = true;
        if let Ok(mut s) = self.status.lock() {
            *s = "Stopped, see log".to_string();
        }
        self.log.info("Craft from storage stopped");
    }

    fn extend(&mut self, choices: usize, context: &str) {
        if !self.active() || choices < MIN_ADDR {
            return;
        }
        let Some(game) = self.game.as_mut() else {
            return;
        };
        let present = kinds(choices);
        let want = if self.anywhere {
            CHESTS.collect()
        } else {
            game.reachable_chests()
        };
        for kind in want {
            if !present.contains(&kind) {
                game.push_kind(choices as u64, kind);
            }
        }
        let now = chest_kinds(choices);
        self.log_usable(context, now);
    }

    fn extend_owned(&mut self) {
        if !self.active() {
            return;
        }
        let anywhere = self.anywhere;
        let room = match self.game.as_mut() {
            Some(game) if game.owned_counts.is_empty() => game.make_room(),
            Some(_) => Ok(()),
            None => return,
        };
        if room.is_err() {
            self.stop();
            return;
        }
        let chests = if anywhere {
            CHESTS.collect()
        } else if let Some(game) = self.game.as_mut() {
            game.reachable_chests()
        } else {
            return;
        };
        if let Some(game) = self.game.as_ref() {
            game.set_owned_chests(&chests);
        }
        self.charge_until = Some(Instant::now() + Duration::from_secs_f64(CHARGE_LINGER_SECONDS));
        self.log_usable("Charging", chests);
    }

    fn build_list(&mut self, choices: u64) {
        if !self.active() {
            return;
        }
        let depth = BUILD_DEPTH.with(Cell::get);
        if depth == 0 || choices < MIN_ADDR as u64 {
            return;
        }
        self.extend(choices as usize, "Build mode");
    }

    fn before_popup(&mut self, popup: u64) {
        if popup < MIN_ADDR as u64 {
            return;
        }
        if !self.active() {
            if !self.inactive_logged && self.setting_true("log_feed") {
                self.inactive_logged = true;
                self.log.info("Log frigate feed: an item popup opened but Craft from Storage is not active (stopped or game data not found), so nothing can be logged");
            }
            return;
        }
        let Some(mode) = read_i32((popup as usize).wrapping_add(POPUP_MODE_AT)) else {
            return;
        };
        if self.fossil || self.setting_true("log_feed") {
            POPUP_MODE.with(|m| m.set(Some(mode)));
        }
        if self.last_mode != Some(mode) {
            self.last_mode = Some(mode);
            self.log.info(&format!("Item popup opened in mode {mode:#x}"));
        }
    }

    fn before_slots(&mut self, popup: u64, vector: u64) {
        let mode = POPUP_MODE.with(Cell::get);
        let Some(mode) = mode else {
            return;
        };
        if !self.active() {
            return;
        }
        if mode == FEED_MODE && self.setting_true("log_feed") {
            if let (Some(tab), Some(tier), Some(held)) = (
                read_i32((popup as usize).wrapping_add(POPUP_SOURCE_TAB)),
                read_i32((popup as usize).wrapping_add(POPUP_SOURCE_TAB + 4)),
                lists::vector_count(vector as usize),
            ) {
                let state = (tab, tier, held);
                if self.feed_state != Some(state) {
                    let reach = self.game.as_mut().map(|g| g.reachable_chests()).unwrap_or_default();
                    self.feed_state = Some(state);
                    self.log.info(&format!(
                        "FEED popup (mode 0x41): source tab {tab}, cook tier {tier}, inventories collected {held}, storage in reach {}",
                        chests_usable_text(&reach)
                    ));
                }
            }
        }
        if mode != FEED_MODE && self.setting_true("log_feed") && self.collect_seen.len() < 64 {
            if let (Some(tab), Some(tier), Some(held)) = (
                read_i32((popup as usize).wrapping_add(POPUP_SOURCE_TAB)),
                read_i32((popup as usize).wrapping_add(POPUP_SOURCE_TAB + 4)),
                lists::vector_count(vector as usize),
            ) {
                if self.collect_seen.insert((mode, tab, tier, held)) {
                    self.log.info(&format!(
                        "popup build (mode {mode:#x}): source tab {tab}, cook tier {tier}, inventories collected {held}"
                    ));
                }
            }
        }
        if POPUP_MODES.contains(&mode) && self.fossil {
            let mut chests = self.chests();
            let tab = read_i32((popup as usize).wrapping_add(POPUP_SOURCE_TAB)).unwrap_or(0);
            let held = lists::vector_count(vector as usize).unwrap_or(0);
            if tab != -1 && held > 0 {
                chests.clear();
            } else if tab == -1 {
                chests.push(PANTRY_KIND);
            }
            let added = self
                .game
                .as_mut()
                .map(|g| g.append_views(vector as usize, &chests))
                .unwrap_or_default();
            let why = self.game.as_ref().map(|g| g.why.clone()).unwrap_or_default();
            let added_text = if added.is_empty() {
                "none".into()
            } else {
                let parts: Vec<String> = added
                    .iter()
                    .map(|(label, n)| {
                        if label == "pantry" {
                            format!("('pantry', {n})")
                        } else {
                            format!("({label}, {n})")
                        }
                    })
                    .collect();
                format!("[{}]", parts.join(", "))
            };
            let state = format!("{mode:#x}|{:?}|{added_text}", chests);
            if self.picker_state.as_ref() != Some(&state) {
                self.picker_state = Some(state);
                self.log.info(&format!(
                    "Item picker (popup mode {mode:#x}): in reach {}, added (container, items) {added_text} [{why}]",
                    picker_reach_text(&chests)
                ));
            }
        }
    }

    fn after_quick_row(&mut self, rows: u64, object_id: u64, enabled: u64) {
        self.quick.log_list(&self.log, rows as usize, object_id as usize);
        if !self.active() || self.quick.broken || self.quick.busy {
            return;
        }
        self.quick.at = Some(Instant::now());
        self.quick.busy = true;
        let expand = self.setting_true("quick_rows");
        let show_all = self.setting_not_false("show_all_containers");
        let push_va = QUICK_ROW_ORIG.load(Ordering::Acquire);
        let push_va = if push_va == 0 { None } else { Some(push_va) };
        if let Some(game) = self.game.as_mut() {
            self.quick.expand(
                &self.log,
                game,
                expand,
                show_all,
                rows as usize,
                object_id as usize,
                enabled,
                push_va,
                &self.image,
            );
        }
        self.quick.busy = false;
    }

    fn after_quick_dest(&mut self, rows: u64, dest_type: u64) {
        if self.quick.broken || self.quick.busy {
            return;
        }
        if dest_type < MIN_ADDR as u64 || read_i32(dest_type as usize) != Some(9) {
            return;
        }
        self.quick.place_cooker(rows as usize);
    }
}

pub(crate) fn with_runtime<F: FnOnce(&mut CraftRuntime)>(f: F) {
    let Some(rt) = RUNTIME.get() else {
        return;
    };
    let Ok(mut state) = rt.try_lock() else {
        return;
    };
    f(&mut state);
}

pub(crate) fn call_original<T, F, R>(orig: &AtomicUsize, call: F, fallback: R) -> R
where
    F: FnOnce(T) -> R,
{
    let addr = orig.load(Ordering::Acquire);
    if addr == 0 {
        return fallback;
    }
    let f: T = unsafe { std::mem::transmute_copy::<usize, T>(&addr) };
    call(f)
}

pub struct CraftFromStorage {
    settings: Arc<Mutex<Settings>>,
    status: Arc<Mutex<String>>,
    log: Arc<Log>,
    #[expect(dead_code)]
    hooks: Arc<HookHandles>,
}

impl CraftFromStorage {
    pub fn new(ctx: &super::Ctx) -> Self {
        let dir = paths::mod_dir(&ctx.root, "CraftFromStorage");
        let settings = Arc::new(Mutex::new(Settings::load(&dir.join("craft_from_storage.json"), defaults())));
        let write_logs = settings.lock().map(|s| s.is_not_false("write_logs")).unwrap_or(false);
        let log = Arc::new(Log::open(&dir.join("craft_from_storage.log"), write_logs));
        // Before any patch: the Quick Transfer row names are written in this language.
        patches::set_lang(crate::common::lang::detect(&ctx.image, &log));
        let status = Arc::new(Mutex::new("Off, see log".to_string()));
        let anywhere = settings.lock().map(|s| s.is_true("anywhere")).unwrap_or(false);
        let fossil = settings.lock().map(|s| s.is_not_false("fossil_storage")).unwrap_or(true);

        let mut image = ctx.image.clone();
        let game = match Game::new(&ctx.image, &log) {
            Ok(g) => {
                if let Ok(mut s) = status.lock() {
                    *s = "On".to_string();
                }
                log.info(&format!(
                    "Craft from storage is on ({})",
                    if anywhere {
                        "anywhere"
                    } else {
                        "Storage Containers in reach"
                    }
                ));
                apply_startup_patches(&settings, &log, &ctx.image, &mut image);
                Some(g)
            }
            Err(error) => {
                log.info(&format!("Craft from storage is off: {error}"));
                None
            }
        };

        SHOW_ALL.store(
            settings.lock().map(|s| s.is_not_false("show_all_containers")).unwrap_or(true),
            Ordering::Release,
        );

        let empty_hooks = Arc::new(HookHandles {
            named: Vec::new(),
            kept: Vec::new(),
        });
        let runtime = CraftRuntime {
            log: log.clone(),
            settings: settings.clone(),
            image: image.clone(),
            status: status.clone(),
            game,
            stopped: false,
            anywhere,
            fossil,
            logged: HashSet::new(),
            build_until: None,
            build_hooks_on: true,
            charge_until: None,
            last_mode: None,
            picker_state: None,
            feed_state: None,
            collect_seen: std::collections::HashSet::new(),
            inactive_logged: false,
            hooks: empty_hooks,
            quick: QuickState::default(),
        };
        let rt = Arc::new(Mutex::new(runtime));
        let _ = RUNTIME.set(rt.clone());

        let installed = Arc::new(install(&ctx.image, log.clone()));
        if let Ok(mut state) = rt.lock() {
            state.hooks = installed.clone();
            state.image = image;
        }

        let watch_game_ok = rt.lock().map(|s| s.game.is_some()).unwrap_or(false);
        let watch_rt = rt.clone();
        let _ = thread::Builder::new()
            .name("CraftFromStorage".into())
            .spawn(move || watch_loop(watch_rt, watch_game_ok));

        Self {
            settings,
            status,
            log,
            hooks: installed,
        }
    }
}

fn apply_startup_patches(
    settings: &Arc<Mutex<Settings>>,
    log: &Log,
    snap: &ProcessImage,
    live: &mut ProcessImage,
) {
    let on = |key: &str| settings.lock().map(|s| s.is_not_false(key)).unwrap_or(false);
    let is_true = |key: &str| settings.lock().map(|s| s.is_true(key)).unwrap_or(false);
    if on("refiner_tab") {
        log.info(&patch_refiner_tab(live, true));
    }
    if on("list_order") {
        log.info(&patch_list_order(live, true));
    }
    if on("cooker_tabs") {
        log.info(&patch_cooker_tabs(live, true));
    }
    if is_true("cooker_elements") {
        log.info(&patch_cooker_substances(live, true));
    }
    if is_true("log_feed") {
        log.info("Log frigate feed: on. Open Feed on a living frigate, close the game, then send this log");
    }
    SHOW_ALL.store(on("show_all_containers"), Ordering::Release);
    log.info(&patch_quick_text(snap, live, true));
    if is_true("quick_rows") {
        log.info(&patch_quick_freighter_row(snap, live, true));
    }
}

fn watch_loop(rt: Arc<Mutex<CraftRuntime>>, game_ok: bool) {
    let deadline = Instant::now() + Duration::from_secs_f64(HOOKS_WAIT_SECONDS);
    loop {
        thread::sleep(Duration::from_secs_f64(WATCH_SECONDS));
        let ready = rt.lock().ok().is_some_and(|s| all_named_enabled(&s.hooks));
        if ready || Instant::now() >= deadline {
            break;
        }
    }
    if !game_ok {
        close_craft_gate();
        return;
    }
    loop {
        thread::sleep(Duration::from_secs_f64(WATCH_SECONDS));
        let Ok(mut state) = rt.lock() else {
            continue;
        };
        let now = Instant::now();
        if let Some(until) = state.charge_until {
            if now >= until {
                state.charge_until = None;
                if let Some(game) = state.game.as_ref() {
                    game.set_owned_chests(&[]);
                }
            }
        }
        if state.build_hooks_on && state.build_until.is_none_or(|until| now >= until) {
            state.build_hooks_on = false;
            set_build_gate(false);
        }
    }
}

impl GameMod for CraftFromStorage {
    fn name(&self) -> &'static str {
        "CraftFromStorage"
    }

    fn specs(&self) -> &'static [SettingSpec] {
        SPECS
    }

    fn get(&self, key: &str) -> Option<Value> {
        match key {
            "status" => Some(json!(self.status.lock().ok()?.clone())),
            "anywhere" => Some(json!(self.settings.lock().ok()?.is_true("anywhere"))),
            "fossil_storage" => Some(json!(self.settings.lock().ok()?.is_not_false("fossil_storage"))),
            "list_order" => Some(json!(self.settings.lock().ok()?.is_not_false("list_order"))),
            "refiner_tab" => Some(json!(self.settings.lock().ok()?.is_not_false("refiner_tab"))),
            "log_feed" => Some(json!(self.settings.lock().ok()?.is_true("log_feed"))),
            "quick_rows" => Some(json!(self.settings.lock().ok()?.is_true("quick_rows"))),
            "show_all_containers" => Some(json!(self.settings.lock().ok()?.is_not_false("show_all_containers"))),
            "cooker_tabs" => Some(json!(self.settings.lock().ok()?.is_not_false("cooker_tabs"))),
            "cooker_elements" => Some(json!(self.settings.lock().ok()?.is_true("cooker_elements"))),
            "write_logs" => Some(json!(self.settings.lock().ok()?.is_not_false("write_logs"))),
            _ => None,
        }
    }

    fn default_value(&self, key: &str) -> Option<Value> {
        match key {
            "status" => None,
            _ => self.settings.lock().ok()?.default(key).cloned(),
        }
    }

    fn set(&self, key: &str, value: &Value) -> Result<String, String> {
        let on = value.as_bool().ok_or_else(|| "expected true or false".to_string())?;
        match key {
            "write_logs" => {
                self.settings.lock().map_err(|_| "settings lock poisoned")?.set(key, json!(on))?;
                self.log.set_enabled(on);
                Ok(format!("Write log {}", if on { "on" } else { "off" }))
            }
            "anywhere" => {
                self.settings.lock().map_err(|_| "settings lock poisoned")?.set(key, json!(on))?;
                with_runtime(|state| state.anywhere = on);
                Ok(format!("anywhere {}", if on { "on" } else { "off" }))
            }
            "fossil_storage" => {
                self.settings.lock().map_err(|_| "settings lock poisoned")?.set(key, json!(on))?;
                with_runtime(|state| state.fossil = on);
                Ok(format!("fossil_storage {}", if on { "on" } else { "off" }))
            }
            "log_feed" => {
                self.settings.lock().map_err(|_| "settings lock poisoned")?.set(key, json!(on))?;
                with_runtime(|state| {
                    state.inactive_logged = false;
                    state.collect_seen.clear();
                    state.feed_state = None;
                });
                self.log.info(&format!("Log frigate feed: {}", if on { "on" } else { "off" }));
                Ok(format!("log_feed {}", if on { "on" } else { "off" }))
            }
            "show_all_containers" => {
                self.settings.lock().map_err(|_| "settings lock poisoned")?.set(key, json!(on))?;
                quick_set_all_containers(on);
                self.log.info(&format!("Quick Transfer All Containers row {}", if on { "on" } else { "off" }));
                Ok(format!("show_all_containers {}", if on { "on" } else { "off" }))
            }
            "list_order" => self.toggle_patch(key, on, patch_list_order),
            "refiner_tab" => self.toggle_patch(key, on, patch_refiner_tab),
            "cooker_tabs" => self.toggle_patch(key, on, patch_cooker_tabs),
            "cooker_elements" => self.toggle_patch(key, on, patch_cooker_substances),
            "quick_rows" => self.toggle_freighter(on),
            _ => Err(format!("unknown setting {key:?}")),
        }
    }
}

impl CraftFromStorage {
    fn toggle_patch(&self, key: &str, on: bool, patch: fn(&mut ProcessImage, bool) -> String) -> Result<String, String> {
        self.settings.lock().map_err(|_| "settings lock poisoned")?.set(key, json!(on))?;
        if let Some(rt) = RUNTIME.get() {
            if let Ok(mut state) = rt.lock() {
                if state.game.is_some() {
                    let mut live = state.image.clone();
                    let msg = patch(&mut live, on);
                    self.log.info(&msg);
                    state.image = live;
                    return Ok(msg);
                }
            }
        }
        Ok(format!("{key} {}", if on { "on" } else { "off" }))
    }

    fn toggle_freighter(&self, on: bool) -> Result<String, String> {
        self.settings
            .lock()
            .map_err(|_| "settings lock poisoned")?
            .set("quick_rows", json!(on))?;
        if let Some(rt) = RUNTIME.get() {
            if let Ok(mut state) = rt.lock() {
                if state.game.is_some() {
                    let snap = state.image.clone();
                    let mut live = state.image.clone();
                    let msg = patch_quick_freighter_row(&snap, &mut live, on);
                    self.log.info(&msg);
                    state.image = live;
                    return Ok(msg);
                }
            }
        }
        Ok(format!("quick_rows {}", if on { "on" } else { "off" }))
    }
}

pub fn register(ctx: &super::Ctx) -> Box<dyn GameMod> {
    Box::new(CraftFromStorage::new(ctx))
}
