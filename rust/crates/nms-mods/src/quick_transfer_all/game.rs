//! Addresses read out of the game code, the Rust `_Game` plus start-of-session locates.
#![allow(clippy::missing_transmute_annotations)]

use std::collections::{HashMap, HashSet};

use nms_core::log::Log;
use nms_core::mem::{self, read_i32, read_u32, read_u64, read_u8, MIN_ADDR};
use nms_core::process::ProcessImage;
use nms_core::{Code, Pattern};
use nms_sdk::sigs::quick_transfer_all as sigs;

use crate::common::custom_filter;

use super::inv::{
    items, item_identity, CHESTS, LIST_COUNT, LIST_DATA, PRODUCT, PRODUCT_CATEGORY, SUBSTANCE, TECHNOLOGY,
    VANILLA_FILTERS, VIEW_OFFSET,
};

const FILTER_FIELD_AT: usize = 0x0A;
const ROOT_DISP_AT: usize = 0x45;
const PRODUCT_CALL_AT: usize = 0x51;
const LOOKUP_CALL_AT: usize = 0xCA;
const LIST_MOVE_AT: usize = 0x14E;
const LIST_MOVE_BYTES: &[u8] = &[
    0x44, 0x8B, 0x4D, 0x40, 0x4C, 0x8D, 0x45, 0xE0, 0x48, 0x8B, 0xD6, 0x48, 0x89, 0x45, 0xE8, 0x49, 0x8B, 0xCE, 0xE8,
];
const NEARBY_IDS_AT: usize = 0x03;
const NEARBY_COUNT_AT: usize = 0x09;
const BASE_TYPES_AT: usize = 0x25;
const OBJECT_TYPE_CALL_AT: usize = 0x46;
const BASE_CHESTS_CALL_AT: usize = 0x77;
const FREE_AT: usize = 0x6CD;
const FREE_BYTES: &[u8] = &[
    0x41, 0xB8, 0xFF, 0xFF, 0xFF, 0xFF, 0x48, 0x8D, 0x4D, 0xF0, 0x48, 0x8B, 0xD7, 0xE8,
];
const VIEW_GLOBAL_AT: usize = 0x64;
const VIEW_GLOBAL_BYTES: &[u8] = &[0x48, 0x8B, 0x0D];
const VIEW_CALL_AT: usize = 0x6B;
const VIEW_CALL_BYTES: &[u8] = &[0x48, 0x81, 0xC1, 0x40, 0xC2, 0x00, 0x00, 0xE8];
const CONFIRM_SLOW_HEAD: &[u8] = &[0x83, 0xF8, 0x02, 0x75, 0x0A, 0xF3, 0x0F, 0x10, 0x35];
const CONFIRM_INSTANT_CMP: &[u8] = &[0x40, 0x38, 0xBB, 0x74, 0xE9, 0x30, 0x00];
const CONFIRM_INSTANT_MOV: &[u8] = &[0x48, 0x8B, 0x1D];
const CONFIRM_INSTANT_OFF: usize = 0x30E974;
const SELL_TOAST_AT: usize = 25;
const AWARD_TOAST_AT: usize = 0x57;
const SELL_VOICE_AT: usize = 14;
const DEPOSIT_FIND_AT: usize = 0x3D;
const DEPOSIT_HASH_AT: usize = 0x5C;
const DEPOSIT_CHILD_AT: usize = 0x6A;
const DEPOSIT_CURSOR_AT: usize = 0x84;
const FILTER_STEAL: &[u8] = &[0x41, 0x8B, 0x44, 0x24, 0x24];

type ChestContents = HashMap<i32, HashSet<(i32, [u8; 16])>>;
type TransferFn = unsafe extern "system" fn(u64, u64, u64, u64, i32, u64) -> u8;
type ListTransferFn = unsafe extern "system" fn(u64, u64, u64, i32) -> i32;
type ObjectTypeFn = unsafe extern "system" fn(u64) -> i32;
type BaseChestsFn = unsafe extern "system" fn(u64, u64, u64) -> i32;
type FreeFn = unsafe extern "system" fn(u64, u64, i32);
type ViewFn = unsafe extern "system" fn(u64, i32, i32) -> u64;
type LookupFn = unsafe extern "system" fn(u64, u64) -> u64;
type WalletFn = unsafe extern "system" fn() -> u64;
type CursorFn = unsafe extern "system" fn(u64) -> u8;

