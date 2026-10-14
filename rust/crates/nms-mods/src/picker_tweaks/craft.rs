//! Search in the inventory craft list (empty slot, Craft: technology and product build list).
//!
//! The craft list (`0x1406CC3B0`, popup
//! kind 2/3) rebuilds its temporary inventories every frame, one `add(inventory, out, element)` call
//! (`0x1404D0780`) per recipe, at three call sites. Hiding cards through the slot-filler predicate
//! did not change what the list shows (in-game test 2026-10-13), so the recipes are filtered where
//! they are added: the three `call add` instructions are sent to a stub that jumps to
//! [`add_detour`]. Without a craft-list search the detour calls the original at once. A dropped
//! recipe gets exactly what `add` itself does when it cannot place an element (the 8-byte store
//! `mov qword [r14], -1` at `0x1404D0B7C`, then return `out`); the callers ignore the result
//! (`0x1406CC649`, `0x1406CC872`, `0x1406CCB9D`).
//!
//! The sites are located and the stub is built once at install (pattern scans and the near
//! allocation never run inside a detour). The patch follows the "Search item pickers" switch: applied
//! when it is on (at start or when it is switched on), the original bytes written back when it is
//! switched off, so off leaves the game code vanilla. The per-frame `sync` only takes the patch lock
//! with `try_lock` and retries next frame when it is busy. Every write checks the bytes first (original or ours, nothing else). If a site does not
//! hold `mov rcx,[rcx+rax*8]; call add` exactly, nothing is written and the hotkey does nothing over
//! the craft list (no box that would do nothing).

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;

use nms_core::mem::{alloc_exec_near, rel32};
use nms_core::process::ProcessImage;
use nms_core::{Code, Pattern};
use nms_sdk::sigs::picker_tweaks as sigs;

static ORIG_ADD: AtomicUsize = AtomicUsize::new(0);
/// The three calls go to our filter now: the hotkey may open the box over the craft list.
pub static READY: AtomicBool = AtomicBool::new(false);

type AddFn = unsafe extern "system" fn(usize, usize, usize) -> usize;

/// The located sites: `(rva, original 5 bytes, our 5 bytes)`, and the image to write through.
struct Patch {
    image: ProcessImage,
    sites: Vec<(usize, [u8; 5], [u8; 5])>,
}

static PATCH: Mutex<Option<Patch>> = Mutex::new(None);
/// Wanted state (the switch), applied by [`sync`] on the game thread.
pub static WANT: AtomicBool = AtomicBool::new(false);
/// Locating or applying failed: stop trying to apply (the log says why). Set only by `init` and
/// `enable`, never by a failed `disable`.
static FAILED: AtomicBool = AtomicBool::new(false);
/// Putting the bytes back failed: the patch stays applied (and `READY` stays true, its filter
/// passes everything through without a search); stop retrying every frame.
static OFF_FAILED: AtomicBool = AtomicBool::new(false);
/// `add_detour` calls in progress (the craft list runs on the game thread only; checked in debug).
static IN_ADD: AtomicUsize = AtomicUsize::new(0);

fn find<C: Code>(image: &C, name: &str, pattern: &str) -> Result<usize, String> {
    let pat = Pattern::parse(pattern).map_err(|e| format!("{name}: {e}"))?;
    image.find_unique(&pat).unique().ok_or_else(|| format!("{name} not found (game update?)"))
}

/// `jmp [rip+0]; dq target`.
fn abs_jump(target: usize) -> [u8; 14] {
    let mut s = [0u8; 14];
    s[..6].copy_from_slice(&[0xFF, 0x25, 0, 0, 0, 0]);
    s[6..].copy_from_slice(&(target as u64).to_le_bytes());
    s
}

/// The recipe add, filtered. Never panics; pass-through unless a craft-list search drops this one.
unsafe extern "system" fn add_detour(inventory: usize, out: usize, element: usize) -> usize {
    IN_ADD.fetch_add(1, Ordering::AcqRel);
    let ret = if !super::search::craft_keeps(element) && out >= nms_core::mem::MIN_ADDR
        // SAFETY: `out` is the caller's 8-byte result slot; `add` itself stores -1 there with an
        // 8-byte `mov qword [r14], -1` when it does not place the element (`CRAFT_ADD_FAIL`).
        && nms_core::mem::write(out, -1i64)
    {
        out
    } else {
        let add: AddFn = std::mem::transmute(ORIG_ADD.load(Ordering::Acquire));
        add(inventory, out, element)
    };
    IN_ADD.fetch_sub(1, Ordering::AcqRel);
    ret
}

