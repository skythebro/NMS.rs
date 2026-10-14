//! Deposit hover cave, lock dim cave, and notify/toast cave.

use std::collections::HashMap;
use std::time::Instant;

use nms_core::mem::{self, alloc_exec_near, read_bytes, read_i32, read_u8, read_u64, MIN_ADDR};
use nms_core::process::ProcessImage;
use nms_core::{Code, Pattern};
use nms_sdk::sigs::quick_transfer_all as sigs;
use windows_sys::Win32::System::Diagnostics::Debug::FlushInstructionCache;
use windows_sys::Win32::System::Memory::{VirtualProtect, PAGE_EXECUTE_READWRITE};
use windows_sys::Win32::System::Threading::GetCurrentProcess;

use super::inv::HOVER_STALE_SECS;
use super::text::{self, TIP_LEN, TOAST_LEN};

pub const DEPOSIT_PATCH_BYTES: &[u8] = &[0x48, 0xC7, 0x45, 0x90, 0xFF, 0xFF, 0xFF, 0xFF];
pub const NOTIFY_PATCH_BYTES: &[u8] = &[0x44, 0x8B, 0x7C, 0x24, 0x74];
pub const FILTER_STEAL: &[u8] = &[0x41, 0x8B, 0x44, 0x24, 0x24];
pub const FILTER_JOIN_AT: usize = 0xFF;
pub const DIM_BITS: usize = 128;
pub const PAGE_ELEMENT: u32 = 0x14478;
pub const HOVER_FLAG_AT: usize = 0x177;
pub const HOVER_FLAG_BYTES: &[u8] = &[0x80, 0xBF, 0x6C, 0x96, 0x01, 0x00, 0x00];
const DEPOSIT_FIND_AT: usize = 0x3D;
const DEPOSIT_HASH_AT: usize = 0x5C;
const DEPOSIT_CHILD_AT: usize = 0x6A;
const DEPOSIT_CURSOR_AT: usize = 0x84;

type ToastAddFn = unsafe extern "system" fn(u64, u64, f32, u64, u32, u64, u64, f32, u8, u8, u8);
type ToastCleanFn = unsafe extern "system" fn(u64);

pub fn rel32_bytes(source: usize, target: usize) -> Result<[u8; 4], String> {
    let rel = mem::rel32(source.wrapping_add(4), target)
        .ok_or_else(|| format!("jump {source:#x} -> {target:#x} does not fit"))?;
    Ok(rel.to_le_bytes())
}

pub fn write_code(site: usize, raw: &[u8]) -> Result<(), String> {
    let mut old = 0u32;
    if unsafe { VirtualProtect(site as *mut _, raw.len(), PAGE_EXECUTE_READWRITE, &mut old) } == 0 {
        return Err(format!("the game code at {site:#x} could not be made writable"));
    }
    unsafe {
        std::ptr::copy_nonoverlapping(raw.as_ptr(), site as *mut u8, raw.len());
        VirtualProtect(site as *mut _, raw.len(), old, &mut old);
        FlushInstructionCache(GetCurrentProcess(), site as _, raw.len());
    }
    Ok(())
}

pub fn write_exec(addr: usize, raw: &[u8]) {
    unsafe {
        std::ptr::copy_nonoverlapping(raw.as_ptr(), addr as *mut u8, raw.len());
        FlushInstructionCache(GetCurrentProcess(), addr as _, raw.len());
    }
}

fn rel32_target(operand: usize) -> Option<usize> {
    let d = read_i32(operand)?;
    Some(operand.wrapping_add(4).wrapping_add(d as usize))
}

pub(crate) fn find_sig(image: &ProcessImage, sig: &str) -> Option<usize> {
    Pattern::parse(sig).ok().and_then(|p| image.find_unique(&p).unique())
}

pub(crate) fn rel32_va(operand: usize) -> Option<usize> {
    rel32_target(operand)
}

fn push_bytes(code: &mut Vec<u8>, raw: &[u8]) -> usize {
    let at = code.len();
    code.extend_from_slice(raw);
    at
}

fn emit_cell(cell: &std::cell::RefCell<Vec<u8>>, raw: &[u8]) -> usize {
    push_bytes(&mut cell.borrow_mut(), raw)
}

fn put_rel32(code: &mut [u8], at: usize, cave: usize, target: usize) -> Result<(), String> {
    let bytes = rel32_bytes(cave.wrapping_add(at), target)?;
    if at.wrapping_add(4) > code.len() {
        return Err("rel32 site missing".into());
    }
    code[at..at + 4].copy_from_slice(&bytes);
    Ok(())
}

