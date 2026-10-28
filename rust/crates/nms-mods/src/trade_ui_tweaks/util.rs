//! Shared helpers for TradeUITweaks (ported from Python).

use nms_core::image::parse_headers;
use nms_core::mem::{self, read_i32, read_ptr, read_u64, read_u8, readable, MIN_ADDR};
use nms_core::process::ProcessImage;
use nms_core::Code;

pub const WATCH_SECONDS: f64 = 0.25;
pub const SHOP_LIST_STALE: f64 = 1.5;
pub const SHOP_LIST_RESTORE: f64 = 0.5;
pub const SHOP_BUYFOR_AT: usize = 16;
pub const SHOP_CAVE_SLOT: usize = 16;
pub const SHOP_CAVE_SIZE: usize = 0x100;
/// Build 183791 (was 0x194D8): `lea r15, [r14+0x19538]` in the shop chrome, `movzx r8d, byte [rdi+0x19538]`.
pub const SHOP_BUY_AT: u64 = 0x19538;
pub const SHOP_PAGE_LEA_AT: usize = 0x44;
pub const SHOP_PAGE_MODE_AT: u64 = 0x14488;
pub const SHOP_PAGE_NEEDLE: &str = "InteractionShop";
pub const SHOP_PLAYER_AT: u64 = 0xAAD0;
pub const SHOP_UNITS_AT: u64 = 0x89C;
pub const SHOP_ITEM_MAX_AT: u64 = 0x20;
pub const SHOP_ITEM_TABLE_AT: u64 = 0x143D8;
pub const SHOP_ROW_AT: u64 = 0x143F8;
/// Build 183791 (was 0x194E4): `SHOP_BUY_AT + 0xC`, constructor `mov dword [rdi+0x19544], -1`.
pub const SHOP_INV_AT: u64 = 0x19544;
pub const SHOP_PAGE_ELEMENT: u64 = 0x14478;
pub const SHOP_LIST_LEA_AT: usize = 0;
pub const SHOP_LIST_COUNT: u64 = 0x4;
pub const SHOP_LIST_DATA: u64 = 0x8;
pub const SHOP_REACH_SECONDS: f64 = 0.5;
pub const SHOP_INV_CLICK: f64 = 0.25;
pub const SHOP_TOGGLE_LOAD_AT: usize = 19;
pub const SHOP_TOGGLE_ACT: u32 = 0xD8;
pub const SHOP_SWITCH_NAME: &str = "SWITCHBAR02";
pub const SHOP_TITLE_NAME: &str = "TRADE_INVENTORY";
pub const SHOP_TITLE_VT: u64 = 0x90;
pub const SHOP_STORE_NAME_AT: u64 = 0x104;
pub const SHOP_ELEMENT_DATA: usize = 0x48;
pub const SHOP_DATA_ID: usize = 0x48;
pub const SHOP_CHILD_MAP: usize = 0x150;
pub const SHOP_VIEW_OFFSET: u64 = 0xC240;
pub const SHOP_VIEW_GLOBAL_AT: usize = 0x64;
pub const SHOP_VIEW_CALL_AT: usize = 0x6B;
pub const SHOP_TITLE_CHEST_FALLBACK: &str = "Storage Container";

pub const SHOP_INV_KINDS: &[i32] = &[
    0,
    4,
    7,
    0x0A,
    0x0C,
    0x0D,
    0x0E,
    0x0F,
    0x10,
    0x11,
    0x12,
    0x13,
    0x14,
    0x15,
    0x20,
];

pub const SHOP_CHESTS: core::ops::Range<i32> = 0x0C..0x16;

pub fn image_span(image: &ProcessImage) -> (usize, usize) {
    let base = image.base();
    let size = image_size(image);
    (base, base.saturating_add(size))
}

pub fn image_size(image: &ProcessImage) -> usize {
    let head = image.read(0, 0x1000).unwrap_or_default();
    parse_headers(&head).map(|h| h.size_of_image).unwrap_or(0)
}

pub fn find_c_string(image: &ProcessImage, needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return None;
    }
    let total = image_size(image);
    let step = 0x10000usize;
    let mut rva = 0usize;
    while rva < total {
        let take = step.min(total.saturating_sub(rva));
        let Some(buf) = image.read(rva, take) else {
            break;
        };
        if let Some(off) = buf.windows(needle.len()).position(|w| w == needle) {
            return Some(rva + off);
        }
        rva = rva.saturating_add(take.saturating_sub(needle.len().saturating_sub(1)));
    }
    None
}

