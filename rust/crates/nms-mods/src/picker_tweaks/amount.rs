//! Type an amount.
//!
//! Two controls, both written where the game itself changes them, on the game's thread:
//! - Stack split (also machine-slot amounts and the Transfer picker, `page_mode` 1): the held
//!   stack struct at `page+0x19670`, amount `page+0x19688` (`0x19610`/`0x19628` before build 183791).
//!   Written in a before-hook on the per-frame held-stack tick `0x1409000B0`, clamped to
//!   `[1, element Amount]`; the game's own drop then moves that many (it reads `page+0x19688`, as after
//!   its own arrow presses).
//! - Refiner amount: `(unit+0x2A0 >> 3) & 0xFFF`, written only through the game's setter
//!   `0x1410D7D50(unit, n)` in a before-hook on the refiner page prologue `0x14081F1A0`, clamped to
//!   the same bounds the page gives its arrows: `[min, min + extra]` with `min = 0x1410D7B90(unit)`
//!   and `extra = 0x1410D7CA0(unit)`.
//!
//! Craft amount and the shop slider are out of scope (the shop is TradeUITweaks').

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

use nms_core::mem::{read_bytes, read_i32, read_ptr, read_u32, readable, MIN_ADDR};

use super::{log_line, now_ms, TYPED_QTY};

/// Held stack struct `S = page + 0x19670`: amount `S+0x18`, slot `S+0x30/+0x34`, inventory `S+0x40`.
/// Build 183791 moved the struct by 0x60 (`0x19610` before): the held tick `0x1409000B0` reads
/// `[rbp+0x196B0]`, `[rbp+0x196A0]`, `[rbp+0x196A4]` and halves `[rbp+0x19688]` (`0x1409007B2..0x1409007F4`).
const HELD_AMOUNT: usize = 0x19688;
const HELD_X: usize = 0x196A0;
const HELD_Y: usize = 0x196A4;
const HELD_INV: usize = 0x196B0;
/// Inventory store: element count `+0x8C`, element array `+0x90` (0x30 bytes each: slot x `+0x10`,
/// y `+0x14`, Amount `+0x18`), as `GetElement 0x1404CE7F0` walks it (amount read at `0x14090084D`).
const STORE_COUNT: usize = 0x8C;
const STORE_DATA: usize = 0x90;
const ELEMENT: usize = 0x30;
const ELEMENTS_MAX: i32 = 4096;
const REFINER_AMOUNT: usize = 0x2A0;
/// A typed amount waits this long for its control to be drawn again.
pub const WRITE_WAIT_MS: u64 = 2000;

pub static ORIG_HELD: AtomicUsize = AtomicUsize::new(0);
pub static ORIG_REFINER: AtomicUsize = AtomicUsize::new(0);
pub static HELD_PAGE: AtomicUsize = AtomicUsize::new(0);
pub static HELD_SEEN_MS: AtomicU64 = AtomicU64::new(0);
pub static REFINER_PAGE: AtomicUsize = AtomicUsize::new(0);
pub static REFINER_UNIT: AtomicUsize = AtomicUsize::new(0);
pub static REFINER_SEEN_MS: AtomicU64 = AtomicU64::new(0);
static WRITE_PENDING: AtomicBool = AtomicBool::new(false);

type HeldFn = unsafe extern "system" fn(usize, f32) -> u64;
type PageFn = unsafe extern "system" fn(usize, usize) -> u64;
type UnitFn = unsafe extern "system" fn(usize) -> usize;
type CountFn = unsafe extern "system" fn(usize) -> u32;
type SetterFn = unsafe extern "system" fn(usize, u32);

/// The refiner routines: machine getter (the `E8` at `RefinerPage+0x28`), bounds and setter.
pub struct RefinerFns {
    pub getter: usize,
    pub min: usize,
    pub extra: usize,
    pub setter: usize,
}
static REFINER_FNS: OnceLock<RefinerFns> = OnceLock::new();

pub fn set_refiner_fns(f: RefinerFns) {
    let _ = REFINER_FNS.set(f);
}

#[derive(Clone, Copy, Debug)]
pub enum Write {
    Split { page: usize, inv: usize, x: i32, y: i32, value: u32, until: u64 },
    Refiner { unit: usize, value: u32, until: u64 },
}

static WRITE: Mutex<Option<Write>> = Mutex::new(None);

