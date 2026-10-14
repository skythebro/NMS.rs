//! Shop slider, Buy/Sell list memory, and extra trade inventories (ported from Python).

mod inv;
mod patches;
mod shop;
mod util;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::Instant;

use nms_core::hook::Hook;
use nms_core::log::Log;
use nms_core::mem::{read_i32, read_u8, MIN_ADDR};
use nms_core::process::ProcessImage;
use nms_core::{paths, Code, GameMod, Kind, Pattern, SettingSpec, Settings};
use nms_sdk::sigs::trade_ui_tweaks;
use serde_json::{json, Value};

use inv::ShopInv;
use patches::{patch_shop_buyfor, patch_shop_more_inv, stores_applied};
use shop::{ShopList, ShopQ, ShopScreen};
use util::{
    shop_clear_painted, shop_item_id, shop_kind_name, shop_page_inv, shop_painted, SHOP_BUY_AT, SHOP_INV_AT,
    SHOP_INV_CLICK, SHOP_ITEM_MAX_AT, SHOP_LIST_RESTORE, SHOP_LIST_STALE, SHOP_PAGE_NEEDLE, WATCH_SECONDS,
};

type PageFn = unsafe extern "system" fn(u64, i32, u64) -> u64;
type PriceFn = unsafe extern "system" fn(u64, u64, u64, u8, u64) -> u64;
type ConfirmFn = unsafe extern "system" fn(u64, u64, u8, u8, u64, u8, u8) -> u8;

static PAGE_ORIG: AtomicUsize = AtomicUsize::new(0);
static PRICE_ORIG: AtomicUsize = AtomicUsize::new(0);
static CONFIRM_ORIG: AtomicUsize = AtomicUsize::new(0);
static CAVE_ADDR: AtomicUsize = AtomicUsize::new(0);
static BUYFOR_SITE: AtomicUsize = AtomicUsize::new(0);
static RUNTIME: OnceLock<Arc<Mutex<TradeRuntime>>> = OnceLock::new();

struct HookAddrs {
    page: Option<usize>,
    price: Option<usize>,
    confirm: Option<usize>,
}

struct TradeRuntime {
    log: Arc<Log>,
    settings: Arc<Mutex<Settings>>,
    image: ProcessImage,
    shop_q: Option<ShopQ>,
    shop_screen: Option<ShopScreen>,
    shop_list: Option<ShopList>,
    shop_inv: Option<ShopInv>,
    shop_inv_stores: bool,
    shop_tab_buy: i32,
    shop_list_at: [i32; 2],
    shop_restore_until: Option<Instant>,
    shop_restore_ok: bool,
    shop_open: bool,
    shop_seen: Option<Instant>,
    shop_qty_was: i32,
    shop_pending: Option<(u64, i32)>,
    shop_skip_page: String,
    shop_last_page: u64,
    shop_inv_at: Option<Instant>,
    shop_inv_want: i32,
    shop_inv_paint: bool,
    shop_inv_d8: bool,
    shop_inv_click_name: String,
    shop_inv_row: Option<Vec<i32>>,
    shop_inv_title: i32,
}

const SPECS: &[SettingSpec] = &[
    SettingSpec {
        key: "status",
        label: "Status",
        kind: Kind::Status,
    },
    SettingSpec {
        key: "shop_q_afford",
        label: "Shop: Q to affordable",
        kind: Kind::Bool,
    },
    SettingSpec {
        key: "shop_reset_list",
        label: "Shop: remember Buy/Sell list",
        kind: Kind::Bool,
    },
    SettingSpec {
        key: "shop_more_inv",
        label: "Shop: more inventories",
        kind: Kind::Bool,
    },
    SettingSpec {
        key: "write_logs",
        label: "Write log",
        kind: Kind::Bool,
    },
];

pub struct TradeUITweaks {
    settings: Arc<Mutex<Settings>>,
    status: Arc<Mutex<String>>,
    log: Arc<Log>,
    #[expect(dead_code)]
    page_hook: Option<Hook>,
    #[expect(dead_code)]
    price_hook: Option<Hook>,
    #[expect(dead_code)]
    confirm_hook: Option<Hook>,
}