/// Four `CONFIRM_SLOW_LOAD` RIP targets must be the same address, or hold timing falls back to tap.
pub fn agreed_target(targets: &[usize]) -> Option<usize> {
    let first = *targets.first()?;
    if targets.iter().all(|t| *t == first) {
        Some(first)
    } else {
        None
    }
}

#[derive(Clone, Copy)]
pub struct VoiceSite {
    pub site: usize,
    pub old: [u8; 5],
}

pub struct Game {
    pub transfer: usize,
    pub list_transfer: Option<usize>,
    pub filter_field: i32,
    pub root_pointer: usize,
    pub product_data: usize,
    pub consumable_lookup: usize,
    pub nearby_ids: usize,
    pub nearby_count: usize,
    pub base_types: u32,
    pub object_type: Option<usize>,
    pub base_chests: Option<usize>,
    pub free: Option<usize>,
    pub view: Option<usize>,
    pub view_owner: usize,
    pub wallet: Option<usize>,
    #[expect(dead_code)]
    pub save_id_ok: bool,
    pub confirm: Option<usize>,
    pub confirm_time_addr: usize,
    pub confirm_mouse_addr: usize,
    pub confirm_slow_addr: usize,
    pub confirm_ok: bool,
    pub instant_ui_addr: usize,
    pub pickup_ok: bool,
    pub sell_ok: bool,
    pub sell_voice_ok: bool,
    pub follow_ok: bool,
    pub lock_persist_ok: bool,
    pub deposit_lookup: Option<usize>,
    pub deposit_find: Option<usize>,
    pub deposit_hash: Option<usize>,
    pub deposit_child: Option<usize>,
    #[expect(dead_code)]
    pub deposit_patch: Option<usize>,
    #[expect(dead_code)]
    pub notify_patch: Option<usize>,
    #[expect(dead_code)]
    pub notify_toast: Option<usize>,
    pub filter: usize,
    pub lock_cursor: Option<usize>,
    pub sell_voice_sites: Vec<VoiceSite>,
    pub last_chests: HashSet<usize>,
}

