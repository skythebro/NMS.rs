//! Generic hooks: one host-owned entry stub serves every hooked game function, whatever its
//! signature. Mods subscribe callbacks that may edit arguments, skip the original, replace the
//! return value and see the caller's address.
//!
//! How: on Windows x64 arguments 1 to 4 are in RCX/RDX/R8/R9 or XMM0-3 (by type) and every later
//! argument is an 8-byte stack slot. The entry stub saves both register sets, a pointer to the stack
//! arguments and the return address into a [`Frame`], and calls the dispatcher; the original is
//! called back through [`call_frame`] with both register sets and N stack slots restored. Return
//! values are taken from RAX and XMM0, so integer and float returns both work. Per hooked function a
//! tiny stub (`mov r11, <hook>; jmp entry`) tells the dispatcher which hook fired.
//!
//! Rules kept from the old hooks: the detour is installed once and never disabled (a toggle is a
//! flag per subscription); nothing on the game thread blocks or allocates; callbacks must not panic
//! (release builds abort). Research: `RESEARCH-plugin-abi.md` section 4 (private repo).
//!
//! Limits: declare the real argument count (only that many are forwarded, so variadic functions are
//! not supported); only the low 64 bits of XMM0-3 and of an XMM0 return are kept (no `__m128` by
//! value, no `__vectorcall`); callbacks must not subscribe (subscribing takes a lock).

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

use crate::hook::Hook;
use crate::mem;

/// Subscribers per hooked function (the run order packs 15 four-bit indices into one u64).
pub const MAX_SUBS: usize = 15;
/// Hooked functions in the process.
pub const MAX_HOOKS: usize = 64;
/// Stack arguments a hook may forward (arguments 5 to 4 + this).
pub const MAX_STACK_ARGS: usize = 16;
/// Nested entries into one hook on one thread; deeper calls go straight to the original.
const MAX_DEPTH: u32 = 8;

/// The registers and stack of one call, as the entry stub saved them. Layout is fixed: the
/// assembly below uses these offsets.
#[repr(C)]
pub struct Frame {
    gpr: [u64; 4],
    xmm: [u64; 4],
    stack_args: *mut u64,
    ret_addr: u64,
    ret: u64,
    retx: u64,
    hook: usize,
}

/// What a before-callback wants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flow {
    Continue,
    /// Do not call the game function; the return value is whatever the callbacks set.
    Skip,
}

/// One call as callbacks see it.
pub struct Call<'a> {
    frame: &'a mut Frame,
    n_args: usize,
    skipped: bool,
}