/// RVAs of the craft list, of `add`, and the three call sites with their current bytes, all checked
/// (including `add`'s own 8-byte "not placed" store, which the detour copies).
#[allow(clippy::type_complexity)]
fn sites<C: Code>(image: &C) -> Result<(usize, usize, Vec<(usize, [u8; 5])>), String> {
    let list = find(image, "craft list", sigs::CRAFT_LIST)?;
    let add = find(image, "craft add", sigs::CRAFT_ADD)?;
    let fail = image.read(add + sigs::CRAFT_ADD_FAIL_AT, sigs::CRAFT_ADD_FAIL.len());
    let ok = image.read(add + sigs::CRAFT_ADD_OK_AT, sigs::CRAFT_ADD_OK.len());
    if fail.as_deref() != Some(&sigs::CRAFT_ADD_FAIL[..]) || ok.as_deref() != Some(&sigs::CRAFT_ADD_OK[..]) {
        return Err("craft add result store changed (game update?)".into());
    }
    let mut old = Vec::new();
    for at in sigs::CRAFT_ADD_SITES.map(|o| list + o) {
        let prefix = image.read(at - sigs::CRAFT_SITE_PREFIX.len(), sigs::CRAFT_SITE_PREFIX.len());
        let call = image.read(at, 5).and_then(|b| <[u8; 5]>::try_from(b).ok()).ok_or("craft list unreadable")?;
        if prefix.as_deref() != Some(&sigs::CRAFT_SITE_PREFIX[..]) || call[0] != 0xE8 || image.rel32_target(at + 1) != Some(add) {
            return Err(format!("craft list call at +{:#x} changed (game update?)", at - list));
        }
        old.push((at, call));
    }
    Ok((list, add, old))
}

/// Locate and build the stub once. Writes nothing into the game code.
fn prepare(image: &ProcessImage) -> Result<Patch, String> {
    let (list, add, old) = sites(image)?;
    let stub = alloc_exec_near(image.va(list), 64).ok_or("no memory free near the craft list for the stub")?;
    let mut out = Vec::new();
    for &(at, orig) in &old {
        let Some(disp) = rel32(image.va(at) + 5, stub) else {
            // SAFETY: our own block, not yet referenced by anything.
            unsafe { windows_sys::Win32::System::Memory::VirtualFree(stub as _, 0, windows_sys::Win32::System::Memory::MEM_RELEASE) };
            return Err("the stub is out of reach of the craft list".into());
        };
        let mut bytes = [0xE8u8; 5];
        bytes[1..].copy_from_slice(&disp.to_le_bytes());
        out.push((at, orig, bytes));
    }
    let jump = abs_jump(add_detour as *const () as usize);
    // SAFETY: a fresh RWX block owned by this mod; no game code points at it yet.
    unsafe { std::ptr::copy_nonoverlapping(jump.as_ptr(), stub as *mut u8, jump.len()) };
    ORIG_ADD.store(image.va(add), Ordering::Release);
    Ok(Patch { image: image.clone(), sites: out })
}

/// Write `to` at each site whose bytes are `from` (or already `to`). Fail-closed: on any problem
/// the sites already written are put back, and each put-back is checked and reported.
fn swap_all(p: &mut Patch, to_ours: bool) -> Result<(), String> {
    let mut done: Vec<usize> = Vec::new();
    let mut fail = None;
    for (i, &(at, orig, ours)) in p.sites.iter().enumerate() {
        let (from, to) = if to_ours { (orig, ours) } else { (ours, orig) };
        let now = p.image.read(at, 5).unwrap_or_default();
        if now == to {
            continue;
        }
        if now != from {
            fail = Some(format!("craft list call {i} was changed by something else"));
            break;
        }
        // Precondition: the 5-byte write is not atomic, so no thread may be executing these
        // instructions. `sync` runs on the game thread from the text `Update` after-hook, the only
        // thread that runs the craft list, and never inside it.
        debug_assert_eq!(IN_ADD.load(Ordering::Acquire), 0, "craft patch written while the craft list runs");
        if let Err(e) = p.image.write(at, &to) {
            fail = Some(format!("craft list call {i} could not be written ({e})"));
            break;
        }
        done.push(i);
    }
    let Some(e) = fail else { return Ok(()) };
    let mut msg = e;
    for i in done {
        let (at, orig, ours) = p.sites[i];
        let back = if to_ours { orig } else { ours };
        if let Err(e2) = p.image.write(at, &back) {
            msg.push_str(&format!("; putting back call {i} failed too ({e2})"));
        }
    }
    Err(msg)
}

