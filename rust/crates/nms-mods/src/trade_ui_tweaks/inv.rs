//! Extra shop inventories and title chrome.

use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::Instant;

use nms_core::log::Log;
use nms_core::mem::{read_u32, read_u64, read_u8, MIN_ADDR};
use nms_core::process::ProcessImage;
use nms_core::{Code, Pattern};
use nms_sdk::sigs::trade_ui_tweaks;

use super::shop::decode_view_helper;

/// Game language for the shop title fallback, read once at start (a change shows after a restart).
pub static LANG: OnceLock<&'static str> = OnceLock::new();

/// Name shown for a container whose name was not read, in the game language.
fn chest_fallback() -> String {
    let text = crate::common::menu_text::ui(LANG.get().copied().unwrap_or("en"), "tut_storage_container");
    if text.is_empty() { SHOP_TITLE_CHEST_FALLBACK.to_string() } else { text.to_string() }
}
use super::shop::{SHOP_PUSH_AT, SHOP_PUSH_BYTES};
use super::util::{
    find_c_string, image_span, shop_children, shop_find_named, shop_kind_name, shop_list_kinds, SHOP_CHESTS,
    SHOP_INV_KINDS, SHOP_ITEM_TABLE_AT, SHOP_PAGE_ELEMENT, SHOP_REACH_SECONDS, SHOP_STORE_NAME_AT, SHOP_SWITCH_NAME,
    SHOP_TITLE_CHEST_FALLBACK, SHOP_TITLE_NAME, SHOP_TITLE_VT, SHOP_TOGGLE_ACT, SHOP_TOGGLE_LOAD_AT, SHOP_VIEW_OFFSET,
};

const SHOP_REACHABLE_PUSH_AT: usize = 0x1C2;
const SHOP_REACHABLE_AT: usize = 0x1CA;
const SHOP_REACHABLE_BYTES: &[u8] = &[0x48, 0x8B, 0xCB, 0xE8];
const SHOP_FREE_AT: usize = 0x6CD;
const SHOP_FREE_BYTES: &[u8] = &[0x41, 0xB8, 0xFF, 0xFF, 0xFF, 0xFF, 0x48, 0x8D, 0x4D, 0xF0, 0x48, 0x8B, 0xD7, 0xE8];
const SHOP_NEARBY_IDS_AT: usize = 0x03;
const SHOP_NEARBY_COUNT_AT: usize = 0x09;
// Build 183791 (was 0x24 / 0x3B / 0x6C): offsets into the SHOP_BASE hit, all inside the fixed-length pattern.
const SHOP_BASE_TYPES_AT: usize = 0x25;
const SHOP_OBJECT_TYPE_CALL_AT: usize = 0x46;
const SHOP_BASE_CHESTS_CALL_AT: usize = 0x77;

type HasInvFn = unsafe extern "system" fn(u64, u32) -> u8;
type ClearFn = unsafe extern "system" fn(u64) -> u64;
type ViewFn = unsafe extern "system" fn(u64, i32, i32) -> u64;
type ReachableFn = unsafe extern "system" fn(u64);
type FreeFn = unsafe extern "system" fn(u64, u64, i32);
type ObjectTypeFn = unsafe extern "system" fn(u64) -> i32;
type BaseChestsFn = unsafe extern "system" fn(u64, u64, u64) -> i32;
type TitleSetFn = unsafe extern "system" fn(u64, u64) -> u64;
type TogglePollFn = unsafe extern "system" fn(u64, u32, u32, u32) -> u8;

