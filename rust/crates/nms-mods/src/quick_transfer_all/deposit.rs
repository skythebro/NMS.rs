//! Quick Deposit: page scope, storage object / CHESTS 0x0C..0x15, notices.

use nms_core::mem::{read_i32, read_u8, MIN_ADDR};

use super::inv::{
    filter_ok, item_identity, items, lock_cargo_store, store_name_bytes, DEST_BYTES, DEST_TYPE, ELEMENT_INSTALLED,
    ELEMENT_TYPE, INV_SHIP_KINDS, INV_SUIT_KINDS, INV_TECH_KINDS, STORAGE_NAME, TECHNOLOGY,
};
use super::runtime::Runtime;

impl Runtime {
    pub fn deposit_held(&mut self) -> bool {
        let hovered = self.deposit_hover.as_mut().is_some_and(|h| h.hovered());
        if !hovered {
            return false;
        }
        self.click_held()
    }

    pub fn click_held(&mut self) -> bool {
        if crate::common::input::async_key_down(super::runtime::MOUSE_LEFT) {
            return true;
        }
        if crate::common::input::face_button_a_xinput() {
            return true;
        }
        self.steam_select_down()
    }

    pub fn mouse_down(&self) -> bool {
        crate::common::input::async_key_down(super::runtime::MOUSE_LEFT)
    }