impl Game {
    pub fn new(image: &ProcessImage) -> Result<Self, String> {
        let transfer_rva = find_sig(image, sigs::TRANSFER)
            .ok_or_else(|| "the game code it relies on was not found (game update?)".to_string())?;
        let filter_rva = find_sig(image, sigs::FILTER)
            .ok_or_else(|| "the game code it relies on was not found (game update?)".to_string())?;
        let transfer = image.va(transfer_rva);
        let block = image.va(filter_rva);
        let list_transfer = call_after(transfer, LIST_MOVE_AT, LIST_MOVE_BYTES).ok();
        let filter_field = read_i32(block.wrapping_add(FILTER_FIELD_AT)).unwrap_or(0);
        let root_pointer = rel32_va(block.wrapping_add(ROOT_DISP_AT)).unwrap_or(0);
        let product_data = rel32_va(block.wrapping_add(PRODUCT_CALL_AT)).unwrap_or(0);
        let consumable_lookup = rel32_va(block.wrapping_add(LOOKUP_CALL_AT)).unwrap_or(0);

        let mut nearby_ids = 0usize;
        let mut nearby_count = 0usize;
        let mut base_types = 0u32;
        let mut object_type = None;
        let mut base_chests = None;
        if let Some(found) = find_sig(image, sigs::BASE) {
            let found = image.va(found);
            nearby_ids = rel32_va(found.wrapping_add(NEARBY_IDS_AT)).unwrap_or(0);
            nearby_count = rel32_va(found.wrapping_add(NEARBY_COUNT_AT)).unwrap_or(0);
            base_types = read_u32(found.wrapping_add(BASE_TYPES_AT)).unwrap_or(0);
            object_type = rel32_va(found.wrapping_add(OBJECT_TYPE_CALL_AT));
            base_chests = rel32_va(found.wrapping_add(BASE_CHESTS_CALL_AT));
        }

        let mut free = None;
        if let Some(choices) = find_sig(image, sigs::CHOICES) {
            free = call_after(image.va(choices), FREE_AT, FREE_BYTES).ok();
        }

        let mut view = None;
        let mut view_owner = 0usize;
        if let Some(helper_rva) = find_sig(image, sigs::VIEW) {
            let helper = image.va(helper_rva);
            let got = mem::read_bytes(helper.wrapping_add(VIEW_GLOBAL_AT), VIEW_GLOBAL_BYTES.len());
            if got.as_deref() == Some(VIEW_GLOBAL_BYTES) {
                if let Some(owner) =
                    rel32_va(helper.wrapping_add(VIEW_GLOBAL_AT).wrapping_add(VIEW_GLOBAL_BYTES.len()))
                {
                    if let Ok(fn_va) = call_after(helper, VIEW_CALL_AT, VIEW_CALL_BYTES) {
                        view_owner = owner;
                        view = Some(fn_va);
                    }
                }
            }
        }

        Ok(Self {
            transfer,
            list_transfer,
            filter_field,
            root_pointer,
            product_data,
            consumable_lookup,
            nearby_ids,
            nearby_count,
            base_types,
            object_type,
            base_chests,
            free,
            view,
            view_owner,
            wallet: find_sig(image, sigs::WALLET).map(|rva| image.va(rva)),
            save_id_ok: find_sig(image, sigs::SAVE_ID).is_some(),
            confirm: find_sig(image, sigs::CONFIRM).map(|rva| image.va(rva)),
            confirm_time_addr: 0,
            confirm_mouse_addr: 0,
            confirm_slow_addr: 0,
            confirm_ok: false,
            instant_ui_addr: 0,
            pickup_ok: find_sig(image, sigs::ADD).is_some(),
            sell_ok: find_sig(image, sigs::TAKE).is_some()
                && find_sig(image, sigs::AWARD_UNITS).is_some()
                && find_sig(image, sigs::AWARD_NANITES).is_some()
                && find_sig(image, sigs::SELL_DEMAND).is_some(),
            sell_voice_ok: false,
            follow_ok: find_sig(image, sigs::CONFIRM).is_some(),
            lock_persist_ok: find_sig(image, sigs::WALLET).is_some() && find_sig(image, sigs::SAVE_ID).is_some(),
            deposit_lookup: find_sig(image, sigs::DEPOSIT_LOOKUP).map(|rva| image.va(rva)),
            deposit_find: None,
            deposit_hash: None,
            deposit_child: None,
            deposit_patch: find_sig(image, sigs::DEPOSIT_PATCH).map(|rva| image.va(rva)),
            notify_patch: find_sig(image, sigs::NOTIFY_PATCH).map(|rva| image.va(rva)),
            notify_toast: find_sig(image, sigs::NOTIFY_TOAST).map(|rva| image.va(rva)),
            filter: block,
            lock_cursor: None,
            sell_voice_sites: Vec::new(),
            last_chests: HashSet::new(),
        })
    }

    pub fn transfer(&self, dest: u64, page: u64, store: u64, index: u64, amount: i32, moved: u64) -> bool {
        if self.transfer == 0 {
            return false;
        }
        let f: TransferFn = unsafe { std::mem::transmute(self.transfer) };
        unsafe { f(dest, page, store, index, amount, moved) != 0 }
    }

    pub fn storage_object(&self) -> Option<u16> {
        if self.nearby_ids == 0 {
            return None;
        }
        let object_type = self.object_type?;
        let ids = read_u64(self.nearby_ids).unwrap_or(0);
        let count = read_u32(self.nearby_count).unwrap_or(0);
        if ids < MIN_ADDR as u64 || count > 4096 {
            return None;
        }
        let f: ObjectTypeFn = unsafe { std::mem::transmute(object_type) };
        for n in 0..count {
            let off = (ids as usize).wrapping_add((n as usize).wrapping_mul(2));
            let lo = read_u8(off).unwrap_or(0);
            let hi = read_u8(off.wrapping_add(1)).unwrap_or(0);
            let oid = u16::from_le_bytes([lo, hi]);
            let mut object_id = oid;
            let kind = unsafe { f((&mut object_id as *mut u16) as u64) };
            if (0..=0x10).contains(&kind) && (self.base_types >> kind) & 1 == 1 {
                return Some(oid);
            }
        }
        None
    }

