//! Restock, sort extract/restore, and surplus on deposit and send-all.

use nms_core::mem::{self, read_i32, read_u32, read_u64, MIN_ADDR};

use crate::common::hooks::inventory_sort;

use super::inv::{
    empty_slot, item_identity, items, slot_at, store_push, store_remove_at, ELEMENT_AMOUNT, ELEMENT_INDEX, ELEMENT_SIZE,
    INV_SUIT_KINDS,
};
use super::runtime::Runtime;

impl Runtime {
    pub fn surplus_amount(&self, store: usize, x: i32, y: i32) -> Option<i32> {
        if !self.suit_lock {
            return None;
        }
        let inv = self.store_inv(store)?;
        let pin = self.lock_pins.get(&(inv, x, y))?;
        let element = slot_at(store, x, y);
        if element < MIN_ADDR {
            return Some(0);
        }
        let extra = read_i32(element.wrapping_add(ELEMENT_AMOUNT)).unwrap_or(0) - pin.count;
        Some(if extra > 0 { extra } else { 0 })
    }

    pub fn surplus_qty(&self, store: usize, x: i32, y: i32, amount: i32) -> Option<i32> {
        let extra = self.surplus_amount(store, x, y)?;
        if extra == 0 {
            return Some(0);
        }
        if amount > 0 {
            Some(amount.min(extra))
        } else {
            Some(extra)
        }
    }

    fn pin_count(&self, store: usize, x: i32, y: i32) -> i32 {
        let element = slot_at(store, x, y);
        if element < MIN_ADDR {
            0
        } else {
            read_i32(element.wrapping_add(ELEMENT_AMOUNT)).unwrap_or(0)
        }
    }

    fn pin_need(&self, store: usize, x: i32, y: i32, pin: &super::inv::Pin) -> i32 {
        let want = pin.count;
        if want <= 0 {
            return 0;
        }
        let element = slot_at(store, x, y);
        if element < MIN_ADDR {
            return want;
        }
        let Some(ident) = item_identity(element) else {
            return want;
        };
        if ident != pin.ident() {
            return 0;
        }
        let short = want - read_i32(element.wrapping_add(ELEMENT_AMOUNT)).unwrap_or(0);
        if short > 0 {
            short
        } else {
            0
        }
    }

    pub fn restock_suit(&mut self, page: u64, store: usize) -> i32 {
        if !self.suit_lock || self.restock_busy || self.deposit_busy || self.qt_busy {
            return 0;
        }
        if self.lock_pins.is_empty() {
            return 0;
        }
        let Some(inv) = self.store_inv(store) else {
            return 0;
        };
        let mut short = Vec::new();
        for (&(pinv, x, y), pin) in &self.lock_pins {
            if pinv != inv {
                continue;
            }
            let need = self.pin_need(store, x, y, pin);
            if need > 0 {
                short.push((x, y, *pin, need));
            }
        }
        if short.is_empty() {
            return 0;
        }
        let nearby = self.game.as_ref().map(|g| g.nearby_ids).unwrap_or(0);
        if nearby == 0 {
            if !self.restock_note {
                self.restock_note = true;
                self.log
                    .info("suit lock restock not changed: nearby storage was not found");
            }
            return 0;
        }
        let object_id = self.game.as_ref().and_then(|g| g.storage_object());
        let Some(object_id) = object_id else {
            self.log.info("suit lock restock: no storage in reach");
            return 0;
        };
        let present = self.game.as_mut().and_then(|g| g.storage_ids(object_id));
        let Some(present) = present else {
            self.log.info("suit lock restock: storage contents could not be read");
            return 0;
        };
        let last_chests: Vec<usize> = self
            .game
            .as_ref()
            .map(|g| g.last_chests.iter().copied().collect())
            .unwrap_or_default();
        if last_chests.is_empty() {
            self.log.info("suit lock restock: storage contents could not be read");
            return 0;
        }
        let mut have_ids = std::collections::HashSet::new();
        for ids in present.values() {
            have_ids.extend(ids.iter().copied());
        }
        let mut dest = [0u8; super::inv::DEST_BYTES];
        dest[..4].copy_from_slice(&super::inv::DEST_SUIT.to_le_bytes());
        let dest_at = dest.as_mut_ptr() as u64;
        let mut moved = 0i32;
        self.restock_busy = true;
        for (x, y, pin, mut need) in short {
            let ident = pin.ident();
            if !have_ids.contains(&ident) {
                continue;
            }
            let name = pin.name();
            let mut landed_off = false;
            for &chest in &last_chests {
                if need <= 0 || landed_off {
                    break;
                }
                if chest == store {
                    continue;
                }
                let chest_items: Vec<(i32, i32)> = items(chest).into_iter().map(|(cy, cx, _)| (cy, cx)).collect();
                for (cy, cx) in chest_items {
                    if need <= 0 || landed_off {
                        break;
                    }
                    let element = slot_at(chest, cx, cy);
                    if element < MIN_ADDR {
                        continue;
                    }
                    if item_identity(element) != Some(ident) {
                        continue;
                    }
                    let have = read_i32(element.wrapping_add(ELEMENT_AMOUNT)).unwrap_or(0);
                    let qty = need.min(have);
                    if qty <= 0 {
                        continue;
                    }
                    let before = self.pin_count(store, x, y);
                    let amounts = ident_amounts(store, ident);
                    let mut slot = [cx, cy];
                    let mut out = 0i32;
                    if let Some(game) = self.game.as_ref() {
                        game.transfer(
                            dest_at,
                            page,
                            chest as u64,
                            slot.as_mut_ptr() as u64,
                            qty,
                            (&mut out as *mut i32) as u64,
                        );
                    }
                    let gained = self.pin_count(store, x, y) - before;
                    if gained > 0 {
                        moved += gained;
                        need -= gained;
                    } else if out > 0 {
                        if let Some((fx, fy)) = pull_onto_pin(store, x, y, ident, &amounts, out) {
                            moved += out;
                            need -= out;
                            self.log.info(&format!(
                                "suit lock restock: {name} landed on {fx},{fy}, moved onto the pin at {x},{y}"
                            ));
                        } else {
                            landed_off = true;
                            self.log.info(&format!(
                                "suit lock restock: {name} for {x},{y} landed off the pin"
                            ));
                        }
                    }
                }
            }
        }
        if moved > 0 {
            self.log
                .info(&format!("suit lock restock: moved {moved} from object {object_id}"));
        } else {
            self.log
                .info(&format!("suit lock restock: nothing pulled from object {object_id}"));
        }
        self.restock_busy = false;
        moved
    }

