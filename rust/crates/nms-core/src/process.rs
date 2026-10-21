//! The running game's own image (NMS.exe in this process).

use std::ops::Range;
use std::sync::Arc;

use windows_sys::Win32::System::Diagnostics::Debug::FlushInstructionCache;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Memory::{VirtualProtect, PAGE_EXECUTE_READWRITE};
use windows_sys::Win32::System::Threading::GetCurrentProcess;

use crate::image::{parse_headers, Code};

/// Pattern searches run on a copy of the executable sections taken at load, before any mod hooks
/// or patches code, so one mod's detour never hides a function from another mod's pattern (pyMHF
/// resolved patterns once and cached them). `read` and `write` are always live.
#[derive(Debug, Clone)]
pub struct ProcessImage {
    base: usize,
    size: usize,
    exec: Vec<Range<usize>>,
    snapshot: Arc<Vec<(Range<usize>, Vec<u8>)>>,
}

impl ProcessImage {
    /// The exe this process was started from.
    pub fn current() -> Result<Self, String> {
        // SAFETY: a null name returns the exe's module handle, which stays loaded for the process.
        let base = unsafe { GetModuleHandleW(std::ptr::null()) } as usize;
        if base == 0 {
            return Err("no module handle for the exe".into());
        }
        // SAFETY: the first page of a loaded image holds its headers and is readable.
        let head = unsafe { std::slice::from_raw_parts(base as *const u8, 0x1000) };
        let headers = parse_headers(head)?;
        let size = headers.size_of_image;
        let exec: Vec<Range<usize>> = headers
            .sections
            .iter()
            .filter(|s| s.executable)
            .map(|s| s.rva..s.rva + s.virtual_size)
            .filter(|r| r.end <= size)
            .collect();
        let snapshot = exec
            .iter()
            // SAFETY: executable sections of the loaded image are readable for the life of the process.
            .map(|r| (r.clone(), unsafe { std::slice::from_raw_parts((base + r.start) as *const u8, r.len()) }.to_vec()))
            .collect();
        Ok(Self { base, size, exec, snapshot: Arc::new(snapshot) })
    }

    /// A view with base 0 over the whole user address space (RVA = address), for tests that patch
    /// code they allocated themselves.
    #[cfg(test)]
    pub(crate) fn for_test_absolute() -> Self {
        Self { base: 0, size: 0x7FFF_FFFF_FFFF, exec: Vec::new(), snapshot: Arc::new(Vec::new()) }
    }

    pub fn base(&self) -> usize {
        self.base
    }

    /// Absolute address of an RVA.
    pub fn va(&self, rva: usize) -> usize {
        self.base + rva
    }

    /// Write at most 8 bytes that sit inside one aligned 8-byte word with a single 8-byte store,
    /// so a thread running that code sees all old or all new bytes, never a mix (an aligned
    /// 8-byte store is atomic on x86-64). `Err` when the bytes cross an 8-byte boundary; the
    /// caller then decides whether a plain `write` is acceptable.
    ///
    /// Precondition: no other thread writes the other bytes of that 8-byte word at the same time
    /// (the word is read, spliced and stored back). Mods call this from one place per site, under
    /// their own lock, as with `write`. Errors are static strings: this runs while other threads are
    /// paused (`jump`), where nothing may allocate. A failed protection restore is reported as an error after
    /// the store (the bytes are written; the page stays writable).
    pub fn write_atomic(&mut self, rva: usize, bytes: &[u8]) -> Result<(), &'static str> {
        let at = self.base + rva;
        let word = at & !7;
        let off = at - word;
        if bytes.is_empty() || off + bytes.len() > 8 {
            return Err("not inside one aligned 8-byte word");
        }
        if word < self.base || word + 8 > self.base + self.size {
            return Err("write outside the image");
        }
        let mut old = 0u32;
        // SAFETY: the 8-byte word is inside the image and 8-aligned; protection is restored right
        // after the store. The other bytes of the word are re-stored with the value just read.
        unsafe {
            if VirtualProtect(word as _, 8, PAGE_EXECUTE_READWRITE, &mut old) == 0 {
                return Err("could not unprotect the code");
            }
            let cell = &*(word as *const std::sync::atomic::AtomicU64);
            let mut value = cell.load(std::sync::atomic::Ordering::Acquire).to_le_bytes();
            value[off..off + bytes.len()].copy_from_slice(bytes);
            cell.store(u64::from_le_bytes(value), std::sync::atomic::Ordering::Release);
            let mut was = 0u32;
            let restored = VirtualProtect(word as _, 8, old, &mut was) != 0;
            FlushInstructionCache(GetCurrentProcess(), word as _, 8);
            if !restored {
                return Err("written, but the code protection could not be restored");
            }
        }
        Ok(())
    }
}

impl Code for ProcessImage {
    fn exec_ranges(&self) -> Vec<Range<usize>> {
        self.exec.clone()
    }

    fn read(&self, rva: usize, len: usize) -> Option<Vec<u8>> {
        if rva + len > self.size || !crate::mem::readable(self.base + rva, len) {
            return None;
        }
        // SAFETY: inside the mapped image and checked readable (gaps between sections are not mapped).
        Some(unsafe { std::slice::from_raw_parts((self.base + rva) as *const u8, len) }.to_vec())
    }

    fn write(&mut self, rva: usize, bytes: &[u8]) -> Result<(), String> {
        if rva + bytes.len() > self.size {
            return Err("write outside the image".into());
        }
        let at = (self.base + rva) as *mut u8;
        let mut old = 0u32;
        // SAFETY: the range is inside the image; protection is restored right after the copy.
        unsafe {
            if VirtualProtect(at as _, bytes.len(), PAGE_EXECUTE_READWRITE, &mut old) == 0 {
                return Err("could not unprotect the code".into());
            }
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), at, bytes.len());
            VirtualProtect(at as _, bytes.len(), old, &mut old);
            FlushInstructionCache(GetCurrentProcess(), at as _, bytes.len());
        }
        Ok(())
    }

    fn with_range(&self, range: Range<usize>, f: &mut dyn FnMut(&[u8])) {
        let held = self
            .snapshot
            .iter()
            .find(|(r, _)| r.start <= range.start && range.end <= r.end)
            .and_then(|(r, bytes)| bytes.get(range.start - r.start..range.end - r.start));
        if let Some(bytes) = held {
            f(bytes);
        }
    }
}