pub struct ShopInv {
    global_at: usize,
    has_va: usize,
    clear_va: usize,
    input_global: usize,
    input_at: u32,
    view_va: usize,
    view_global: usize,
    reachable_va: usize,
    free_va: usize,
    nearby_ids: usize,
    nearby_count: usize,
    base_types: u32,
    object_type_va: usize,
    base_chests_va: usize,
    pub chests_ok: bool,
    reach_at: Instant,
    reach_list: Vec<i32>,
    locs: HashMap<i32, usize>,
    loc_chest: usize,
    title_buf: [u8; 80],
    title_kids: bool,
    title_img_logged: bool,
    names: HashMap<i32, String>,
    view_cache: HashMap<i32, (Instant, bool)>,
    log: Option<std::sync::Arc<Log>>,
    image_start: usize,
    image_end: usize,
}

impl ShopInv {
    pub fn new(image: &ProcessImage) -> Result<Self, String> {
        let has_pat = Pattern::parse(trade_ui_tweaks::SHOP_HAS_INV).map_err(|e| e.to_string())?;
        let glob_pat = Pattern::parse(trade_ui_tweaks::SHOP_HAS_GLOBAL).map_err(|e| e.to_string())?;
        let clear_pat = Pattern::parse(trade_ui_tweaks::SHOP_INV_CLEAR).map_err(|e| e.to_string())?;
        let tog_pat = Pattern::parse(trade_ui_tweaks::SHOP_TOGGLE).map_err(|e| e.to_string())?;
        let Some(has_rva) = image.find_unique(&has_pat).unique() else {
            return Err("game code not found (game update?)".into());
        };
        let Some(glob_rva) = image.find_unique(&glob_pat).unique() else {
            return Err("game code not found (game update?)".into());
        };
        let Some(clear_rva) = image.find_unique(&clear_pat).unique() else {
            return Err("game code not found (game update?)".into());
        };
        let Some(tog_rva) = image.find_unique(&tog_pat).unique() else {
            return Err("game code not found (game update?)".into());
        };
        let lea_rva = glob_rva;
        let Some(glob_head) = image.read(lea_rva, 3) else {
            return Err("unexpected game code (game update?)".into());
        };
        if glob_head != [0x48, 0x8B, 0x0D] {
            return Err("unexpected game code (game update?)".into());
        }
        let global_at = image
            .rel32_target(lea_rva.wrapping_add(3))
            .map(|r| image.va(r))
            .ok_or_else(|| "unexpected game code (game update?)".to_string())?;
        let has_va = image.va(has_rva);
        let clear_va = image.va(clear_rva);
        let tog_va = image.va(tog_rva);
        let Some(tog_head) = image.read(tog_rva, 3) else {
            return Err("unexpected game code (game update?)".into());
        };
        if tog_head != [0x48, 0x8B, 0x0D] {
            return Err("unexpected game code (game update?)".into());
        }
        let load_rva = tog_rva.wrapping_add(SHOP_TOGGLE_LOAD_AT);
        let Some(load_head) = image.read(load_rva, 3) else {
            return Err("unexpected game code (game update?)".into());
        };
        if load_head != [0x48, 0x8B, 0x89] {
            return Err("unexpected game code (game update?)".into());
        }
        let input_global = image
            .rel32_target(tog_rva.wrapping_add(3))
            .map(|r| image.va(r))
            .ok_or_else(|| "unexpected game code (game update?)".to_string())?;
        let input_at = read_u32(tog_va.wrapping_add(SHOP_TOGGLE_LOAD_AT + 3)).unwrap_or(0);
        if input_at < 8 {
            return Err("unexpected game code (game update?)".into());
        }
        let (image_start, image_end) = image_span(image);
        let mut inv = Self {
            global_at,
            has_va,
            clear_va,
            input_global,
            input_at,
            view_va: 0,
            view_global: 0,
            reachable_va: 0,
            free_va: 0,
            nearby_ids: 0,
            nearby_count: 0,
            base_types: 0,
            object_type_va: 0,
            base_chests_va: 0,
            chests_ok: false,
            reach_at: Instant::now(),
            reach_list: Vec::new(),
            locs: HashMap::new(),
            loc_chest: 0,
            title_buf: [0u8; 80],
            title_kids: false,
            title_img_logged: false,
            names: HashMap::new(),
            view_cache: HashMap::new(),
            log: None,
            image_start,
            image_end,
        };
        inv.init_titles(image);
        if let Some(helper_rva) = Pattern::parse(trade_ui_tweaks::SHOP_VIEW)
            .ok()
            .and_then(|p| image.find_unique(&p).unique())
        {
            if let Some((vg, vv)) = decode_view_helper(image, helper_rva) {
                inv.view_global = vg;
                inv.view_va = vv;
            }
        }
        if inv.init_chests(image).is_err() {
            inv.chests_ok = false;
        }
        Ok(inv)
    }