    fn sort_extract(&mut self, store: usize) -> Vec<(i32, i32, Vec<u8>)> {
        let Some(inv) = self.store_inv(store) else {
            return Vec::new();
        };
        let wanted: std::collections::HashSet<(i32, i32)> = self
            .lock_pins
            .keys()
            .filter(|(pinv, _, _)| *pinv == inv)
            .map(|(_, x, y)| (*x, *y))
            .collect();
        if wanted.is_empty() {
            return Vec::new();
        }
        let Some(count) = read_u32(store.wrapping_add(super::inv::STORE_COUNT)) else {
            return Vec::new();
        };
        let Some(data) = read_u64(store.wrapping_add(super::inv::STORE_DATA)) else {
            return Vec::new();
        };
        if data < MIN_ADDR as u64 || count == 0 || count > 10000 {
            return Vec::new();
        }
        let mut held = Vec::new();
        let mut n = count as i32;
        n -= 1;
        while n >= 0 {
            let element = (data as usize).wrapping_add((n as usize).wrapping_mul(ELEMENT_SIZE));
            let x = read_i32(element.wrapping_add(ELEMENT_INDEX)).unwrap_or(0);
            let y = read_i32(element.wrapping_add(ELEMENT_INDEX).wrapping_add(4)).unwrap_or(0);
            if wanted.contains(&(x, y)) {
                if let Some(blob) = nms_core::mem::read_bytes(element, ELEMENT_SIZE) {
                    held.push((x, y, blob));
                    store_remove_at(store, n);
                }
            }
            n -= 1;
        }
        held
    }

    fn sort_restore(&self, store: usize, held: &[(i32, i32, Vec<u8>)], sorted: bool) -> i32 {
        let mut restored = 0i32;
        let mut reserved: std::collections::HashSet<(i32, i32)> = held.iter().map(|(x, y, _)| (*x, *y)).collect();
        if sorted && !reflow_around(store, &reserved) {
            self.log.info("suit lock sort: no room to lay the sort around the pins");
        }
        for (x, y, blob) in held {
            let occupant = slot_at(store, *x, *y);
            if occupant >= MIN_ADDR {
                if let Some(hole) = empty_slot(store, &reserved) {
                    let _ = unsafe { mem::write(occupant.wrapping_add(ELEMENT_INDEX), hole.0) };
                    let _ = unsafe { mem::write(occupant.wrapping_add(ELEMENT_INDEX).wrapping_add(4), hole.1) };
                    reserved.insert(hole);
                } else {
                    self.log.info(&format!(
                        "suit lock sort: no empty slot for the item that landed on {x},{y}"
                    ));
                }
            }
            if store_push(store, blob) {
                restored += 1;
            } else {
                self.log
                    .info(&format!("suit lock sort: could not put the pin back at {x},{y}"));
            }
        }
        restored
    }

