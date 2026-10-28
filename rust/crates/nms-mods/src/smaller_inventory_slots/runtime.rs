//! Per-frame inventory state: filters, scroll, freighter tab, storage sort, probe.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;

use nms_core::log::Log;
use nms_core::mem::{self, read_i32};

use super::layout::{
    chest_panel_width, fixed_f32, is_cargo_height, sane_y, scaled_cells, vanilla_kind, CATEGORIES, CHEST_ROWS,
    CHEST_SLOT, CHEST_WIDTH, FREIGHTER_CORE_KIND, FREIGHTER_FIRST, FREIGHTER_LAST, FREIGHTER_ROWS, FREIGHTER_SELECTOR,
    FREIGHTER_WIDTH, MIN_ADDR, PAGE_BUSY_FROM_SCREEN, PAGE_ELEMENT, PAGE_OPENED_FROM_SELECTOR, PROBE_DIFF_CAP,
    PROBE_PAGE_DIFF, PROBE_SCREEN_WINDOW, SORT_BUTTONS, STORAGE_NAME, STORE_STRIDE, VANILLA_FILTERS, VANILLA_SLOT,
    Plan,
};
use super::layout::{CUSTOM_INDEX, FOODS_INDEX};
use super::patches::{field_in_range, CustomCave};
use crate::common::custom_filter;
use super::ui::{
    child_named, children, element_info, find_named, page_dwords, read_dwords, read_u64_checked, set_hidden,
    stack_group, store_name, write_layer_y, FilterHover,
};
use crate::common::input::{face_button_a_xinput, SteamInput};

pub struct SlotsRuntime {
    pub log: Arc<Log>,
    pub remember_filter: bool,
    pub remember_scroll: bool,
    pub remember_freighter: bool,
    pub sort_storage: bool,
    pub shrink_chest: bool,
    pub shrink_freighter: bool,
    pub probe_diff: bool,
    pub probe: bool,
    pub filter_field: Option<i32>,
    pub filter_foods: bool,
    pub filter_custom: bool,
    /// Keep Custom marks after leaving the Custom filter (setting `custom_remember`).
    pub custom_remember: bool,
    custom_leave: custom_filter::LeaveWatch,
    started: Instant,
    foods_cave: Option<usize>,
    custom_cave: Option<CustomCave>,
    hide_on: Option<usize>,
    custom_gen: Option<u32>,
    pub plan: Option<Plan>,
    pub status: String,
    pub slots: String,
    pub filters: String,
    on_locker: bool,
    sort_note: Option<String>,
    container_layer: usize,
    sort_target: u64,
    drawing: u64,
    redirect_logged: bool,
    freighter_saved: Option<i32>,
    freighter_base: Option<u64>,
    freighter_opened: Option<i32>,
    freighter_fresh: bool,
    freighter_busy: Option<i32>,
    freighter_tries: i32,
    freighter_screen: Option<i32>,
    freighter_note: Option<(String, i32, u64)>,
    tab_filter: HashMap<u64, i32>,
    tab_scroll: HashMap<(u64, &'static str), f32>,
    layer_store: HashMap<u64, u64>,
    tab_key: u64,
    /// The page's screen id at the last slot draw. Only for the restore log lines: the exosuit
    /// upgrade station bug report needs to show which screen the restore happened on.
    last_screen: Option<i32>,
    hold_zero: bool,
    filter_hover: Option<FilterHover>,
    filter_stick: Option<i32>,
    filter_confirm_down: bool,
    filter_click_off: bool,
    steam: Option<SteamInput>,
    steam_select: Vec<u64>,
    steam_off: bool,
    probe_storage_store: HashMap<u64, u64>,
    probe_last: HashMap<u64, ProbeSig>,
    probe_screen: HashSet<(u64, u64)>,
    probe_screen_at: f64,
    probe_regions: HashMap<String, Vec<Option<u32>>>,
    probe_store_base: Option<u64>,
    logged: HashSet<String>,
}

#[derive(Clone, PartialEq, Eq)]
struct ProbeSig {
    store: u64,
    name: String,
    group: Option<u16>,
    slot_w: i32,
    render_h: i32,
    ident: String,
    width: String,
    pos_y: String,
    filter: String,
    scroll: String,
    selector: String,
}

impl SlotsRuntime {
    pub fn new(log: Arc<Log>) -> Self {
        Self {
            log,
            remember_filter: true,
            remember_scroll: true,
            remember_freighter: true,
            sort_storage: true,
            shrink_chest: true,
            shrink_freighter: false,
            probe_diff: PROBE_PAGE_DIFF,
            probe: false,
            filter_field: None,
            filter_foods: false,
            filter_custom: false,
            custom_remember: false,
            custom_leave: custom_filter::LeaveWatch::default(),
            started: Instant::now(),
            foods_cave: None,
            custom_cave: None,
            hide_on: None,
            custom_gen: None,
            plan: None,
            status: "Off, see log".into(),
            slots: "-".into(),
            filters: "Off, see log".into(),
            on_locker: false,
            sort_note: None,
            container_layer: 0,
            sort_target: 0,
            drawing: 0,
            redirect_logged: false,
            freighter_saved: None,
            freighter_base: None,
            freighter_opened: None,
            freighter_fresh: true,
            freighter_busy: None,
            freighter_tries: 0,
            freighter_screen: None,
            freighter_note: None,
            tab_filter: HashMap::new(),
            tab_scroll: HashMap::new(),
            layer_store: HashMap::new(),
            tab_key: 0,
            last_screen: None,
            hold_zero: false,
            filter_hover: None,
            filter_stick: None,
            filter_confirm_down: false,
            filter_click_off: false,
            steam: None,
            steam_select: Vec::new(),
            steam_off: false,
            probe_storage_store: HashMap::new(),
            probe_last: HashMap::new(),
            probe_screen: HashSet::new(),
            probe_screen_at: 0.0,
            probe_regions: HashMap::new(),
            probe_store_base: None,
            logged: HashSet::new(),
        }
    }

