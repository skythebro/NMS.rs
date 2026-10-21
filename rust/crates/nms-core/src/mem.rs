//! Reading and writing game memory from mods, without crashing on a bad pointer.
//!
//! Game objects are reached through pointers read out of other objects. A stale or garbage
//! pointer must turn into `None`, never into an access violation, so every read checks the
//! range first. Addresses below `MIN_ADDR` are treated as null.

use windows_sys::Win32::System::Memory::{
    IsBadReadPtr, VirtualAlloc, VirtualFree, VirtualQuery, MEMORY_BASIC_INFORMATION, MEM_COMMIT, MEM_FREE,
    MEM_RELEASE, MEM_RESERVE, PAGE_EXECUTE_READWRITE, PAGE_NOACCESS,
};

pub const MIN_ADDR: usize = 0x10000;

/// True when the whole range can be read right now.
pub fn readable(addr: usize, len: usize) -> bool {
    // SAFETY: IsBadReadPtr only probes the range; it does not dereference for us.
    addr >= MIN_ADDR && unsafe { IsBadReadPtr(addr as *const _, len) } == 0
}

fn read<T: Copy>(addr: usize) -> Option<T> {
    if !readable(addr, std::mem::size_of::<T>()) {
        return None;
    }
    // SAFETY: the range was just checked readable; unaligned reads are allowed.
    Some(unsafe { std::ptr::read_unaligned(addr as *const T) })
}

pub fn read_u8(addr: usize) -> Option<u8> {
    read(addr)
}
pub fn read_u32(addr: usize) -> Option<u32> {
    read(addr)
}
pub fn read_i32(addr: usize) -> Option<i32> {
    read(addr)
}
pub fn read_u64(addr: usize) -> Option<u64> {
    read(addr)
}
pub fn read_f32(addr: usize) -> Option<f32> {
    read(addr)
}

/// A pointer stored at `addr`, or `None` when it is null-ish or the read fails.
pub fn read_ptr(addr: usize) -> Option<usize> {
    read_u64(addr).map(|v| v as usize).filter(|&p| p >= MIN_ADDR)
}

pub fn read_bytes(addr: usize, len: usize) -> Option<Vec<u8>> {
    if !readable(addr, len) {
        return None;
    }
    // SAFETY: the range was just checked readable.
    Some(unsafe { std::slice::from_raw_parts(addr as *const u8, len) }.to_vec())
}

/// Write game data (not code: use `ProcessImage::write` for code). Returns false if the range is not readable.
///
/// # Safety
/// `addr` must be a field the game expects to change, of the size of `T`.
pub unsafe fn write<T: Copy>(addr: usize, value: T) -> bool {
    if !readable(addr, std::mem::size_of::<T>()) {
        return false;
    }
    std::ptr::write_unaligned(addr as *mut T, value);
    true
}

/// Executable memory within rel32 reach (+-0x70000000) of `near`, for stubs that the game jumps to
/// with an `E8`/`E9`. Never freed: game code may still point at it.
pub fn alloc_exec_near(near: usize, size: usize) -> Option<usize> {
    let size = size.max(1);
    // Under Wine the address space next to the game image can be fully reserved by the time the
    // mods build their caves; the blocks held since boot are then given back one at a time.
    loop {
        if let Some(got) = alloc_exec_near_once(near, size) {
            return Some(got);
        }
        if !release_held_one() {
            return None;
        }
    }
}

const REACH: usize = 0x7000_0000;
const GRANULARITY: usize = 0x10000;

/// Free, 64 KB aligned spots of `size` bytes within rel32 reach of `near`, the closest per free
/// region, nearest first. Walks both sides of `near`, not only a window below it.
fn free_spots(near: usize, size: usize) -> Vec<usize> {
    let mut spots: Vec<usize> = Vec::new();
    let mut at = near.saturating_sub(REACH).max(GRANULARITY);
    let end = near.saturating_add(REACH);
    while at < end {
        // SAFETY: VirtualQuery only reads the address-space map; `info` is a plain out struct.
        let mut info: MEMORY_BASIC_INFORMATION = unsafe { std::mem::zeroed() };
        let got = unsafe { VirtualQuery(at as _, &mut info, std::mem::size_of::<MEMORY_BASIC_INFORMATION>()) };
        if got == 0 || info.RegionSize == 0 {
            break;
        }
        let base = info.BaseAddress as usize;
        let region_end = base.saturating_add(info.RegionSize);
        if info.State == MEM_FREE {
            let first = base.saturating_add(GRANULARITY - 1) & !(GRANULARITY - 1);
            if first.saturating_add(size) <= region_end {
                let last = (region_end - size) & !(GRANULARITY - 1);
                // Closest block to `near` inside this region.
                let spot = if last < near { last } else { first };
                if spot >= first && spot.abs_diff(near) < REACH && (spot + size).abs_diff(near) < REACH {
                    spots.push(spot);
                }
            }
        }
        if region_end <= at {
            break;
        }
        at = region_end;
    }
    spots.sort_by_key(|s| s.abs_diff(near));
    spots
}