fn rel8(code: &mut [u8], at: usize, target: usize) -> Result<(), String> {
    let rel = target as i32 - (at as i32 + 1);
    if !(-128..=127).contains(&rel) {
        return Err(format!("short jump does not fit ({rel})"));
    }
    if let Some(slot) = code.get_mut(at) {
        *slot = rel as u8;
        Ok(())
    } else {
        Err("short jump site missing".into())
    }
}

pub struct DepositHover {
    pub hover: usize,
    pub enabled: usize,
    pub same: usize,
    pub ticks: usize,
    pub lock_hover: usize,
    pub lock_enabled: usize,
    seen: u32,
    seen_at: Instant,
}

impl DepositHover {
    fn new(hover: usize, enabled: usize, same: usize, ticks: usize, lock_hover: usize, lock_enabled: usize) -> Self {
        Self {
            hover,
            enabled,
            same,
            ticks,
            lock_hover,
            lock_enabled,
            seen: read_u64(ticks).map(|v| v as u32).unwrap_or(0),
            seen_at: Instant::now(),
        }
    }

    pub fn set_enabled(&self, value: bool) {
        let _ = unsafe { mem::write(self.enabled, if value { 1u8 } else { 0u8 }) };
    }

    pub fn set_lock_enabled(&self, value: bool) {
        let _ = unsafe { mem::write(self.lock_enabled, if value { 1u8 } else { 0u8 }) };
    }

    pub fn set_same(&self, value: bool) {
        let _ = unsafe { mem::write(self.same, if value { 1u8 } else { 0u8 }) };
    }

    fn fresh(&mut self) -> bool {
        let now = Instant::now();
        let ticks = read_u64(self.ticks).map(|v| v as u32).unwrap_or(self.seen);
        if ticks != self.seen {
            self.seen = ticks;
            self.seen_at = now;
        }
        now.duration_since(self.seen_at).as_secs_f64() <= HOVER_STALE_SECS
    }

    pub fn hovered(&mut self) -> bool {
        self.fresh() && read_u8(self.hover).unwrap_or(0) != 0
    }

    pub fn lock_hovered(&mut self) -> bool {
        self.fresh() && read_u8(self.lock_hover).unwrap_or(0) != 0
    }
}

pub struct LockDim {
    pub site: usize,
    pub cave: usize,
    pub enabled: usize,
    pub bits: usize,
    pub applied: bool,
    pub dead: bool,
}

impl LockDim {
    pub fn apply(&mut self) -> bool {
        if self.applied {
            return true;
        }
        if self.dead {
            return false;
        }
        let Some(head) = read_bytes(self.site, FILTER_STEAL.len()) else {
            self.dead = true;
            return false;
        };
        if head != FILTER_STEAL {
            self.dead = true;
            return false;
        }
        let Ok(rel) = rel32_bytes(self.site.wrapping_add(1), self.cave) else {
            self.dead = true;
            return false;
        };
        let mut jmp = vec![0xE9];
        jmp.extend_from_slice(&rel);
        if write_code(self.site, &jmp).is_err() {
            self.dead = true;
            return false;
        }
        self.applied = true;
        true
    }

    pub fn undo(&mut self) {
        if !self.applied {
            return;
        }
        let _ = write_code(self.site, FILTER_STEAL);
        self.applied = false;
    }

    pub fn set_enabled(&self, value: bool) {
        let _ = unsafe { mem::write(self.enabled, if value { 1u8 } else { 0u8 }) };
    }

    pub fn load_pins(&self, pins: &HashMap<(i32, i32, i32), super::inv::Pin>, inv: Option<i32>) {
        for n in 0..DIM_BITS {
            let _ = unsafe { mem::write(self.bits.wrapping_add(n), 0u8) };
        }
        let Some(inv) = inv else {
            return;
        };
        for &(owned, x, y) in pins.keys() {
            if owned != inv || !(0..32).contains(&x) || !(0..32).contains(&y) {
                continue;
            }
            let byte = self.bits.wrapping_add((y as usize).wrapping_mul(4)).wrapping_add((x as usize) >> 3);
            let old = read_u8(byte).unwrap_or(0);
            let _ = unsafe { mem::write(byte, old | (1u8 << (x & 7))) };
        }
    }
}

pub struct ButtonNotify {
    pub lock_mode: usize,
    pub lock_fire: usize,
    pub dep_fire: usize,
    pub dep_osd: usize,
    pub visual: usize,
    pub dep_click: usize,
    pub lock_click: usize,
    toast: Option<(usize, usize, usize, usize, i32)>,
    lock_text: [u8; TOAST_LEN],
    timer: [u8; 16],
}