    pub fn set_filter_hover(&mut self, hover: Option<FilterHover>) {
        self.filter_hover = hover;
    }

    pub fn set_extra_caves(&mut self, foods_on: Option<usize>, custom: Option<CustomCave>, hide_on: Option<usize>) {
        self.foods_cave = foods_on;
        self.custom_cave = custom;
        self.hide_on = hide_on;
        self.custom_gen = None;
    }

    /// Foods is usable: its dim test is installed.
    pub fn foods_ready(&self) -> bool {
        self.foods_cave.is_some()
    }

    /// Custom is usable: its dim test is installed.
    pub fn custom_ready(&self) -> bool {
        self.custom_cave.is_some()
    }

    /// Writes the Foods and Custom switches into the caves and the shared state QuickTransferAll
    /// reads. A switch whose dim test is missing stays off, and its button stays hidden.
    pub fn apply_extra_switches(&mut self) {
        let foods = self.filter_foods && self.foods_ready();
        let custom = self.filter_custom && self.custom_ready();
        if let Some(byte) = self.foods_cave {
            let _ = unsafe { mem::write(byte, u8::from(foods)) };
        }
        if let Some(cave) = self.custom_cave {
            let _ = unsafe { mem::write(cave.on, u8::from(custom)) };
        }
        if let Some(on) = self.hide_on {
            let _ = unsafe { mem::write(on.wrapping_add(FOODS_INDEX), u8::from(foods)) };
            let _ = unsafe { mem::write(on.wrapping_add(CUSTOM_INDEX), u8::from(custom)) };
        }
        custom_filter::set_foods_on(foods);
        custom_filter::set_custom_on(custom);
        self.custom_gen = None;
    }

    /// Copies the marks into the Custom cave table when they changed. Count goes to 0 first so the
    /// cave never reads a half-written entry as a match.
    fn sync_custom_marks(&mut self) {
        let Some(cave) = self.custom_cave else {
            return;
        };
        let generation = custom_filter::generation();
        if self.custom_gen == Some(generation) {
            return;
        }
        let Some(marks) = custom_filter::snapshot() else {
            return;
        };
        let _ = unsafe { mem::write(cave.count, 0u32) };
        let mut n = 0u32;
        for (index, mark) in marks.iter().take(custom_filter::MAX_MARKS).enumerate() {
            let entry = custom_filter::table_entry(mark);
            let at = cave.table.wrapping_add(index.wrapping_mul(entry.len()));
            for (offset, byte) in entry.iter().enumerate() {
                let _ = unsafe { mem::write(at.wrapping_add(offset), *byte) };
            }
            n = n.wrapping_add(1);
        }
        let _ = unsafe { mem::write(cave.count, n) };
        self.custom_gen = Some(generation);
    }