impl Call<'_> {
    fn slot(&self, i: usize) -> Option<*mut u64> {
        if i >= self.n_args {
            return None;
        }
        if i < 4 {
            return None;
        }
        // SAFETY: the stub saved a pointer to the caller's stack arguments; `i < n_args` and the
        // hook declared `n_args`, so the slot belongs to this call.
        Some(unsafe { self.frame.stack_args.add(i - 4) })
    }

    /// Argument `i` (0-based) as an integer or pointer.
    pub fn arg(&self, i: usize) -> u64 {
        match i {
            0..=3 if i < self.n_args => self.frame.gpr.get(i).copied().unwrap_or(0),
            _ => self.slot(i).map_or(0, |p| unsafe { p.read() }),
        }
    }

    pub fn set_arg(&mut self, i: usize, v: u64) {
        match i {
            0..=3 if i < self.n_args => {
                if let Some(r) = self.frame.gpr.get_mut(i) {
                    *r = v;
                }
            }
            _ => {
                if let Some(p) = self.slot(i) {
                    // SAFETY: see `slot`; the caller's stack slot is ours for the length of the call.
                    unsafe { p.write(v) }
                }
            }
        }
    }

    /// Argument `i` as an `f32` (XMM register for 0 to 3, stack slot after).
    pub fn arg_f32(&self, i: usize) -> f32 {
        f32::from_bits(self.arg_float_bits(i) as u32)
    }

    pub fn set_arg_f32(&mut self, i: usize, v: f32) {
        self.set_arg_float_bits(i, v.to_bits() as u64);
    }

    pub fn arg_f64(&self, i: usize) -> f64 {
        f64::from_bits(self.arg_float_bits(i))
    }

    pub fn set_arg_f64(&mut self, i: usize, v: f64) {
        self.set_arg_float_bits(i, v.to_bits());
    }

    fn arg_float_bits(&self, i: usize) -> u64 {
        match i {
            0..=3 if i < self.n_args => self.frame.xmm.get(i).copied().unwrap_or(0),
            _ => self.slot(i).map_or(0, |p| unsafe { p.read() }),
        }
    }

    fn set_arg_float_bits(&mut self, i: usize, v: u64) {
        match i {
            0..=3 if i < self.n_args => {
                if let Some(r) = self.frame.xmm.get_mut(i) {
                    *r = v;
                }
            }
            _ => {
                if let Some(p) = self.slot(i) {
                    // SAFETY: see `slot`.
                    unsafe { p.write(v) }
                }
            }
        }
    }

    /// The return value (RAX). Before the original ran it is whatever a skipping callback set.
    pub fn ret(&self) -> u64 {
        self.frame.ret
    }

    pub fn set_ret(&mut self, v: u64) {
        self.frame.ret = v;
    }

    /// The return value as a float (XMM0).
    pub fn ret_f32(&self) -> f32 {
        f32::from_bits(self.frame.retx as u32)
    }

    pub fn set_ret_f32(&mut self, v: f32) {
        self.frame.retx = v.to_bits() as u64;
    }

    pub fn ret_f64(&self) -> f64 {
        f64::from_bits(self.frame.retx)
    }

    pub fn set_ret_f64(&mut self, v: f64) {
        self.frame.retx = v.to_bits();
    }

    /// Absolute address the game function returns to (inside its caller).
    pub fn caller(&self) -> usize {
        self.frame.ret_addr as usize
    }

    /// True once a before-callback skipped the original.
    pub fn skipped(&self) -> bool {
        self.skipped
    }
}

pub type Before = fn(&mut Call) -> Flow;
pub type After = fn(&mut Call);

struct Sub {
    before: AtomicUsize,
    after: AtomicUsize,
    enabled: AtomicBool,
    priority: AtomicI32,
}

impl Sub {
    const fn new() -> Self {
        Self {
            before: AtomicUsize::new(0),
            after: AtomicUsize::new(0),
            enabled: AtomicBool::new(false),
            priority: AtomicI32::new(0),
        }
    }

    fn used(&self) -> bool {
        self.before.load(Ordering::Acquire) != 0 || self.after.load(Ordering::Acquire) != 0
    }
}

/// One hooked game function.
pub struct HookCtx {
    target: AtomicUsize,
    trampoline: AtomicUsize,
    n_args: AtomicUsize,
    /// Run order (by priority, then subscription order): 4-bit subscriber indices from the low end,
    /// ended by 0xF. One atomic, so the game thread always sees a whole order, never half an update.
    order: AtomicU64,
    subs: [Sub; MAX_SUBS],
    skip_conflicts: AtomicU32,
    name: OnceLock<&'static str>,
}

impl HookCtx {
    const fn new() -> Self {
        Self {
            target: AtomicUsize::new(0),
            trampoline: AtomicUsize::new(0),
            n_args: AtomicUsize::new(0),
            order: AtomicU64::new(u64::MAX),
            subs: [const { Sub::new() }; MAX_SUBS],
            skip_conflicts: AtomicU32::new(0),
            name: OnceLock::new(),
        }
    }
}

static HOOKS: [HookCtx; MAX_HOOKS] = [const { HookCtx::new() }; MAX_HOOKS];
/// Installs and subscriptions are serialised (never on the game thread's hot path).
static INSTALL: Mutex<()> = Mutex::new(());
static RETOUR: Mutex<Vec<Hook>> = Mutex::new(Vec::new());

