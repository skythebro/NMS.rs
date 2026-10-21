//! A detour written without a race: for a function whose first instructions are known and have no
//! RIP-relative operand, build the trampoline ourselves, pause every other thread of the process,
//! make sure none of them is in the middle of those instructions, and write the 5-byte jump with
//! one aligned 8-byte store.
//!
//! retour (used by [`crate::hook::Hook`]) rewrites the prologue with a plain copy while other threads
//! run, and its `enable` allocates and takes a lock, so it cannot run while threads are paused (a
//! paused thread may hold the heap lock). Here everything that allocates happens before the pause;
//! while paused only system calls run (`GetThreadContext`, `VirtualProtect`, the store,
//! `FlushInstructionCache`, `ResumeThread`).

use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, ERROR_INVALID_PARAMETER, HANDLE, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::System::Diagnostics::Debug::{GetThreadContext, CONTEXT, CONTEXT_CONTROL_AMD64};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32,
};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcessId, GetCurrentThreadId, OpenThread, ResumeThread, SuspendThread, THREAD_GET_CONTEXT,
    THREAD_SUSPEND_RESUME,
};

use crate::image::Code;
use crate::mem;
use crate::process::ProcessImage;

const JMP_LEN: usize = 5;
/// `jmp qword ptr [rip+0]` followed by the 8-byte target.
const ABS_JMP: [u8; 6] = [0xFF, 0x25, 0, 0, 0, 0];
const ABS_JMP_LEN: usize = 14;
const STUB_SIZE: usize = 64;
const TRAMPOLINE_AT: usize = 16;
const TRIES: usize = 200;

/// Every thread of this process except the calling one.
fn other_threads() -> Result<Vec<u32>, String> {
    // SAFETY: plain Win32 calls; the snapshot handle is closed before returning.
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
        if snap == INVALID_HANDLE_VALUE {
            return Err("no thread snapshot".into());
        }
        let pid = GetCurrentProcessId();
        let me = GetCurrentThreadId();
        let mut ids = Vec::new();
        let mut e: THREADENTRY32 = std::mem::zeroed();
        e.dwSize = std::mem::size_of::<THREADENTRY32>() as u32;
        let mut more = Thread32First(snap, &mut e) != 0;
        while more {
            if e.th32OwnerProcessID == pid && e.th32ThreadID != me {
                ids.push(e.th32ThreadID);
            }
            more = Thread32Next(snap, &mut e) != 0;
        }
        CloseHandle(snap);
        Ok(ids)
    }
}

/// `GetThreadContext` needs a 16-byte aligned CONTEXT (error 998 otherwise); the windows-sys type
/// does not force that alignment.
#[repr(C, align(16))]
struct AlignedContext(CONTEXT);

/// Instruction pointer of a suspended thread, or `None` if it cannot be read.
fn rip(thread: HANDLE) -> Option<usize> {
    // SAFETY: CONTEXT is plain data, aligned by the wrapper; the thread is suspended.
    unsafe {
        let mut ctx: AlignedContext = std::mem::zeroed();
        ctx.0.ContextFlags = CONTEXT_CONTROL_AMD64;
        (GetThreadContext(thread, &mut ctx.0) != 0).then_some(ctx.0.Rip as usize)
    }
}

/// Threads paused by us. Dropping it resumes and closes every one, so no path (early return,
/// panic) can leave the game frozen. Never allocates.
struct Paused<'a> {
    handles: &'a mut Vec<HANDLE>,
}

impl Drop for Paused<'_> {
    fn drop(&mut self) {
        for &h in self.handles.iter() {
            // SAFETY: each handle was opened and suspended by us; a failed resume is retried a few
            // times (it only fails on a dead handle).
            unsafe {
                for _ in 0..3 {
                    if ResumeThread(h) != u32::MAX {
                        break;
                    }
                }
                CloseHandle(h);
            }
        }
        self.handles.clear();
    }
}

/// Pause every thread in `ids` that still exists. Returns false if one exists but could not be
/// paused (the write must then wait). `handles` must have room for `ids.len()` entries.
///
/// # Safety
/// Plain Win32 calls; the caller drops the guard to resume.
unsafe fn pause_all(ids: &[u32], paused: &mut Paused<'_>) -> bool {
    let mut all = true;
    for &id in ids {
        let h = OpenThread(THREAD_SUSPEND_RESUME | THREAD_GET_CONTEXT, 0, id);
        if h.is_null() {
            // Gone since the snapshot is fine; anything else is a thread we cannot see.
            all &= GetLastError() == ERROR_INVALID_PARAMETER;
            continue;
        }
        if SuspendThread(h) == u32::MAX {
            CloseHandle(h);
            all = false;
            continue;
        }
        if paused.handles.len() == paused.handles.capacity() {
            // Never happens (room was reserved); refuse rather than allocate while paused.
            ResumeThread(h);
            CloseHandle(h);
            all = false;
            continue;
        }
        paused.handles.push(h);
    }
    all
}