    /// Tells the Custom cave which inventory store is being drawn (per-slot marks match on it); 0
    /// after the draw, so a skipped hook call lights no per-slot mark rather than a wrong one.
    fn set_custom_store(&self, store: u64) {
        if let Some(cave) = self.custom_cave {
            let _ = unsafe { mem::write(cave.store, store) };
        }
    }

    /// Clears the Custom marks once the player has left the Custom filter on the screen where it was
    /// picked, unless `custom_remember` (`custom_filter::LeaveWatch`).
    fn watch_custom_leave(&mut self, page: u64) {
        if self.custom_remember || !custom_filter::custom_on() {
            self.custom_leave = custom_filter::LeaveWatch::default();
            return;
        }
        let (Some(current), Some(screen)) = (self.read_filter(page), self.page_screen(page)) else {
            return;
        };
        let now = u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX);
        if self.custom_leave.step(current, screen, now) {
            let n = custom_filter::clear();
            if n > 0 {
                self.log.info(&format!("Custom: left the filter, {n} mark(s) cleared"));
            }
        }
    }

    /// The Custom switch went off: drop the marks unless `custom_remember`.
    pub fn custom_switched_off(&mut self) {
        self.custom_leave = custom_filter::LeaveWatch::default();
        if !self.custom_remember {
            let n = custom_filter::clear();
            if n > 0 {
                self.log.info(&format!("Custom: switched off, {n} mark(s) cleared"));
            }
        }
    }

    /// A filter whose switch is off (picked earlier, or restored by filter memory) falls back to All.
    fn drop_disabled_filter(&mut self, page: u64) {
        let Some(current) = self.read_filter(page) else {
            return;
        };
        let off = (current == custom_filter::FOODS_ID && !custom_filter::foods_on())
            || (current == custom_filter::CUSTOM_ID && !custom_filter::custom_on());
        if !off {
            return;
        }
        let address = self.filter_address(page);
        if address != 0 && unsafe { mem::write(address, 0i32) } {
            self.log.info(&format!("Filter: {current} is switched off, back to All"));
        }
    }

    fn filter_text(&self, page: u64) -> String {
        let Some(field) = self.filter_field else {
            return "filter_field unknown".into();
        };
        let address = (page as usize).wrapping_add(field as usize);
        if (page as usize) < MIN_ADDR || address < MIN_ADDR {
            return "filter unreadable".into();
        }
        match read_i32(address) {
            Some(v) => v.to_string(),
            None => "filter unreadable".into(),
        }
    }

    fn filter_address(&self, page: u64) -> usize {
        let Some(field) = self.filter_field else {
            return 0;
        };
        if (page as usize) < MIN_ADDR || !field_in_range(field) {
            return 0;
        }
        let address = (page as usize).wrapping_add(field as usize);
        if address < MIN_ADDR {
            0
        } else {
            address
        }
    }

    fn read_filter(&self, page: u64) -> Option<i32> {
        let address = self.filter_address(page);
        if address == 0 {
            None
        } else {
            read_i32(address)
        }
    }

    fn store_filter(&mut self, page: u64, tab: u64) {
        let Some(current) = self.read_filter(page) else {
            return;
        };
        let max = VANILLA_FILTERS as i32 + CATEGORIES as i32 - 1;
        if current < 0 || current > max {
            return;
        }
        self.tab_filter.insert(tab, current);
    }

    fn remember_filter_tab(&mut self, page: u64, store: u64, render_h: i32, slot_w: i32) {
        if !self.remember_filter || !is_cargo_height(render_h) {
            return;
        }
        if (page as usize) < MIN_ADDR || (store as usize) < MIN_ADDR {
            return;
        }
        if slot_w == CHEST_SLOT {
            let name = store_name(store as usize);
            if name == STORAGE_NAME || name == "?" {
                return;
            }
        }
        let tab = store;
        if tab != self.tab_key {
            let previous = self.tab_key;
            self.tab_key = tab;
            if previous == 0 {
                self.store_filter(page, tab);
                return;
            }
            let saved = self.tab_filter.get(&tab).copied();
            self.hold_zero = true;
            let Some(saved) = saved else {
                return;
            };
            if self.read_filter(page) == Some(saved) {
                return;
            }
            let address = self.filter_address(page);
            if address == 0 {
                return;
            }
            if unsafe { mem::write(address, saved) } {
                self.log.info(&format!(
                    "Filter: restored {saved} for store {tab:#x} (screen {})",
                    self.screen_text()
                ));
            }
            return;
        }
        if self.hold_zero {
            let current = self.read_filter(page);
            let saved = self.tab_filter.get(&tab).copied();
            if current.is_none() || (current == Some(0) && saved.is_some() && saved != Some(0)) {
                return;
            }
            self.hold_zero = false;
        }
        self.store_filter(page, tab);
    }