    pub fn attach_log(&mut self, log: std::sync::Arc<Log>) {
        self.log = Some(log);
    }

    fn init_titles(&mut self, image: &ProcessImage) {
        let title_locs: &[(i32, &[u8])] = &[
            (0, b"TRADE_SUIT_INVENTORY_L"),
            (4, b"TRADE_SHIP_INVENTORY_L"),
            (7, b"TRADE_FREIGHT_INVENTORY_L"),
            (0x0A, b"ALL_VEHICLES"),
            (0x20, b"UI_BIGGS_STORE_BOX_U"),
        ];
        for (kind, ident) in title_locs {
            let mut needle = ident.to_vec();
            needle.push(0);
            if let Some(rva) = find_c_string(image, &needle) {
                self.locs.insert(*kind, image.va(rva));
            }
        }
        let mut chest = b"STORAGE_CONTAINER_INV".to_vec();
        chest.push(0);
        if let Some(rva) = find_c_string(image, &chest) {
            self.loc_chest = image.va(rva);
        }
    }

    fn init_chests(&mut self, image: &ProcessImage) -> Result<(), String> {
        let choices_pat = Pattern::parse(trade_ui_tweaks::SHOP_CHOICES).map_err(|e| e.to_string())?;
        let base_pat = Pattern::parse(trade_ui_tweaks::SHOP_BASE).map_err(|e| e.to_string())?;
        let Some(choices_rva) = image.find_unique(&choices_pat).unique() else {
            return Err("game code not found (game update?)".into());
        };
        let Some(block_rva) = image.find_unique(&base_pat).unique() else {
            return Err("game code not found (game update?)".into());
        };
        let push = super::util::call_after_rva(image, choices_rva, SHOP_PUSH_AT, SHOP_PUSH_BYTES)
        .ok_or_else(|| "unexpected game code (game update?)".to_string())?;
        let reach_push = super::util::call_after_rva(
            image,
            choices_rva,
            SHOP_REACHABLE_PUSH_AT,
            SHOP_REACHABLE_BYTES,
        )
        .ok_or_else(|| "unexpected game code (game update?)".to_string())?;
        if reach_push != push {
            return Err("unexpected game code (game update?)".into());
        }
        let reachable_rva = super::util::call_after_rva(
            image,
            choices_rva,
            SHOP_REACHABLE_AT,
            SHOP_REACHABLE_BYTES,
        )
        .ok_or_else(|| "unexpected game code (game update?)".to_string())?;
        let free_rva = super::util::call_after_rva(image, choices_rva, SHOP_FREE_AT, SHOP_FREE_BYTES)
            .ok_or_else(|| "unexpected game code (game update?)".to_string())?;
        self.reachable_va = image.va(reachable_rva);
        self.free_va = image.va(free_rva);
        let site = block_rva;
        self.nearby_ids = image
            .rel32_target(site.wrapping_add(SHOP_NEARBY_IDS_AT))
            .map(|r| image.va(r))
            .ok_or_else(|| "unexpected game code (game update?)".to_string())?;
        self.nearby_count = image
            .rel32_target(site.wrapping_add(SHOP_NEARBY_COUNT_AT))
            .map(|r| image.va(r))
            .ok_or_else(|| "unexpected game code (game update?)".to_string())?;
        self.base_types = read_u32(image.va(site).wrapping_add(SHOP_BASE_TYPES_AT)).unwrap_or(0);
        self.object_type_va = image
            .rel32_target(site.wrapping_add(SHOP_OBJECT_TYPE_CALL_AT))
            .map(|r| image.va(r))
            .ok_or_else(|| "unexpected game code (game update?)".to_string())?;
        self.base_chests_va = image
            .rel32_target(site.wrapping_add(SHOP_BASE_CHESTS_CALL_AT))
            .map(|r| image.va(r))
            .ok_or_else(|| "unexpected game code (game update?)".to_string())?;
        self.chests_ok = true;
        Ok(())
    }