    pub fn before_sort(&mut self, args: &mut inventory_sort::Args) {
        self.sort_hold.clear();
        self.sort_store = 0;
        if !self.suit_lock || self.lock_screen != Some(0) {
            return;
        }
        let store = args.inventory as usize;
        if store < MIN_ADDR || self.lock_pins.is_empty() {
            return;
        }
        let held = self.sort_extract(store);
        if !held.is_empty() {
            self.sort_hold = held;
            self.sort_store = store;
        }
    }

    pub fn after_sort(&mut self, args: &inventory_sort::Args, _ret: u64) {
        let held = std::mem::take(&mut self.sort_hold);
        let store = self.sort_store;
        self.sort_store = 0;
        if held.is_empty() || store < MIN_ADDR {
            return;
        }
        let restored = self.sort_restore(store, &held, args.inventory as usize == store);
        self.log.info(&format!(
            "suit lock sort: left {restored} pinned slot{}",
            if restored == 1 { "" } else { "s" }
        ));
    }
}

impl Runtime {
    pub fn store_inv(&self, store: usize) -> Option<i32> {
        if !self.lock_persist_ok {
            return None;
        }
        let player = self.game.as_ref().map(|g| g.player_state()).unwrap_or(0);
        if player < MIN_ADDR || store < player.wrapping_add(super::inv::INV_AT) {
            return None;
        }
        let delta = store.wrapping_sub(player.wrapping_add(super::inv::INV_AT));
        if !delta.is_multiple_of(super::inv::STORE_STRIDE) {
            return None;
        }
        let index = (delta / super::inv::STORE_STRIDE) as i32;
        if (0..super::inv::INV_COUNT).contains(&index) {
            Some(index)
        } else {
            None
        }
    }

    pub fn kind_store(&self, player: usize, kind: i32) -> usize {
        if player < MIN_ADDR || !(0..super::inv::INV_COUNT).contains(&kind) {
            return 0;
        }
        player
            .wrapping_add(super::inv::INV_AT)
            .wrapping_add((kind as usize).wrapping_mul(super::inv::STORE_STRIDE))
    }
}

/// The game packs a sort into cells from 0,0 in row order, pinned cells included because the pins
/// were taken out first. Lay that order back out with the pinned cells skipped.
fn reflow_around(store: usize, pinned: &std::collections::HashSet<(i32, i32)>) -> bool {
    let width = i32::from(super::inv::read_i16(store.wrapping_add(super::inv::STORE_WIDTH)).unwrap_or(0));
    let height = i32::from(super::inv::read_i16(store.wrapping_add(super::inv::STORE_HEIGHT)).unwrap_or(0));
    if !(0 < width && width <= 32) || !(0 < height && height <= 16) {
        return false;
    }
    let mut cells = Vec::new();
    for y in 0..height {
        for x in 0..width {
            if !pinned.contains(&(x, y)) && super::inv::slot_valid(store, x, y) {
                cells.push((x, y));
            }
        }
    }
    let elements = items(store);
    if elements.len() > cells.len() {
        return false;
    }
    for ((_, _, element), (x, y)) in elements.into_iter().zip(cells) {
        let _ = unsafe { mem::write(element.wrapping_add(ELEMENT_INDEX), x) };
        let _ = unsafe { mem::write(element.wrapping_add(ELEMENT_INDEX).wrapping_add(4), y) };
    }
    true
}

/// Amounts of one item per cell, so a restock that landed on another stack can be found.
fn ident_amounts(store: usize, ident: (i32, [u8; 16])) -> std::collections::HashMap<(i32, i32), i32> {
    items(store)
        .into_iter()
        .filter(|(_, _, element)| item_identity(*element) == Some(ident))
        .map(|(y, x, element)| ((x, y), read_i32(element.wrapping_add(ELEMENT_AMOUNT)).unwrap_or(0)))
        .collect()
}