    fn screen_text(&self) -> String {
        self.last_screen.map_or_else(|| "?".to_string(), |v| v.to_string())
    }

    fn page_screen(&self, page: u64) -> Option<i32> {
        let offset = match self.filter_field {
            Some(field) => (field as usize).wrapping_add(0x3C),
            None => 0x14488,
        };
        read_i32((page as usize).wrapping_add(offset))
    }

    fn page_opened(&self, page: u64) -> Option<i32> {
        read_i32((page as usize).wrapping_add(FREIGHTER_SELECTOR).wrapping_add(PAGE_OPENED_FROM_SELECTOR))
    }

    fn page_busy(&self, page: u64) -> Option<i32> {
        let offset = match self.filter_field {
            Some(field) => (field as usize).wrapping_add(0x3C).wrapping_add(PAGE_BUSY_FROM_SCREEN),
            None => 0x14C84,
        };
        read_i32((page as usize).wrapping_add(offset))
    }

    fn remember_freighter_tab(&mut self, page: u64, store: u64, render_h: i32) {
        if page == 0 {
            return;
        }
        let busy = self.page_busy(page);
        let opened = self.page_opened(page);
        if busy != self.freighter_busy || (opened == Some(1) && self.freighter_opened != Some(1)) {
            self.freighter_fresh = true;
            self.freighter_tries = 0;
        }
        self.freighter_busy = busy;
        self.freighter_opened = opened;
        if render_h != 5 {
            return;
        }
        let address = (page as usize).wrapping_add(FREIGHTER_SELECTOR);
        let Some(sel) = read_i32(address) else {
            return;
        };
        let screen = self.page_screen(page);
        let saved = self.freighter_saved;
        if (FREIGHTER_FIRST..=FREIGHTER_LAST).contains(&sel) {
            let kind = 12i64 + sel as i64 - FREIGHTER_FIRST as i64;
            if self.freighter_base.is_none() {
                self.freighter_base = Some(store.wrapping_sub((kind as u64).wrapping_mul(STORE_STRIDE)));
            }
            if let Some(base) = self.freighter_base {
                if store == base.wrapping_add((kind as u64).wrapping_mul(STORE_STRIDE)) {
                    if let Some(put) = saved {
                        if put != sel && self.freighter_fresh && self.freighter_tries < 4 {
                            self.freighter_tries = self.freighter_tries.saturating_add(1);
                            if unsafe { mem::write(address, put) } {
                                self.freighter_log("restore-open", put, store);
                            }
                            return;
                        }
                    }
                    if saved != Some(sel) {
                        self.freighter_log("container", sel, store);
                    }
                    self.freighter_saved = Some(sel);
                    if let Some(screen) = screen {
                        self.freighter_screen = Some(screen);
                    }
                    self.freighter_fresh = false;
                    return;
                }
            }
        }
        let on_core = sel == 0
            && screen.is_some()
            && screen == self.freighter_screen
            && self.freighter_base.is_some()
            && store
                == self
                    .freighter_base
                    .map(|b| b.wrapping_add(FREIGHTER_CORE_KIND.wrapping_mul(STORE_STRIDE)))
                    .unwrap_or(0);
        if on_core {
            if !self.freighter_fresh || saved.is_none() || saved == Some(0) || self.freighter_tries >= 4 {
                if saved != Some(0) {
                    self.freighter_log("core", 0, store);
                }
                self.freighter_saved = Some(0);
                self.freighter_fresh = false;
                return;
            }
            self.freighter_tries = self.freighter_tries.saturating_add(1);
            if let Some(put) = saved {
                if unsafe { mem::write(address, put) } {
                    self.freighter_log("restore", put, store);
                }
            }
            return;
        }
        let Some(put) = saved else {
            return;
        };
        if sel == put {
            return;
        }
        if unsafe { mem::write(address, put) } {
            self.freighter_log("restore", put, store);
        }
    }

    fn freighter_log(&mut self, kind: &str, saved: i32, store: u64) {
        let note = (kind.to_string(), saved, store);
        if self.freighter_note.as_ref() == Some(&note) {
            return;
        }
        self.freighter_note = Some(note);
        self.log
            .info(&format!("Freighter tab: {kind} selector {saved} (store {store:#x})"));
    }