/// At install (boot thread, not a detour): locate the three sites and build the stub. Writes
/// nothing into the game code. On failure the craft list stays vanilla and the hotkey does nothing
/// there.
pub fn init(image: &ProcessImage) -> Result<(), String> {
    let prepared = prepare(image);
    let mut slot = PATCH.lock().map_err(|_| "craft patch lock poisoned".to_string())?;
    match prepared {
        Ok(p) => {
            *slot = Some(p);
            Ok(())
        }
        Err(e) => {
            FAILED.store(true, Ordering::Release);
            Err(e)
        }
    }
}

/// Once per frame on the game thread (text `Update` after-hook): apply or remove the patch when the
/// switch changed. Cheap when nothing changed.
pub fn sync() {
    let want = WANT.load(Ordering::Acquire);
    if want == READY.load(Ordering::Acquire)
        || (want && FAILED.load(Ordering::Acquire))
        || (!want && OFF_FAILED.load(Ordering::Acquire))
    {
        return;
    }
    let result = if want { enable() } else { disable() };
    match result {
        // The patch lock is busy (never for long): try again next frame.
        Ok(false) => {}
        Ok(true) if want => super::log_line("craft list search on (recipe adds filtered)"),
        Ok(true) => super::log_line("craft list search off (game code put back)"),
        Err(e) if want => {
            FAILED.store(true, Ordering::Release);
            super::log_line(&format!("craft list search not available, the hotkey does nothing there: {e}"));
        }
        Err(e) => {
            OFF_FAILED.store(true, Ordering::Release);
            super::log_line(&format!("craft list search: game code not put back ({e}); it passes every recipe through until restart"));
        }
    }
}

/// The patch, without blocking: `Ok(None)` when another thread holds it right now.
fn try_patch() -> Result<Option<std::sync::MutexGuard<'static, Option<Patch>>>, String> {
    match PATCH.try_lock() {
        Ok(g) => Ok(Some(g)),
        Err(std::sync::TryLockError::WouldBlock) => Ok(None),
        Err(std::sync::TryLockError::Poisoned(_)) => Err("craft patch lock poisoned".into()),
    }
}

/// Apply the patch prepared at install. `Ok(false)`: lock busy, nothing done.
fn enable() -> Result<bool, String> {
    let Some(mut slot) = try_patch()? else { return Ok(false) };
    let p = slot.as_mut().ok_or("craft list sites were not located at start")?;
    swap_all(p, true)?;
    READY.store(true, Ordering::Release);
    Ok(true)
}

/// Write the original bytes back. Nothing to do when it was never applied. `Ok(false)`: busy.
fn disable() -> Result<bool, String> {
    let Some(mut slot) = try_patch()? else { return Ok(false) };
    let Some(p) = slot.as_mut() else {
        READY.store(false, Ordering::Release);
        return Ok(true);
    };
    // READY drops only once the original bytes are back; on failure `swap_all` has put our bytes
    // back, so the patch is still applied and READY stays true.
    swap_all(p, false)?;
    READY.store(false, Ordering::Release);
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stub_is_an_absolute_jump() {
        let j = abs_jump(0x1122_3344_5566_7788);
        assert_eq!(&j[..6], &[0xFF, 0x25, 0, 0, 0, 0]);
        assert_eq!(u64::from_le_bytes(j[6..].try_into().unwrap()), 0x1122_3344_5566_7788);
    }

    /// Runs whenever the game's exe is next to the repo (skipped otherwise).
    #[test]
    fn locates_the_three_recipe_adds_in_the_exe() {
        let exe = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().map(|d| d.join("Binaries").join("NMS.exe")).find(|p| p.is_file());
        let Some(exe) = exe else { return };
        let image = nms_core::FileImage::load(&exe).unwrap();
        let (list, add, old) = sites(&image).unwrap();
        // The three sites sit where the signature file says, and all call `add`.
        assert_eq!(old.iter().map(|o| o.0 - list).collect::<Vec<_>>(), sigs::CRAFT_ADD_SITES.to_vec());
        if image.read(0x6CC3B0, 4).is_some() && list == 0x6CC3B0 {
            // Build 88,560,712.
            assert_eq!(add, 0x4D0780);
        }
    }
}
