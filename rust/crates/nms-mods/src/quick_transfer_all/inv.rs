//! Inventory store walks and NGui child walks used by send-all, deposit, and lock.

use std::collections::HashSet;

use nms_core::mem::{self, read_bytes, read_i32, read_ptr, read_u32, read_u64, read_u8, MIN_ADDR};

use crate::common::custom_filter;

pub const STORE_WIDTH: usize = 0x80;
pub const STORE_HEIGHT: usize = 0x82;
pub const STORE_ALLOC: usize = 0x88;
pub const STORE_COUNT: usize = 0x8C;
pub const STORE_DATA: usize = 0x90;
pub const STORE_NAME: usize = 0x104;
pub const ELEMENT_SIZE: usize = 0x30;
pub const ELEMENT_INDEX: usize = 0x10;
pub const ELEMENT_AMOUNT: usize = 0x18;
pub const ELEMENT_TYPE: usize = 0x24;
pub const ELEMENT_INSTALLED: usize = 0x29;
pub const SUBSTANCE: i32 = 0;
pub const TECHNOLOGY: i32 = 1;
pub const PRODUCT: i32 = 2;
pub const VANILLA_FILTERS: i32 = 5;
/// One past the last side-button filter id: 12 product categories, then Foods (17) and Custom (18)
/// from SmallerInventorySlots (`common::custom_filter`).
pub const FILTERS_END: i32 = VANILLA_FILTERS + 14;
pub const PRODUCT_CATEGORY: usize = 0x1E8;
pub const STORAGE_NAME: &[u8] = b"BLD_STORAGE_NAME";
pub const INV_SUIT: i32 = 0;
pub const INV_SUIT_TECH: i32 = 1;
pub const INV_SUIT_CARGO: i32 = 2;
pub const INV_SHIP: i32 = 4;
pub const INV_SHIP_TECH: i32 = 5;
pub const INV_SHIP_CARGO: i32 = 6;
pub const INV_FREIGHTER_TECH: i32 = 8;
pub const INV_VEHICLE_TECH: i32 = 0xB;
pub const INV_SUIT_KINDS: [i32; 2] = [INV_SUIT, INV_SUIT_CARGO];
pub const INV_SHIP_KINDS: [i32; 2] = [INV_SHIP, INV_SHIP_CARGO];
pub const INV_TECH_KINDS: [i32; 4] = [INV_SUIT_TECH, INV_SHIP_TECH, INV_FREIGHTER_TECH, INV_VEHICLE_TECH];
pub const INV_SUIT_ALL: [i32; 3] = [INV_SUIT, INV_SUIT_TECH, INV_SUIT_CARGO];
pub const DEST_TYPE: i32 = 7;
pub const DEST_SUIT: i32 = 0;
pub const DEST_BYTES: usize = 0x40;
pub const LIST_COUNT: usize = 0x4;
pub const LIST_DATA: usize = 0x8;
pub const CHESTS: std::ops::Range<i32> = 0x0C..0x16;
pub const VIEW_OFFSET: u64 = 0xC240;
pub const ELEMENT_DATA: usize = 0x48;
pub const DATA_ID: usize = 0x48;
pub const CHILD_MAP: usize = 0x150;
pub const CHILD_COUNT: usize = 0x6C;
pub const CHILD_ARRAY: usize = 0x70;
pub const IS_HIDDEN: usize = 0x61;
pub const PAGE_BUSY: usize = 0x14C84;
pub const SCREEN_FROM_FILTER: usize = 0x3C;
pub const COLUMNS: usize = 10;
pub const SLOT_CHILD_CAP: i32 = 512;
pub const PLAYER_AT: usize = 0xAAD0;
pub const SAVE_ID_AT: usize = 0x187E8;
pub const INV_AT: usize = 0x910;
pub const STORE_STRIDE: usize = 0x248;
pub const INV_COUNT: i32 = 33;
pub const SELL_PAIR_KIND: [(i32, i32); 3] = [(0, 2), (4, 6), (7, 9)];
pub const HOVER_STALE_SECS: f64 = 0.25;
pub const CONFIRM_NEAR_ZERO: f32 = 1.0 / 120.0;
pub const CONFIRM_INSTANT_ACTIONS: f32 = 0.1;
pub const STEAM_HOLD_ACTIONS: [&str; 2] = ["Unbound", "chat_pastehold"];
pub const STEAM_CLICK_ACTIONS: [&str; 1] = ["select"];

#[derive(Clone, Copy, Debug)]
pub struct Pin {
    pub id: [u8; 16],
    pub kind: i32,
    pub count: i32,
}

impl Pin {
    pub fn ident(&self) -> (i32, [u8; 16]) {
        (self.kind, self.id)
    }

    pub fn name(&self) -> String {
        id_name(&self.id)
    }
}