pub fn call_after_rva(image: &ProcessImage, start_rva: usize, at: usize, expected: &[u8]) -> Option<usize> {
    let got = image.read(start_rva.wrapping_add(at), expected.len())?;
    if got != expected {
        return None;
    }
    let disp = start_rva.wrapping_add(at).wrapping_add(expected.len());
    image.rel32_target(disp)
}

pub fn write_bytes(addr: usize, bytes: &[u8]) -> bool {
    if !readable(addr, bytes.len()) {
        return false;
    }
    // SAFETY: range was checked readable.
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), addr as *mut u8, bytes.len());
    }
    true
}

pub fn shop_kind_name(kind: i32) -> String {
    match kind {
        0 => "exosuit".into(),
        1 => "weapon".into(),
        4 => "starship".into(),
        7 => "freighter".into(),
        0x0A => "vehicle".into(),
        0x1A => "pantry".into(),
        0x1D => "cold storage".into(),
        0x20 => "corvette parts".into(),
        k if (SHOP_CHESTS.start..SHOP_CHESTS.end).contains(&k) => {
            format!("container {}", k - SHOP_CHESTS.start + 1)
        }
        k => format!("kind {k}"),
    }
}

pub fn shop_item_id(item: u64) -> String {
    if item < MIN_ADDR as u64 {
        return String::new();
    }
    let item = item as usize;
    for start in [item, item.wrapping_add(8)] {
        let Some(raw) = mem::read_bytes(start, 16) else {
            continue;
        };
        let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
        let Ok(ident) = std::str::from_utf8(&raw[..end]) else {
            continue;
        };
        if ident.is_ascii()
            && !ident.is_empty()
            && ident.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
        {
            return ident.to_string();
        }
    }
    String::new()
}

pub fn shop_page_inv(page: u64) -> String {
    if page < MIN_ADDR as u64 {
        return "page=0".into();
    }
    let page = page as usize;
    let kind = read_i32(page.wrapping_add(SHOP_INV_AT as usize)).unwrap_or(0);
    let table = page.wrapping_add(SHOP_ITEM_TABLE_AT as usize);
    format!(
        "kind={} {} buy={} row={} n18={}",
        kind,
        shop_kind_name(kind),
        read_u8(page.wrapping_add(SHOP_BUY_AT as usize)).unwrap_or(0),
        read_i32(page.wrapping_add(SHOP_ROW_AT as usize)).unwrap_or(0),
        read_i32(table.wrapping_add(0x18)).unwrap_or(0),
    )
}