    fn root(&self) -> u64 {
        read_u64(self.global_at).unwrap_or(0).wrapping_add(0xB940)
    }

    pub fn switch_has(&self, kind: u32) -> bool {
        let root = self.root();
        if root < MIN_ADDR as u64 {
            return false;
        }
        let has: HasInvFn = unsafe { std::mem::transmute(self.has_va) };
        unsafe { has(root, kind) != 0 }
    }

    pub fn view_has(&mut self, kind: i32) -> bool {
        let now = Instant::now();
        if let Some((when, value)) = self.view_cache.get(&kind) {
            if now.duration_since(*when).as_secs_f64() < SHOP_REACH_SECONDS {
                return *value;
            }
        }
        let mut value = false;
        if self.view_va != 0 && self.view_global != 0 {
            if let Some(owner) = read_u64(self.view_global) {
                if owner != 0 {
                    let view: ViewFn = unsafe { std::mem::transmute(self.view_va) };
                    let ret = unsafe { view(owner.wrapping_add(SHOP_VIEW_OFFSET), kind, -1) };
                    value = ret != 0;
                }
            }
        }
        self.view_cache.insert(kind, (now, value));
        value
    }

    fn collect<F: FnOnce(u64)>(&self, fill: F) -> Vec<i32> {
        let mut found = [0u64; 2];
        let addr = found.as_mut_ptr() as u64;
        fill(addr);
        let kinds: Vec<i32> = shop_list_kinds(addr as usize)
            .into_iter()
            .filter(|k| SHOP_CHESTS.contains(k))
            .collect();
        if found[1] != 0 {
            let free: FreeFn = unsafe { std::mem::transmute(self.free_va) };
            unsafe { free(found.as_mut_ptr() as u64, found[1], -1) };
        }
        kinds
    }

    fn nearby_bases(&self) -> Vec<u16> {
        let ids = read_u64(self.nearby_ids).unwrap_or(0);
        let count = read_u32(self.nearby_count).unwrap_or(0);
        if ids < MIN_ADDR as u64 || count > 4096 {
            return Vec::new();
        }
        let ids = ids as usize;
        let mut bases = Vec::new();
        for n in 0..count {
            let off = ids.wrapping_add((n as usize).wrapping_mul(2));
            let lo = read_u8(off).unwrap_or(0);
            let hi = read_u8(off.wrapping_add(1)).unwrap_or(0);
            let oid = u16::from_le_bytes([lo, hi]);
            let mut object_id = oid;
            let object_type: ObjectTypeFn = unsafe { std::mem::transmute(self.object_type_va) };
            let kind = unsafe { object_type((&mut object_id as *mut u16) as u64) };
            if (0..=0x10).contains(&kind) && (self.base_types >> kind) & 1 == 1 {
                bases.push(oid);
            }
        }
        bases
    }