/// Move `out` units that Quick Transfer stacked elsewhere onto the pin at `x,y`.
/// Returns the cell they came from.
fn pull_onto_pin(
    store: usize,
    x: i32,
    y: i32,
    ident: (i32, [u8; 16]),
    before: &std::collections::HashMap<(i32, i32), i32>,
    out: i32,
) -> Option<(i32, i32)> {
    let (from, src, src_now, was) = items(store).into_iter().find_map(|(sy, sx, element)| {
        if (sx, sy) == (x, y) || item_identity(element) != Some(ident) {
            return None;
        }
        let now = read_i32(element.wrapping_add(ELEMENT_AMOUNT))?;
        let was = before.get(&(sx, sy)).copied().unwrap_or(0);
        (now - was == out).then_some(((sx, sy), element, now, was))
    })?;
    let pin = slot_at(store, x, y);
    if pin >= MIN_ADDR {
        let have = read_i32(pin.wrapping_add(ELEMENT_AMOUNT))?;
        unsafe { mem::write(pin.wrapping_add(ELEMENT_AMOUNT), have + out) }.then_some(())?;
        if was == 0 {
            let data = read_u64(store.wrapping_add(super::inv::STORE_DATA))? as usize;
            store_remove_at(store, (src.wrapping_sub(data) / ELEMENT_SIZE) as i32);
        } else {
            unsafe { mem::write(src.wrapping_add(ELEMENT_AMOUNT), src_now - out) }.then_some(())?;
        }
    } else if was == 0 {
        unsafe { mem::write(src.wrapping_add(ELEMENT_INDEX), x) }.then_some(())?;
        unsafe { mem::write(src.wrapping_add(ELEMENT_INDEX).wrapping_add(4), y) }.then_some(())?;
    } else {
        let mut blob = mem::read_bytes(src, ELEMENT_SIZE)?;
        blob[ELEMENT_INDEX..ELEMENT_INDEX + 4].copy_from_slice(&x.to_le_bytes());
        blob[ELEMENT_INDEX + 4..ELEMENT_INDEX + 8].copy_from_slice(&y.to_le_bytes());
        blob[ELEMENT_AMOUNT..ELEMENT_AMOUNT + 4].copy_from_slice(&out.to_le_bytes());
        if !store_push(store, &blob) {
            return None;
        }
        unsafe { mem::write(src.wrapping_add(ELEMENT_AMOUNT), src_now - out) }.then_some(())?;
    }
    Some(from)
}

pub fn inv_is_suit(inv: i32) -> bool {
    INV_SUIT_KINDS.contains(&inv)
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::inv::{STORE_ALLOC, STORE_COUNT, STORE_DATA, STORE_HEIGHT, STORE_WIDTH};

    fn fake_store(cells: &[(i32, i32)], data: &mut [u8]) -> Vec<u8> {
        let mut store = vec![0u8; 0x248];
        store[0..8].copy_from_slice(&0xFu64.to_le_bytes());
        store[8..16].copy_from_slice(&0xFu64.to_le_bytes());
        store[STORE_WIDTH..STORE_WIDTH + 2].copy_from_slice(&4i16.to_le_bytes());
        store[STORE_HEIGHT..STORE_HEIGHT + 2].copy_from_slice(&2i16.to_le_bytes());
        store[STORE_ALLOC..STORE_ALLOC + 4].copy_from_slice(&8u32.to_le_bytes());
        store[STORE_COUNT..STORE_COUNT + 4].copy_from_slice(&(cells.len() as u32).to_le_bytes());
        store[STORE_DATA..STORE_DATA + 8].copy_from_slice(&(data.as_ptr() as u64).to_le_bytes());
        for (n, (x, y)) in cells.iter().enumerate() {
            let e = &mut data[n * ELEMENT_SIZE..(n + 1) * ELEMENT_SIZE];
            e[0] = b'A';
            e[ELEMENT_INDEX..ELEMENT_INDEX + 4].copy_from_slice(&x.to_le_bytes());
            e[ELEMENT_INDEX + 4..ELEMENT_INDEX + 8].copy_from_slice(&y.to_le_bytes());
            e[ELEMENT_AMOUNT..ELEMENT_AMOUNT + 4].copy_from_slice(&(n as i32 + 1).to_le_bytes());
        }
        store
    }

    #[test]
    fn sort_flows_around_pins_in_order() {
        let mut data = vec![0u8; 8 * ELEMENT_SIZE];
        let store = fake_store(&[(0, 0), (1, 0), (2, 0), (3, 0), (0, 1), (1, 1)], &mut data);
        let pinned = [(0, 0), (2, 0)].into_iter().collect();
        assert!(reflow_around(store.as_ptr() as usize, &pinned));
        let laid: Vec<(i32, i32, i32)> = items(store.as_ptr() as usize)
            .into_iter()
            .map(|(y, x, e)| (x, y, read_i32(e + ELEMENT_AMOUNT).unwrap()))
            .collect();
        assert_eq!(laid, vec![(1, 0, 1), (3, 0, 2), (0, 1, 3), (1, 1, 4), (2, 1, 5), (3, 1, 6)]);
    }
}