    pub fn deposit_page_source(&mut self, page: u64, store: usize) -> Option<&'static str> {
        if store < MIN_ADDR {
            return None;
        }
        let kind = self.store_inv(store);
        if kind.is_none() && self.game.as_ref().map(|g| g.player_state()).unwrap_or(0) < MIN_ADDR {
            if !self.deposit_scope_off {
                self.deposit_scope_off = true;
                self.log.info("Deposit skipped: player inventories were not found");
            }
            return None;
        }
        let screen = self.page_screen(page);
        if kind.is_some_and(|k| INV_TECH_KINDS.contains(&k)) {
            return None;
        }
        if screen == Some(0) {
            return if kind.is_some_and(|k| INV_SUIT_KINDS.contains(&k)) {
                Some("suit")
            } else {
                None
            };
        }
        if kind.is_some_and(|k| INV_SUIT_KINDS.contains(&k)) {
            return None;
        }
        if kind.is_some_and(|k| INV_SHIP_KINDS.contains(&k)) {
            return if !lock_cargo_store(store) && !items(store).is_empty() {
                None
            } else {
                Some("ship")
            };
        }
        if kind.is_some() {
            return if !lock_cargo_store(store) && !items(store).is_empty() {
                None
            } else {
                Some("other")
            };
        }
        if lock_cargo_store(store) {
            return Some(if screen == Some(3) { "ship" } else { "other" });
        }
        None
    }

    pub fn try_deposit_page(&mut self, page: u64, store: usize) {
        if store < MIN_ADDR || !self.deposit_stores.is_empty() {
            return;
        }
        let Some(source) = self.deposit_page_source(page, store) else {
            return;
        };
        self.deposit_stores.insert(store);
        self.log.info(&format!("Deposit source: {source}"));
        self.deposit(page, store);
        self.write_deposit_osd();
    }

    pub fn write_deposit_osd(&mut self) {
        let Some(note) = self.button_notify.as_ref() else {
            return;
        };
        let lang = self.lang;
        let text = if self.deposit_sent > 0 {
            super::text::count(lang, "deposited", self.deposit_sent)
        } else if self.deposit_block == "no_storage" {
            super::text::get(lang, "no_storage").to_string()
        } else if self.deposit_tried > 0 {
            super::text::get(lang, "deposit_failed").to_string()
        } else {
            super::text::get(lang, "nothing").to_string()
        };
        note.set_deposit_osd(&text);
        note.set_deposit_fire(true);
    }

    pub fn deposit(&mut self, page: u64, store: usize) {
        if !self.deposit || self.deposit_busy || self.qt_busy || self.restock_busy {
            return;
        }
        let nearby = self.game.as_ref().map(|g| g.nearby_ids).unwrap_or(0);
        if nearby == 0 {
            self.deposit_block = "no_storage".into();
            return;
        }
        let count = read_i32(store.wrapping_add(super::inv::STORE_COUNT)).unwrap_or(-1);
        let data = nms_core::mem::read_u64(store.wrapping_add(super::inv::STORE_DATA)).unwrap_or(0);
        if store < MIN_ADDR || data < MIN_ADDR as u64 || !(0..=10000).contains(&count) {
            self.log.info("Deposit skipped: this grid has no inventory");
            return;
        }
        let name = store_name_bytes(store, STORAGE_NAME.len());
        if name == STORAGE_NAME {
            return;
        }
        let filter_field = self.game.as_ref().map(|g| g.filter_field).unwrap_or(0);
        let current_filter = read_i32((page as usize).wrapping_add(filter_field as usize)).unwrap_or(-1);
        if !filter_ok(current_filter) {
            self.log.info(&format!("Deposit skipped: unknown filter {current_filter}"));
            return;
        }
        let object_id = self.game.as_ref().and_then(|g| g.storage_object());
        let Some(object_id) = object_id else {
            self.log.info("Deposit skipped: no storage in reach");
            self.deposit_block = "no_storage".into();
            return;
        };
        let mut lit = Vec::new();
        for (y, x, element) in items(store) {
            if read_i32(element.wrapping_add(ELEMENT_TYPE)) == Some(TECHNOLOGY)
                && read_u8(element.wrapping_add(ELEMENT_INSTALLED)).unwrap_or(0) != 0
            {
                continue;
            }
            if self.game.as_ref().is_some_and(|g| g.highlighted(store, element, current_filter)) {
                lit.push((x, y, element));
            }
        }
        self.deposit_lit += lit.len() as i32;
        let same = self.same_item;
        let present = self.game.as_mut().and_then(|g| g.storage_ids(object_id));
        let Some(present) = present else {
            self.log.info("Deposit skipped: storage contents could not be read");
            self.deposit_block = "failed".into();
            return;
        };
        let last_chests: Vec<usize> = self
            .game
            .as_ref()
            .map(|g| g.last_chests.iter().copied().collect())
            .unwrap_or_default();
        if last_chests.contains(&store) {
            if self.deposit_self_logged != store as i32 {
                self.deposit_self_logged = store as i32;
                self.log
                    .info(&format!("Deposit skipped: this grid is the storage itself ({store:#x})"));
            }
            return;
        }
        if self.game.as_ref().and_then(|g| g.list_transfer).is_none() && (same || !present.is_empty()) {
            self.log
                .info("Deposit skipped: the game's container move was not found (game update?)");
            self.deposit_block = "failed".into();
            return;
        }
        let mut plan = Vec::new();
        let mut kept = 0i32;
        for (x, y, element) in &lit {
            let qty = self.surplus_qty(store, *x, *y, 0);
            if qty == Some(0) {
                kept += 1;
                continue;
            }
            let ident = item_identity(*element);
            let homes: Vec<i32> = present
                .iter()
                .filter_map(|(kind, ids)| ident.filter(|id| ids.contains(id)).map(|_| *kind))
                .collect();
            if !homes.is_empty() || !same {
                plan.push((*x, *y, homes, qty.unwrap_or(0)));
            }
        }
        self.deposit_tried += plan.len() as i32;
        let mut dest = [0u8; DEST_BYTES];
        dest[..4].copy_from_slice(&DEST_TYPE.to_le_bytes());
        dest[4..6].copy_from_slice(&object_id.to_le_bytes());
        self.deposit_busy = true;
        let mut sent = 0i32;
        for (x, y, homes, qty) in &plan {
            let mut slot = [*x, *y];
            if !homes.is_empty() {
                for kind in homes {
                    if self
                        .game
                        .as_ref()
                        .is_some_and(|g| g.transfer_to_chest(store, slot.as_mut_ptr() as u64, object_id, *kind, *qty) > 0)
                    {
                        sent += 1;
                        break;
                    }
                }
                continue;
            }
            let mut out = 0i32;
            if self.game.as_ref().is_some_and(|g| {
                g.transfer(
                    dest.as_mut_ptr() as u64,
                    page,
                    store as u64,
                    slot.as_mut_ptr() as u64,
                    *qty,
                    (&mut out as *mut i32) as u64,
                )
            }) {
                sent += 1;
            }
        }
        self.deposit_sent += sent;
        if !plan.is_empty() && sent == 0 {
            self.deposit_block = "failed".into();
        }
        let storage: String = last_chests.iter().map(|c| format!("{c:#x}")).collect::<Vec<_>>().join(", ");
        self.log.info(&format!(
            "Deposit: filter {current_filter}, object {object_id}, {}, moved {sent} of {} lit, reserved left on {kept} (grid {store:#x}, storage {storage})",
            if same { "same item only" } else { "everything lit" },
            lit.len(),
        ));
        self.deposit_busy = false;
    }
}