    pub fn reachable_chests(&mut self) -> Vec<i32> {
        if !self.chests_ok {
            return Vec::new();
        }
        let now = Instant::now();
        if now.duration_since(self.reach_at).as_secs_f64() < SHOP_REACH_SECONDS {
            return self.reach_list.clone();
        }
        let mut chests = Vec::new();
        for object_id in self.nearby_bases() {
            let mut key = object_id;
            let key_ptr = (&mut key as *mut u16) as u64;
            let base_chests: BaseChestsFn = unsafe { std::mem::transmute(self.base_chests_va) };
            chests.extend(self.collect(|found| unsafe {
                base_chests(0, key_ptr, found);
            }));
        }
        let reachable: ReachableFn = unsafe { std::mem::transmute(self.reachable_va) };
        chests.extend(self.collect(|found| unsafe {
            reachable(found);
        }));
        let mut uniq = Vec::new();
        for k in chests {
            if !uniq.contains(&k) {
                uniq.push(k);
            }
        }
        self.reach_at = now;
        self.reach_list = uniq.clone();
        uniq
    }

    pub fn has(&mut self, kind: i32) -> bool {
        if kind == 0 {
            return true;
        }
        if SHOP_CHESTS.contains(&kind) {
            return self.reachable_chests().contains(&kind);
        }
        if kind == 4 {
            return self.switch_has(4) || self.view_has(4);
        }
        if kind == 7 || kind == 0x0A || kind == 0x20 {
            return self.switch_has(kind as u32);
        }
        false
    }

    pub fn cycle(&mut self) -> Vec<i32> {
        let row: Vec<i32> = SHOP_INV_KINDS
            .iter()
            .copied()
            .filter(|&kind| kind == 0 || self.has(kind))
            .collect();
        if row.is_empty() {
            vec![0]
        } else {
            row
        }
    }

    pub fn next_kind(&mut self, current: i32) -> i32 {
        let row = self.cycle();
        if let Some(pos) = row.iter().position(|&k| k == current) {
            let next = (pos + 1) % row.len();
            row[next]
        } else {
            row[0]
        }
    }

    pub fn clear_table(&self, page: u64) {
        if page < MIN_ADDR as u64 {
            return;
        }
        let clear: ClearFn = unsafe { std::mem::transmute(self.clear_va) };
        let _ = unsafe { clear(page.wrapping_add(SHOP_ITEM_TABLE_AT)) };
    }

    pub fn toggle_held(&self) -> bool {
        let owner = read_u64(self.input_global).unwrap_or(0);
        let inp = if owner != 0 {
            read_u64((owner as usize).wrapping_add(self.input_at as usize)).unwrap_or(0)
        } else {
            0
        };
        let vtable = read_u64(inp as usize).unwrap_or(0);
        let fn_ptr = read_u64((vtable as usize).wrapping_add(0x20)).unwrap_or(0);
        if fn_ptr < MIN_ADDR as u64 {
            return false;
        }
        let poll: TogglePollFn = unsafe { std::mem::transmute(fn_ptr as usize) };
        unsafe { poll(inp, SHOP_TOGGLE_ACT, 1, 0xFFFF_FFFF) != 0 }
    }

    pub fn switch_widget(&self, page: u64) -> u64 {
        if page < MIN_ADDR as u64 {
            return 0;
        }
        let root = read_u64((page as usize).wrapping_add(SHOP_PAGE_ELEMENT as usize)).unwrap_or(0);
        shop_find_named(root, SHOP_SWITCH_NAME, 6, 64)
    }

    pub fn is_switch(&self, page: u64, layer: u64) -> bool {
        if layer == 0 {
            return false;
        }
        if super::util::shop_element_name(layer) == SHOP_SWITCH_NAME {
            return true;
        }
        let widget = self.switch_widget(page);
        if widget == 0 {
            return false;
        }
        if layer == widget {
            return true;
        }
        shop_children(widget).iter().any(|(_, child)| *child == layer)
    }

