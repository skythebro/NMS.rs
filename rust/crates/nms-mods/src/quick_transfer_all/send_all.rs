//! Hold-to-send-all: Steam Unbound / XInput left trigger / hold VK, highlighted, surplus rewrite.

use nms_core::mem::{read_i32, read_u8, MIN_ADDR};

use crate::common::hooks::quick_transfer_pick;
use crate::common::input::{async_key_down, left_trigger_xinput, SteamInput};

use super::inv::{filter_ok, items, ELEMENT_INSTALLED, ELEMENT_TYPE, STEAM_HOLD_ACTIONS, TECHNOLOGY};
use super::runtime::Runtime;

impl Runtime {
    pub fn qt_held(&mut self) -> bool {
        if self.game.is_none() {
            return false;
        }
        if let Some(vk) = self.qt_vk {
            if async_key_down(vk) {
                return true;
            }
        }
        if left_trigger_xinput() {
            return true;
        }
        self.steam_hold_down()
    }

    pub fn steam_hold_down(&mut self) -> bool {
        self.steam_actions_down(true, &STEAM_HOLD_ACTIONS)
    }

    pub fn steam_select_down(&mut self) -> bool {
        self.steam_actions_down(false, &super::inv::STEAM_CLICK_ACTIONS)
    }

    fn steam_actions_down(&mut self, hold: bool, names: &[&str]) -> bool {
        if self.steam_off {
            return false;
        }
        if self.steam.is_none() {
            match SteamInput::new() {
                Ok(steam) => self.steam = Some(steam),
                Err(error) => {
                    self.steam_off = true;
                    self.log.info(&format!(
                        "Quick transfer all: Steam Input is not available ({error}); keyboard and XInput only"
                    ));
                    return false;
                }
            }
        }
        let Some(steam) = self.steam.as_mut() else {
            return false;
        };
        let controllers = steam.controllers();
        if controllers.is_empty() {
            return false;
        }
        let cache = if hold {
            &mut self.steam_hold
        } else {
            &mut self.steam_select
        };
        if cache.is_empty() {
            *cache = names.iter().map(|n| steam.digital(n)).filter(|h| *h != 0).collect();
        }
        let handles = cache.clone();
        controllers
            .iter()
            .any(|c| handles.iter().any(|h| steam.down(*c, *h)))
    }

    pub fn before_transfer(&mut self, args: &mut quick_transfer_pick::Args) {
        self.qt_picked = None;
        if self.qt_busy || self.restock_busy {
            return;
        }
        let address = args.index as usize;
        if address >= MIN_ADDR && self.qt_held() {
            let x = read_i32(address).unwrap_or(0);
            let y = read_i32(address.wrapping_add(4)).unwrap_or(0);
            self.qt_picked = Some((x, y));
        }
        self.qt_surplus_args(args);
    }

    pub fn after_transfer(&mut self, args: &quick_transfer_pick::Args, _ret: u8) {
        let picked = self.qt_picked.take();
        if picked.is_none() || self.qt_busy || self.restock_busy {
            return;
        }
        let Some(picked) = picked else {
            return;
        };
        let Some(game) = self.game.as_ref() else {
            return;
        };
        let target = args.dest;
        let page = args.page;
        let store = args.store as usize;
        if target < MIN_ADDR as u64 || page < MIN_ADDR as u64 || store < MIN_ADDR {
            return;
        }
        self.qt_busy = true;
        let current_filter = read_i32((page as usize).wrapping_add(game.filter_field as usize)).unwrap_or(-1);
        if !filter_ok(current_filter) {
            self.log
                .info(&format!("Quick transfer all skipped: unknown filter {current_filter}"));
            self.qt_busy = false;
            return;
        }
        let mut chosen = Vec::new();
        for (y, x, element) in items(store) {
            if (x, y) == picked {
                continue;
            }
            if read_i32(element.wrapping_add(ELEMENT_TYPE)) == Some(TECHNOLOGY)
                && read_u8(element.wrapping_add(ELEMENT_INSTALLED)).unwrap_or(0) != 0
            {
                continue;
            }
            if game.highlighted(store, element, current_filter) {
                chosen.push((x, y));
            }
        }
        let mut sent = 0i32;
        let mut kept = 0i32;
        for (x, y) in &chosen {
            let qty = self.surplus_qty(store, *x, *y, 0);
            if qty == Some(0) {
                kept += 1;
                continue;
            }
            let mut slot = [*x, *y];
            let mut out = 0i32;
            let move_n = qty.unwrap_or(0);
            if game.transfer(
                target,
                page,
                store as u64,
                slot.as_mut_ptr() as u64,
                move_n,
                (&mut out as *mut i32) as u64,
            ) {
                sent += 1;
            }
        }
        self.log.info(&format!(
            "Quick transfer all: filter {current_filter}, moved {sent} of {} more items, reserved left on {kept}",
            chosen.len()
        ));
        self.qt_busy = false;
    }

    fn qt_surplus_args(&mut self, args: &mut quick_transfer_pick::Args) {
        let address = args.index as usize;
        let store = args.store as usize;
        if address < MIN_ADDR || store < MIN_ADDR {
            return;
        }
        let x = read_i32(address).unwrap_or(0);
        let y = read_i32(address.wrapping_add(4)).unwrap_or(0);
        let amount = args.amount as i32;
        let qty = self.surplus_qty(store, x, y, if amount != 0 { amount } else { 0 });
        let Some(qty) = qty else {
            return;
        };
        if qty == 0 {
            self.log.info(&format!(
                "suit lock: surplus none at {x},{y}, reserved stays"
            ));
            args.index = self.surplus_hold.as_mut_ptr() as u64;
            return;
        }
        self.log.info(&format!("suit lock: surplus {qty} at {x},{y}"));
        args.amount = i64::from(qty);
    }
}
