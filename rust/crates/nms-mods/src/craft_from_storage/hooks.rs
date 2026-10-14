//! Detours. Each argument matches the Python FUNCDEF. The original always runs.
#![allow(clippy::too_many_arguments)]

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

use nms_core::hook::Hook;
use nms_core::log::Log;
use nms_core::mem::MIN_ADDR;
use nms_core::process::ProcessImage;
use nms_core::{Code, Pattern};
use nms_sdk::sigs::craft_from_storage as sigs;

use crate::common::hooks::quick_transfer_pick;

use super::lists::{BUILD_LINGER_SECONDS, QUICK_STATE_AT};
use super::{call_original, with_runtime, BUILD_DEPTH, POPUP_MODE};

pub static CHOICES_ORIG: AtomicUsize = AtomicUsize::new(0);
pub static BUILD_AFFORD_ORIG: AtomicUsize = AtomicUsize::new(0);
pub static BUILD_PAY_ORIG: AtomicUsize = AtomicUsize::new(0);
pub static COUNT_ORIG: AtomicUsize = AtomicUsize::new(0);
pub static REMOVE_ORIG: AtomicUsize = AtomicUsize::new(0);
pub static CHARGE_ORIG: AtomicUsize = AtomicUsize::new(0);
pub static SOURCES_ORIG: AtomicUsize = AtomicUsize::new(0);
pub static POPUP_ORIG: AtomicUsize = AtomicUsize::new(0);
pub static SLOTS_ORIG: AtomicUsize = AtomicUsize::new(0);
pub static COOK_ORIG: AtomicUsize = AtomicUsize::new(0);
pub static QUICK_ROW_ORIG: AtomicUsize = AtomicUsize::new(0);
pub static QUICK_DEST_ORIG: AtomicUsize = AtomicUsize::new(0);
pub static QUICK_FREE_ORIG: AtomicUsize = AtomicUsize::new(0);
pub static OBJECT_CHESTS_ORIG: AtomicUsize = AtomicUsize::new(0);
pub static QUICK_BUTTON_ORIG: AtomicUsize = AtomicUsize::new(0);

type ChoicesFn = unsafe extern "system" fn(u64, i32, u64, u8) -> u64;
type BuildAffordFn = unsafe extern "system" fn(u64, u64) -> i32;
type BuildPayFn = unsafe extern "system" fn(u64, i32, u64) -> u64;
type CountFn = unsafe extern "system" fn(u64, u64, u64) -> i32;
type RemoveFn = unsafe extern "system" fn(u64, u64, u64, u64, u64) -> i32;
type ChargeFn = unsafe extern "system" fn(u64, u64) -> u64;
type SourcesFn = unsafe extern "system" fn(u64, u64, u64) -> u64;
type PopupFn = unsafe extern "system" fn(u64, u64) -> u64;
type SlotsFn = unsafe extern "system" fn(u64, u64, i32, u64, u64, u64, u64, u64, u64, u64, u64, u64) -> u64;
type CookFn = unsafe extern "system" fn(u64) -> u8;
type QuickRowFn = unsafe extern "system" fn(u64, u64, u64) -> u64;
type QuickFreeFn = unsafe extern "system" fn(u64, u64) -> i32;
type ObjectChestsFn = unsafe extern "system" fn(u64, u64, u64) -> u64;
type QuickButtonFn = unsafe extern "system" fn(u64, u64, i32, i32, u64, u8) -> u8;