thread_local! {
    static DEPTH: [std::cell::Cell<u32>; MAX_HOOKS] = const { [const { std::cell::Cell::new(0) }; MAX_HOOKS] };
}

/// A subscription. Toggle it live from any thread.
#[derive(Clone, Copy, Debug)]
pub struct SubRef {
    hook: usize,
    sub: usize,
}

impl SubRef {
    fn sub(&self) -> Option<&'static Sub> {
        HOOKS.get(self.hook)?.subs.get(self.sub)
    }

    pub fn set_enabled(&self, on: bool) {
        if let Some(s) = self.sub() {
            s.enabled.store(on, Ordering::Release);
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.sub().is_some_and(|s| s.enabled.load(Ordering::Acquire))
    }

    /// The game function without any subscriber (to call it from a callback without recursion).
    pub fn original(&self) -> usize {
        HOOKS.get(self.hook).map_or(0, |h| h.trampoline.load(Ordering::Acquire))
    }
}

/// Options for one subscription.
#[derive(Clone, Copy, Debug, Default)]
pub struct Options {
    /// Lower runs first; equal priorities run in subscription order.
    pub priority: i32,
}

// ---- assembly ------------------------------------------------------------------------------

/// Entry for every hook. R11 holds the hook's `HookCtx` (set by its stub).
/// Stack: entry RSP is 8 mod 16; `sub 0x98` aligns it. Frame at RSP+0x20 (0x68 bytes); the
/// return address at RSP+0x98; stack arguments from RSP+0xC0 (return address, 32-byte home area).
#[unsafe(naked)]
#[no_mangle]
unsafe extern "C-unwind" fn nms_rs_detour_entry_v1() {
    core::arch::naked_asm!(
        ".seh_proc nms_rs_detour_entry_v1",
        "sub rsp, 0x98",
        ".seh_stackalloc 0x98",
        ".seh_endprologue",
        "mov [rsp+0x20], rcx",
        "mov [rsp+0x28], rdx",
        "mov [rsp+0x30], r8",
        "mov [rsp+0x38], r9",
        "movq qword ptr [rsp+0x40], xmm0",
        "movq qword ptr [rsp+0x48], xmm1",
        "movq qword ptr [rsp+0x50], xmm2",
        "movq qword ptr [rsp+0x58], xmm3",
        "lea rax, [rsp+0xC0]",
        "mov [rsp+0x60], rax",
        "mov rax, [rsp+0x98]",
        "mov [rsp+0x68], rax",
        "mov qword ptr [rsp+0x70], 0",
        "mov qword ptr [rsp+0x78], 0",
        "mov [rsp+0x80], r11",
        "lea rcx, [rsp+0x20]",
        "call {dispatch}",
        "mov rax, [rsp+0x70]",
        "movq xmm0, qword ptr [rsp+0x78]",
        "add rsp, 0x98",
        "ret",
        ".seh_endproc",
        dispatch = sym dispatch,
    )
}

/// Call `target` with the frame's registers and `nstack` stack arguments; store RAX and XMM0 back.
#[unsafe(naked)]
#[no_mangle]
unsafe extern "C-unwind" fn nms_rs_detour_call_v1(_frame: *mut Frame, _target: usize, _nstack: usize) {
    core::arch::naked_asm!(
        ".seh_proc nms_rs_detour_call_v1",
        "push rbp",
        ".seh_pushreg rbp",
        "push rbx",
        ".seh_pushreg rbx",
        "push rsi",
        ".seh_pushreg rsi",
        "push rdi",
        ".seh_pushreg rdi",
        "sub rsp, 8",
        ".seh_stackalloc 8",
        "mov rbp, rsp",
        ".seh_setframe rbp, 0",
        ".seh_endprologue",
        "mov rbx, rcx",
        "mov r10, rdx",
        "lea r11, [r8*8 + 0x2F]",
        "and r11, -16",
        "sub rsp, r11",
        "mov rsi, [rbx+0x40]",
        "lea rdi, [rsp+0x20]",
        "mov rcx, r8",
        "cld",
        "rep movsq",
        "mov rcx, [rbx]",
        "mov rdx, [rbx+0x8]",
        "mov r8, [rbx+0x10]",
        "mov r9, [rbx+0x18]",
        "movq xmm0, qword ptr [rbx+0x20]",
        "movq xmm1, qword ptr [rbx+0x28]",
        "movq xmm2, qword ptr [rbx+0x30]",
        "movq xmm3, qword ptr [rbx+0x38]",
        "call r10",
        "mov [rbx+0x50], rax",
        "movq qword ptr [rbx+0x58], xmm0",
        "lea rsp, [rbp+8]",
        "pop rdi",
        "pop rsi",
        "pop rbx",
        "pop rbp",
        "ret",
        ".seh_endproc",
    )
}

