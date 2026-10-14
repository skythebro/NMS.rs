//! Shared-hook subscribers plus QTA-owned Add / Take / SellDemand / Award detours.
#![allow(clippy::too_many_arguments, clippy::missing_transmute_annotations)]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use nms_core::hook::Hook;
use nms_core::log::Log;
use nms_core::process::ProcessImage;
use nms_core::{Code, Pattern};
use nms_sdk::sigs::quick_transfer_all as sigs;

use crate::common::hooks::{do_inventory_slots, inventory_sort, quick_transfer_pick};

use super::with_runtime;

static ADD_ORIG: AtomicUsize = AtomicUsize::new(0);
static TAKE_ORIG: AtomicUsize = AtomicUsize::new(0);
static SELL_DEMAND_ORIG: AtomicUsize = AtomicUsize::new(0);
static AWARD_UNITS_ORIG: AtomicUsize = AtomicUsize::new(0);
static AWARD_NANITES_ORIG: AtomicUsize = AtomicUsize::new(0);

type AddFn = unsafe extern "system" fn(u64, u64, u64) -> u64;
type TakeFn = unsafe extern "system" fn(u64, i32, u64, i64) -> i32;
type SellDemandFn = unsafe extern "system" fn(u64, u64, i32, i32, u64, u64) -> u64;
type AwardFn = unsafe extern "system" fn(u64, i32) -> u64;

pub struct HookSet {
    pub all_ok: bool,
    pub kept: Vec<Arc<Hook>>,
}

pub fn install(image: &ProcessImage, log: &Log) -> HookSet {
    let mut all_ok = true;
    let mut kept = Vec::new();

    match do_inventory_slots::subscribe(
        image,
        sigs::NMSPY_DO_INVENTORY_SLOTS,
        Some(before_slots),
        Some(after_slots),
    ) {
        Ok(()) => log.info("DoInventorySlots: on"),
        Err(e) => {
            all_ok = false;
            log.info(&format!("DoInventorySlots not changed: {e}"));
        }
    }
    match inventory_sort::subscribe(image, sigs::SORT, Some(before_sort), Some(after_sort)) {
        Ok(()) => log.info("InventorySort: on"),
        Err(e) => {
            all_ok = false;
            log.info(&format!("InventorySort not changed: {e}"));
        }
    }

    match quick_transfer_pick::subscribe(image, sigs::TRANSFER, Some(before_transfer), Some(after_transfer)) {
        Ok(()) => log.info("QuickTransferToTarget: on"),
        Err(e) => {
            all_ok = false;
            log.info(&format!("QuickTransferToTarget not changed: {e}"));
        }
    }

    let mut own = |name: &'static str, sig: &str, detour: *const (), orig: &AtomicUsize| {
        match install_one(image, name, sig, detour, orig) {
            Some(h) => {
                log.info(&format!("{name}: on"));
                kept.push(h);
            }
            None => {
                all_ok = false;
                log.info(&format!("{name} not changed"));
            }
        }
    };
    own("InventoryAdd", sigs::ADD, add_detour as *const (), &ADD_ORIG);
    own(
        "InventoryTakeKind",
        sigs::TAKE,
        take_detour as *const (),
        &TAKE_ORIG,
    );
    own(
        "SellDemand",
        sigs::SELL_DEMAND,
        sell_demand_detour as *const (),
        &SELL_DEMAND_ORIG,
    );
    own(
        "AwardUnits",
        sigs::AWARD_UNITS,
        award_units_detour as *const (),
        &AWARD_UNITS_ORIG,
    );
    own(
        "AwardNanites",
        sigs::AWARD_NANITES,
        award_nanites_detour as *const (),
        &AWARD_NANITES_ORIG,
    );

    HookSet { all_ok, kept }
}

fn find_sig(image: &ProcessImage, sig: &str) -> Option<usize> {
    Pattern::parse(sig).ok().and_then(|p| image.find_unique(&p).unique())
}

fn install_one(
    image: &ProcessImage,
    name: &'static str,
    sig: &str,
    detour: *const (),
    orig: &AtomicUsize,
) -> Option<Arc<Hook>> {
    let rva = find_sig(image, sig)?;
    // SAFETY: unique pattern is the function entry; the detour declares every Python argument.
    match unsafe { Hook::install(name, image.va(rva), detour, orig) } {
        Ok(hook) => Some(Arc::new(hook)),
        Err(_) => None,
    }
}

fn before_slots(args: &mut do_inventory_slots::Args) {
    let _ = with_runtime(|rt| rt.before_slots(args));
}