impl TradeUITweaks {
    pub fn new(ctx: &super::Ctx) -> Self {
        let dir = paths::mod_dir(&ctx.root, "TradeUITweaks");
        let settings_path = dir.join("trade_ui_tweaks.json");
        let defaults = json!({
            "write_logs": false,
            "shop_q_afford": true,
            "shop_reset_list": true,
            "shop_more_inv": true,
        });
        let settings = Arc::new(Mutex::new(load_settings(&settings_path, &dir.join("shop_window.json"), defaults)));
        let write_logs = settings
            .lock()
            .map(|s| s.is_not_false("write_logs"))
            .unwrap_or(false);
        let log = Arc::new(Log::open(&dir.join("trade_ui_tweaks.log"), write_logs));
        let _ = inv::LANG.set(crate::common::lang::detect(&ctx.image, &log));
        let status = Arc::new(Mutex::new("Off, see log".to_string()));

        let mut runtime = TradeRuntime {
            log: log.clone(),
            settings: settings.clone(),
            image: ctx.image.clone(),
            shop_q: None,
            shop_screen: None,
            shop_list: None,
            shop_inv: None,
            shop_inv_stores: false,
            shop_tab_buy: -1,
            shop_list_at: [0, 0],
            shop_restore_until: None,
            shop_restore_ok: false,
            shop_open: false,
            shop_seen: None,
            shop_qty_was: -1,
            shop_pending: None,
            shop_skip_page: String::new(),
            shop_last_page: 0,
            shop_inv_at: None,
            shop_inv_want: 0,
            shop_inv_paint: false,
            shop_inv_d8: false,
            shop_inv_click_name: String::new(),
            shop_inv_row: None,
            shop_inv_title: -2,
        };

        let mut landed: Vec<&str> = Vec::new();

        match ShopQ::new(&ctx.image) {
            Ok(q) => {
                runtime.shop_q = Some(q);
                if settings.lock().map(|s| s.is_not_false("shop_q_afford")).unwrap_or(true) {
                    let snap = ctx.image.clone();
                    let mut live = ctx.image.clone();
                    log.info(&patch_shop_buyfor(
                        &snap,
                        &mut live,
                        true,
                        &CAVE_ADDR,
                        &BUYFOR_SITE,
                    ));
                    runtime.image = live;
                }
                log.info(&format!(
                    "Shop: Q to affordable {}",
                    if settings.lock().map(|s| s.is_not_false("shop_q_afford")).unwrap_or(true) {
                        "on"
                    } else {
                        "off"
                    }
                ));
                landed.push("Q");
            }
            Err(e) => {
                runtime.shop_q = None;
                log.info(&format!("Shop: Q to affordable not changed: {e}"));
            }
        }

        match ShopScreen::new(&ctx.image) {
            Ok(screen) => {
                runtime.shop_screen = Some(screen);
                log.info(&format!("Shop screen check on ({SHOP_PAGE_NEEDLE})"));
            }
            Err(e) => {
                runtime.shop_screen = None;
                log.info(&format!("Shop screen check not changed: {e}"));
            }
        }

        match ShopList::new(&ctx.image) {
            Ok(list) => {
                runtime.shop_list = Some(list);
                log.info(&format!(
                    "Shop: remember Buy/Sell list {}",
                    if settings.lock().map(|s| s.is_not_false("shop_reset_list")).unwrap_or(true) {
                        "on"
                    } else {
                        "off"
                    }
                ));
                landed.push("list");
            }
            Err(e) => {
                runtime.shop_list = None;
                log.info(&format!("Shop: remember Buy/Sell list not changed: {e}"));
            }
        }

        match ShopInv::new(&ctx.image) {
            Ok(mut helper) => {
                helper.attach_log(log.clone());
                let want = settings.lock().map(|s| s.is_not_false("shop_more_inv")).unwrap_or(true);
                let mut live = runtime.image.clone();
                let msg = patch_shop_more_inv(&mut live, want);
                log.info(&msg);
                runtime.image = live;
                runtime.shop_inv_stores = want && stores_applied(&msg);
                runtime.shop_inv = Some(helper);
                log.info(&format!(
                    "Shop: more inventories {}",
                    if runtime.shop_inv_on() { "on" } else { "off" }
                ));
                if runtime.shop_inv.as_ref().is_some_and(|h| !h.chests_ok) {
                    log.info("Shop: nearby storage not changed: game code not found (game update?)");
                }
                if runtime.shop_inv_on() {
                    landed.push("inv");
                }
            }
            Err(e) => {
                runtime.shop_inv = None;
                log.info(&format!("Shop: more inventories not changed: {e}"));
            }
        }

        if !landed.is_empty() {
            if let Ok(mut s) = status.lock() {
                *s = "On".to_string();
            }
            log.info(&format!("Trade UI Tweaks is on ({})", landed.join(", ")));
        } else {
            log.info("Trade UI Tweaks is off: no shop code found (game update?)");
        }

        let rt = Arc::new(Mutex::new(runtime));
        let _ = RUNTIME.set(rt.clone());

        thread::Builder::new()
            .name("TradeUITweaks".into())
            .spawn(move || watch_loop(rt))
            .ok();

        let hooks = install_hooks(&ctx.image, log.clone());
        Self {
            settings,
            status,
            log,
            page_hook: hooks.0,
            price_hook: hooks.1,
            confirm_hook: hooks.2,
        }
    }
}