fn call_original(frame: &mut Frame, hook: &HookCtx) {
    let target = hook.trampoline.load(Ordering::Acquire);
    let nstack = hook.n_args.load(Ordering::Acquire).saturating_sub(4).min(MAX_STACK_ARGS);
    if target != 0 {
        // SAFETY: the trampoline runs the game function's moved prologue then jumps back; the frame
        // holds the registers and stack pointer the caller passed.
        unsafe { nms_rs_detour_call_v1(frame, target, nstack) };
    }
}

/// Runs on the game thread for every hooked call. No locks, no allocation, no panics.
extern "C-unwind" fn dispatch(frame: &mut Frame) {
    // The stub stores the address of one of the static `HOOKS` entries in R11; find which one
    // without trusting the value blindly.
    let index = frame.hook.wrapping_sub(HOOKS.as_ptr() as usize) / std::mem::size_of::<HookCtx>();
    // Unreachable (only our stubs enter here, each with its own entry); without a hook there is no
    // original to call.
    let Some(hook) = HOOKS.get(index) else { return };
    let deep = DEPTH
        .try_with(|d| {
            let slot = d.get(index)?;
            let n = slot.get();
            if n >= MAX_DEPTH {
                return None;
            }
            slot.set(n + 1);
            Some(())
        })
        .ok()
        .flatten()
        .is_none();
    if deep {
        call_original(frame, hook);
        return;
    }
    let n_args = hook.n_args.load(Ordering::Acquire);
    let mut call = Call { frame, n_args, skipped: false };
    let order = hook.order.load(Ordering::Acquire);
    for i in run_order(order) {
        let Some(s) = hook.subs.get(i) else { break };
        let f = s.before.load(Ordering::Acquire);
        if f == 0 || !s.enabled.load(Ordering::Acquire) {
            continue;
        }
        // SAFETY: only `subscribe` stores here, always a `Before`.
        let cb = unsafe { std::mem::transmute::<usize, Before>(f) };
        if cb(&mut call) == Flow::Skip {
            if call.skipped {
                hook.skip_conflicts.fetch_add(1, Ordering::Relaxed);
            }
            call.skipped = true;
        }
    }
    if !call.skipped {
        call_original(call.frame, hook);
    }
    for i in run_order(order) {
        let Some(s) = hook.subs.get(i) else { break };
        let f = s.after.load(Ordering::Acquire);
        if f == 0 || !s.enabled.load(Ordering::Acquire) {
            continue;
        }
        // SAFETY: only `subscribe` stores here, always an `After`.
        let cb = unsafe { std::mem::transmute::<usize, After>(f) };
        cb(&mut call);
    }
    let _ = DEPTH.try_with(|d| {
        if let Some(slot) = d.get(index) {
            slot.set(slot.get().saturating_sub(1));
        }
    });
}

// ---- install and subscribe -----------------------------------------------------------------