fn alloc_exec_near_once(near: usize, size: usize) -> Option<usize> {
    const MAX_TRIES: usize = 32;
    for wanted in free_spots(near, size).into_iter().take(MAX_TRIES) {
        // SAFETY: asking for fresh pages at a free address; failure returns null.
        let got = unsafe { VirtualAlloc(wanted as _, size, MEM_COMMIT | MEM_RESERVE, PAGE_EXECUTE_READWRITE) } as usize;
        if got == 0 {
            continue;
        }
        if got.abs_diff(near) < REACH {
            return Some(got);
        }
        // SAFETY: releasing the block we just got.
        unsafe { VirtualFree(got as _, 0, MEM_RELEASE) };
    }
    None
}

/// Address space reserved near the game image at boot, before Wine takes the window around it.
static HELD: std::sync::Mutex<Vec<usize>> = std::sync::Mutex::new(Vec::new());

/// Reserve up to `count` blocks of `block` bytes next to `near` (address space only, nothing
/// committed) and keep them until a hook or cave allocation needs the room. Returns how many were
/// reserved. Cheap and harmless on Windows, where the space next to the image is normally free.
pub fn hold_near(near: usize, block: usize, count: usize) -> usize {
    let mut held = 0;
    for _ in 0..count {
        let Some(&spot) = free_spots(near, block).first() else {
            break;
        };
        // SAFETY: reserving fresh address space at a free spot; failure returns null.
        let got = unsafe { VirtualAlloc(spot as _, block, MEM_RESERVE, PAGE_NOACCESS) } as usize;
        if got == 0 {
            break;
        }
        match HELD.lock() {
            Ok(mut list) => list.push(got),
            Err(_) => {
                // SAFETY: releasing the block we just reserved.
                unsafe { VirtualFree(got as _, 0, MEM_RELEASE) };
                break;
            }
        }
        held += 1;
    }
    held
}

/// Give one held block back to the system so an allocation that just failed can use the room.
/// Returns false when nothing is held. Never blocks: a busy lock counts as nothing held.
pub fn release_held_one() -> bool {
    let block = match HELD.try_lock() {
        Ok(mut list) if !list.is_empty() => list.remove(0),
        _ => return false,
    };
    // SAFETY: the block was reserved by `hold_near` and is released once.
    unsafe { VirtualFree(block as _, 0, MEM_RELEASE) };
    true
}

pub fn held_count() -> usize {
    HELD.try_lock().map(|l| l.len()).unwrap_or(0)
}

/// The rel32 operand that makes an instruction ending at `end` reach `target`, or `None` if out of range.
pub fn rel32(end: usize, target: usize) -> Option<i32> {
    i32::try_from(target as i64 - end as i64).ok()
}

#[cfg(test)]
mod tests {
    #[test]
    fn allocates_within_reach() {
        let near = reads_live_memory_and_refuses_null as *const () as usize;
        let cave = alloc_exec_near(near, 0x100).expect("no memory near the test binary");
        assert!(rel32(near, cave).is_some());
        assert!(unsafe { write(cave, 0xC3u8) });
    }

    use super::*;

    #[test]
    fn held_blocks_are_released_one_at_a_time() {
        let near = reads_live_memory_and_refuses_null as *const () as usize;
        let before = held_count();
        let held = hold_near(near, 0x10000, 3);
        assert_eq!(held_count(), before + held);
        assert!(held > 0, "no address space free near the test binary");
        assert!(release_held_one());
        assert_eq!(held_count(), before + held - 1);
        while held_count() > before {
            assert!(release_held_one());
        }
    }

    #[test]
    fn reads_live_memory_and_refuses_null() {
        let v: u64 = 0x1122_3344_5566_7788;
        let a = &v as *const u64 as usize;
        assert_eq!(read_u64(a), Some(v));
        assert_eq!(read_u8(a), Some(0x88));
        assert_eq!(read_u64(0), None);
        assert_eq!(read_ptr(0x100), None);
    }

    #[test]
    fn writes_only_readable_memory() {
        let mut v: i32 = 1;
        let a = &mut v as *mut i32 as usize;
        assert!(unsafe { write(a, 7i32) });
        assert_eq!(v, 7);
        assert!(!unsafe { write(8usize, 7i32) });
    }
}