    pub fn storage_ids(&mut self, object_id: u16) -> Option<ChestContents> {
        let base_chests = self.base_chests?;
        let free_va = self.free?;
        let view_va = self.view?;
        if self.view_owner < MIN_ADDR {
            return None;
        }
        let mut key = object_id;
        let mut found = [0u64; 2];
        let address = found.as_mut_ptr() as u64;
        let chests: BaseChestsFn = unsafe { std::mem::transmute(base_chests) };
        let free: FreeFn = unsafe { std::mem::transmute(free_va) };
        let mut kinds = Vec::new();
        unsafe { chests(0, (&mut key as *mut u16) as u64, address) };
        let count = read_i32((address as usize).wrapping_add(LIST_COUNT)).unwrap_or(0);
        let data = read_u64((address as usize).wrapping_add(LIST_DATA)).unwrap_or(0);
        if data >= MIN_ADDR as u64 && (0..=64).contains(&count) {
            for n in 0..count {
                let kind = read_i32((data as usize).wrapping_add((n as usize).wrapping_mul(4))).unwrap_or(-1);
                if CHESTS.contains(&kind) {
                    kinds.push(kind);
                }
            }
        }
        if found[1] != 0 {
            unsafe { free(address, found[1], -1) };
        }
        let owner = read_u64(self.view_owner).unwrap_or(0);
        if owner < MIN_ADDR as u64 {
            return None;
        }
        let view: ViewFn = unsafe { std::mem::transmute(view_va) };
        let mut present: ChestContents = HashMap::new();
        self.last_chests.clear();
        for kind in kinds {
            let chest = unsafe { view(owner.wrapping_add(VIEW_OFFSET), kind, -1) };
            if chest < MIN_ADDR as u64 {
                continue;
            }
            self.last_chests.insert(chest as usize);
            let ids = present.entry(kind).or_default();
            for (_y, _x, element) in items(chest as usize) {
                if let Some(ident) = item_identity(element) {
                    ids.insert(ident);
                }
            }
        }
        Some(present)
    }

    pub fn transfer_to_chest(&self, store: usize, slot: u64, object_id: u16, kind: i32, amount: i32) -> i32 {
        let Some(base_chests) = self.base_chests else {
            return 0;
        };
        let Some(list_transfer) = self.list_transfer else {
            return 0;
        };
        let mut key = object_id;
        let mut found = [0u64; 2];
        let address = found.as_mut_ptr() as u64;
        let chests: BaseChestsFn = unsafe { std::mem::transmute(base_chests) };
        unsafe { chests(0, (&mut key as *mut u16) as u64, address) };
        let count = read_i32((address as usize).wrapping_add(LIST_COUNT)).unwrap_or(0);
        let data = read_u64((address as usize).wrapping_add(LIST_DATA)).unwrap_or(0);
        if data < MIN_ADDR as u64 || !(1..=64).contains(&count) {
            if found[1] != 0 {
                if let Some(free_va) = self.free {
                    let free: FreeFn = unsafe { std::mem::transmute(free_va) };
                    unsafe { free(address, found[1], -1) };
                }
            }
            return 0;
        }
        let _ = unsafe { mem::write(data as usize, kind) };
        let _ = unsafe { mem::write((address as usize).wrapping_add(LIST_COUNT), 1i32) };
        let f: ListTransferFn = unsafe { std::mem::transmute(list_transfer) };
        unsafe { f(store as u64, slot, address, amount) }
    }

