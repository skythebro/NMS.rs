//! Reading and writing game memory from mods, without crashing on a bad pointer.
//!
//! Game objects are reached through pointers read out of other objects. A stale or garbage
//! pointer must turn into `None`, never into an access violation, so every read checks the
//! range first. Addresses below `MIN_ADDR` are treated as null.

use windows_sys::Win32::System::Memory::{
    IsBadReadPtr, VirtualAlloc, VirtualFree, MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_EXECUTE_READWRITE,
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
    for step in 1..4000usize {
        let wanted = near.wrapping_sub(0x1000_0000).wrapping_add(step * 0x10000);
        // SAFETY: asking for fresh pages at a hint address; failure returns null.
        let got = unsafe { VirtualAlloc(wanted as _, size, MEM_COMMIT | MEM_RESERVE, PAGE_EXECUTE_READWRITE) } as usize;
        if got == 0 {
            continue;
        }
        if got.abs_diff(near) < 0x7000_0000 {
            return Some(got);
        }
        // SAFETY: releasing the block we just got.
        unsafe { VirtualFree(got as _, 0, MEM_RELEASE) };
    }
    None
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