impl ButtonNotify {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        lock_mode: usize,
        lock_fire: usize,
        dep_fire: usize,
        dep_osd: usize,
        toast: Option<(usize, usize, usize, usize, i32)>,
        visual: usize,
        dep_click: usize,
        lock_click: usize,
    ) -> Self {
        Self {
            lock_mode,
            lock_fire,
            dep_fire,
            dep_osd,
            visual,
            dep_click,
            lock_click,
            toast,
            lock_text: [0; TOAST_LEN],
            timer: [0; 16],
        }
    }

    pub fn has_visual(&self) -> bool {
        self.visual != 0
    }

    pub fn set_visual(&self, value: bool) {
        if self.visual >= MIN_ADDR {
            let _ = unsafe { mem::write(self.visual, if value { 1u8 } else { 0u8 }) };
        }
    }

    fn take_click(&self, address: usize) -> bool {
        if address < MIN_ADDR {
            return false;
        }
        let hit = read_u8(address).unwrap_or(0) != 0;
        if hit {
            let _ = unsafe { mem::write(address, 0u8) };
        }
        hit
    }

    pub fn take_dep_click(&self) -> bool {
        self.take_click(self.dep_click)
    }

    pub fn take_lock_click(&self) -> bool {
        self.take_click(self.lock_click)
    }

    pub fn set_lock_mode(&self, value: bool) {
        let _ = unsafe { mem::write(self.lock_mode, if value { 1u8 } else { 0u8 }) };
    }

    pub fn set_lock_fire(&self, value: bool) {
        let _ = unsafe { mem::write(self.lock_fire, if value { 1u8 } else { 0u8 }) };
    }

    pub fn set_deposit_fire(&self, value: bool) {
        let _ = unsafe { mem::write(self.dep_fire, if value { 1u8 } else { 0u8 }) };
    }

    /// The Deposit toast text in its `TOAST_LEN` cave slot (cut on a character boundary, NUL-ended).
    pub fn set_deposit_osd(&self, text: &str) {
        if self.dep_osd < MIN_ADDR {
            return;
        }
        write_exec(self.dep_osd, &text::toast(text));
    }

    pub fn show_text(&self, text: &str) {
        self.set_deposit_osd(text);
        self.set_deposit_fire(true);
    }

    pub fn show_now(&mut self, text: &str) {
        let Some((add, clean, holder_at, title, object_off)) = self.toast else {
            return;
        };
        let holder = read_u64(holder_at).unwrap_or(0) as usize;
        if holder < MIN_ADDR {
            return;
        }
        self.lock_text = text::toast(text);
        self.timer = [0; 16];
        let timer = self.timer.as_mut_ptr() as u64;
        let add: ToastAddFn = unsafe { std::mem::transmute(add) };
        let clean: ToastCleanFn = unsafe { std::mem::transmute(clean) };
        unsafe {
            add(
                holder.wrapping_add(object_off as usize) as u64,
                self.lock_text.as_ptr() as u64,
                1.5,
                title as u64,
                0x18AECCF6,
                timer,
                0,
                0.0,
                0,
                0,
                0,
            );
            clean(timer);
        }
    }
}

fn pad16(name: &[u8]) -> [u8; 16] {
    let mut out = [0u8; 16];
    let n = name.len().min(16);
    out[..n].copy_from_slice(&name[..n]);
    out
}