pub fn id_name(id: &[u8; 16]) -> String {
    let end = id.iter().position(|&b| b == 0).unwrap_or(id.len());
    String::from_utf8_lossy(&id[..end]).into_owned()
}

/// The page filter is one this mod understands. Foods and Custom count only while their switch is on
/// in SmallerInventorySlots (and their dim test is installed).
pub fn filter_ok(filter: i32) -> bool {
    filter_ok_with(filter, custom_filter::foods_on(), custom_filter::custom_on())
}

pub fn filter_ok_with(filter: i32, foods: bool, custom: bool) -> bool {
    if filter == custom_filter::FOODS_ID {
        return foods;
    }
    if filter == custom_filter::CUSTOM_ID {
        return custom;
    }
    (0..FILTERS_END).contains(&filter)
}

/// Held stack struct `S = page + 0x19670` (build 183791; was `0x19610`): source slot x, y
/// (`-1` when nothing is held) and source inventory pointer (null when nothing is held).
pub const HELD_X: usize = 0x196A0;
pub const HELD_Y: usize = 0x196A4;
pub const HELD_INV: usize = 0x196B0;

/// The player is carrying a stack picked up from a slot, the same three fields `DoInventorySlots`
/// tests before it treats a click as a drop.
pub fn holding_item(inv: usize, x: i32, y: i32) -> bool {
    inv >= MIN_ADDR && x >= 0 && y >= 0
}

/// [`holding_item`] read from the page.
pub fn page_holding_item(page: usize) -> bool {
    if page < MIN_ADDR {
        return false;
    }
    let inv = read_ptr(page.wrapping_add(HELD_INV)).unwrap_or(0);
    let x = read_i32(page.wrapping_add(HELD_X)).unwrap_or(-1);
    let y = read_i32(page.wrapping_add(HELD_Y)).unwrap_or(-1);
    holding_item(inv, x, y)
}

/// Slot (x, y) of an inventory element.
pub fn element_slot(element: usize) -> Option<(i32, i32)> {
    Some((
        read_i32(element.wrapping_add(ELEMENT_INDEX))?,
        read_i32(element.wrapping_add(ELEMENT_INDEX).wrapping_add(4))?,
    ))
}

/// What each occupied slot of a store holds, or `None` when the store cannot be read (so a failed
/// read never looks like an empty inventory).
pub fn store_contents(store: usize) -> Option<std::collections::HashMap<(i32, i32), custom_filter::Identity>> {
    let count = read_i32(store.wrapping_add(STORE_COUNT))?;
    let data = read_u64(store.wrapping_add(STORE_DATA))?;
    if data < MIN_ADDR as u64 || !(0..=10000).contains(&count) {
        return None;
    }
    Some(
        items(store)
            .into_iter()
            .filter_map(|(y, x, element)| item_identity(element).map(|ident| ((x, y), ident)))
            .collect(),
    )
}

pub fn item_identity(element: usize) -> Option<(i32, [u8; 16])> {
    let raw = read_bytes(element, 16)?;
    if raw.iter().all(|&b| b == 0) {
        return None;
    }
    let kind = read_i32(element.wrapping_add(ELEMENT_TYPE))?;
    let mut id = [0u8; 16];
    id.copy_from_slice(&raw);
    Some((kind, id))
}