pub struct HookHandles {
    pub named: Vec<(&'static str, Arc<Hook>)>,
    /// All installed hooks, including Quick Transfer ones the watcher never disables.
    #[expect(dead_code)]
    pub kept: Vec<Arc<Hook>>,
}

pub const HOOK_NAMES: &[&str] = &[
    "CraftingInventoryChoices",
    "BuildAfford",
    "BuildPay",
    "CountInChoices",
    "RemoveFromChoices",
    "ChargePopup",
    "ItemSources",
    "PopupBuild",
    "PopupSlots",
];

// Python flipped hooks with MinHook, which suspends every other thread while it rewrites a prologue.
// retour does not, so a live enable/disable can tear the bytes under a running game thread. The
// hooks stay installed for the session; these gates decide whether the detour bodies run.
static CRAFT_GATE: AtomicBool = AtomicBool::new(true);
static BUILD_GATE: AtomicBool = AtomicBool::new(true);

pub fn set_build_gate(open: bool) {
    BUILD_GATE.store(open, Ordering::Release);
}

/// The game code was not found: every crafting hook passes straight through from now on.
pub fn close_craft_gate() {
    CRAFT_GATE.store(false, Ordering::Release);
}

fn craft_open() -> bool {
    CRAFT_GATE.load(Ordering::Acquire)
}

fn build_open() -> bool {
    craft_open() && BUILD_GATE.load(Ordering::Acquire)
}

pub fn all_named_enabled(handles: &HookHandles) -> bool {
    HOOK_NAMES.iter().all(|want| {
        handles
            .named
            .iter()
            .any(|(name, hook)| name == want && hook.is_enabled())
    })
}

fn find_sig(image: &ProcessImage, sig: &str) -> Option<usize> {
    Pattern::parse(sig).ok().and_then(|p| image.find_unique(&p).unique())
}

fn install_one(
    image: &ProcessImage,
    log: &Log,
    name: &'static str,
    sig: &str,
    detour: *const (),
    orig: &AtomicUsize,
) -> Option<Arc<Hook>> {
    let rva = find_sig(image, sig)?;
    // SAFETY: unique pattern is the function entry; the detour declares every Python argument.
    match unsafe { Hook::install(name, image.va(rva), detour, orig) } {
        Ok(hook) => Some(Arc::new(hook)),
        Err(e) => {
            log.info(&format!("{name} not changed: {e}"));
            None
        }
    }
}

pub fn install(image: &ProcessImage, log: Arc<Log>) -> HookHandles {
    let mut named = Vec::new();
    let mut kept = Vec::new();
    let add_named = |named: &mut Vec<(&'static str, Arc<Hook>)>,
                     kept: &mut Vec<Arc<Hook>>,
                     name: &'static str,
                     hook: Option<Arc<Hook>>| {
        if let Some(h) = hook {
            named.push((name, h.clone()));
            kept.push(h);
        }
    };

    add_named(
        &mut named,
        &mut kept,
        "CraftingInventoryChoices",
        install_one(
            image,
            &log,
            "CraftingInventoryChoices",
            sigs::CHOICES,
            choices_detour as *const (),
            &CHOICES_ORIG,
        ),
    );
    add_named(
        &mut named,
        &mut kept,
        "BuildAfford",
        install_one(
            image,
            &log,
            "BuildAfford",
            sigs::BUILD_AFFORD,
            build_afford_detour as *const (),
            &BUILD_AFFORD_ORIG,
        ),
    );
    add_named(
        &mut named,
        &mut kept,
        "BuildPay",
        install_one(
            image,
            &log,
            "BuildPay",
            sigs::BUILD_PAY,
            build_pay_detour as *const (),
            &BUILD_PAY_ORIG,
        ),
    );
    add_named(
        &mut named,
        &mut kept,
        "CountInChoices",
        install_one(
            image,
            &log,
            "CountInChoices",
            sigs::COUNT,
            count_detour as *const (),
            &COUNT_ORIG,
        ),
    );
    add_named(
        &mut named,
        &mut kept,
        "RemoveFromChoices",
        install_one(
            image,
            &log,
            "RemoveFromChoices",
            sigs::REMOVE,
            remove_detour as *const (),
            &REMOVE_ORIG,
        ),
    );
    add_named(
        &mut named,
        &mut kept,
        "ChargePopup",
        install_one(
            image,
            &log,
            "ChargePopup",
            sigs::CHARGE,
            charge_detour as *const (),
            &CHARGE_ORIG,
        ),
    );
    add_named(
        &mut named,
        &mut kept,
        "ItemSources",
        install_one(
            image,
            &log,
            "ItemSources",
            sigs::SOURCES,
            sources_detour as *const (),
            &SOURCES_ORIG,
        ),
    );
    add_named(
        &mut named,
        &mut kept,
        "PopupBuild",
        install_one(image, &log, "PopupBuild", sigs::POPUP, popup_detour as *const (), &POPUP_ORIG),
    );
    add_named(
        &mut named,
        &mut kept,
        "PopupSlots",
        install_one(image, &log, "PopupSlots", sigs::SLOTS, slots_detour as *const (), &SLOTS_ORIG),
    );

    if let Some(h) = install_one(
        image,
        &log,
        "CookingIngredient",
        sigs::COOK_ITEM,
        cook_detour as *const (),
        &COOK_ORIG,
    ) {
        kept.push(h);
    }
    if let Some(h) = install_one(
        image,
        &log,
        "QuickTransferRow",
        sigs::QUICK_ROW,
        quick_row_detour as *const (),
        &QUICK_ROW_ORIG,
    ) {
        kept.push(h);
    }
    if let Some(h) = install_one(
        image,
        &log,
        "QuickTransferDest",
        sigs::QUICK_DEST,
        quick_dest_detour as *const (),
        &QUICK_DEST_ORIG,
    ) {
        kept.push(h);
    }
    if let Some(h) = install_one(
        image,
        &log,
        "QuickTransferFree",
        sigs::QUICK_FREE,
        quick_free_detour as *const (),
        &QUICK_FREE_ORIG,
    ) {
        kept.push(h);
    }
    if let Err(e) = quick_transfer_pick::subscribe(image, sigs::QUICK_PICK, Some(before_quick_pick), Some(after_quick_pick)) {
        log.info(&format!("QuickTransferPick not changed: {e}"));
    }
    if let Some(h) = install_one(
        image,
        &log,
        "ObjectChests",
        sigs::OBJECT_CHESTS,
        object_chests_detour as *const (),
        &OBJECT_CHESTS_ORIG,
    ) {
        kept.push(h);
    }
    if let Some(h) = install_one(
        image,
        &log,
        "QuickButton",
        sigs::QUICK_BUTTON,
        quick_button_detour as *const (),
        &QUICK_BUTTON_ORIG,
    ) {
        kept.push(h);
    }

    HookHandles { named, kept }
}

fn enter_build() {
    with_runtime(|state| {
        if !state.active() {
            return;
        }
        BUILD_DEPTH.with(|d| d.set(d.get().saturating_add(1)));
        state.build_until = Some(std::time::Instant::now() + std::time::Duration::from_secs_f64(BUILD_LINGER_SECONDS));
        if !state.build_hooks_on {
            state.build_hooks_on = true;
            set_build_gate(true);
        }
    });
}

fn leave_build() {
    BUILD_DEPTH.with(|d| {
        let v = d.get();
        if v > 0 {
            d.set(v.saturating_sub(1));
        }
    });
}

unsafe extern "system" fn choices_detour(owner: u64, context: i32, choices: u64, include_tech: u8) -> u64 {
    let ret = call_original(&CHOICES_ORIG, |f: ChoicesFn| unsafe { f(owner, context, choices, include_tech) }, 0);
    if !craft_open() {
        return ret;
    }
    with_runtime(|state| {
        if state.active() && choices >= MIN_ADDR as u64 && super::lists::context_ok(context) {
            state.extend(choices as usize, &format!("Crafting (inventory kind {context})"));
        }
    });
    ret
}

unsafe extern "system" fn build_afford_detour(player_state: u64, part: u64) -> i32 {
    if !craft_open() {
        return call_original(&BUILD_AFFORD_ORIG, |f: BuildAffordFn| unsafe { f(player_state, part) }, 0);
    }
    enter_build();
    let ret = call_original(&BUILD_AFFORD_ORIG, |f: BuildAffordFn| unsafe { f(player_state, part) }, 0);
    leave_build();
    ret
}

unsafe extern "system" fn build_pay_detour(player_state: u64, flags: i32, part: u64) -> u64 {
    if !craft_open() {
        return call_original(&BUILD_PAY_ORIG, |f: BuildPayFn| unsafe { f(player_state, flags, part) }, 0);
    }
    enter_build();
    let ret = call_original(&BUILD_PAY_ORIG, |f: BuildPayFn| unsafe { f(player_state, flags, part) }, 0);
    leave_build();
    ret
}

unsafe extern "system" fn count_detour(owner: u64, choices: u64, item: u64) -> i32 {
    if build_open() {
        with_runtime(|state| state.build_list(choices));
    }
    call_original(&COUNT_ORIG, |f: CountFn| unsafe { f(owner, choices, item) }, 0)
}

unsafe extern "system" fn remove_detour(owner: u64, choices: u64, item: u64, removed: u64, flags: u64) -> i32 {
    if build_open() {
        with_runtime(|state| state.build_list(choices));
    }
    call_original(&REMOVE_ORIG, |f: RemoveFn| unsafe { f(owner, choices, item, removed, flags) }, 0)
}

unsafe extern "system" fn charge_detour(popup: u64, target: u64) -> u64 {
    if craft_open() {
        with_runtime(|state| state.extend_owned());
    }
    call_original(&CHARGE_ORIG, |f: ChargeFn| unsafe { f(popup, target) }, 0)
}

unsafe extern "system" fn sources_detour(player_state: u64, sources: u64, item: u64) -> u64 {
    if craft_open() {
        with_runtime(|state| state.extend_owned());
    }
    call_original(&SOURCES_ORIG, |f: SourcesFn| unsafe { f(player_state, sources, item) }, 0)
}

unsafe extern "system" fn popup_detour(popup: u64, page: u64) -> u64 {
    if craft_open() {
        with_runtime(|state| state.before_popup(popup));
    }
    let ret = call_original(&POPUP_ORIG, |f: PopupFn| unsafe { f(popup, page) }, 0);
    POPUP_MODE.with(|m| m.set(None));
    ret
}

unsafe extern "system" fn slots_detour(
    popup: u64,
    page: u64,
    count: i32,
    vector: u64,
    s20: u64,
    s28: u64,
    s30: u64,
    s38: u64,
    s40: u64,
    s48: u64,
    s50: u64,
    s58: u64,
) -> u64 {
    if craft_open() {
        with_runtime(|state| state.before_slots(popup, vector));
    }
    call_original(
        &SLOTS_ORIG,
        |f: SlotsFn| unsafe { f(popup, page, count, vector, s20, s28, s30, s38, s40, s48, s50, s58) },
        0,
    )
}

unsafe extern "system" fn cook_detour(element: u64) -> u8 {
    let ret = call_original(&COOK_ORIG, |f: CookFn| unsafe { f(element) }, 0);
    with_runtime(|state| {
        state.quick.qt_element = element;
    });
    ret
}

unsafe extern "system" fn quick_row_detour(rows: u64, object_id: u64, enabled: u64) -> u64 {
    let ret = call_original(&QUICK_ROW_ORIG, |f: QuickRowFn| unsafe { f(rows, object_id, enabled) }, 0);
    with_runtime(|state| state.after_quick_row(rows, object_id, enabled));
    ret
}

unsafe extern "system" fn quick_dest_detour(rows: u64, dest_type: u64, enabled: u64) -> u64 {
    let ret = call_original(&QUICK_DEST_ORIG, |f: QuickRowFn| unsafe { f(rows, dest_type, enabled) }, 0);
    with_runtime(|state| state.after_quick_dest(rows, dest_type));
    ret
}

unsafe extern "system" fn quick_free_detour(row: u64, extra: u64) -> i32 {
    with_runtime(|state| state.quick.force_from_row(row));
    let ret = call_original(&QUICK_FREE_ORIG, |f: QuickFreeFn| unsafe { f(row, extra) }, 0);
    with_runtime(|state| state.quick.forced = 0);
    ret
}

fn before_quick_pick(args: &mut quick_transfer_pick::Args) {
    let row = args.dest;
    with_runtime(|state| state.quick.force_from_row(row));
}

fn after_quick_pick(_args: &quick_transfer_pick::Args, _ret: u64) {
    with_runtime(|state| state.quick.forced = 0);
}

unsafe extern "system" fn object_chests_detour(owner: u64, object_id: u64, out: u64) -> u64 {
    let ret = call_original(&OBJECT_CHESTS_ORIG, |f: ObjectChestsFn| unsafe { f(owner, object_id, out) }, 0);
    with_runtime(|state| {
        if out >= MIN_ADDR as u64 {
            state.quick.filter_object_chests(out as usize);
        }
    });
    ret
}

unsafe extern "system" fn quick_button_detour(state_ptr: u64, a2: u64, a3: i32, a4: i32, a5: u64, a6: u8) -> u8 {
    with_runtime(|state| {
        if let Some(at) = state.quick.at {
            if at.elapsed().as_secs_f64() < 0.1 && state_ptr >= MIN_ADDR as u64 {
                state.quick.page = (state_ptr as usize).wrapping_sub(QUICK_STATE_AT);
            }
        }
    });
    call_original(
        &QUICK_BUTTON_ORIG,
        |f: QuickButtonFn| unsafe { f(state_ptr, a2, a3, a4, a5, a6) },
        0,
    )
}