/// The three tooltip keys of the deposit cave, `(label, text)`, in the game's language.
fn tip_slots(lang: &str) -> [(&'static str, [u8; TIP_LEN]); 3] {
    text::TIP_IDS.map(|id| (id, text::tip(lang, id)))
}

#[allow(clippy::too_many_arguments)]
fn deposit_image(
    cave: usize,
    back: usize,
    find: usize,
    name_hash: usize,
    child: usize,
    cursor: usize,
    lang: &str,
) -> Result<(Vec<u8>, HashMap<&'static str, usize>), String> {
    let code_cell = std::cell::RefCell::new(Vec::new());
    let mut fix: Vec<(usize, usize, &'static str)> = Vec::new();
    let inc_tick = emit_cell(&code_cell, &[0xFF, 0x05]);
    let ticks_disp = emit_cell(&code_cell, &[0, 0, 0, 0]);
    fix.push((ticks_disp, inc_tick + 6, "ticks"));
    emit_cell(&code_cell, &[0x57, 0x53, 0x48, 0x83, 0xEC, 0x20]);
    let lea_scratch = emit_cell(&code_cell, &[0x48, 0x8D, 0x1D]);
    let scratch_disp = emit_cell(&code_cell, &[0, 0, 0, 0]);
    fix.push((scratch_disp, lea_scratch + 7, "scratch"));
    emit_cell(&code_cell, &[0x66, 0x0F, 0x6F, 0x45, 0xB0, 0x66, 0x0F, 0x7F, 0x03]);
    let mut lea_page_el = vec![0x49, 0x8B, 0x8E];
    lea_page_el.extend_from_slice(&PAGE_ELEMENT.to_le_bytes());
    emit_cell(&code_cell, &lea_page_el);
    emit_cell(&code_cell, &[0x48, 0x85, 0xC9]);
    let jz_page = emit_cell(&code_cell, &[0x0F, 0x84, 0, 0, 0, 0]);
    let mov_page = emit_cell(&code_cell, &[0x66, 0x0F, 0x6F, 0x05]);
    let page_disp = emit_cell(&code_cell, &[0, 0, 0, 0]);
    fix.push((page_disp, mov_page + 8, "page"));
    emit_cell(&code_cell, &[0x66, 0x0F, 0x7F, 0x45, 0xB0, 0x48, 0x8D, 0x55, 0xB0, 0x45, 0x33, 0xC0]);
    let call1 = emit_cell(&code_cell, &[0xE8, 0, 0, 0, 0]);
    emit_cell(&code_cell, &[0x48, 0x85, 0xC0]);
    let jz_pc = emit_cell(&code_cell, &[0x0F, 0x84, 0, 0, 0, 0]);
    emit_cell(&code_cell, &[0x48, 0x8B, 0xC8]);
    let mov_filters = emit_cell(&code_cell, &[0x66, 0x0F, 0x6F, 0x05]);
    let filters_disp = emit_cell(&code_cell, &[0, 0, 0, 0]);
    fix.push((filters_disp, mov_filters + 8, "filters"));
    emit_cell(&code_cell, &[0x66, 0x0F, 0x7F, 0x45, 0xB0, 0x48, 0x8D, 0x55, 0xB0, 0x45, 0x33, 0xC0]);
    let call2 = emit_cell(&code_cell, &[0xE8, 0, 0, 0, 0]);
    emit_cell(&code_cell, &[0x48, 0x85, 0xC0]);
    let jz_filters = emit_cell(&code_cell, &[0x0F, 0x84, 0, 0, 0, 0]);
    emit_cell(&code_cell, &[0x48, 0x8B, 0xF8]);
    let lea_name = emit_cell(&code_cell, &[0x48, 0x8D, 0x15]);
    let name_disp = emit_cell(&code_cell, &[0, 0, 0, 0]);
    fix.push((name_disp, lea_name + 7, "name"));
    emit_cell(&code_cell, &[0x48, 0x8D, 0x4D, 0xB0, 0x45, 0x33, 0xC0]);
    let call_hash = emit_cell(&code_cell, &[0xE8, 0, 0, 0, 0]);
    emit_cell(&code_cell, &[0x48, 0x8B, 0xD0, 0x48, 0x8B, 0xCF, 0x45, 0x33, 0xC0]);
    let call_child = emit_cell(&code_cell, &[0xE8, 0, 0, 0, 0]);
    emit_cell(&code_cell, &[0x48, 0x85, 0xC0]);
    let jz_child = emit_cell(&code_cell, &[0x0F, 0x84, 0, 0, 0, 0]);
    emit_cell(&code_cell, &[0x48, 0x8B, 0x48, 0x48, 0x40, 0x84, 0xF6]);
    let je_hide = emit_cell(&code_cell, &[0x74, 0]);
    let cmp_en = emit_cell(&code_cell, &[0x80, 0x3D]);
    let en_disp = emit_cell(&code_cell, &[0, 0, 0, 0]);
    fix.push((en_disp, cmp_en + 7, "enabled"));
    emit_cell(&code_cell, &[0x00]);
    let je_off = emit_cell(&code_cell, &[0x74, 0]);
    emit_cell(&code_cell, &[0xC6, 0x41, super::inv::IS_HIDDEN as u8, 0x00]);
    emit_cell(&code_cell, &[0x48, 0x8B, 0xC8]);
    let call_cursor = emit_cell(&code_cell, &[0xE8, 0, 0, 0, 0]);
    emit_cell(&code_cell, &[0x84, 0xC0]);
    let jz_cursor = emit_cell(&code_cell, &[0x0F, 0x84, 0, 0, 0, 0]);
    let cmp_same = emit_cell(&code_cell, &[0x80, 0x3D]);
    let same_disp = emit_cell(&code_cell, &[0, 0, 0, 0]);
    fix.push((same_disp, cmp_same + 7, "same"));
    emit_cell(&code_cell, &[0x00]);
    let je_all = emit_cell(&code_cell, &[0x74, 0]);
    let lea_same = emit_cell(&code_cell, &[0x48, 0x8D, 0x05]);
    let same_tip = emit_cell(&code_cell, &[0, 0, 0, 0]);
    fix.push((same_tip, lea_same + 7, "tip_same"));
    let jmp_copy = emit_cell(&code_cell, &[0xEB, 0]);
    let lea_all_at = code_cell.borrow().len();
    let lea_all = emit_cell(&code_cell, &[0x48, 0x8D, 0x05]);
    let all_tip = emit_cell(&code_cell, &[0, 0, 0, 0]);
    fix.push((all_tip, lea_all + 7, "tip_all"));
    emit_cell(&code_cell, &[0x0F, 0x10, 0x00, 0x0F, 0x10, 0x48, 0x10, 0x0F, 0x11, 0x45, 0xE8, 0x0F, 0x11, 0x4D, 0xF8]);
    let mov_on = emit_cell(&code_cell, &[0xC6, 0x05]);
    let on_disp = emit_cell(&code_cell, &[0, 0, 0, 0]);
    fix.push((on_disp, mov_on + 7, "hover"));
    emit_cell(&code_cell, &[0x01]);
    let jmp_lock = emit_cell(&code_cell, &[0xE9, 0, 0, 0, 0]);
    let hide = emit_cell(&code_cell, &[0xC6, 0x41, super::inv::IS_HIDDEN as u8, 0x01]);
    let dep_miss = code_cell.borrow().len();
    let mov_off = emit_cell(&code_cell, &[0xC6, 0x05]);
    let off_disp = emit_cell(&code_cell, &[0, 0, 0, 0]);
    fix.push((off_disp, mov_off + 7, "hover"));
    emit_cell(&code_cell, &[0x00]);
    let lock_walk = code_cell.borrow().len();
    let lea_lock = emit_cell(&code_cell, &[0x48, 0x8D, 0x15]);
    let lock_disp = emit_cell(&code_cell, &[0, 0, 0, 0]);
    fix.push((lock_disp, lea_lock + 7, "lock_name"));
    emit_cell(&code_cell, &[0x48, 0x8D, 0x4D, 0xB0, 0x45, 0x33, 0xC0]);
    let call_hash2 = emit_cell(&code_cell, &[0xE8, 0, 0, 0, 0]);
    emit_cell(&code_cell, &[0x48, 0x8B, 0xD0, 0x48, 0x8B, 0xCF, 0x45, 0x33, 0xC0]);
    let call_child2 = emit_cell(&code_cell, &[0xE8, 0, 0, 0, 0]);
    emit_cell(&code_cell, &[0x48, 0x85, 0xC0]);
    let jz_child2 = emit_cell(&code_cell, &[0x0F, 0x84, 0, 0, 0, 0]);
    emit_cell(&code_cell, &[0x48, 0x8B, 0x48, 0x48, 0x40, 0x84, 0xF6]);
    let je_hide2 = emit_cell(&code_cell, &[0x74, 0]);
    let cmp_lock = emit_cell(&code_cell, &[0x80, 0x3D]);
    let lock_en = emit_cell(&code_cell, &[0, 0, 0, 0]);
    fix.push((lock_en, cmp_lock + 7, "lock_enabled"));
    emit_cell(&code_cell, &[0x00]);
    let je_off2 = emit_cell(&code_cell, &[0x74, 0]);
    emit_cell(&code_cell, &[0xC6, 0x41, super::inv::IS_HIDDEN as u8, 0x00]);
    emit_cell(&code_cell, &[0x48, 0x8B, 0xC8]);
    let call_cursor2 = emit_cell(&code_cell, &[0xE8, 0, 0, 0, 0]);
    emit_cell(&code_cell, &[0x84, 0xC0]);
    let jz_cursor2 = emit_cell(&code_cell, &[0x0F, 0x84, 0, 0, 0, 0]);
    let lea_ltip = emit_cell(&code_cell, &[0x48, 0x8D, 0x05]);
    let ltip_disp = emit_cell(&code_cell, &[0, 0, 0, 0]);
    fix.push((ltip_disp, lea_ltip + 7, "lock_tip"));
    emit_cell(&code_cell, &[0x0F, 0x10, 0x00, 0x0F, 0x10, 0x48, 0x10, 0x0F, 0x11, 0x45, 0xE8, 0x0F, 0x11, 0x4D, 0xF8]);
    let mov_lon = emit_cell(&code_cell, &[0xC6, 0x05]);
    let lon_disp = emit_cell(&code_cell, &[0, 0, 0, 0]);
    fix.push((lon_disp, mov_lon + 7, "lock_hover"));
    emit_cell(&code_cell, &[0x01]);
    let jmp_restore = emit_cell(&code_cell, &[0xEB, 0]);
    let hide2 = emit_cell(&code_cell, &[0xC6, 0x41, super::inv::IS_HIDDEN as u8, 0x01]);
    let lock_miss = code_cell.borrow().len();
    let mov_loff = emit_cell(&code_cell, &[0xC6, 0x05]);
    let loff_disp = emit_cell(&code_cell, &[0, 0, 0, 0]);
    fix.push((loff_disp, mov_loff + 7, "lock_hover"));
    emit_cell(&code_cell, &[0x00]);
    let restore = code_cell.borrow().len();
    emit_cell(&code_cell, &[0x66, 0x0F, 0x6F, 0x03, 0x66, 0x0F, 0x7F, 0x45, 0xB0, 0x48, 0x83, 0xC4, 0x20, 0x5B, 0x5F]);
    emit_cell(&code_cell, DEPOSIT_PATCH_BYTES);
    let jmp_back = emit_cell(&code_cell, &[0xE9, 0, 0, 0, 0]);
    let miss = code_cell.borrow().len();
    let mov_m1 = emit_cell(&code_cell, &[0xC6, 0x05]);
    let m1 = emit_cell(&code_cell, &[0, 0, 0, 0]);
    fix.push((m1, mov_m1 + 7, "hover"));
    emit_cell(&code_cell, &[0x00]);
    let mov_m2 = emit_cell(&code_cell, &[0xC6, 0x05]);
    let m2 = emit_cell(&code_cell, &[0, 0, 0, 0]);
    fix.push((m2, mov_m2 + 7, "lock_hover"));
    emit_cell(&code_cell, &[0x00]);
    let jmp_miss = emit_cell(&code_cell, &[0xE9, 0, 0, 0, 0]);
    let _ = (hide, hide2);
    let mut code = code_cell.into_inner();
    rel8(&mut code, je_all + 1, lea_all_at)?;
    rel8(&mut code, jmp_copy + 1, lea_all_at + 7)?;
    rel8(&mut code, je_hide + 1, hide)?;
    rel8(&mut code, je_off + 1, hide)?;
    rel8(&mut code, je_hide2 + 1, hide2)?;
    rel8(&mut code, je_off2 + 1, hide2)?;
    rel8(&mut code, jmp_restore + 1, restore)?;
    let jumps = [
        (jz_page + 2, cave + miss),
        (jz_pc + 2, cave + miss),
        (jz_filters + 2, cave + miss),
        (jz_child + 2, cave + dep_miss),
        (jz_cursor + 2, cave + dep_miss),
        (jmp_lock + 1, cave + lock_walk),
        (jz_child2 + 2, cave + lock_miss),
        (jz_cursor2 + 2, cave + lock_miss),
        (jmp_miss + 1, cave + restore),
        (jmp_back + 1, back),
        (call1 + 1, find),
        (call2 + 1, find),
        (call_hash + 1, name_hash),
        (call_child + 1, child),
        (call_cursor + 1, cursor),
        (call_hash2 + 1, name_hash),
        (call_child2 + 1, child),
        (call_cursor2 + 1, cursor),
    ];
    for (at, target) in jumps {
        put_rel32(&mut code, at, cave, target)?;
    }
    let body_len = code.len();
    let pad = (16 - (body_len % 16)) % 16;
    code.extend(std::iter::repeat_n(0u8, pad));
    let mut labels = HashMap::new();
    let mut add = |label: &'static str, raw: &[u8], code: &mut Vec<u8>| {
        labels.insert(label, code.len());
        code.extend_from_slice(raw);
    };
    add("scratch", &[0u8; 16], &mut code);
    add("page", &pad16(b"PAGECONTENT"), &mut code);
    add("filters", &pad16(b"FILTERS"), &mut code);
    add("name", &pad16(b"DEPOSIT"), &mut code);
    add("lock_name", &pad16(b"LOCKMODE"), &mut code);
    // Tooltip keys: the cave copies all 32 bytes into the game's key (`TIP_LEN`, see `text`).
    for (label, tip) in tip_slots(lang) {
        add(label, &tip, &mut code);
    }
    add("ticks", &[0, 0, 0, 0], &mut code);
    add("hover", &[0], &mut code);
    add("lock_hover", &[0], &mut code);
    add("enabled", &[1], &mut code);
    add("lock_enabled", &[0], &mut code);
    add("same", &[1], &mut code);
    if code.len() > 0xC00 {
        return Err("the side-bar button pass does not fit the cave".into());
    }
    for (at, end, label) in fix {
        let dest = *labels.get(label).ok_or("cave label missing")?;
        let bytes = rel32_bytes(cave.wrapping_add(end).wrapping_sub(4), cave.wrapping_add(dest))?;
        code[at..at + 4].copy_from_slice(&bytes);
    }
    let abs = labels
        .into_iter()
        .map(|(k, v)| (k, cave + v))
        .collect();
    Ok((code, abs))
}

pub fn install_deposit_hover(
    image: &ProcessImage,
    enabled: bool,
    same: bool,
    lock_enabled: bool,
    lang: &str,
) -> Result<DepositHover, String> {
    let lookup_rva = find_sig(image, sigs::DEPOSIT_LOOKUP)
        .ok_or_else(|| "the side bar's button pass was not found (game update?)".to_string())?;
    let patch_rva = find_sig(image, sigs::DEPOSIT_PATCH)
        .ok_or_else(|| "the side bar's button pass was not found (game update?)".to_string())?;
    let lookup = image.va(lookup_rva);
    let site = image.va(patch_rva);
    let mut calls = [0usize; 4];
    for (i, at) in [DEPOSIT_FIND_AT, DEPOSIT_HASH_AT, DEPOSIT_CHILD_AT, DEPOSIT_CURSOR_AT]
        .into_iter()
        .enumerate()
    {
        if read_u8(lookup.wrapping_add(at)) != Some(0xE8) {
            return Err("the side bar's call is not where it was".into());
        }
        calls[i] = rel32_target(lookup.wrapping_add(at).wrapping_add(1)).unwrap_or(0);
    }
    let head = read_bytes(site, DEPOSIT_PATCH_BYTES.len()).unwrap_or_default();
    let cave = if head == DEPOSIT_PATCH_BYTES {
        let cave = alloc_exec_near(site, 0x1000).ok_or_else(|| "no memory free near the game code for the deposit button".to_string())?;
        let (image_bytes, labels) = deposit_image(cave, site + DEPOSIT_PATCH_BYTES.len(), calls[0], calls[1], calls[2], calls[3], lang)?;
        write_exec(cave, &image_bytes);
        let rel = rel32_bytes(site.wrapping_add(1), cave)?;
        let mut jmp = vec![0xE9];
        jmp.extend_from_slice(&rel);
        jmp.extend_from_slice(&[0x90, 0x90, 0x90]);
        write_code(site, &jmp)?;
        labels
    } else if head.first() == Some(&0xE9) {
        let cave = rel32_target(site.wrapping_add(1)).ok_or_else(|| "the deposit button pass was already changed".to_string())?;
        let (image_bytes, labels) = deposit_image(cave, site + DEPOSIT_PATCH_BYTES.len(), calls[0], calls[1], calls[2], calls[3], lang)?;
        let got = read_bytes(cave, 16).unwrap_or_default();
        if got != image_bytes.get(..16).unwrap_or(&[]) {
            return Err("the deposit button pass was already changed".into());
        }
        // Our own cave from an earlier start in this process: refresh only the tooltip keys.
        for (label, tip) in tip_slots(lang) {
            if let Some(&at) = labels.get(label) {
                write_exec(at, &tip);
            }
        }
        labels
    } else {
        return Err(format!(
            "the side bar tooltip pass is not where it was (found {})",
            head.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" ")
        ));
    };
    let hover = DepositHover::new(
        *cave.get("hover").unwrap_or(&0),
        *cave.get("enabled").unwrap_or(&0),
        *cave.get("same").unwrap_or(&0),
        *cave.get("ticks").unwrap_or(&0),
        *cave.get("lock_hover").unwrap_or(&0),
        *cave.get("lock_enabled").unwrap_or(&0),
    );
    hover.set_enabled(enabled);
    hover.set_same(same);
    hover.set_lock_enabled(lock_enabled);
    Ok(hover)
}

