//! Byte patches and the Buy-for stub (ported from Python).

use std::sync::atomic::{AtomicUsize, Ordering};

use nms_core::mem::{self, alloc_exec_near};
use nms_core::patch::CodePatch;
use nms_core::process::ProcessImage;
use nms_core::{Code, Pattern};
use nms_sdk::sigs::trade_ui_tweaks;

use super::util::{
    buyfor_old_bytes, cave_fmt, is_cave, write_trade_stub, SHOP_BUYFOR_AT, SHOP_CAVE_SIZE,
};

const SHOP_INV_FORCE: &[CodePatch] = &[
    CodePatch {
        label: "Shop inv force",
        sig: "45 84 C0 74 16 49 8D 8E D8 43 01 00 E8",
        at: 3,
        old: &[0x74, 0x16],
        new: &[0x90, 0x90],
    },
    CodePatch {
        label: "Shop inv force",
        // Build 183791: frame slot 0x1E70 -> 0x1E60, and the store block one byte shorter (je +0x26 -> +0x25).
        sig: "84 D2 74 25 48 8B 8D 60 1E 00 00 48 81 C1 D8 43 01 00",
        at: 2,
        old: &[0x74, 0x25],
        new: &[0x90, 0x90],
    },
];

const SHOP_INV_STORES: &[CodePatch] = &[
    trade_ui_tweaks::SHOP_INV_STORE_0,
    trade_ui_tweaks::SHOP_INV_STORE_1,
    trade_ui_tweaks::SHOP_INV_STORE_2,
    trade_ui_tweaks::SHOP_INV_STORE_3,
    trade_ui_tweaks::SHOP_INV_STORE_4,
    trade_ui_tweaks::SHOP_INV_STORE_5,
];

pub fn patch_shop_more_inv(image: &mut ProcessImage, apply: bool) -> String {
    for patch in SHOP_INV_FORCE {
        let _ = patch.set(image, false);
    }
    let mut notes = Vec::new();
    for patch in SHOP_INV_STORES {
        notes.push(patch.set(image, apply).message);
    }
    let bad: Vec<_> = notes.iter().filter(|n| n.contains("not changed")).collect();
    if !bad.is_empty() && apply {
        for patch in SHOP_INV_STORES {
            let _ = patch.set(image, false);
        }
        return bad[0].clone();
    }
    if let Some(first) = bad.first() {
        return (*first).clone();
    }
    if apply {
        "Shop: more inventories applied".into()
    } else {
        "Shop: more inventories turned off".into()
    }
}

pub fn stores_applied(msg: &str) -> bool {
    !msg.contains("not changed")
}

fn buyfor_site_va(image: &ProcessImage, site_cache: &AtomicUsize) -> Option<usize> {
    let pat = Pattern::parse(trade_ui_tweaks::SHOP_BUYFOR).ok()?;
    let rva = image.find_unique(&pat).unique()?;
    let va = image.va(rva.wrapping_add(SHOP_BUYFOR_AT));
    site_cache.store(va, Ordering::Release);
    Some(va)
}

fn buyfor_site_va_cached(image: &ProcessImage, site_cache: &AtomicUsize) -> Option<usize> {
    let cached = site_cache.load(Ordering::Acquire);
    if cached != 0 {
        return Some(cached);
    }
    buyfor_site_va(image, site_cache)
}

pub fn patch_shop_buyfor(
    image: &ProcessImage,
    live: &mut ProcessImage,
    apply: bool,
    cave_addr: &AtomicUsize,
    site_cache: &AtomicUsize,
) -> String {
    let label = "Shop Buy for";
    let address = match buyfor_site_va_cached(image, site_cache) {
        Some(a) => a,
        None => {
            let cached = site_cache.load(Ordering::Acquire);
            if cached != 0 {
                cached
            } else {
                return format!("{label} not changed: game code not found (game update?)");
            }
        }
    };
    let rva = address.wrapping_sub(image.base());
    let current = match live.read(rva, 5) {
        Some(b) => b,
        None => return format!("{label} not changed: unexpected game code (game update?)"),
    };
    if current.first() != Some(&0xE8) {
        return format!("{label} not changed: unexpected game code (game update?)");
    }
    let disp = i32::from_le_bytes([current[1], current[2], current[3], current[4]]);
    let dest = address.wrapping_add(5).wrapping_add(disp as usize);
    let cave = cave_addr.load(Ordering::Acquire);
    if is_cave(image, cave, dest) {
        if cave == 0 {
            cave_addr.store(dest, Ordering::Release);
        }
        let fmt = match cave_fmt(dest) {
            Some(f) => f,
            None => return format!("{label} not changed: unexpected game code (game update?)"),
        };
        if !apply {
            let Some(old) = buyfor_old_bytes(address, fmt) else {
                return format!("{label} not changed: unexpected game code (game update?)");
            };
            if live.write(rva, &old).is_err() {
                return format!("{label} not changed: could not unprotect the code");
            }
            return format!("{label} turned off");
        }
        if !write_trade_stub(dest, fmt) {
            return format!("{label} not changed: could not unprotect the code");
        }
        return format!("{label} already applied");
    }
    let (start, end) = super::util::image_span(image);
    if !(start <= dest && dest < end) {
        return format!("{label} not changed: unexpected game code (game update?)");
    }
    let fmt = dest;
    if !apply {
        return format!("{label} already turned off");
    }
    let cave = match alloc_exec_near(image.base(), SHOP_CAVE_SIZE) {
        Some(c) => c,
        None => return format!("{label} not changed: could not unprotect the code"),
    };
    cave_addr.store(cave, Ordering::Release);
    if !write_trade_stub(cave, fmt) {
        return format!("{label} not changed: could not unprotect the code");
    }
    let rel = match mem::rel32(address.wrapping_add(5), cave) {
        Some(r) => r,
        None => return format!("{label} not changed: unexpected game code (game update?)"),
    };
    let mut new = vec![0xE8];
    new.extend_from_slice(&rel.to_le_bytes());
    if live.write(rva, &new).is_err() {
        return format!("{label} not changed: could not unprotect the code");
    }
    format!("{label} applied")
}