    fn remember_scroll_y(&mut self, layer: u64, store: u64, render_h: i32, lf_scroll: f32) -> Option<f32> {
        if !self.remember_scroll || (layer as usize) < MIN_ADDR || (store as usize) < MIN_ADDR {
            return None;
        }
        let kind = match vanilla_kind(render_h) {
            Some(k) => k,
            None if is_cargo_height(render_h) => "cargo",
            None => return None,
        };
        let (ident, layout) = element_info(layer as usize);
        if ident != "INVENTORY" {
            return None;
        }
        let current = layout.map(|l| l.y).unwrap_or(lf_scroll);
        if !sane_y(current) {
            return None;
        }
        let key = (store, kind);
        let previous = self.layer_store.get(&layer).copied();
        if previous != Some(store) {
            self.layer_store.insert(layer, store);
            if previous.is_none() {
                self.tab_scroll.insert(key, current);
                return None;
            }
            let saved = self.tab_scroll.get(&key).copied();
            let y = match saved {
                Some(v) if sane_y(v) => v,
                _ => 0.0,
            };
            if write_layer_y(layer as usize, y).is_ok() {
                self.tab_scroll.insert(key, y);
                self.log.info(&format!(
                    "Scroll: restored {} for store {store:#x} {kind} (screen {})",
                    fixed_f32(y),
                    self.screen_text()
                ));
                return Some(y);
            }
            return None;
        }
        self.tab_scroll.insert(key, current);
        None
    }

    #[allow(clippy::too_many_arguments)]
    fn try_shrink(
        &mut self,
        layer: u64,
        vanilla_slot: i32,
        vanilla_rows: i32,
        panel_w: &[f32],
        label: &str,
        store: u64,
        storage_only: bool,
    ) -> Option<(i32, i32)> {
        if (layer as usize) < MIN_ADDR {
            return None;
        }
        let (ident, layout) = element_info(layer as usize);
        let seen = layout.map(|l| l.w).unwrap_or(0.0);
        if ident != "INVENTORY" || !panel_w.iter().any(|width| (seen - width).abs() <= 0.5) {
            return None;
        }
        if storage_only && store_name(store as usize) != STORAGE_NAME {
            return None;
        }
        let plan = self.plan.as_ref()?;
        let (slot, rows) = scaled_cells(vanilla_slot, vanilla_rows, plan)?;
        if self.logged.insert(label.to_string()) {
            self.log.info(&format!(
                "{label} grid: slot {vanilla_slot} -> {slot}, rows {vanilla_rows} -> {rows}"
            ));
        }
        Some((slot, rows))
    }

    fn show_locker_buttons(&mut self, page: u64, show: bool) {
        let root = read_u64_checked((page as usize).wrapping_add(PAGE_ELEMENT)) as usize;
        let content = child_named(root, "PAGECONTENT");
        let host = child_named(content, "FILTERS");
        let buttons = children(host);
        let mut found = Vec::new();
        for name in SORT_BUTTONS {
            if let Some((_, child)) = buttons.iter().find(|(ident, _)| ident == name) {
                found.push(*child);
            }
        }
        if found.len() != 4 {
            if self.sort_note.as_deref() != Some("missing") {
                self.sort_note = Some("missing".into());
                let names: Vec<String> = children(content).into_iter().map(|(n, _)| n).collect();
                self.log.info(&format!(
                    "Storage sort: page children {}",
                    if names.is_empty() {
                        "none".into()
                    } else {
                        names.join(", ")
                    }
                ));
                let filt: Vec<String> = buttons.into_iter().map(|(n, _)| n).collect();
                self.log.info(&format!(
                    "Storage sort: filter children {}",
                    if filt.is_empty() {
                        "none".into()
                    } else {
                        filt.join(", ")
                    }
                ));
            }
            return;
        }
        for button in found {
            set_hidden(button, !show);
        }
    }

    fn aim_storage_sort(&mut self, page: u64) {
        let root = read_u64_checked((page as usize).wrapping_add(PAGE_ELEMENT)) as usize;
        let content = child_named(root, "PAGECONTENT");
        let marks: Vec<String> = children(content).into_iter().map(|(n, _)| n).collect();
        let locker = marks.iter().any(|n| n == "LEFT")
            && marks.iter().any(|n| n == "RIGHT")
            && marks.iter().any(|n| n == "FILTERS");
        if !locker || !self.sort_storage {
            if locker && self.on_locker {
                self.show_locker_buttons(page, false);
            }
            self.on_locker = false;
            return;
        }
        if !self.on_locker {
            self.on_locker = true;
            self.sort_target = 0;
            self.redirect_logged = false;
            self.container_layer = find_named(child_named(content, "RIGHT"), "INVENTORY", 6, 64);
            self.log.info(&format!(
                "Storage sort: transfer page, buttons shown, container grid {}",
                if self.container_layer != 0 {
                    "found"
                } else {
                    "not found (a click sorts both grids)"
                }
            ));
        }
        self.show_locker_buttons(page, true);
    }