    pub fn container_name(&self, kind: i32) -> String {
        if self.view_va == 0 || self.view_global == 0 || !SHOP_CHESTS.contains(&kind) {
            return String::new();
        }
        let owner = read_u64(self.view_global).unwrap_or(0);
        if owner == 0 {
            return String::new();
        }
        let view: ViewFn = unsafe { std::mem::transmute(self.view_va) };
        let ret = unsafe { view(owner.wrapping_add(SHOP_VIEW_OFFSET), kind, -1) };
        if ret == 0 {
            return String::new();
        }
        let Some(raw) = nms_core::mem::read_bytes(ret as usize + SHOP_STORE_NAME_AT as usize, 64) else {
            return String::new();
        };
        let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
        let text = String::from_utf8_lossy(&raw[..end]);
        if text.is_empty() || !text.chars().all(|c| !c.is_control()) || text == "BLD_STORAGE_NAME" {
            return String::new();
        }
        text.into_owned()
    }

    pub fn title_widget(&self, page: u64) -> u64 {
        if page < MIN_ADDR as u64 {
            return 0;
        }
        let root = read_u64((page as usize).wrapping_add(SHOP_PAGE_ELEMENT as usize)).unwrap_or(0);
        shop_find_named(root, SHOP_TITLE_NAME, 6, 64)
    }

    pub fn remember_names(&mut self) {
        let mut names = HashMap::new();
        for kind in self.reachable_chests() {
            names.insert(kind, self.container_name(kind));
        }
        self.names = names;
    }

    fn title_ptr(&mut self, kind: i32) -> u64 {
        if SHOP_CHESTS.contains(&kind) {
            let name = self
                .names
                .get(&kind)
                .cloned()
                .unwrap_or_else(chest_fallback);
            // UTF-8 (player names and the fallback may be non-ASCII): cut on a character boundary.
            self.title_buf = crate::common::menu_text::fixed(&format!("<IMG>INVCARGO<> {name}"));
            return self.title_buf.as_ptr() as u64;
        }
        self.locs.get(&kind).copied().unwrap_or(0) as u64
    }

    pub fn set_title(&mut self, page: u64, kind: i32) -> String {
        let widget = self.title_widget(page);
        let loc = self.title_ptr(kind);
        if widget == 0 || loc == 0 {
            return String::new();
        }
        let vtable = read_u64(widget as usize).unwrap_or(0);
        let fn_ptr = read_u64((vtable as usize).wrapping_add(SHOP_TITLE_VT as usize)).unwrap_or(0);
        let vtable = vtable as usize;
        let fn_ptr = fn_ptr as usize;
        if !(self.image_start <= vtable && vtable < self.image_end)
            || !(self.image_start <= fn_ptr && fn_ptr < self.image_end)
        {
            return String::new();
        }
        if !self.title_kids {
            self.title_kids = true;
            if let Some(log) = &self.log {
                let kids: Vec<_> = shop_children(widget).into_iter().map(|(id, _)| id).collect();
                let switch = self.switch_widget(page);
                let switch_kids = if switch != 0 {
                    shop_children(switch)
                        .into_iter()
                        .map(|(id, _)| id)
                        .collect::<Vec<_>>()
                } else {
                    Vec::new()
                };
                log.info(&format!(
                    "Shop title chrome: {} children={} switch={}",
                    SHOP_TITLE_NAME,
                    if kids.is_empty() { "-".into() } else { format!("{kids:?}") },
                    if switch_kids.is_empty() {
                        "-".into()
                    } else {
                        format!("{switch_kids:?}")
                    }
                ));
            }
        }
        if SHOP_CHESTS.contains(&kind) && !self.title_img_logged {
            self.title_img_logged = true;
            if let Some(log) = &self.log {
                log.info(
                    "Shop title storage image: INVCARGO glyph, art from \
                     ICONS/CATEGORIES/INVCAT.BASE.DDS (TransferBase)",
                );
            }
        }
        let setter: TitleSetFn = unsafe { std::mem::transmute(fn_ptr as usize) };
        let _ = unsafe { setter(widget, loc) };
        self.names
            .get(&kind)
            .cloned()
            .unwrap_or_else(|| shop_kind_name(kind))
    }
}