fn load_settings(path: &std::path::Path, legacy: &std::path::Path, defaults: Value) -> Settings {
    if path.is_file() {
        return Settings::load(path, defaults);
    }
    let mut bootstrap = match defaults {
        Value::Object(map) => map,
        _ => serde_json::Map::new(),
    };
    if let Ok(text) = std::fs::read_to_string(legacy) {
        if let Ok(Value::Object(map)) = serde_json::from_str(&text) {
            for key in ["write_logs", "shop_q_afford", "shop_reset_list", "shop_more_inv"] {
                if let Some(v) = map.get(key) {
                    bootstrap.insert(key.to_string(), v.clone());
                }
            }
        }
    }
    Settings::load(path, Value::Object(bootstrap))
}

fn watch_loop(rt: Arc<Mutex<TradeRuntime>>) {
    loop {
        thread::sleep(std::time::Duration::from_secs_f64(WATCH_SECONDS));
        let Ok(mut state) = rt.lock() else {
            continue;
        };
        if state.shop_open {
            if let Some(seen) = state.shop_seen {
                if seen.elapsed().as_secs_f64() > SHOP_LIST_STALE {
                    state.shop_close_list(false);
                }
            }
        }
    }
}

fn install_hooks(image: &ProcessImage, log: Arc<Log>) -> (Option<Hook>, Option<Hook>, Option<Hook>) {
    let addrs = locate_hooks(image);
    let install = |name: &'static str, rva: Option<usize>, detour: *const (), orig: &AtomicUsize| {
        let rva = rva?;
        // SAFETY: unique pattern hits are function entry points matching the detour signature.
        unsafe { Hook::install(name, image.va(rva), detour, orig) }
            .map_err(|e| log.info(&format!("Trade UI hook not changed: {e}")))
            .ok()
    };
    let page = install("SetFrontendPage", addrs.page, page_detour as *const (), &PAGE_ORIG);
    let price = install("ShopPriceRoot", addrs.price, price_detour as *const (), &PRICE_ORIG);
    let confirm = install(
        "cGcFrontendPage.Confirm",
        addrs.confirm,
        confirm_detour as *const (),
        &CONFIRM_ORIG,
    );
    (page, price, confirm)
}