/// Hook the function at `rva` whose first bytes are exactly `prologue` (whole instructions, at least
/// 5 bytes, none RIP-relative). Publishes the trampoline to `original` before the jump goes in.
///
/// # Safety
/// `prologue` must be whole instructions that are safe to run from another address (no RIP-relative
/// operand, no branch), and `detour` must have the target function's exact signature.
pub unsafe fn install(image: &ProcessImage, rva: usize, prologue: &[u8], detour: usize, original: &AtomicUsize) -> Result<(), String> {
    let n = prologue.len();
    if !(JMP_LEN..=TRAMPOLINE_AT).contains(&n) {
        return Err("prologue must be 5 to 16 bytes".into());
    }
    if image.read(rva, n).as_deref() != Some(prologue) {
        return Err("the function does not start with the expected bytes (already hooked or changed)".into());
    }
    let target = image.va(rva);
    if (target & 7) + JMP_LEN > 8 {
        return Err("the jump would cross an 8-byte boundary".into());
    }

    // Stub near the target: [0] jump to the detour, [16] the moved prologue then a jump back.
    let stub = mem::alloc_exec_near(target, STUB_SIZE).ok_or("no memory near the function for the stub")?;
    let mut code = [0xCCu8; STUB_SIZE];
    code[..6].copy_from_slice(&ABS_JMP);
    code[6..ABS_JMP_LEN].copy_from_slice(&(detour as u64).to_le_bytes());
    let t = TRAMPOLINE_AT;
    code[t..t + n].copy_from_slice(prologue);
    code[t + n..t + n + 6].copy_from_slice(&ABS_JMP);
    code[t + n + 6..t + n + ABS_JMP_LEN].copy_from_slice(&((target + n) as u64).to_le_bytes());
    std::ptr::copy_nonoverlapping(code.as_ptr(), stub as *mut u8, STUB_SIZE);

    let rel = mem::rel32(target + JMP_LEN, stub).ok_or("stub out of rel32 reach")?;
    let mut jmp = [0u8; JMP_LEN];
    jmp[0] = 0xE9;
    jmp[1..].copy_from_slice(&rel.to_le_bytes());
    original.store(stub + TRAMPOLINE_AT, Ordering::Release);
    let written = write_paused(image, rva, target + 1..target + n, &jmp);
    if written.is_err() {
        original.store(0, Ordering::Release);
    }
    written
}

/// Write `jmp` at `rva` while every other thread is paused and none is inside `inside`. Retries
/// with the threads running in between. Everything that allocates happens outside the pause.
unsafe fn write_paused(image: &ProcessImage, rva: usize, inside: std::ops::Range<usize>, jmp: &[u8]) -> Result<(), String> {
    let mut writer = image.clone();
    let mut handles: Vec<HANDLE> = Vec::new();
    for _ in 0..TRIES {
        let ids = other_threads()?;
        handles.reserve(ids.len());
        let result = {
            let mut paused = Paused { handles: &mut handles };
            let all = pause_all(&ids, &mut paused);
            // A thread whose position cannot be read counts as inside.
            let clear = all && paused.handles.iter().all(|&h| rip(h).is_some_and(|r| !inside.contains(&r)));
            if clear {
                Some(writer.write_atomic(rva, jmp))
            } else {
                None
            }
            // `paused` drops here: every thread resumes before anything below can allocate.
        };
        match result {
            Some(r) => return r.map_err(String::from),
            None => std::thread::sleep(Duration::from_millis(1)),
        }
    }
    Err("could not pause every thread outside the function's first instructions".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;
    use std::sync::Arc;

    static ORIG: AtomicUsize = AtomicUsize::new(0);

    type Fn1 = unsafe extern "system" fn(u64) -> u64;

    unsafe extern "system" fn detour(x: u64) -> u64 {
        let orig: Fn1 = std::mem::transmute::<usize, Fn1>(ORIG.load(Ordering::Acquire));
        orig(x) + 1000
    }

    /// A function in a fresh executable page of the test process: the same prologue as
    /// `cGcApplication::Update`, then `lea rax,[rcx+7]`, epilogue, `ret`.
    fn make_function() -> usize {
        let anchor = make_function as *const () as usize;
        let f = mem::alloc_exec_near(anchor, 0x100).expect("exec memory");
        let body: [u8; 15] = [
            0x40, 0x53, // push rbx
            0x48, 0x83, 0xEC, 0x20, // sub rsp,0x20
            0x48, 0x8D, 0x41, 0x07, // lea rax,[rcx+7]
            0x48, 0x83, 0xC4, 0x20, // add rsp,0x20
            0x5B, // pop rbx
        ];
        unsafe {
            std::ptr::copy_nonoverlapping(body.as_ptr(), f as *mut u8, body.len());
            *((f + body.len()) as *mut u8) = 0xC3;
        }
        f
    }

    #[test]
    fn hooks_a_live_function_while_another_thread_runs() {
        let f = make_function();
        // The test function lives outside the exe image, so use a view of the whole address space.
        let view = ProcessImage::for_test_absolute();
        let call: Fn1 = unsafe { std::mem::transmute::<usize, Fn1>(f) };
        assert_eq!(unsafe { call(5) }, 12);

        let stop = Arc::new(AtomicBool::new(false));
        let s = stop.clone();
        let spinner = std::thread::spawn(move || {
            while !s.load(Ordering::Relaxed) {
                std::hint::spin_loop();
            }
        });
        unsafe { install(&view, f, &[0x40, 0x53, 0x48, 0x83, 0xEC, 0x20], detour as *const () as usize, &ORIG) }.unwrap();
        stop.store(true, Ordering::Relaxed);
        spinner.join().unwrap();

        assert_eq!(unsafe { call(5) }, 1012, "detour ran and called the original");
        let again = unsafe { install(&view, f, &[0x40, 0x53, 0x48, 0x83, 0xEC, 0x20], detour as *const () as usize, &ORIG) };
        assert!(again.is_err(), "a second install sees the jump, not the prologue");
    }
}