    fn steam_select_down(&mut self) -> bool {
        if self.steam_off {
            return false;
        }
        match self.ensure_steam() {
            Ok(()) => {}
            Err(error) => {
                self.steam_off = true;
                self.log
                    .info(&format!("Filters: Steam Input is not available ({error}); XInput A only"));
                return false;
            }
        }
        let Some(steam) = self.steam.as_mut() else {
            return false;
        };
        let controllers = steam.controllers();
        if controllers.is_empty() {
            return false;
        }
        if self.steam_select.is_empty() {
            let handle = steam.digital("select");
            if handle != 0 {
                self.steam_select.push(handle);
            }
        }
        for controller in controllers {
            for handle in &self.steam_select {
                if steam.down(controller, *handle) {
                    return true;
                }
            }
        }
        false
    }

    fn ensure_steam(&mut self) -> Result<(), String> {
        if self.steam.is_some() {
            return Ok(());
        }
        self.steam = Some(SteamInput::new()?);
        Ok(())
    }

    fn filter_confirm_held(&mut self) -> bool {
        if face_button_a_xinput() {
            return true;
        }
        self.steam_select_down()
    }

    pub fn click_extra_filter(&mut self, page: u64) {
        if self.filter_hover.is_none() || self.filter_click_off || self.filter_field.is_none() {
            return;
        }
        if !self.filter_confirm_held() {
            self.filter_stick = None;
            self.filter_confirm_down = false;
            return;
        }
        if page == 0 {
            return;
        }
        let rising = !self.filter_confirm_down;
        self.filter_confirm_down = true;
        let field = match self.filter_field {
            Some(f) => f,
            None => return,
        };
        if rising {
            let wanted = match self.filter_hover.as_mut().and_then(|h| h.hovered_filter()) {
                Some(w) => w as i32,
                None => {
                    self.filter_stick = None;
                    return;
                }
            };
            let current = read_i32((page as usize).wrapping_add(field as usize)).unwrap_or(0);
            if current == wanted {
                self.filter_stick = Some(0);
                self.log.info("Filters: controller Confirm toggled All");
            } else {
                self.filter_stick = Some(wanted);
                self.log.info(&format!("Filters: controller Confirm set filter {wanted}"));
            }
        }
        let Some(stick) = self.filter_stick else {
            return;
        };
        let current = read_i32((page as usize).wrapping_add(field as usize)).unwrap_or(0);
        if current == stick {
            return;
        }
        let _ = unsafe { mem::write((page as usize).wrapping_add(field as usize), stick) };
    }

    pub fn before_sort(&mut self, inventory: u64, order: i32) -> u64 {
        let drawing = self.drawing;
        let target = self.sort_target;
        if !(self.sort_storage && drawing != 0 && target != 0 && drawing != target && inventory == drawing) {
            return inventory;
        }
        if !self.redirect_logged {
            self.redirect_logged = true;
            self.log.info(&format!(
                "Storage sort: sorted the container (order {order}), the exosuit grid is left as it is"
            ));
        }
        target
    }

    fn screen_changed(&mut self, page: u64, store: u64, now: f64) -> bool {
        if (page as usize) < MIN_ADDR || (store as usize) < MIN_ADDR {
            return false;
        }
        let key = (page, store);
        if self.probe_screen.contains(&key) {
            return false;
        }
        let same_page = self.probe_screen.iter().any(|(seen_page, _)| *seen_page == page);
        if self.probe_screen.is_empty() || (same_page && (now - self.probe_screen_at) < PROBE_SCREEN_WINDOW) {
            self.probe_screen.insert(key);
            self.probe_screen_at = now;
            return self.probe_screen.len() == 1;
        }
        self.probe_screen.clear();
        self.probe_screen.insert(key);
        self.probe_screen_at = now;
        true
    }