fn locate_hooks(image: &ProcessImage) -> HookAddrs {
    let page = Pattern::parse(trade_ui_tweaks::SHOP_PAGE)
        .ok()
        .and_then(|p| image.find_unique(&p).unique());
    let price = Pattern::parse(trade_ui_tweaks::SHOP_Q)
        .ok()
        .and_then(|p| image.find_unique(&p).unique());
    let confirm = Pattern::parse(trade_ui_tweaks::SHOP_CONFIRM)
        .ok()
        .and_then(|p| image.find_unique(&p).unique());
    HookAddrs { page, price, confirm }
}

impl TradeRuntime {
    fn shop_q_on(&self) -> bool {
        self.shop_q.is_some() && self.settings.lock().map(|s| s.is_not_false("shop_q_afford")).unwrap_or(true)
    }

    fn shop_reset_on(&self) -> bool {
        self.shop_list.is_some() && self.settings.lock().map(|s| s.is_not_false("shop_reset_list")).unwrap_or(true)
    }

    fn shop_inv_on(&self) -> bool {
        self.shop_inv.is_some()
            && self.shop_inv_stores
            && self.settings.lock().map(|s| s.is_not_false("shop_more_inv")).unwrap_or(true)
    }

    fn shop_close_list(&mut self, force: bool) {
        if !force && !self.shop_open && self.shop_tab_buy < 0 {
            return;
        }
        let was = self.shop_list.as_ref().map(ShopList::reset).unwrap_or(0);
        let saved = self.shop_list_at;
        self.shop_list_at = [0, 0];
        self.shop_tab_buy = -1;
        self.shop_restore_until = None;
        self.shop_restore_ok = false;
        self.shop_open = false;
        self.shop_seen = None;
        if was != 0 || saved[0] != 0 || saved[1] != 0 || force {
            self.log.info(&format!("Shop list cleared (window closed, was {was})"));
        }
    }

    fn shop_release_restore(&mut self, live: i32, keep: i32) {
        self.shop_restore_until = None;
        self.shop_restore_ok = false;
        let buy = self.shop_tab_buy;
        if buy < 0 {
            return;
        }
        let slot = if buy == 0 { 0 } else { 1 };
        self.shop_list_at[slot] = live;
        self.log.info(&format!(
            "Shop list restore released: {} {keep} -> {live}",
            if buy == 0 { "buy" } else { "sell" }
        ));
    }

    fn shop_restore_step(&mut self, keep: i32) {
        let live = self.shop_list.as_ref().map(ShopList::step).unwrap_or(0);
        if live > keep {
            self.shop_release_restore(live, keep);
            return;
        }
        if live == keep {
            self.shop_restore_ok = true;
            return;
        }
        if self.shop_restore_ok && keep - live == 1 {
            self.shop_release_restore(live, keep);
            return;
        }
        if let Some(store) = self.shop_list.as_ref() {
            store.set(keep);
        }
    }

    fn shop_hold_list(&mut self) {
        if self.shop_restore_until.is_none() || !self.shop_reset_on() {
            return;
        }
        let Some(until) = self.shop_restore_until else {
            return;
        };
        if Instant::now() >= until {
            return;
        }
        let buy = self.shop_tab_buy;
        if buy < 0 {
            return;
        }
        let slot = if buy == 0 { 0 } else { 1 };
        let keep = self.shop_list_at[slot];
        self.shop_restore_step(keep);
    }

