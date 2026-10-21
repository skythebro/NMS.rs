//! Lifecycle callbacks for mods: per-frame ticks (before or after the game's own update), state
//! changes, "fully booted" once, and jobs posted from other threads to run on the main thread.
//!
//! Tables are fixed-size and lock-free on the main thread: a slot is a function pointer, an enable
//! flag and timing counters. Registering takes a lock, but only on the registering thread; the main
//! thread never blocks (`post` drains with `try_lock`). Callbacks must not panic (release builds
//! abort), block, sleep, or do file I/O; slow ones are reported in `core.log` by the housekeeping
//! thread, never from the main thread. Toggling a slot is live and never unhooks anything.

use std::sync::atomic::{fence, AtomicBool, AtomicPtr, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use super::live::{is_main_thread, AppState};

/// Slots per kind of callback.
pub const SLOTS: usize = 32;
/// A callback slower than this is counted as an overrun.
const OVER_NS: u64 = 1_000_000;
/// Posted jobs run per frame at most.
const JOBS_PER_FRAME: usize = 16;

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TickPhase {
    /// Before the game's update: before this frame's state change, input and game logic.
    Pre = 0,
    /// After the game's update and after this frame's state change callbacks (the default).
    Post = 1,
}

/// Passed to tick and fully-booted callbacks. `#[repr(C)]` with a leading size so the plugin
/// interface (L2) can hand it to DLL mods unchanged.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Tick {
    pub size: u32,
    pub phase: TickPhase,
    pub frame: u64,
    /// Seconds since the previous frame (our own clock, not the game's scaled step).
    pub dt: f32,
    pub state: AppState,
    pub generation: u64,
}

/// Passed to state change callbacks.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct StateChange {
    pub size: u32,
    pub from: AppState,
    pub to: AppState,
    pub frame: u64,
    pub generation: u64,
}

pub type TickFn = fn(&Tick);
pub type StateFn = fn(&StateChange);

struct Slot {
    f: AtomicUsize,
    enabled: AtomicBool,
    /// Fully-booted slots only: already called.
    fired: AtomicBool,
    calls: AtomicU64,
    max_ns: AtomicU64,
    over: AtomicU32,
}

impl Slot {
    const fn new() -> Self {
        Self {
            f: AtomicUsize::new(0),
            enabled: AtomicBool::new(false),
            fired: AtomicBool::new(false),
            calls: AtomicU64::new(0),
            max_ns: AtomicU64::new(0),
            over: AtomicU32::new(0),
        }
    }

    fn time(&self, started: Instant) {
        let ns = started.elapsed().as_nanos().min(u64::MAX as u128) as u64;
        self.calls.fetch_add(1, Ordering::Relaxed);
        self.max_ns.fetch_max(ns, Ordering::Relaxed);
        if ns > OVER_NS {
            self.over.fetch_add(1, Ordering::Relaxed);
        }
    }
}