    fn probe_observe(&mut self, page: u64, layer: u64, store: u64, slot_w: i32, render_h: i32, scroll: f32) {
        let now = probe_now();
        let name = store_name(store as usize);
        let group = stack_group(store as usize);
        let (ident, layout) = if (layer as usize) < MIN_ADDR {
            ("?".into(), None)
        } else {
            element_info(layer as usize)
        };
        let ident = if ident.is_empty() { "?" } else { ident.as_str() };
        let (width, pos_y) = if ident != "INVENTORY" {
            ("n/a".into(), "n/a".into())
        } else {
            (
                layout.map(|l| fixed_f32(l.w)).unwrap_or_else(|| "n/a".into()),
                layout.map(|l| fixed_f32(l.y)).unwrap_or_else(|| "n/a".into()),
            )
        };
        let current_filter = self.filter_text(page);
        let scroll_s = fixed_f32(scroll);
        let selector = if page == 0 {
            "-".into()
        } else {
            let words = read_dwords((page as usize).wrapping_add(FREIGHTER_SELECTOR), 4);
            super::ui::dword_text(words.first().copied().flatten())
        };
        let sig = ProbeSig {
            store,
            name: name.clone(),
            group,
            slot_w,
            render_h,
            ident: ident.to_string(),
            width,
            pos_y,
            filter: current_filter.clone(),
            scroll: scroll_s.clone(),
            selector: selector.clone(),
        };
        if self.probe_last.get(&layer) != Some(&sig) {
            self.log.info(&format!(
                "Probe: sel={selector} t={now:.3} layer={} store={} name={name:?} group={} slot_w={slot_w} render_h={render_h} id={ident} W={} Y={} filter={current_filter} scroll={scroll_s}",
                if layer != 0 { format!("{layer:#x}") } else { "0".into() },
                if store != 0 { format!("{store:#x}") } else { "0".into() },
                group.map(|g| g.to_string()).unwrap_or_else(|| "?".into()),
                sig.width,
                sig.pos_y,
            ));
            self.probe_last.insert(layer, sig);
        }
        self.probe_storage_diff(page, layer, store, &name);
        if !self.screen_changed(page, store, now) {
            return;
        }
        match self.filter_field {
            None => self.log.info(&format!(
                "Probe dump: t={now:.3} page={} store={} filter_field unknown",
                if page != 0 { format!("{page:#x}") } else { "0".into() },
                if store != 0 { format!("{store:#x}") } else { "0".into() },
            )),
            Some(field) => self.log.info(&format!(
                "Probe dump: t={now:.3} page={} store={} filter_field={field:+#x} filter_at=16 dwords={}",
                if page != 0 { format!("{page:#x}") } else { "0".into() },
                if store != 0 { format!("{store:#x}") } else { "0".into() },
                page_dwords(page as usize, field),
            )),
        }
    }

    fn probe_storage_diff(&mut self, page: u64, layer: u64, store: u64, name: &str) {
        if !self.probe_diff || name != STORAGE_NAME {
            return;
        }
        if (page as usize) < MIN_ADDR || (layer as usize) < MIN_ADDR || (store as usize) < MIN_ADDR {
            return;
        }
        let previous = self.probe_storage_store.get(&layer).copied();
        if previous == Some(store) {
            return;
        }
        self.probe_storage_store.insert(layer, store);
        if previous.is_none() {
            self.log.info(&format!("Probe diff: store {store:#x}"));
        } else if let Some(prev) = previous {
            self.log.info(&format!("Probe diff: store {prev:#x} -> {store:#x}"));
        }
        let base = match self.probe_store_base {
            Some(b) if store >= b => b,
            _ => {
                self.probe_store_base = Some(store);
                store
            }
        };
        self.log
            .info(&format!("Probe kind: {} (store {store:#x})", (store - base) / STORE_STRIDE));
        let regions = [
            ("page", page as usize, 0x20000usize),
            ("layer", layer as usize, 0x400),
            ("store", store as usize, 0x248),
        ];
        for (label, address, nbytes) in regions {
            let words = read_dwords(address, nbytes);
            let prior = self.probe_regions.get(label).cloned();
            self.probe_regions.insert(label.to_string(), words.clone());
            let Some(prior) = prior else {
                continue;
            };
            let mut logged = 0usize;
            let n = prior.len().min(words.len());
            let mut index = 0usize;
            while index < n {
                let old = prior[index];
                let new = words[index];
                if let (Some(old), Some(new)) = (old, new) {
                    if old != new && old < 0x100 && new < 0x100 {
                        self.log.info(&format!(
                            "Probe diff[{label}]: {:+#x} {}->{}",
                            index * 4,
                            super::ui::dword_text(Some(old)),
                            super::ui::dword_text(Some(new))
                        ));
                        logged = logged.saturating_add(1);
                        if logged >= PROBE_DIFF_CAP {
                            self.log
                                .info(&format!("Probe diff[{label}]: capped at {PROBE_DIFF_CAP}"));
                            break;
                        }
                    }
                }
                index = index.wrapping_add(1);
            }
            if logged == 0 {
                self.log.info(&format!("Probe diff[{label}]: no small-int changes"));
            }
        }
    }