    fn shop_maybe_reset_list(&mut self, page: u64) {
        if !self.shop_reset_on() || page < MIN_ADDR as u64 {
            return;
        }
        let Some(store) = self.shop_list.as_ref() else {
            return;
        };
        let now_t = Instant::now();
        self.shop_open = true;
        self.shop_seen = Some(now_t);
        let buy = read_u8(page as usize + SHOP_BUY_AT as usize).unwrap_or(0);
        let last = self.shop_tab_buy;
        let now_i = if buy == 0 { 0 } else { 1 };
        let live = store.step();
        if last < 0 {
            self.shop_tab_buy = buy as i32;
            self.shop_list_at[now_i] = live;
            return;
        }
        if buy as i32 != last {
            let keep = self.shop_list_at[now_i];
            store.set(keep);
            self.shop_tab_buy = buy as i32;
            self.shop_restore_until = Some(now_t + std::time::Duration::from_secs_f64(SHOP_LIST_RESTORE));
            self.shop_restore_ok = false;
            let other = if last == 0 { 0 } else { 1 };
            self.log.info(&format!(
                "Shop list kept: {} at {keep} (saved {} {})",
                if buy == 0 { "buy" } else { "sell" },
                if last == 0 { "buy" } else { "sell" },
                self.shop_list_at[other]
            ));
            return;
        }
        if let Some(until) = self.shop_restore_until {
            if now_t < until {
                let keep = self.shop_list_at[now_i];
                self.shop_restore_step(keep);
                return;
            }
            self.shop_restore_until = None;
            self.shop_restore_ok = false;
            let live = store.step();
            let keep = self.shop_list_at[now_i];
            if live < keep {
                self.log.info(&format!(
                    "Shop list restore clamped: {} {keep} -> {live}",
                    if buy == 0 { "buy" } else { "sell" }
                ));
                self.shop_list_at[now_i] = live;
            }
            return;
        }
        self.shop_list_at[now_i] = live;
    }

    fn shop_page_ok(&mut self, page: u64) -> bool {
        let Some(screen) = self.shop_screen.as_ref() else {
            return true;
        };
        if page < MIN_ADDR as u64 {
            return false;
        }
        let name = screen.name(page);
        if screen.is_list(page) {
            return true;
        }
        if name != self.shop_skip_page {
            self.shop_skip_page = name.clone();
            self.log.info(&format!("Shop Q skipped: screen is {}", if name.is_empty() { "unknown" } else { &name }));
        }
        false
    }

    fn shop_in_shop(&self, page: u64) -> bool {
        if page < MIN_ADDR as u64 {
            return false;
        }
        if self.shop_last_page != 0 && page == self.shop_last_page {
            return true;
        }
        self.shop_screen.as_ref().is_some_and(|s| s.is_list(page))
    }

    fn shop_set_title(&mut self, page: u64, kind: i32) {
        if !self.shop_inv_on() {
            return;
        }
        if kind == self.shop_inv_title {
            return;
        }
        let Some(helper) = self.shop_inv.as_mut() else {
            return;
        };
        let label = helper.set_title(page, kind);
        if label.is_empty() {
            if self.shop_inv_title != -3 {
                self.shop_inv_title = -3;
                self.log.info("Shop title not changed: TRADE_INVENTORY not found");
            }
            return;
        }
        self.shop_inv_title = kind;
        self.log.info(&format!("Shop title {label}"));
    }

    fn shop_advance(&mut self, page: u64) {
        if page < MIN_ADDR as u64 {
            return;
        }
        let now = Instant::now();
        if self
            .shop_inv_at
            .is_some_and(|t| now.duration_since(t).as_secs_f64() < SHOP_INV_CLICK)
        {
            return;
        }
        let current = read_i32(page as usize + SHOP_INV_AT as usize).unwrap_or(0);
        let nxt = self.shop_inv.as_mut().map(|h| h.next_kind(current)).unwrap_or(current);
        if nxt == current {
            return;
        }
        self.shop_inv_at = Some(now);
        let _ = unsafe { nms_core::mem::write(page as usize + SHOP_INV_AT as usize, nxt) };
        if let Some(h) = self.shop_inv.as_ref() {
            h.clear_table(page);
        }
        self.shop_inv_want = nxt;
        self.shop_inv_paint = true;
        self.log.info(&format!(
            "Shop inv {} -> {}",
            shop_kind_name(current),
            shop_kind_name(nxt)
        ));
        self.shop_set_title(page, nxt);
    }