/// Per-hook stub near nothing in particular: `mov r11, imm64; mov rax, imm64; jmp rax`.
fn make_stub(ctx: usize) -> Result<usize, String> {
    let entry = nms_rs_detour_entry_v1 as *const () as usize;
    let stub = mem::alloc_exec_near(entry, 32).ok_or("no memory for a hook stub")?;
    let mut code = [0xCCu8; 32];
    code[0..2].copy_from_slice(&[0x49, 0xBB]); // mov r11, imm64
    code[2..10].copy_from_slice(&(ctx as u64).to_le_bytes());
    code[10..12].copy_from_slice(&[0x48, 0xB8]); // mov rax, imm64
    code[12..20].copy_from_slice(&(entry as u64).to_le_bytes());
    code[20..22].copy_from_slice(&[0xFF, 0xE0]); // jmp rax
    // SAFETY: fresh executable memory we own; never freed (game code will jump here).
    unsafe { std::ptr::copy_nonoverlapping(code.as_ptr(), stub as *mut u8, code.len()) };
    Ok(stub)
}

/// The hook for `target`, installing it on first use. `n_args` is the number of arguments the game
/// function takes (stack ones included); the largest value any subscriber gives is used.
///
/// # Safety
/// `target` must be the first byte of a game function taking at least `n_args` arguments.
unsafe fn hook_for(name: &'static str, target: usize, n_args: usize) -> Result<usize, String> {
    if n_args > 4 + MAX_STACK_ARGS {
        return Err(format!("{name}: more than {} arguments", 4 + MAX_STACK_ARGS));
    }
    if let Some(i) = HOOKS.iter().position(|h| h.target.load(Ordering::Acquire) == target) {
        let h = &HOOKS[i];
        h.n_args.fetch_max(n_args, Ordering::AcqRel);
        return Ok(i);
    }
    let i = HOOKS
        .iter()
        .position(|h| h.target.load(Ordering::Acquire) == 0)
        .ok_or_else(|| format!("{name}: all {MAX_HOOKS} hook slots are taken"))?;
    let h = &HOOKS[i];
    let _ = h.name.set(name);
    h.n_args.store(n_args, Ordering::Release);
    let stub = make_stub(h as *const HookCtx as usize)?;
    let hook = Hook::install(name, target, stub as *const (), &h.trampoline)?;
    h.target.store(target, Ordering::Release);
    // A poisoned lock must not drop the hook (dropping a retour detour disables it).
    RETOUR.lock().unwrap_or_else(|e| e.into_inner()).push(hook);
    Ok(i)
}

/// Rebuild the run order of one hook after a subscription changed (off the game thread).
fn reorder(h: &HookCtx) {
    let mut used: Vec<(i32, usize)> =
        (0..MAX_SUBS).filter(|&i| h.subs[i].used()).map(|i| (h.subs[i].priority.load(Ordering::Acquire), i)).collect();
    used.sort_by_key(|&(p, i)| (p, i));
    h.order.store(pack_order(used.iter().map(|&(_, i)| i)), Ordering::Release);
}

/// Pack up to `MAX_SUBS` subscriber indices (each below 15) into one u64, ended by 0xF nibbles.
fn pack_order(indices: impl Iterator<Item = usize>) -> u64 {
    let mut packed = u64::MAX;
    for (k, i) in indices.take(MAX_SUBS).enumerate() {
        packed &= !(0xF << (k * 4));
        packed |= ((i as u64) & 0xF) << (k * 4);
    }
    packed
}

/// The indices of a packed run order, until the first 0xF.
fn run_order(packed: u64) -> impl Iterator<Item = usize> {
    (0..MAX_SUBS).map(move |k| ((packed >> (k * 4)) & 0xF) as usize).take_while(|&i| i != 0xF)
}