pub fn install_lock_dim(image: &ProcessImage) -> Result<LockDim, String> {
    let rva = find_sig(image, sigs::FILTER).ok_or_else(|| "the inventory dim test was not found (game update?)".to_string())?;
    let site = image.va(rva);
    let head = read_bytes(site, FILTER_STEAL.len()).unwrap_or_default();
    if head != FILTER_STEAL {
        return Err(format!(
            "the inventory dim test is not where it was (found {})",
            head.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" ")
        ));
    }
    let cave = alloc_exec_near(site, 0x1000).ok_or_else(|| "no memory free near the game code for the deposit button".to_string())?;
    let mut code = Vec::new();
    let cmp_en = push_bytes(&mut code, &[0x80, 0x3D, 0, 0, 0, 0, 0]);
    let je_orig = push_bytes(&mut code, &[0x74, 0]);
    push_bytes(&mut code, &[0x51]);
    push_bytes(&mut code, &[0x41, 0x8B, 0x44, 0x24, 0x14]);
    push_bytes(&mut code, &[0x83, 0xF8, 0x1F]);
    let ja_dim = push_bytes(&mut code, &[0x77, 0]);
    push_bytes(&mut code, &[0xC1, 0xE0, 0x02]);
    let lea_bits = push_bytes(&mut code, &[0x48, 0x8D, 0x0D, 0, 0, 0, 0]);
    push_bytes(&mut code, &[0x8B, 0x04, 0x01, 0x90]);
    push_bytes(&mut code, &[0x41, 0x8B, 0x4C, 0x24, 0x10]);
    push_bytes(&mut code, &[0x83, 0xF9, 0x1F]);
    let ja_dim2 = push_bytes(&mut code, &[0x77, 0]);
    push_bytes(&mut code, &[0x0F, 0xA3, 0xC8]);
    let jc_lit = push_bytes(&mut code, &[0x72, 0]);
    let dim = push_bytes(&mut code, &[0xB3, 0x01]);
    let jmp_pop = push_bytes(&mut code, &[0xEB, 0]);
    let lit = push_bytes(&mut code, &[0x32, 0xDB]);
    let popjoin = push_bytes(&mut code, &[0x59]);
    let jmp_join = push_bytes(&mut code, &[0xE9, 0, 0, 0, 0]);
    let orig = push_bytes(&mut code, FILTER_STEAL);
    let jmp_back = push_bytes(&mut code, &[0xE9, 0, 0, 0, 0]);
    rel8(&mut code, je_orig + 1, orig)?;
    rel8(&mut code, ja_dim + 1, dim)?;
    rel8(&mut code, ja_dim2 + 1, dim)?;
    rel8(&mut code, jc_lit + 1, lit)?;
    rel8(&mut code, jmp_pop + 1, popjoin)?;
    put_rel32(&mut code, jmp_join + 1, cave, site + FILTER_JOIN_AT)?;
    put_rel32(&mut code, jmp_back + 1, cave, site + FILTER_STEAL.len())?;
    let pad = (16 - (code.len() % 16)) % 16;
    code.extend(std::iter::repeat_n(0u8, pad));
    let enabled_at = code.len();
    code.push(0);
    let bits_at = code.len();
    code.extend(std::iter::repeat_n(0u8, DIM_BITS));
    let en_rel = (cave.wrapping_add(enabled_at) as i64 - (cave.wrapping_add(cmp_en).wrapping_add(7) as i64)) as i32;
    code[cmp_en + 2..cmp_en + 6].copy_from_slice(&en_rel.to_le_bytes());
    let bits_rel = (cave.wrapping_add(bits_at) as i64 - (cave.wrapping_add(lea_bits).wrapping_add(7) as i64)) as i32;
    code[lea_bits + 3..lea_bits + 7].copy_from_slice(&bits_rel.to_le_bytes());
    write_exec(cave, &code);
    Ok(LockDim {
        site,
        cave,
        enabled: cave + enabled_at,
        bits: cave + bits_at,
        applied: false,
        dead: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Targets of every `lea rax, [rip+disp32]` (`48 8D 05`) in `code`, which starts at `cave`.
    fn lea_rax_targets(code: &[u8], cave: usize) -> Vec<usize> {
        code.windows(7)
            .enumerate()
            .filter(|(_, w)| w.starts_with(&[0x48, 0x8D, 0x05]))
            .map(|(at, w)| {
                let disp = i32::from_le_bytes([w[3], w[4], w[5], w[6]]);
                cave.wrapping_add(at + 7).wrapping_add(disp as isize as usize)
            })
            .collect()
    }

    #[test]
    fn tooltip_keys_hold_the_language_and_keep_the_layout() {
        let cave = 0x1_5000_0000usize;
        for lang in ["en", "ko", "ja", "ru"] {
            let (code, labels) =
                deposit_image(cave, 0x1_4000_1000, 0x1_4000_2000, 0x1_4000_3000, 0x1_4000_4000, 0x1_4000_5000, lang)
                    .expect("cave builds");
            assert!(code.len() <= 0xC00, "{lang}");
            // Each key is a 32-byte slot right after the other, then the tick counter.
            assert_eq!(labels["tip_all"] - labels["tip_same"], TIP_LEN);
            assert_eq!(labels["lock_tip"] - labels["tip_all"], TIP_LEN);
            assert_eq!(labels["ticks"] - labels["lock_tip"], TIP_LEN);
            for id in text::TIP_IDS {
                let at = labels[id] - cave;
                let want = text::get(lang, id).as_bytes();
                assert!(want.len() < TIP_LEN, "{lang} {id}");
                assert_eq!(&code[at..at + want.len()], want, "{lang} {id}");
                assert!(code[at + want.len()..at + TIP_LEN].iter().all(|&b| b == 0), "{lang} {id}");
            }
            // The two 16-byte copies read from these three keys, in this order.
            assert_eq!(lea_rax_targets(&code, cave), vec![labels["tip_same"], labels["tip_all"], labels["lock_tip"]], "{lang}");
        }
    }
}