    fn shop_mark_wrap(&mut self, page: u64, item: u64, qty_ptr: u64) {
        if !self.shop_q_on() || self.shop_qty_was != 1 {
            return;
        }
        if !self.shop_page_ok(page) {
            return;
        }
        if page == 0 || item == 0 || qty_ptr == 0 {
            return;
        }
        if read_u8(page as usize + SHOP_BUY_AT as usize).unwrap_or(1) != 0 {
            return;
        }
        let now = read_i32(qty_ptr as usize).unwrap_or(0);
        let stock = read_i32(item as usize + SHOP_ITEM_MAX_AT as usize).unwrap_or(0);
        if now != stock || stock <= 1 {
            return;
        }
        shop_clear_painted(CAVE_ADDR.load(Ordering::Acquire));
        self.shop_pending = Some((qty_ptr, stock));
        self.log.info(&format!("Shop Q wrap pending stock={stock}"));
    }

    fn shop_flush(&mut self) {
        if !self.shop_q_on() {
            return;
        }
        let Some((qty_ptr, stock)) = self.shop_pending else {
            return;
        };
        let cave = CAVE_ADDR.load(Ordering::Acquire);
        let need = shop_painted(cave);
        if need <= 0 {
            return;
        }
        self.shop_pending = None;
        shop_clear_painted(cave);
        let wallet = self.shop_q.as_ref().map(ShopQ::units).unwrap_or(0);
        if need as u32 <= wallet {
            self.log.info(&format!(
                "Shop Q to affordable not changed: {stock} costs {need}, wallet {wallet}"
            ));
            return;
        }
        let aff = (wallet as i64 * stock as i64) / need as i64;
        let mut aff = aff as i32;
        if aff < 1 {
            aff = 1;
        }
        if aff >= stock {
            return;
        }
        let _ = unsafe { nms_core::mem::write(qty_ptr as usize, aff) };
        self.log.info(&format!(
            "Shop Q to affordable: {stock} -> {aff} (wallet={wallet} need={need})"
        ));
    }

    fn before_price(&mut self, page: u64, item: u64, qty_ptr: u64) {
        self.shop_qty_was = -1;
        if page >= MIN_ADDR as u64 {
            self.shop_last_page = page;
            if self.shop_inv_on() {
                if let Some(helper) = self.shop_inv.as_mut() {
                    let row = helper.cycle();
                    if self.shop_inv_row.as_ref() != Some(&row) {
                        self.shop_inv_row = Some(row.clone());
                        let names: Vec<String> = row
                            .iter()
                            .map(|k| format!("{}={k}", shop_kind_name(*k)))
                            .collect();
                        let chests: Vec<i32> = row
                            .iter()
                            .copied()
                            .filter(|k| util::SHOP_CHESTS.contains(k))
                            .map(|k| k - util::SHOP_CHESTS.start + 1)
                            .collect();
                        self.log.info(&format!(
                            "Shop inventories in reach: {} (ship switch {} view {}, freighter {}, chests {})",
                            names.join(", "),
                            if helper.switch_has(4) { "yes" } else { "no" },
                            if helper.view_has(4) { "yes" } else { "no" },
                            if helper.switch_has(7) { "yes" } else { "no" },
                            if chests.is_empty() {
                                "none".into()
                            } else {
                                format!("{chests:?}")
                            }
                        ));
                        helper.remember_names();
                    }
                    let kind = read_i32(page as usize + SHOP_INV_AT as usize).unwrap_or(0);
                    self.shop_set_title(page, kind);
                }
                if self.shop_inv_paint {
                    self.shop_inv_paint = false;
                    let kind = read_i32(page as usize + SHOP_INV_AT as usize).unwrap_or(0);
                    let ident = shop_item_id(item);
                    let stock = if item != 0 {
                        read_i32(item as usize + SHOP_ITEM_MAX_AT as usize).unwrap_or(-1)
                    } else {
                        -1
                    };
                    let lost = if kind != self.shop_inv_want {
                        format!(" overwritten want={}", self.shop_inv_want)
                    } else {
                        String::new()
                    };
                    self.log.info(&format!(
                        "Shop paint {} item={} stock={stock}{lost}",
                        shop_page_inv(page),
                        if ident.is_empty() { "-" } else { &ident }
                    ));
                }
            }
        }
        self.shop_maybe_reset_list(page);
        self.shop_flush();
        if self.shop_q_on() && self.shop_page_ok(page) && qty_ptr != 0 {
            self.shop_qty_was = read_i32(qty_ptr as usize).unwrap_or(-1);
        }
    }