pub fn items(store: usize) -> Vec<(i32, i32, usize)> {
    let Some(count) = read_i32(store.wrapping_add(STORE_COUNT)) else {
        return Vec::new();
    };
    let Some(data) = read_u64(store.wrapping_add(STORE_DATA)) else {
        return Vec::new();
    };
    if data < MIN_ADDR as u64 || !(0..=10000).contains(&count) {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut n = 0i32;
    while n < count {
        let element = (data as usize).wrapping_add((n as usize).wrapping_mul(ELEMENT_SIZE));
        let x = read_i32(element.wrapping_add(ELEMENT_INDEX)).unwrap_or(0);
        let y = read_i32(element.wrapping_add(ELEMENT_INDEX).wrapping_add(4)).unwrap_or(0);
        out.push((y, x, element));
        n = n.wrapping_add(1);
    }
    out.sort_by_key(|(y, x, _)| (*y, *x));
    out
}

pub fn slot_at(store: usize, x: i32, y: i32) -> usize {
    for (sy, sx, element) in items(store) {
        if sx == x && sy == y {
            return element;
        }
    }
    0
}

pub fn slot_valid(store: usize, x: i32, y: i32) -> bool {
    if !(0..64).contains(&x) || !(0..16).contains(&y) {
        return false;
    }
    let Some(bits) = read_u64(store.wrapping_add((y as usize).wrapping_mul(8))) else {
        return false;
    };
    (bits >> x) & 1 != 0
}

pub fn read_i16(addr: usize) -> Option<i16> {
    let lo = read_u8(addr)?;
    let hi = read_u8(addr.wrapping_add(1))?;
    Some(i16::from_le_bytes([lo, hi]))
}

pub fn empty_slot(store: usize, reserved: &HashSet<(i32, i32)>) -> Option<(i32, i32)> {
    let width = i32::from(read_i16(store.wrapping_add(STORE_WIDTH)).unwrap_or(0));
    let height = i32::from(read_i16(store.wrapping_add(STORE_HEIGHT)).unwrap_or(0));
    if !(0 < width && width <= 32) || !(0 < height && height <= 16) {
        return None;
    }
    let mut used: HashSet<(i32, i32)> = items(store).into_iter().map(|(sy, sx, _)| (sx, sy)).collect();
    used.extend(reserved.iter().copied());
    let mut y = 0i32;
    while y < height {
        let mut x = 0i32;
        while x < width {
            if !used.contains(&(x, y)) && slot_valid(store, x, y) {
                return Some((x, y));
            }
            x = x.wrapping_add(1);
        }
        y = y.wrapping_add(1);
    }
    None
}

pub fn store_remove_at(store: usize, index: i32) -> bool {
    let Some(count) = read_u32(store.wrapping_add(STORE_COUNT)) else {
        return false;
    };
    let Some(data) = read_u64(store.wrapping_add(STORE_DATA)) else {
        return false;
    };
    if data < MIN_ADDR as u64 || index < 0 || (index as u32) >= count {
        return false;
    }
    let left = count.wrapping_sub(index as u32).wrapping_sub(1);
    if left > 0 {
        let dest = (data as usize).wrapping_add((index as usize).wrapping_mul(ELEMENT_SIZE));
        let src = dest.wrapping_add(ELEMENT_SIZE);
        let bytes = (left as usize).wrapping_mul(ELEMENT_SIZE);
        if let Some(raw) = read_bytes(src, bytes) {
            unsafe {
                std::ptr::copy_nonoverlapping(raw.as_ptr(), dest as *mut u8, bytes);
            }
        }
    }
    unsafe { mem::write(store.wrapping_add(STORE_COUNT), count.wrapping_sub(1)) }
}

pub fn store_push(store: usize, blob: &[u8]) -> bool {
    if blob.len() != ELEMENT_SIZE {
        return false;
    }
    let Some(count) = read_u32(store.wrapping_add(STORE_COUNT)) else {
        return false;
    };
    let Some(cap) = read_u32(store.wrapping_add(STORE_ALLOC)) else {
        return false;
    };
    let Some(data) = read_u64(store.wrapping_add(STORE_DATA)) else {
        return false;
    };
    if data < MIN_ADDR as u64 || count >= cap {
        return false;
    }
    let dest = (data as usize).wrapping_add((count as usize).wrapping_mul(ELEMENT_SIZE));
    unsafe {
        std::ptr::copy_nonoverlapping(blob.as_ptr(), dest as *mut u8, ELEMENT_SIZE);
        mem::write(store.wrapping_add(STORE_COUNT), count.wrapping_add(1))
    }
}

pub fn lock_cargo_store(store: usize) -> bool {
    if store < MIN_ADDR {
        return false;
    }
    for (_y, _x, element) in items(store) {
        let kind = read_i32(element.wrapping_add(ELEMENT_TYPE)).unwrap_or(-1);
        if kind == SUBSTANCE || kind == PRODUCT {
            return true;
        }
        if kind == TECHNOLOGY && read_u8(element.wrapping_add(ELEMENT_INSTALLED)).unwrap_or(1) == 0 {
            return true;
        }
    }
    false
}

pub fn element_name(element: usize) -> String {
    if element < MIN_ADDR {
        return String::new();
    }
    let Some(data) = read_ptr(element.wrapping_add(ELEMENT_DATA)) else {
        return String::new();
    };
    let Some(raw) = read_bytes(data.wrapping_add(DATA_ID), 16) else {
        return String::new();
    };
    let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
    let Ok(text) = std::str::from_utf8(&raw[..end]) else {
        return String::new();
    };
    if text.is_empty() || !text.chars().all(|ch| ch.is_ascii() && !ch.is_control()) {
        return String::new();
    }
    text.to_string()
}

pub fn children(element: usize) -> Vec<(String, usize)> {
    if element < MIN_ADDR {
        return Vec::new();
    }
    let mut found = Vec::new();
    if let Some(table) = read_ptr(element.wrapping_add(CHILD_MAP)) {
        let count = read_i32(table.wrapping_add(0x1C)).unwrap_or(-1);
        let buckets_n = read_i32(table.wrapping_add(0x20)).unwrap_or(-1);
        let buckets = read_ptr(table.wrapping_add(8)).unwrap_or(0);
        if (0..=64).contains(&count) && (1..=256).contains(&buckets_n) && buckets >= MIN_ADDR {
            let mut index = 0i32;
            while index < buckets_n {
                let mut node = read_ptr(buckets.wrapping_add((index as usize).wrapping_mul(8))).unwrap_or(0);
                let mut hops = 0i32;
                while node >= MIN_ADDR && hops < 8 && found.len() < 48 {
                    if let Some(child) = read_ptr(node.wrapping_add(0x18)) {
                        let ident = element_name(child);
                        if !ident.is_empty() {
                            found.push((ident, child));
                        }
                    }
                    node = read_ptr(node.wrapping_add(0x28)).unwrap_or(0);
                    hops = hops.wrapping_add(1);
                }
                index = index.wrapping_add(1);
            }
        }
    }
    let extra = read_i32(element.wrapping_add(CHILD_COUNT)).unwrap_or(0);
    let array = read_ptr(element.wrapping_add(CHILD_ARRAY)).unwrap_or(0);
    if array >= MIN_ADDR && extra > 0 && extra <= 128 {
        let mut seen: HashSet<usize> = found.iter().map(|(_, c)| *c).collect();
        let mut index = 0i32;
        while index < extra {
            let child = read_ptr(array.wrapping_add((index as usize).wrapping_mul(8))).unwrap_or(0);
            if child >= MIN_ADDR && !seen.contains(&child) {
                let ident = element_name(child);
                if !ident.is_empty() {
                    found.push((ident, child));
                    seen.insert(child);
                }
            }
            index = index.wrapping_add(1);
        }
    }
    found
}

pub fn child_named(element: usize, name: &str) -> usize {
    children(element)
        .into_iter()
        .find(|(ident, _)| ident == name)
        .map(|(_, child)| child)
        .unwrap_or(0)
}

pub fn ordered_slots(layer: usize) -> Vec<usize> {
    if layer < MIN_ADDR {
        return Vec::new();
    }
    let extra = read_i32(layer.wrapping_add(CHILD_COUNT)).unwrap_or(0);
    let array = read_ptr(layer.wrapping_add(CHILD_ARRAY)).unwrap_or(0);
    if array < MIN_ADDR || extra <= 0 || extra > SLOT_CHILD_CAP {
        return Vec::new();
    }
    let mut slots = Vec::new();
    let mut index = 0i32;
    while index < extra {
        let child = read_ptr(array.wrapping_add((index as usize).wrapping_mul(8))).unwrap_or(0);
        let ident = element_name(child);
        if ident.starts_with("SLOT") {
            slots.push(child);
        }
        index = index.wrapping_add(1);
    }
    slots
}

pub fn store_name_bytes(store: usize, len: usize) -> Vec<u8> {
    read_bytes(store.wrapping_add(STORE_NAME), len).unwrap_or_default()
}

pub fn pair_kind(kind: i32) -> Option<i32> {
    SELL_PAIR_KIND.iter().find(|(a, _)| *a == kind).map(|(_, b)| *b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extra_filters_follow_their_switch() {
        for id in 0..17 {
            assert!(filter_ok_with(id, false, false), "{id}");
        }
        assert!(!filter_ok_with(custom_filter::FOODS_ID, false, true));
        assert!(filter_ok_with(custom_filter::FOODS_ID, true, false));
        assert!(!filter_ok_with(custom_filter::CUSTOM_ID, true, false));
        assert!(filter_ok_with(custom_filter::CUSTOM_ID, false, true));
        assert!(!filter_ok_with(FILTERS_END, true, true));
        assert!(!filter_ok_with(-1, true, true));
        assert_eq!(FILTERS_END, custom_filter::CUSTOM_ID + 1);
    }

    #[test]
    fn mark_identity_matches_the_dim_cave() {
        // The Custom dim cave compares the 16-byte id at element +0 and the type at +0x24, the same
        // pair `item_identity` reads for lock pins and marks.
        assert_eq!(ELEMENT_TYPE, custom_filter::ELEMENT_KIND);
        // Per-slot marks: the cave reads slot x, y at the same offsets `items` sorts by.
        assert_eq!(ELEMENT_INDEX, custom_filter::ELEMENT_X);
        assert_eq!(ELEMENT_INDEX + 4, custom_filter::ELEMENT_Y);
    }

    #[test]
    fn held_stack_means_a_drop() {
        let inv = 0x7FF6_0000_1000usize;
        assert!(holding_item(inv, 0, 0));
        assert!(holding_item(inv, 9, 6));
        // Nothing held: null inventory, or the -1 slot the game writes when the hand is empty.
        assert!(!holding_item(0, 0, 0));
        assert!(!holding_item(inv, -1, -1));
        assert!(!holding_item(inv, 3, -1));
        assert!(!page_holding_item(0));
        assert_eq!(HELD_INV - HELD_X, 0x10);
    }
}