/// What the player holds, read from the held struct.
#[derive(Clone, Copy, Debug)]
pub struct Held {
    pub inv: usize,
    pub x: i32,
    pub y: i32,
    pub amount: u32,
}

pub fn held(page: usize) -> Option<Held> {
    if page < MIN_ADDR || !readable(page + HELD_AMOUNT, HELD_INV + 8 - HELD_AMOUNT) {
        return None;
    }
    let inv = read_ptr(page + HELD_INV)?;
    let (x, y) = (read_i32(page + HELD_X)?, read_i32(page + HELD_Y)?);
    if x < 0 || y < 0 {
        return None;
    }
    Some(Held { inv, x, y, amount: read_u32(page + HELD_AMOUNT)? })
}

/// The live element's Amount at slot `(x, y)` of an inventory store: the split maximum.
pub fn element_amount(inv: usize, x: i32, y: i32) -> Option<u32> {
    let count = read_i32(inv + STORE_COUNT)?.clamp(0, ELEMENTS_MAX) as usize;
    let data = read_ptr(inv + STORE_DATA)?;
    for i in 0..count {
        let e = data + i * ELEMENT;
        let cell = read_bytes(e + 0x10, 12)?;
        let ex = i32::from_le_bytes([cell[0], cell[1], cell[2], cell[3]]);
        let ey = i32::from_le_bytes([cell[4], cell[5], cell[6], cell[7]]);
        if ex == x && ey == y {
            let amount = i32::from_le_bytes([cell[8], cell[9], cell[10], cell[11]]);
            return u32::try_from(amount).ok();
        }
    }
    None
}

/// The refiner amount as the game packs it: `(w >> 3) & 0xFFF`.
pub fn refiner_amount(unit: usize) -> Option<u32> {
    let w = read_bytes(unit + REFINER_AMOUNT, 2)?;
    Some((u32::from(u16::from_le_bytes([w[0], w[1]])) >> 3) & 0xFFF)
}

/// Queue a typed amount; the next call of its control writes it.
pub fn queue(write: Write) {
    // `try_lock` with a short spin: the only other holder is a detour, briefly.
    for _ in 0..2000 {
        if let Ok(mut w) = WRITE.try_lock() {
            *w = Some(write);
            WRITE_PENDING.store(true, Ordering::Release);
            return;
        }
        std::hint::spin_loop();
    }
    log_line("typed amount not used: busy, try again");
}

/// Drop any queued amount (switch turned off).
pub fn clear() {
    if let Ok(mut w) = WRITE.lock() {
        *w = None;
    }
    WRITE_PENDING.store(false, Ordering::Release);
}

/// Every frame: drop a queued amount whose control was not drawn again in time.
pub fn expire(now: u64) {
    if !WRITE_PENDING.load(Ordering::Acquire) {
        return;
    }
    let Ok(mut w) = WRITE.try_lock() else { return };
    let until = match *w {
        Some(Write::Split { until, .. } | Write::Refiner { until, .. }) => until,
        None => return,
    };
    if now > until {
        *w = None;
        WRITE_PENDING.store(false, Ordering::Release);
        log_line("typed amount not used: the amount control was closed");
    }
}

fn take_if(pred: impl Fn(&Write) -> bool) -> Option<Write> {
    if !WRITE_PENDING.load(Ordering::Acquire) {
        return None;
    }
    let mut w = WRITE.try_lock().ok()?;
    if !w.as_ref().is_some_and(pred) {
        return None;
    }
    WRITE_PENDING.store(false, Ordering::Release);
    w.take()
}

/// Write a typed split amount into the held struct, if the same stack is still held.
fn apply_split(page: usize) {
    let Some(Write::Split { inv, x, y, value, .. }) =
        take_if(|w| matches!(w, Write::Split { page: p, .. } if *p == page))
    else {
        return;
    };
    let Some(h) = held(page).filter(|h| h.inv == inv && h.x == x && h.y == y) else {
        log_line("typed amount not used: the stack is no longer held");
        return;
    };
    let Some(max) = element_amount(inv, x, y).filter(|&m| m > 0) else {
        log_line("typed amount not used: the held stack was not found");
        return;
    };
    let n = value.clamp(1, max);
    // SAFETY: the held amount, a dword the game changes the same way on every arrow press.
    let ok = unsafe { nms_core::mem::write(page + HELD_AMOUNT, n) };
    log_line(&format!(
        "split amount {} -> {n} (typed {}, range 1 to {max}){}",
        h.amount,
        typed_text(value),
        if ok { "" } else { ": write failed" }
    ));
}