    /// `store` is the inventory the element sits in (per-slot Custom marks match on it).
    pub fn highlighted(&self, store: usize, element: usize, current_filter: i32) -> bool {
        // Custom (SmallerInventorySlots): only marked items (per item, or the marked stacks per
        // slot), of any kind, the same rule its dim cave applies.
        if current_filter == custom_filter::CUSTOM_ID {
            let (Some((kind, id)), Some((x, y))) = (item_identity(element), super::inv::element_slot(element)) else {
                return false;
            };
            return custom_filter::lit(kind, &id, custom_filter::Slot { store, x, y });
        }
        let kind = read_i32(element.wrapping_add(super::inv::ELEMENT_TYPE)).unwrap_or(-1);
        if kind == SUBSTANCE {
            return current_filter <= 1;
        }
        if kind == TECHNOLOGY {
            return current_filter == 0;
        }
        if kind != PRODUCT {
            return true;
        }
        if current_filter == 1 {
            return false;
        }
        let table = read_u64(self.root_pointer).unwrap_or(0).wrapping_add(0x60);
        if table < MIN_ADDR as u64 || self.product_data < MIN_ADDR {
            return true;
        }
        let product: LookupFn = unsafe { std::mem::transmute(self.product_data) };
        let data = unsafe { product(table, element as u64) };
        if data < MIN_ADDR as u64 {
            return true;
        }
        let data = data as usize;
        // Foods (SmallerInventorySlots): the same rule as its dim cave (custom_filter::foods_lit).
        if current_filter == custom_filter::FOODS_ID {
            let Some(cooking) = read_u32(data.wrapping_add(custom_filter::COOKING_VALUE)) else {
                return false;
            };
            let ingredient = read_u8(data.wrapping_add(custom_filter::COOKING_INGREDIENT)).unwrap_or(0);
            let category = read_i32(data.wrapping_add(PRODUCT_CATEGORY)).unwrap_or(-1);
            return custom_filter::foods_lit(cooking, ingredient, category);
        }
        if current_filter >= VANILLA_FILTERS {
            return read_i32(data.wrapping_add(PRODUCT_CATEGORY)) == Some(current_filter - VANILLA_FILTERS);
        }
        let deploys = read_u64(data.wrapping_add(0x110)).unwrap_or(0) != 0
            || read_u64(data.wrapping_add(0x118)).unwrap_or(0) != 0;
        if current_filter == 2 {
            return read_i32(data.wrapping_add(0x194)).unwrap_or(0) >= 100000
                || read_u8(data.wrapping_add(0x2F4)).unwrap_or(0) != 0
                || read_i32(data.wrapping_add(0x1E8)) == Some(2);
        }
        if current_filter == 3 {
            if deploys {
                return false;
            }
            if read_i32(data.wrapping_add(0x1E8)) == Some(1) {
                return true;
            }
            if self.consumable_lookup < MIN_ADDR {
                return false;
            }
            let lookup: LookupFn = unsafe { std::mem::transmute(self.consumable_lookup) };
            return unsafe { lookup(table, data.wrapping_add(0x150) as u64) } != 0;
        }
        if current_filter == 4 {
            return deploys;
        }
        true
    }

    pub fn player_state(&self) -> usize {
        let Some(wallet) = self.wallet else {
            return 0;
        };
        let f: WalletFn = unsafe { std::mem::transmute(wallet) };
        let game = unsafe { f() } as usize;
        if game < MIN_ADDR {
            0
        } else {
            game.wrapping_add(super::inv::PLAYER_AT)
        }
    }

    pub fn cursor_hit(&self, widget: u64) -> bool {
        let Some(cursor) = self.lock_cursor else {
            return false;
        };
        let f: CursorFn = unsafe { std::mem::transmute(cursor) };
        unsafe { f(widget) != 0 }
    }

    pub fn finish_locates(&mut self, image: &ProcessImage, log: &Log) {
        match locate_confirm_slow(image) {
            Some(slow) => {
                self.confirm_slow_addr = slow;
                self.confirm_time_addr = slow.wrapping_sub(0xC);
                self.confirm_mouse_addr = slow.wrapping_sub(4);
                self.confirm_ok = self.confirm_time_addr >= MIN_ADDR;
            }
            None => {
                log.info("confirm hold not changed");
            }
        }
        if let Some(confirm) = self.confirm {
            self.instant_ui_addr = instant_ui_addr(confirm);
        }
        if let Some(lookup) = self.deposit_lookup {
            self.lock_cursor = rel32_va(lookup.wrapping_add(DEPOSIT_CURSOR_AT).wrapping_add(1));
            self.deposit_find = rel32_va(lookup.wrapping_add(DEPOSIT_FIND_AT).wrapping_add(1));
            self.deposit_hash = rel32_va(lookup.wrapping_add(DEPOSIT_HASH_AT).wrapping_add(1));
            self.deposit_child = rel32_va(lookup.wrapping_add(DEPOSIT_CHILD_AT).wrapping_add(1));
        }
        let _steal_ok = mem::read_bytes(self.filter, FILTER_STEAL.len()).as_deref() == Some(FILTER_STEAL);
        self.sell_voice_sites.clear();
        add_voice(image, sigs::SELL_TOAST, SELL_TOAST_AT, &mut self.sell_voice_sites);
        add_voice(image, sigs::AWARD_UNITS, AWARD_TOAST_AT, &mut self.sell_voice_sites);
        add_voice(image, sigs::AWARD_NANITES, AWARD_TOAST_AT, &mut self.sell_voice_sites);
        let voice_n = self.sell_voice_sites.len();
        add_voice(image, sigs::SELL_VOICE, SELL_VOICE_AT, &mut self.sell_voice_sites);
        self.sell_voice_ok = self.sell_voice_sites.len() > voice_n;
        if !self.pickup_ok {
            log.info("suit lock pickup not changed");
        } else {
            log.info("suit lock pickup: skip mismatch on first empty");
        }
        if self.sell_ok {
            log.info("suit lock sell: surplus only, reserved stays");
        } else {
            log.info("suit lock sell floor not changed");
        }
        if self.sell_ok && !self.sell_voice_ok {
            log.info("suit lock sell voice not changed");
        }
        if !self.follow_ok {
            log.info("suit lock follow not changed");
        }
        if !self.lock_persist_ok {
            log.info("suit lock persist not changed");
        }
    }
}