    fn after_price(&mut self, page: u64, item: u64, qty_ptr: u64) {
        self.shop_hold_list();
        self.shop_mark_wrap(page, item, qty_ptr);
    }

    fn after_confirm(&mut self, page: u64, layer: u64, result: u8) {
        if !self.shop_inv_on() {
            self.shop_inv_d8 = false;
            return;
        }
        if page == 0 || !self.shop_in_shop(page) {
            self.shop_inv_d8 = false;
            return;
        }
        let Some(helper) = self.shop_inv.as_ref() else {
            self.shop_inv_d8 = false;
            return;
        };
        if result != 0 && helper.is_switch(page, layer) {
            self.shop_advance(page);
            return;
        }
        if result != 0 && self.shop_inv_click_name.is_empty() {
            let ident = util::shop_element_name(layer);
            let name = if ident.is_empty() { "-" } else { ident.as_str() };
            self.shop_inv_click_name = name.to_string();
            self.log.info(&format!("Shop click widget {name} (not the switch)"));
        }
        let held = helper.toggle_held();
        let edge = held && !self.shop_inv_d8;
        self.shop_inv_d8 = held;
        if edge {
            self.shop_advance(page);
        }
    }
}

// Detours use try_lock: a game call made while the runtime is locked can re-enter a hooked function on
// the same thread, and std Mutex is not reentrant. Skipping one frame stays vanilla; blocking would hang.
unsafe extern "system" fn page_detour(page: u64, index: i32, extra: u64) -> u64 {
    if let Some(rt) = RUNTIME.get() {
        if let Ok(mut state) = rt.try_lock() {
            let stay = state
                .shop_screen
                .as_ref()
                .is_some_and(|s| s.name_at(index).contains(SHOP_PAGE_NEEDLE));
            if !stay {
                state.shop_last_page = 0;
            }
            if state.shop_reset_on() && state.shop_open && !stay {
                state.shop_close_list(false);
            }
        }
    }
    let original: PageFn = std::mem::transmute(PAGE_ORIG.load(Ordering::Acquire));
    original(page, index, extra)
}

unsafe extern "system" fn price_detour(page: u64, widget: u64, item: u64, flag: u8, qty_ptr: u64) -> u64 {
    if let Some(rt) = RUNTIME.get() {
        if let Ok(mut state) = rt.try_lock() {
            state.before_price(page, item, qty_ptr);
        }
    }
    let original: PriceFn = std::mem::transmute(PRICE_ORIG.load(Ordering::Acquire));
    let ret = original(page, widget, item, flag, qty_ptr);
    if let Some(rt) = RUNTIME.get() {
        if let Ok(mut state) = rt.try_lock() {
            state.after_price(page, item, qty_ptr);
        }
    }
    ret
}

unsafe extern "system" fn confirm_detour(
    page: u64,
    layer: u64,
    a: u8,
    b: u8,
    audio: u64,
    c: u8,
    d: u8,
) -> u8 {
    let original: ConfirmFn = std::mem::transmute(CONFIRM_ORIG.load(Ordering::Acquire));
    let result = original(page, layer, a, b, audio, c, d);
    if let Some(rt) = RUNTIME.get() {
        if let Ok(mut state) = rt.try_lock() {
            state.after_confirm(page, layer, result);
        }
    }
    result
}