pub struct Table {
    kind: &'static str,
    slots: [Slot; SLOTS],
    /// Owner names; read and written only off the main thread.
    owners: Mutex<[&'static str; SLOTS]>,
}

impl Table {
    const fn new(kind: &'static str) -> Self {
        Self { kind, slots: [const { Slot::new() }; SLOTS], owners: Mutex::new([""; SLOTS]) }
    }
}

static PRE: Table = Table::new("tick (before update)");
static POST: Table = Table::new("tick");
static STATE: Table = Table::new("state change");
static BOOT: Table = Table::new("fully booted");
static TABLES: [&Table; 4] = [&PRE, &POST, &STATE, &BOOT];

/// Set when a fully-booted callback registers after boot: the main thread calls it on the next tick.
static BOOT_PENDING: AtomicBool = AtomicBool::new(false);
static BOOTED_SEEN: AtomicBool = AtomicBool::new(false);

/// Posted jobs: a lock-free stack (pushed from any thread, including the main thread from inside a
/// callback), taken whole by the main thread and kept there in FIFO order.
struct Node {
    job: Box<dyn FnOnce() + Send>,
    next: *mut Node,
}

static INBOX: AtomicPtr<Node> = AtomicPtr::new(std::ptr::null_mut());
/// Jobs taken from the inbox in FIFO order, waiting for a frame. Main thread only.
static READY: AtomicPtr<Node> = AtomicPtr::new(std::ptr::null_mut());
static QUEUED: AtomicUsize = AtomicUsize::new(0);
static DROPPED: AtomicUsize = AtomicUsize::new(0);
/// More waiting jobs than this and new ones are dropped (and reported).
const MAX_QUEUED: usize = 4096;
/// Timing of posted jobs, reported like a callback.
static JOBS: Slot = Slot::new();

/// A registered callback. Copy it freely; toggling is live and safe from any thread.
#[derive(Clone, Copy)]
pub struct SlotRef {
    table: &'static Table,
    index: usize,
}

impl SlotRef {
    pub fn set_enabled(&self, on: bool) {
        if let Some(s) = self.table.slots.get(self.index) {
            s.enabled.store(on, Ordering::Release);
            // A fully-booted callback that was off at boot runs once when it is switched on.
            if on && std::ptr::eq(self.table, &BOOT) {
                fence(Ordering::SeqCst);
                if BOOTED_SEEN.load(Ordering::Acquire) {
                    BOOT_PENDING.store(true, Ordering::Release);
                }
            }
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.table.slots.get(self.index).is_some_and(|s| s.enabled.load(Ordering::Acquire))
    }

    /// How often the callback ran so far.
    pub fn calls(&self) -> u64 {
        self.table.slots.get(self.index).map_or(0, |s| s.calls.load(Ordering::Relaxed))
    }
}

fn register(table: &'static Table, owner: &'static str, f: usize) -> Result<SlotRef, String> {
    if f == 0 {
        return Err("null callback".into());
    }
    if is_main_thread() {
        // Registering takes a lock; the main thread never blocks. Register at load, or `post` it.
        return Err("callbacks cannot be registered from the game's main thread".into());
    }
    let mut owners = table.owners.lock().map_err(|_| "callback table lock poisoned".to_string())?;
    let index = table
        .slots
        .iter()
        .position(|s| s.f.load(Ordering::Acquire) == 0)
        .ok_or_else(|| format!("all {SLOTS} {} slots are taken", table.kind))?;
    if let Some(name) = owners.get_mut(index) {
        *name = owner;
    }
    let slot = &table.slots[index];
    slot.enabled.store(true, Ordering::Relaxed);
    slot.fired.store(false, Ordering::Relaxed);
    slot.f.store(f, Ordering::Release);
    Ok(SlotRef { table, index })
}

/// Run `f` every frame on the main thread, before or after the game's update.
pub fn on_tick(owner: &'static str, phase: TickPhase, f: TickFn) -> Result<SlotRef, String> {
    let table = match phase {
        TickPhase::Pre => &PRE,
        TickPhase::Post => &POST,
    };
    register(table, owner, f as *const () as usize)
}

/// Run `f` on the main thread when the game's state changes (an applied change, once per change).
pub fn on_state_change(owner: &'static str, f: StateFn) -> Result<SlotRef, String> {
    register(&STATE, owner, f as *const () as usize)
}

/// Run `f` once on the main thread when the game reaches the main menu for the first time. A
/// callback registered later runs once on the next frame.
pub fn on_fully_booted(owner: &'static str, f: TickFn) -> Result<SlotRef, String> {
    let slot = register(&BOOT, owner, f as *const () as usize)?;
    // Publish-then-check here and on the main thread (`run_booted`): with a full fence on both
    // sides, at least one of them sees the other, so the callback cannot be missed.
    fence(Ordering::SeqCst);
    if BOOTED_SEEN.load(Ordering::Acquire) {
        BOOT_PENDING.store(true, Ordering::Release);
    }
    Ok(slot)
}

/// Run `job` once on the main thread after the game's update (before the frame's after-update
/// ticks). Lock-free, so it may be called from any thread, including inside a callback. Use it from
/// other threads (settings pipe, timers) for anything that touches the game.
pub fn post(_owner: &'static str, job: Box<dyn FnOnce() + Send>) {
    if QUEUED.fetch_add(1, Ordering::AcqRel) >= MAX_QUEUED {
        QUEUED.fetch_sub(1, Ordering::AcqRel);
        DROPPED.fetch_add(1, Ordering::Relaxed);
        return;
    }
    let node = Box::into_raw(Box::new(Node { job, next: std::ptr::null_mut() }));
    let mut head = INBOX.load(Ordering::Acquire);
    loop {
        // SAFETY: `node` is ours until the exchange below publishes it.
        unsafe { (*node).next = head };
        match INBOX.compare_exchange_weak(head, node, Ordering::Release, Ordering::Acquire) {
            Ok(_) => return,
            Err(now) => head = now,
        }
    }
}

// ---- main thread ---------------------------------------------------------------------------

pub(super) fn run_ticks(phase: TickPhase, tick: &Tick) {
    let table = match phase {
        TickPhase::Pre => &PRE,
        TickPhase::Post => &POST,
    };
    for s in &table.slots {
        let f = s.f.load(Ordering::Acquire);
        if f == 0 || !s.enabled.load(Ordering::Acquire) {
            continue;
        }
        let started = Instant::now();
        // SAFETY: only `on_tick` stores into this table, always a `TickFn`.
        let cb = unsafe { std::mem::transmute::<usize, TickFn>(f) };
        cb(tick);
        s.time(started);
    }
}

pub(super) fn run_state_change(change: &StateChange) {
    for s in &STATE.slots {
        let f = s.f.load(Ordering::Acquire);
        if f == 0 || !s.enabled.load(Ordering::Acquire) {
            continue;
        }
        let started = Instant::now();
        // SAFETY: only `on_state_change` stores into this table, always a `StateFn`.
        let cb = unsafe { std::mem::transmute::<usize, StateFn>(f) };
        cb(change);
        s.time(started);
    }
}

/// Call every fully-booted callback that has not run yet. `now` is true on the boot frame.
pub(super) fn run_booted(now: bool, tick: &Tick) {
    if now {
        BOOTED_SEEN.store(true, Ordering::Release);
        fence(Ordering::SeqCst); // pairs with the fence in `on_fully_booted` / `set_enabled`
    } else if !BOOT_PENDING.swap(false, Ordering::AcqRel) {
        return;
    }
    for s in &BOOT.slots {
        let f = s.f.load(Ordering::Acquire);
        if f == 0 || !s.enabled.load(Ordering::Acquire) || s.fired.swap(true, Ordering::AcqRel) {
            continue;
        }
        let started = Instant::now();
        // SAFETY: only `on_fully_booted` stores into this table, always a `TickFn`.
        let cb = unsafe { std::mem::transmute::<usize, TickFn>(f) };
        cb(tick);
        s.time(started);
    }
}

/// Run up to `JOBS_PER_FRAME` posted jobs, oldest first. Main thread only (the single consumer).
pub(super) fn run_posted() {
    for _ in 0..JOBS_PER_FRAME {
        let mut ready = READY.load(Ordering::Relaxed);
        if ready.is_null() {
            // Take everything posted so far (newest first) and reverse it into FIFO order.
            let mut taken = INBOX.swap(std::ptr::null_mut(), Ordering::AcqRel);
            while !taken.is_null() {
                // SAFETY: nodes in the taken list are owned by this thread now.
                let next = unsafe { (*taken).next };
                unsafe { (*taken).next = ready };
                ready = taken;
                taken = next;
            }
            if ready.is_null() {
                READY.store(ready, Ordering::Relaxed);
                return;
            }
        }
        // SAFETY: `ready` came from `Box::into_raw` in `post` and is owned by this thread.
        let node = unsafe { Box::from_raw(ready) };
        READY.store(node.next, Ordering::Relaxed);
        QUEUED.fetch_sub(1, Ordering::AcqRel);
        let started = Instant::now();
        (node.job)();
        JOBS.time(started);
    }
}

// ---- housekeeping thread -------------------------------------------------------------------

/// One line per callback that overran since the last report, then reset its counters.
pub(super) fn overrun_report() -> Vec<String> {
    let mut lines = Vec::new();
    for table in TABLES {
        let owners = table.owners.lock().map(|o| *o).unwrap_or([""; SLOTS]);
        for (i, s) in table.slots.iter().enumerate() {
            let over = s.over.swap(0, Ordering::Relaxed);
            let max = s.max_ns.swap(0, Ordering::Relaxed);
            if over == 0 || s.f.load(Ordering::Relaxed) == 0 {
                continue;
            }
            let owner = owners.get(i).copied().unwrap_or("?");
            lines.push(format!(
                "lifecycle: {owner} {} callback took up to {:.2} ms ({over} calls over {:.1} ms since the last report)",
                table.kind,
                max as f64 / 1e6,
                OVER_NS as f64 / 1e6
            ));
        }
    }
    let over = JOBS.over.swap(0, Ordering::Relaxed);
    let max = JOBS.max_ns.swap(0, Ordering::Relaxed);
    if over > 0 {
        lines.push(format!("lifecycle: a posted job took up to {:.2} ms ({over} jobs over {:.1} ms)", max as f64 / 1e6, OVER_NS as f64 / 1e6));
    }
    let dropped = DROPPED.swap(0, Ordering::Relaxed);
    if dropped > 0 {
        lines.push(format!("lifecycle: {dropped} posted jobs dropped (more than {MAX_QUEUED} waiting)"));
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    static PRE_N: AtomicU64 = AtomicU64::new(0);
    static POST_N: AtomicU64 = AtomicU64::new(0);
    static STATE_N: AtomicU64 = AtomicU64::new(0);
    static BOOT_N: AtomicU64 = AtomicU64::new(0);
    static JOB_N: AtomicU64 = AtomicU64::new(0);

    fn pre(_: &Tick) {
        PRE_N.fetch_add(1, Ordering::Relaxed);
    }
    fn post_cb(t: &Tick) {
        assert_eq!(t.phase, TickPhase::Post);
        POST_N.fetch_add(1, Ordering::Relaxed);
    }
    fn slow(_: &Tick) {
        let t = Instant::now();
        while t.elapsed().as_nanos() < 2 * OVER_NS as u128 {}
    }
    fn state(c: &StateChange) {
        assert_eq!(c.frame, 7);
        STATE_N.fetch_add(1, Ordering::Relaxed);
    }
    fn boot(_: &Tick) {
        BOOT_N.fetch_add(1, Ordering::Relaxed);
    }

    fn t(phase: TickPhase) -> Tick {
        Tick { size: 0, phase, frame: 7, dt: 0.016, state: AppState::NONE, generation: 1 }
    }

    /// One test: the tables are process-wide statics.
    #[test]
    fn ticks_toggles_state_booted_once_and_posted_jobs() {
        let p = on_tick("test", TickPhase::Pre, pre).unwrap();
        on_tick("test", TickPhase::Post, post_cb).unwrap();
        on_state_change("test", state).unwrap();
        on_fully_booted("test", boot).unwrap();

        run_ticks(TickPhase::Pre, &t(TickPhase::Pre));
        run_ticks(TickPhase::Post, &t(TickPhase::Post));
        assert_eq!((PRE_N.load(Ordering::Relaxed), POST_N.load(Ordering::Relaxed)), (1, 1));
        assert_eq!(p.calls(), 1);

        p.set_enabled(false);
        run_ticks(TickPhase::Pre, &t(TickPhase::Pre));
        assert_eq!(PRE_N.load(Ordering::Relaxed), 1, "a disabled slot is skipped");
        p.set_enabled(true);

        run_state_change(&StateChange { size: 0, from: AppState::NONE, to: AppState::NONE, frame: 7, generation: 2 });
        assert_eq!(STATE_N.load(Ordering::Relaxed), 1);

        run_booted(true, &t(TickPhase::Post));
        run_booted(true, &t(TickPhase::Post));
        run_booted(false, &t(TickPhase::Post));
        assert_eq!(BOOT_N.load(Ordering::Relaxed), 1, "fully booted fires once");
        // Registered after boot: runs once on the next frame.
        on_fully_booted("late", boot).unwrap();
        run_booted(false, &t(TickPhase::Post));
        run_booted(false, &t(TickPhase::Post));
        assert_eq!(BOOT_N.load(Ordering::Relaxed), 2);

        for _ in 0..20 {
            post("test", Box::new(|| {
                JOB_N.fetch_add(1, Ordering::Relaxed);
            }));
        }
        run_posted();
        assert_eq!(JOB_N.load(Ordering::Relaxed), JOBS_PER_FRAME as u64, "bounded per frame");
        run_posted();
        assert_eq!(JOB_N.load(Ordering::Relaxed), 20);

        on_tick("slowpoke", TickPhase::Post, slow).unwrap();
        run_ticks(TickPhase::Post, &t(TickPhase::Post));
        let report = overrun_report();
        assert!(report.iter().any(|l| l.contains("slowpoke")), "{report:?}");
        assert!(overrun_report().iter().all(|l| !l.contains("slowpoke")), "counters reset after a report");
        assert!(summary().contains("slowpoke"));
    }
}

/// Registered callbacks, for one log line at boot and on request.
pub fn summary() -> String {
    let mut parts = Vec::new();
    for table in TABLES {
        let owners = table.owners.lock().map(|o| *o).unwrap_or([""; SLOTS]);
        let names: Vec<&str> =
            table.slots.iter().enumerate().filter(|(_, s)| s.f.load(Ordering::Relaxed) != 0).map(|(i, _)| owners.get(i).copied().unwrap_or("?")).collect();
        if !names.is_empty() {
            parts.push(format!("{}: {}", table.kind, names.join(", ")));
        }
    }
    if parts.is_empty() {
        "lifecycle callbacks: none".into()
    } else {
        format!("lifecycle callbacks: {}", parts.join("; "))
    }
}