fn find_sig(image: &ProcessImage, sig: &str) -> Option<usize> {
    Pattern::parse(sig).ok().and_then(|p| image.find_unique(&p).unique())
}

fn rel32_va(operand: usize) -> Option<usize> {
    let d = read_i32(operand)?;
    Some(operand.wrapping_add(4).wrapping_add(d as usize))
}

fn call_after(start: usize, at: usize, expected: &[u8]) -> Result<usize, String> {
    let addr = start.wrapping_add(at);
    let got = mem::read_bytes(addr, expected.len());
    if got.as_deref() != Some(expected) {
        return Err(format!("unexpected game code at +{at:#x} (game update?)"));
    }
    rel32_va(addr.wrapping_add(expected.len()))
        .ok_or_else(|| format!("unexpected game code at +{at:#x} (game update?)"))
}

fn locate_confirm_slow(image: &ProcessImage) -> Option<usize> {
    let pat = Pattern::parse(sigs::CONFIRM_SLOW_LOAD).ok()?;
    let hits = image.find_all(&pat, 8);
    if hits.len() != 4 {
        return None;
    }
    let mut targets = Vec::new();
    for rva in hits {
        let va = image.va(rva);
        if mem::read_bytes(va, CONFIRM_SLOW_HEAD.len()).as_deref() != Some(CONFIRM_SLOW_HEAD) {
            return None;
        }
        let disp = read_i32(va.wrapping_add(9))?;
        targets.push(va.wrapping_add(13).wrapping_add(disp as usize));
    }
    agreed_target(&targets)
}

fn instant_ui_addr(start: usize) -> usize {
    let Some(window) = mem::read_bytes(start, 0x100) else {
        return 0;
    };
    let at = window.windows(CONFIRM_INSTANT_CMP.len()).position(|w| w == CONFIRM_INSTANT_CMP);
    let Some(at) = at else {
        return 0;
    };
    if at < 7 {
        return 0;
    }
    let search = &window[..at];
    let mov = search
        .windows(CONFIRM_INSTANT_MOV.len())
        .rposition(|w| w == CONFIRM_INSTANT_MOV);
    let Some(mov) = mov else {
        return 0;
    };
    if at - mov > 16 {
        return 0;
    }
    let Some(global) = rel32_va(start.wrapping_add(mov).wrapping_add(3)) else {
        return 0;
    };
    let Some(app) = read_u64(global) else {
        return 0;
    };
    if (app as usize) < MIN_ADDR {
        return 0;
    }
    (app as usize).wrapping_add(CONFIRM_INSTANT_OFF)
}

fn add_voice(image: &ProcessImage, sig: &str, at: usize, sites: &mut Vec<VoiceSite>) {
    let Some(found) = find_sig(image, sig) else {
        return;
    };
    let site = image.va(found).wrapping_add(at);
    let Some(call) = mem::read_bytes(site, 5) else {
        return;
    };
    if call.len() == 5 && call[0] == 0xE8 {
        sites.push(VoiceSite {
            site,
            old: [call[0], call[1], call[2], call[3], call[4]],
        });
    }
}

#[cfg(test)]
mod tests {
    use super::agreed_target;

    #[test]
    fn confirm_slow_hits_agree() {
        assert_eq!(agreed_target(&[0x1000, 0x1000, 0x1000, 0x1000]), Some(0x1000));
    }

    #[test]
    fn confirm_slow_hits_disagree() {
        assert_eq!(agreed_target(&[0x1000, 0x1000, 0x2000, 0x1000]), None);
        assert_eq!(agreed_target(&[]), None);
    }
}
