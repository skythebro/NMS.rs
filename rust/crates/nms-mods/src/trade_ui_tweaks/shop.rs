//! Shop screen, list step, and wallet helpers.

use nms_core::mem::{read_i32, read_u32, read_u64, MIN_ADDR};
use nms_core::process::ProcessImage;
use nms_core::{Code, Pattern};
use nms_sdk::sigs::trade_ui_tweaks;

use super::util::{
    call_after_rva, SHOP_LIST_LEA_AT, SHOP_PAGE_LEA_AT, SHOP_PAGE_MODE_AT, SHOP_PAGE_NEEDLE, SHOP_PLAYER_AT,
    SHOP_UNITS_AT, SHOP_VIEW_CALL_AT,
};
pub(crate) const SHOP_PUSH_AT: usize = 0x89;
pub(crate) const SHOP_PUSH_BYTES: &[u8] =
    &[0x48, 0x8D, 0x55, 0x30, 0x44, 0x89, 0x7D, 0x30, 0x48, 0x8B, 0xCB, 0xE8];
const SHOP_VIEW_GLOBAL_BYTES: &[u8] = &[0x48, 0x8B, 0x0D];
const SHOP_VIEW_CALL_BYTES: &[u8] = &[0x48, 0x81, 0xC1, 0x40, 0xC2, 0x00, 0x00, 0xE8];

pub struct ShopQ {
    wallet_va: usize,
}

impl ShopQ {
    pub fn new(image: &ProcessImage) -> Result<Self, String> {
        let pat = Pattern::parse(trade_ui_tweaks::SHOP_WALLET).map_err(|e| e.to_string())?;
        let Some(rva) = image.find_unique(&pat).unique() else {
            return Err("game code not found (game update?)".into());
        };
        Ok(Self {
            wallet_va: image.va(rva),
        })
    }

    pub fn units(&self) -> u32 {
        type WalletFn = unsafe extern "system" fn() -> u64;
        let wallet: WalletFn = unsafe { std::mem::transmute(self.wallet_va) };
        let base = unsafe { wallet() } as usize;
        if base < MIN_ADDR {
            return 0;
        }
        let player = base.wrapping_add(SHOP_PLAYER_AT as usize);
        read_u32(player.wrapping_add(SHOP_UNITS_AT as usize)).unwrap_or(0)
    }
}

pub struct ShopScreen {
    table: usize,
}

impl ShopScreen {
    pub fn new(image: &ProcessImage) -> Result<Self, String> {
        let pat = Pattern::parse(trade_ui_tweaks::SHOP_PAGE).map_err(|e| e.to_string())?;
        let Some(found) = image.find_unique(&pat).unique() else {
            return Err("game code not found (game update?)".into());
        };
        let lea_rva = found.wrapping_add(SHOP_PAGE_LEA_AT);
        let lea_va = image.va(lea_rva);
        let Some(head) = image.read(lea_rva, 3) else {
            return Err("unexpected game code (game update?)".into());
        };
        if head != [0x48, 0x8D, 0x0D] {
            return Err("unexpected game code (game update?)".into());
        }
        let Some(disp_bytes) = image.read(lea_rva.wrapping_add(3), 4) else {
            return Err("unexpected game code (game update?)".into());
        };
        let disp = i32::from_le_bytes([
            disp_bytes[0],
            disp_bytes[1],
            disp_bytes[2],
            disp_bytes[3],
        ]) as i64;
        let table = lea_va.wrapping_add(7).wrapping_add(disp as usize);
        Ok(Self { table })
    }

    pub fn name_at(&self, index: i32) -> String {
        if !(0..=0x200).contains(&index) {
            return String::new();
        }
        let ptr = read_u64(
            self.table
                .wrapping_add((index as usize).wrapping_mul(8)),
        )
        .unwrap_or(0);
        if ptr < MIN_ADDR as u64 {
            return String::new();
        }
        let ptr = ptr as usize;
        let Some(raw) = nms_core::mem::read_bytes(ptr, 64) else {
            return String::new();
        };
        let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
        String::from_utf8_lossy(&raw[..end]).into_owned()
    }

    pub fn name(&self, page: u64) -> String {
        if page == 0 {
            return String::new();
        }
        let index = read_i32(page as usize + SHOP_PAGE_MODE_AT as usize).unwrap_or(-1);
        self.name_at(index)
    }

    pub fn is_list(&self, page: u64) -> bool {
        self.name(page).contains(SHOP_PAGE_NEEDLE)
    }
}

pub struct ShopList {
    scroll: usize,
}

impl ShopList {
    pub fn new(image: &ProcessImage) -> Result<Self, String> {
        let pat = Pattern::parse(trade_ui_tweaks::SHOP_LIST).map_err(|e| e.to_string())?;
        let Some(found) = image.find_unique(&pat).unique() else {
            return Err("game code not found (game update?)".into());
        };
        let lea_rva = found.wrapping_add(SHOP_LIST_LEA_AT);
        let lea_va = image.va(lea_rva);
        if lea_va < MIN_ADDR {
            return Err("unexpected game code (game update?)".into());
        }
        let Some(head) = image.read(lea_rva, 3) else {
            return Err("unexpected game code (game update?)".into());
        };
        if head != [0x48, 0x8D, 0x15] {
            return Err("unexpected game code (game update?)".into());
        }
        let Some(disp_bytes) = image.read(lea_rva.wrapping_add(3), 4) else {
            return Err("unexpected game code (game update?)".into());
        };
        let disp = i32::from_le_bytes([
            disp_bytes[0],
            disp_bytes[1],
            disp_bytes[2],
            disp_bytes[3],
        ]) as i64;
        let scroll = lea_va.wrapping_add(7).wrapping_add(disp as usize);
        Ok(Self { scroll })
    }

    pub fn step(&self) -> i32 {
        read_i32(self.scroll).unwrap_or(0)
    }

    pub fn set(&self, value: i32) {
        let v = value.max(0);
        let _ = unsafe { nms_core::mem::write(self.scroll, v) };
    }

    pub fn reset(&self) -> i32 {
        let was = self.step();
        if was != 0 {
            self.set(0);
        }
        was
    }
}

#[expect(dead_code)]
pub fn verify_choices_push(image: &ProcessImage, choices_rva: usize) -> bool {
    let push = call_after_rva(image, choices_rva, SHOP_PUSH_AT, SHOP_PUSH_BYTES);
    push.is_some()
}

pub fn decode_view_helper(image: &ProcessImage, helper_rva: usize) -> Option<(usize, usize)> {
    let global_head = image.read(helper_rva.wrapping_add(super::util::SHOP_VIEW_GLOBAL_AT), 3)?;
    if global_head != SHOP_VIEW_GLOBAL_BYTES {
        return None;
    }
    let global_rva = image.rel32_target(helper_rva.wrapping_add(super::util::SHOP_VIEW_GLOBAL_AT + 3))?;
    let view_rva = call_after_rva(image, helper_rva, SHOP_VIEW_CALL_AT, SHOP_VIEW_CALL_BYTES)?;
    Some((image.va(global_rva), image.va(view_rva)))
}