pub fn shop_element_name(element: u64) -> String {
    let element = element as usize;
    if element < MIN_ADDR {
        return String::new();
    }
    let Some(data) = read_ptr(element.wrapping_add(SHOP_ELEMENT_DATA)) else {
        return String::new();
    };
    let Some(raw) = mem::read_bytes(data.wrapping_add(SHOP_DATA_ID), 16) else {
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

pub fn shop_children(element: u64) -> Vec<(String, u64)> {
    let element = element as usize;
    if element < MIN_ADDR {
        return Vec::new();
    }
    let Some(table) = read_ptr(element.wrapping_add(SHOP_CHILD_MAP)) else {
        return Vec::new();
    };
    let Some(count) = read_i32(table.wrapping_add(0x1C)) else {
        return Vec::new();
    };
    let Some(buckets_n) = read_i32(table.wrapping_add(0x20)) else {
        return Vec::new();
    };
    let Some(buckets) = read_ptr(table.wrapping_add(8)) else {
        return Vec::new();
    };
    if !(0..=64).contains(&count) || !(1..=256).contains(&buckets_n) || buckets < MIN_ADDR {
        return Vec::new();
    }
    let mut found = Vec::new();
    for index in 0..buckets_n {
        let Some(mut node) = read_ptr(buckets.wrapping_add((index as usize).wrapping_mul(8))) else {
            continue;
        };
        let mut hops = 0;
        while node >= MIN_ADDR && hops < 8 && found.len() < 48 {
            if let Some(child) = read_ptr(node.wrapping_add(0x18)) {
                let ident = shop_element_name(child as u64);
                if !ident.is_empty() {
                    found.push((ident, child as u64));
                }
            }
            node = read_ptr(node.wrapping_add(0x28)).unwrap_or(0);
            hops += 1;
        }
    }
    let extra = read_i32(element.wrapping_add(0x6C)).unwrap_or(0);
    let array = read_ptr(element.wrapping_add(0x70)).unwrap_or(0);
    if array >= MIN_ADDR && extra > 0 && extra <= 64 {
        let mut seen: std::collections::HashSet<u64> = found.iter().map(|(_, c)| *c).collect();
        for index in 0..extra {
            let Some(child) = read_ptr(array.wrapping_add((index as usize).wrapping_mul(8))) else {
                continue;
            };
            if child < MIN_ADDR || seen.contains(&(child as u64)) {
                continue;
            }
            let ident = shop_element_name(child as u64);
            if !ident.is_empty() {
                found.push((ident, child as u64));
                seen.insert(child as u64);
            }
        }
    }
    found
}

pub fn shop_find_named(element: u64, name: &str, depth: usize, budget: usize) -> u64 {
    let mut level = vec![element];
    let mut budget = budget;
    for _ in 0..=depth {
        let mut nxt = Vec::new();
        for parent in level {
            if budget == 0 {
                return 0;
            }
            budget = budget.saturating_sub(1);
            for (ident, child) in shop_children(parent) {
                if ident == name {
                    return child;
                }
                nxt.push(child);
            }
        }
        level = nxt;
    }
    0
}

pub fn shop_list_kinds(choices: usize) -> Vec<i32> {
    let Some(count) = read_i32(choices.wrapping_add(SHOP_LIST_COUNT as usize)) else {
        return Vec::new();
    };
    let Some(data) = read_u64(choices.wrapping_add(SHOP_LIST_DATA as usize)) else {
        return Vec::new();
    };
    if data < MIN_ADDR as u64 || !(0..=256).contains(&count) {
        return Vec::new();
    }
    let data = data as usize;
    let mut out = Vec::new();
    for n in 0..count {
        if let Some(k) = read_i32(data.wrapping_add((n as usize).wrapping_mul(4))) {
            out.push(k);
        }
    }
    out
}

pub fn write_trade_stub(cave: usize, fmt: usize) -> bool {
    let mut stub = [0u8; 0x20];
    stub[0..6].copy_from_slice(&[0x89, 0x0D, 0x0A, 0x00, 0x00, 0x00]);
    stub[6] = 0xE9;
    let Some(rel) = mem::rel32(cave.wrapping_add(11), fmt) else {
        return false;
    };
    stub[7..11].copy_from_slice(&rel.to_le_bytes());
    write_bytes(cave, &stub)
}

pub fn cave_fmt(cave: usize) -> Option<usize> {
    let head = mem::read_bytes(cave, 7)?;
    if head != [0x89, 0x0D, 0x0A, 0x00, 0x00, 0x00, 0xE9] {
        return None;
    }
    let disp = read_i32(cave.wrapping_add(7))?;
    Some(cave.wrapping_add(11).wrapping_add(disp as usize))
}

pub fn is_cave(image: &ProcessImage, cave_atomic: usize, address: usize) -> bool {
    if address == 0 {
        return false;
    }
    if cave_atomic != 0 && address == cave_atomic {
        return true;
    }
    let (start, end) = image_span(image);
    !(start <= address && address < end)
}

pub fn buyfor_old_bytes(address: usize, fmt: usize) -> Option<Vec<u8>> {
    let rel = mem::rel32(address.wrapping_add(5), fmt)?;
    let mut out = vec![0xE8];
    out.extend_from_slice(&rel.to_le_bytes());
    Some(out)
}

pub fn shop_painted(cave: usize) -> i32 {
    if cave == 0 {
        return 0;
    }
    read_i32(cave.wrapping_add(SHOP_CAVE_SLOT)).unwrap_or(0)
}

pub fn shop_clear_painted(cave: usize) {
    if cave != 0 {
        let _ = unsafe { mem::write(cave.wrapping_add(SHOP_CAVE_SLOT), 0i32) };
    }
}