/// Subscribe to the game function at `target` (absolute address).
///
/// # Safety
/// `target` must be the first byte of a game function taking at least `n_args` arguments.
pub unsafe fn subscribe_at(
    name: &'static str,
    target: usize,
    n_args: usize,
    opts: Options,
    before: Option<Before>,
    after: Option<After>,
) -> Result<SubRef, String> {
    if before.is_none() && after.is_none() {
        return Err(format!("{name}: no callback"));
    }
    let _guard = INSTALL.lock().map_err(|_| format!("{name}: hook lock poisoned"))?;
    let hook = hook_for(name, target, n_args)?;
    let h = &HOOKS[hook];
    let sub = (0..MAX_SUBS).find(|&i| !h.subs[i].used()).ok_or_else(|| format!("{name}: more than {MAX_SUBS} subscribers"))?;
    let s = &h.subs[sub];
    s.priority.store(opts.priority, Ordering::Release);
    s.enabled.store(true, Ordering::Release);
    s.before.store(before.map_or(0, |f| f as usize), Ordering::Release);
    s.after.store(after.map_or(0, |f| f as usize), Ordering::Release);
    reorder(h);
    Ok(SubRef { hook, sub })
}

/// Subscribe to the game function found by a unique pattern.
///
/// # Safety
/// The pattern must find the first byte of a game function taking at least `n_args` arguments.
pub unsafe fn subscribe(
    image: &crate::process::ProcessImage,
    name: &'static str,
    pattern: &str,
    n_args: usize,
    opts: Options,
    before: Option<Before>,
    after: Option<After>,
) -> Result<SubRef, String> {
    use crate::Code as _;
    let pat = crate::Pattern::parse(pattern).map_err(|e| format!("{name}: {e:?}"))?;
    let rva = image.find_unique(&pat).unique().ok_or_else(|| format!("{name} not found (game update?)"))?;
    subscribe_at(name, image.va(rva), n_args, opts, before, after)
}