/// Set a typed refiner amount through the game's own setter, inside the bounds of its arrows.
fn apply_refiner(unit: usize) {
    let Some(f) = REFINER_FNS.get() else { return };
    let Some(Write::Refiner { value, .. }) = take_if(|w| matches!(w, Write::Refiner { unit: u, .. } if *u == unit)) else {
        return;
    };
    // SAFETY: the bounds routines and the setter the refiner page itself calls every frame
    // every frame, with the machine object it uses.
    let (min, extra) = unsafe {
        (
            std::mem::transmute::<usize, CountFn>(f.min)(unit),
            std::mem::transmute::<usize, CountFn>(f.extra)(unit),
        )
    };
    let (min, extra) = (min as i32, extra as i32);
    // The field holds 12 bits; the setter caps at 0xFFF too.
    let max = min.saturating_add(extra).min(0xFFF);
    if max <= 0 || min < 0 || min > max {
        log_line(&format!("refiner amount not changed: no range (min {min}, max {max})"));
        return;
    }
    let before = refiner_amount(unit).unwrap_or(0);
    let n = (value.min(0xFFF) as i32).clamp(min, max) as u32;
    unsafe { std::mem::transmute::<usize, SetterFn>(f.setter)(unit, n) };
    log_line(&format!(
        "refiner amount {before} -> {} (typed {}, range {min} to {max})",
        refiner_amount(unit).map_or_else(|| "?".to_string(), |a| a.to_string()),
        typed_text(value)
    ));
}

/// The held-stack tick (`rcx` page, `xmm1` frame time): note the page, apply a typed amount.
pub unsafe extern "system" fn held_detour(page: usize, dt: f32) -> u64 {
    HELD_PAGE.store(page, Ordering::Release);
    HELD_SEEN_MS.store(now_ms(), Ordering::Release);
    if WRITE_PENDING.load(Ordering::Acquire) {
        apply_split(page);
    }
    let original: HeldFn = std::mem::transmute(ORIG_HELD.load(Ordering::Acquire));
    original(page, dt)
}

/// The refiner page prologue: note the machine, apply a typed amount before the page draws it.
pub unsafe extern "system" fn refiner_detour(this: usize, page: usize) -> u64 {
    if TYPED_QTY.load(Ordering::Acquire) && page >= MIN_ADDR {
        if let Some(f) = REFINER_FNS.get() {
            // SAFETY: the same call `RefinerPage` makes first (`0x14081F1C8`); a component lookup.
            let unit = std::mem::transmute::<usize, UnitFn>(f.getter)(page);
            if unit >= MIN_ADDR && readable(unit + REFINER_AMOUNT, 2) {
                REFINER_PAGE.store(page, Ordering::Release);
                REFINER_UNIT.store(unit, Ordering::Release);
                REFINER_SEEN_MS.store(now_ms(), Ordering::Release);
                if WRITE_PENDING.load(Ordering::Acquire) {
                    apply_refiner(unit);
                }
            }
        }
    }
    let original: PageFn = std::mem::transmute(ORIG_REFINER.load(Ordering::Acquire));
    original(this, page)
}

/// What the player typed, for the log: an empty Accept is queued as `u32::MAX` ("the most").
fn typed_text(value: u32) -> String {
    if value == u32::MAX {
        "nothing: the most".to_string()
    } else {
        value.to_string()
    }
}

/// A typed number: digits only (spaces and thousands separators allowed), above zero.
pub fn parse(text: &str) -> Option<u32> {
    let digits: String = text.chars().filter(|c| !matches!(c, ' ' | ',' | '\'' | '_')).collect();
    if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let n: u64 = digits.parse().ok()?;
    (n > 0).then(|| n.min(u64::from(u32::MAX)) as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_positive_whole_numbers_only() {
        assert_eq!(parse("7"), Some(7));
        assert_eq!(parse(" 1,250 "), Some(1250));
        assert_eq!(parse("0"), None);
        assert_eq!(parse("-3"), None);
        assert_eq!(parse("ten"), None);
        assert_eq!(parse(""), None);
        assert_eq!(parse("99999999999"), Some(u32::MAX));
    }
}