    pub fn after_slots(&mut self, page: u64) {
        self.set_custom_store(0);
        self.drawing = 0;
        self.click_extra_filter(page);
    }

    #[allow(clippy::too_many_arguments)]
    pub fn before_slots(
        &mut self,
        page: u64,
        layer: u64,
        store: u64,
        slot_width: i32,
        slot_height: i32,
        render_height: i32,
        scroll: f32,
    ) -> SlotArgs {
        let mut out = SlotArgs {
            slot_width,
            slot_height,
            render_height,
            scroll,
        };
        if self.probe {
            self.probe_observe(page, layer, store, slot_width, render_height, scroll);
        }
        self.aim_storage_sort(page);
        self.drawing = if self.on_locker && self.container_layer != 0 {
            store
        } else {
            0
        };
        if self.drawing != 0 && layer as usize == self.container_layer {
            self.sort_target = store;
        }
        if self.remember_filter || self.remember_scroll {
            self.last_screen = if (page as usize) < MIN_ADDR { None } else { self.page_screen(page) };
        }
        if self.remember_filter {
            self.remember_filter_tab(page, store, render_height, slot_width);
        }
        if self.filter_field.is_some() {
            self.drop_disabled_filter(page);
            self.watch_custom_leave(page);
            self.sync_custom_marks();
        }
        self.set_custom_store(store);
        if self.remember_freighter {
            self.remember_freighter_tab(page, store, render_height);
        }
        if self.remember_scroll {
            if let Some(restored) = self.remember_scroll_y(layer, store, render_height, scroll) {
                out.scroll = restored;
            }
        }
        if self.shrink_chest && self.plan.is_some() && slot_width == CHEST_SLOT && render_height == CHEST_ROWS {
            if let Some(plan) = self.plan {
                let widths = [chest_panel_width(&plan), CHEST_WIDTH];
                if let Some((slot, rows)) = self.try_shrink(layer, CHEST_SLOT, CHEST_ROWS, &widths, "chest", store, false)
                {
                    out.slot_width = slot;
                    out.slot_height = slot;
                    out.render_height = rows;
                    return out;
                }
            }
        }
        if self.shrink_freighter
            && self.plan.is_some()
            && slot_width == VANILLA_SLOT
            && render_height == FREIGHTER_ROWS
        {
            if let Some((slot, rows)) = self.try_shrink(
                layer,
                VANILLA_SLOT,
                FREIGHTER_ROWS,
                &[FREIGHTER_WIDTH],
                "freighter",
                store,
                true,
            ) {
                out.slot_width = slot;
                out.slot_height = slot;
                out.render_height = rows;
                return out;
            }
        }
        let Some(plan) = self.plan else {
            return out;
        };
        if slot_width != VANILLA_SLOT {
            return out;
        }
        let Some(kind) = vanilla_kind(render_height) else {
            return out;
        };
        if layer == 0 {
            return out;
        }
        let (ident, layout) = element_info(layer as usize);
        let seen = layout.map(|l| l.w).unwrap_or(0.0);
        if ident != "INVENTORY" || (seen - plan.panel_w).abs() > 0.5 {
            return out;
        }
        let rows = if kind == "cargo" {
            plan.cargo_rows
        } else {
            plan.tech_rows
        };
        let slot = plan.slot.round() as i32;
        if self.logged.insert(kind.to_string()) {
            self.log.info(&format!(
                "{kind} grid: slot {slot_width} -> {slot}, rows in view {render_height} -> {rows}"
            ));
        }
        out.slot_width = slot;
        out.slot_height = slot;
        out.render_height = rows;
        out
    }
}

fn probe_now() -> f64 {
    static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    let start = START.get_or_init(Instant::now);
    Instant::now().duration_since(*start).as_secs_f64()
}

pub struct SlotArgs {
    pub slot_width: i32,
    pub slot_height: i32,
    pub render_height: i32,
    pub scroll: f32,
}