/// Calls where a second before-callback also asked to skip (the first one wins), per hook name.
pub fn skip_conflicts() -> Vec<(&'static str, u32)> {
    HOOKS
        .iter()
        .filter(|h| h.target.load(Ordering::Relaxed) != 0)
        .filter_map(|h| {
            let n = h.skip_conflicts.swap(0, Ordering::Relaxed);
            (n > 0).then(|| (h.name.get().copied().unwrap_or("?"), n))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Each test function hooks its own private "game" function, so tests can run in parallel.

    #[inline(never)]
    extern "system" fn game6(a: u64, b: f64, c: u64, d: f32, e: u64, f: u64) -> u64 {
        a + (b as u64) * 10 + c * 100 + (d as u64) * 1000 + e * 10000 + f * 100000
    }
    type Game6 = extern "system" fn(u64, f64, u64, f32, u64, u64) -> u64;

    fn add_one_to_arg0(c: &mut Call) -> Flow {
        c.set_arg(0, c.arg(0) + 1);
        Flow::Continue
    }
    fn double_stack_arg5(c: &mut Call) -> Flow {
        c.set_arg(5, c.arg(5) * 2);
        assert_eq!(c.arg_f64(1), 2.0);
        assert_eq!(c.arg_f32(3), 4.0);
        Flow::Continue
    }
    fn add_7_to_result(c: &mut Call) {
        c.set_ret(c.ret() + 7);
    }

    #[test]
    fn edits_args_including_stack_and_replaces_the_result_in_priority_order() {
        let f: Game6 = game6;
        assert_eq!(f(1, 2.0, 3, 4.0, 5, 6), 654321);
        let target = game6 as *const () as usize;
        unsafe {
            // Lower priority first: arg5 doubled (6 -> 12) runs before arg0 + 1.
            subscribe_at("game6", target, 6, Options { priority: 10 }, Some(add_one_to_arg0), Some(add_7_to_result)).unwrap();
            subscribe_at("game6", target, 6, Options { priority: -5 }, Some(double_stack_arg5), None).unwrap();
        }
        let f: Game6 = unsafe { std::mem::transmute::<usize, Game6>(std::hint::black_box(target)) };
        assert_eq!(f(1, 2.0, 3, 4.0, 5, 6), 1254322 + 7);
    }

    #[inline(never)]
    extern "system" fn game_float(a: f32, b: u64, c: f64, d: u64, e: f32) -> f32 {
        a * 2.0 + b as f32 + c as f32 * 0.5 + d as f32 + e
    }
    type GameF = extern "system" fn(f32, u64, f64, u64, f32) -> f32;

    fn half_float_stack_arg(c: &mut Call) -> Flow {
        c.set_arg_f32(4, c.arg_f32(4) / 2.0);
        Flow::Continue
    }
    fn float_ret_plus_one(c: &mut Call) {
        c.set_ret_f32(c.ret_f32() + 1.0);
    }

    #[test]
    fn floats_in_registers_and_on_the_stack_and_float_returns() {
        let target = game_float as *const () as usize;
        unsafe {
            subscribe_at("game_float", target, 5, Options::default(), Some(half_float_stack_arg), Some(float_ret_plus_one)).unwrap();
        }
        let f: GameF = unsafe { std::mem::transmute::<usize, GameF>(std::hint::black_box(target)) };
        assert_eq!(f(1.5, 10, 4.0, 100, 0.5), 1.5 * 2.0 + 10.0 + 2.0 + 100.0 + 0.25 + 1.0);
    }

    #[inline(never)]
    extern "system" fn game_counted(a: u64) -> u64 {
        COUNTED.fetch_add(1, Ordering::SeqCst);
        a * 3
    }
    type Game1 = extern "system" fn(u64) -> u64;
    static COUNTED: AtomicUsize = AtomicUsize::new(0);
    static SECOND_BEFORE: AtomicUsize = AtomicUsize::new(0);
    static AFTER_SAW_SKIP: AtomicBool = AtomicBool::new(false);
    static CALLER: AtomicUsize = AtomicUsize::new(0);

    fn skip_with_99(c: &mut Call) -> Flow {
        CALLER.store(c.caller(), Ordering::SeqCst);
        c.set_ret(99);
        Flow::Skip
    }
    fn also_skip(c: &mut Call) -> Flow {
        assert!(c.skipped());
        SECOND_BEFORE.fetch_add(1, Ordering::SeqCst);
        Flow::Skip
    }
    fn after_sees(c: &mut Call) {
        AFTER_SAW_SKIP.store(c.skipped(), Ordering::SeqCst);
    }

    #[inline(never)]
    fn call_it(f: Game1) -> u64 {
        f(std::hint::black_box(5))
    }

    #[test]
    fn skip_runs_every_callback_but_not_the_original_and_toggles_are_live() {
        let target = game_counted as *const () as usize;
        let s1 = unsafe { subscribe_at("game_counted", target, 1, Options::default(), Some(skip_with_99), Some(after_sees)).unwrap() };
        unsafe { subscribe_at("game_counted", target, 1, Options::default(), Some(also_skip), None).unwrap() };
        let f: Game1 = unsafe { std::mem::transmute::<usize, Game1>(std::hint::black_box(target)) };
        assert_eq!(call_it(f), 99);
        assert_eq!(COUNTED.load(Ordering::SeqCst), 0, "original skipped");
        assert_eq!(SECOND_BEFORE.load(Ordering::SeqCst), 1, "second before still ran");
        assert!(AFTER_SAW_SKIP.load(Ordering::SeqCst));
        let caller = CALLER.load(Ordering::SeqCst);
        let here = call_it as *const () as usize;
        assert!(caller > here && caller < here + 0x200, "caller {caller:#x} inside call_it {here:#x}");
        assert_eq!(skip_conflicts(), vec![("game_counted", 1)]);

        s1.set_enabled(false);
        // The second subscriber still skips but sets no result; switch it off too.
        let all: Vec<SubRef> = (0..2).map(|sub| SubRef { hook: s1.hook, sub }).collect();
        all.iter().for_each(|s| s.set_enabled(false));
        assert_eq!(call_it(f), 15, "with every subscriber off the original runs");
        assert_eq!(COUNTED.load(Ordering::SeqCst), 1);
        let orig: Game1 = unsafe { std::mem::transmute::<usize, Game1>(s1.original()) };
        assert_eq!(orig(2), 6, "the raw original skips subscribers");
    }
}