fn after_slots(args: &do_inventory_slots::Args, _ret: u64) {
    let _ = with_runtime(|rt| rt.after_slots(args));
}

fn before_sort(args: &mut inventory_sort::Args) {
    let _ = with_runtime(|rt| rt.before_sort(args));
}

fn after_sort(args: &inventory_sort::Args, ret: u64) {
    let _ = with_runtime(|rt| rt.after_sort(args, ret));
}

fn before_transfer(args: &mut quick_transfer_pick::Args) {
    let _ = with_runtime(|rt| rt.before_transfer(args));
}

fn after_transfer(args: &quick_transfer_pick::Args, ret: u64) {
    let _ = with_runtime(|rt| rt.after_transfer(args, ret as u8));
}

unsafe extern "system" fn add_detour(store: u64, result: u64, element: u64) -> u64 {
    let mut store = store;
    let mut result = result;
    let mut element = element;
    if let Some(Some(rewritten)) = with_runtime(|rt| rt.before_add(store, result, element)) {
        store = rewritten.0;
        result = rewritten.1;
        element = rewritten.2;
    }
    let orig = ADD_ORIG.load(Ordering::Acquire);
    if orig == 0 {
        return 0;
    }
    let original: AddFn = unsafe { std::mem::transmute(orig) };
    let ret = unsafe { original(store, result, element) };
    let _ = with_runtime(|rt| rt.after_add(store, result, element, ret));
    ret
}

unsafe extern "system" fn take_detour(player: u64, kind: i32, element: u64, index: i64) -> i32 {
    let _ = with_runtime(|rt| rt.before_take(player, kind, element, index));
    let orig = TAKE_ORIG.load(Ordering::Acquire);
    if orig == 0 {
        return 0;
    }
    let original: TakeFn = unsafe { std::mem::transmute(orig) };
    let ret = unsafe { original(player, kind, element, index) };
    let _ = with_runtime(|rt| rt.after_take(player, kind, element, index, ret));
    ret
}

unsafe extern "system" fn sell_demand_detour(
    owner: u64,
    item: u64,
    flag: i32,
    qty: i32,
    extra: u64,
    mark: u64,
) -> u64 {
    let mut owner = owner;
    let mut item = item;
    let mut flag = flag;
    let mut qty = qty;
    let mut extra = extra;
    let mut mark = mark;
    if let Some(Some(rewritten)) = with_runtime(|rt| rt.before_sell_demand(owner, item, flag, qty, extra, mark)) {
        owner = rewritten.0;
        item = rewritten.1;
        flag = rewritten.2;
        qty = rewritten.3;
        extra = rewritten.4;
        mark = rewritten.5;
    }
    let orig = SELL_DEMAND_ORIG.load(Ordering::Acquire);
    if orig == 0 {
        return 0;
    }
    let original: SellDemandFn = unsafe { std::mem::transmute(orig) };
    let ret = unsafe { original(owner, item, flag, qty, extra, mark) };
    let _ = with_runtime(|rt| rt.after_sell_demand(owner, item, flag, qty, extra, mark, ret));
    ret
}

unsafe extern "system" fn award_units_detour(player: u64, amount: i32) -> u64 {
    award_detour(
        player,
        amount,
        &AWARD_UNITS_ORIG,
        |rt, player, amount| rt.before_award_units(player, amount),
        |rt, player, amount, ret| rt.after_award_units(player, amount, ret),
    )
}

unsafe extern "system" fn award_nanites_detour(player: u64, amount: i32) -> u64 {
    award_detour(
        player,
        amount,
        &AWARD_NANITES_ORIG,
        |rt, player, amount| rt.before_award_nanites(player, amount),
        |rt, player, amount, ret| rt.after_award_nanites(player, amount, ret),
    )
}

fn award_detour(
    player: u64,
    amount: i32,
    orig_slot: &AtomicUsize,
    before: impl FnOnce(&mut super::runtime::Runtime, u64, i32) -> Option<(i32, bool)>,
    after: impl FnOnce(&mut super::runtime::Runtime, u64, i32, u64),
) -> u64 {
    let mut amount = amount;
    if let Some(Some((scaled, skip))) = with_runtime(|rt| before(rt, player, amount)) {
        if skip {
            return 0;
        }
        amount = scaled;
    }
    let orig = orig_slot.load(Ordering::Acquire);
    if orig == 0 {
        return 0;
    }
    let original: AwardFn = unsafe { std::mem::transmute(orig) };
    let ret = unsafe { original(player, amount) };
    let _ = with_runtime(|rt| after(rt, player, amount, ret));
    ret
}