impl GameMod for TradeUITweaks {
    fn name(&self) -> &'static str {
        "TradeUITweaks"
    }

    fn specs(&self) -> &'static [SettingSpec] {
        SPECS
    }

    fn get(&self, key: &str) -> Option<Value> {
        // Runtime methods lock `settings` themselves: never hold it while taking the runtime lock.
        let setting = |name: &str| self.settings.lock().map(|s| s.is_not_false(name)).unwrap_or(false);
        match key {
            "status" => Some(json!(self.status.lock().ok()?.clone())),
            "write_logs" => Some(json!(setting("write_logs"))),
            "shop_q_afford" => {
                let q = RUNTIME.get()?.lock().ok()?.shop_q.is_some();
                Some(json!(q && setting("shop_q_afford")))
            }
            "shop_reset_list" => {
                let list = RUNTIME.get()?.lock().ok()?.shop_list.is_some();
                Some(json!(list && setting("shop_reset_list")))
            }
            "shop_more_inv" => Some(json!(RUNTIME.get()?.lock().ok()?.shop_inv_on())),
            _ => None,
        }
    }

    fn default_value(&self, key: &str) -> Option<Value> {
        // Not read from Settings: a first run seeds its defaults from the legacy shop_window.json.
        match key {
            "write_logs" => Some(json!(false)),
            "shop_q_afford" | "shop_reset_list" | "shop_more_inv" => Some(json!(true)),
            _ => None,
        }
    }

    fn set(&self, key: &str, value: &Value) -> Result<String, String> {
        let on = value
            .as_bool()
            .ok_or_else(|| "expected true or false".to_string())?;
        match key {
            "write_logs" => {
                self.settings.lock().map_err(|_| "settings lock poisoned")?.set(key, json!(on))?;
                self.log.set_enabled(on);
                Ok(format!("Write log {}", if on { "on" } else { "off" }))
            }
            "shop_q_afford" => {
                self.settings.lock().map_err(|_| "settings lock poisoned")?.set(key, json!(on))?;
                if let Some(rt) = RUNTIME.get() {
                    if let Ok(mut state) = rt.lock() {
                        let snap = state.image.clone();
                        let mut live = state.image.clone();
                        self.log.info(&patch_shop_buyfor(&snap, &mut live, on, &CAVE_ADDR, &BUYFOR_SITE));
                        state.image = live;
                    }
                }
                self.log.info(&format!("Shop: Q to affordable {}", if on { "on" } else { "off" }));
                Ok(format!("shop_q_afford {}", if on { "on" } else { "off" }))
            }
            "shop_reset_list" => {
                self.settings.lock().map_err(|_| "settings lock poisoned")?.set(key, json!(on))?;
                if let Some(rt) = RUNTIME.get() {
                    if let Ok(mut state) = rt.lock() {
                        state.shop_close_list(true);
                    }
                }
                self.log.info(&format!("Shop: remember Buy/Sell list {}", if on { "on" } else { "off" }));
                Ok(format!("shop_reset_list {}", if on { "on" } else { "off" }))
            }
            "shop_more_inv" => {
                self.settings.lock().map_err(|_| "settings lock poisoned")?.set(key, json!(on))?;
                if let Some(rt) = RUNTIME.get() {
                    if let Ok(mut state) = rt.lock() {
                        let apply = on && state.shop_inv.is_some();
                        let mut live = state.image.clone();
                        let msg = patch_shop_more_inv(&mut live, apply);
                        self.log.info(&msg);
                        state.image = live;
                        state.shop_inv_stores = on && state.shop_inv.is_some() && stores_applied(&msg);
                    }
                }
                let inv_on = RUNTIME
                    .get()
                    .and_then(|rt| rt.lock().ok())
                    .map(|s| s.shop_inv_on())
                    .unwrap_or(false);
                self.log.info(&format!("Shop: more inventories {}", if inv_on { "on" } else { "off" }));
                Ok(format!("shop_more_inv {}", if on { "on" } else { "off" }))
            }
            _ => Err(format!("unknown setting {key:?}")),
        }
    }
}

pub fn register(ctx: &super::Ctx) -> Box<dyn GameMod> {
    Box::new(TradeUITweaks::new(ctx))
}
