//! The game context and the per-frame lifecycle hook inside the running game.
//!
//! One detour on `cGcApplication::Update`, owned by the core. It runs on the game's main thread
//! every frame, so it never blocks, allocates, logs or panics: it records frame counts and state
//! changes into atomics and a fixed ring, and a housekeeping thread writes `core.log`.
//!
//! L1.2 has no subscribers yet: the hook only observes (state changes, when "fully booted" would
//! fire, frame rate). Mods get callbacks in L1.3.

use std::sync::atomic::{fence, AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use windows_sys::Win32::System::Threading::GetCurrentThreadId;

use super::dispatch::{self, StateChange, Tick, TickPhase};
use super::{resolve, sigs, Layout, FSM_CURRENT_STATE};
use crate::jump;
use crate::log::Log;
use crate::mem;
use crate::process::ProcessImage;

/// States in which player and simulation data belong to a loaded save.
const IN_GAME_STATES: &[&str] = &["APPVIEW", "GALAXYMAP", "YOUAREDEAD"];
/// States that run before the game has finished booting.
const BOOT_STATES: &[&str] = &["APPBOOT", "APPCORESERVICES", "APPGLOBALLOAD"];
const BOOTED_STATE: &str = "MODESELECTOR";
const SHUTDOWN_STATE: &str = "APPSHUTDOWN";

/// The game context: addresses resolved once, members read on every call.
pub struct Game {
    base: usize,
    layout: Layout,
}

static GAME: OnceLock<Game> = OnceLock::new();

/// Resolve the context from the running game's code. Safe to call more than once.
pub fn init(image: &ProcessImage) -> Result<&'static Game, String> {
    if let Some(g) = GAME.get() {
        return Ok(g);
    }
    let layout = resolve(image)?;
    Ok(GAME.get_or_init(|| Game { base: image.base(), layout }))
}

/// The context, once [`init`] succeeded.
pub fn get() -> Option<&'static Game> {
    GAME.get()
}

/// The application's current state, as an index into the game's own state table.
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AppState(u16);

impl AppState {
    /// No state yet (the pointer is still null early in boot).
    pub const NONE: Self = Self(u16::MAX);
    /// A state object that is not in the table (should not happen; logged by address).
    pub const UNKNOWN: Self = Self(u16::MAX - 1);

    pub fn index(self) -> Option<usize> {
        (self.0 < Self::UNKNOWN.0).then_some(self.0 as usize)
    }

    fn pack(self) -> u32 {
        self.0 as u32
    }

    fn unpack(v: u32) -> Self {
        Self(v as u16)
    }
}

impl Game {
    pub fn layout(&self) -> &Layout {
        &self.layout
    }

    /// Address of the `cGcApplication` object.
    pub fn app_obj(&self) -> usize {
        self.base + self.layout.app_obj
    }

    /// `cGcApplication::Data`, re-read on every call. `None` until the game allocated it.
    pub fn data(&self) -> Option<usize> {
        mem::read_ptr(self.base + self.layout.data_slot)
    }

    fn state_of(&self, state_obj: usize) -> AppState {
        if state_obj < mem::MIN_ADDR {
            return AppState::NONE;
        }
        match state_obj.checked_sub(self.app_obj()).and_then(|off| self.layout.state_at_offset(off)) {
            Some(i) => AppState(i as u16),
            None => AppState::UNKNOWN,
        }
    }

    /// The state the game is in now.
    pub fn state(&self) -> AppState {
        self.state_of(mem::read_ptr(self.app_obj() + FSM_CURRENT_STATE).unwrap_or(0))
    }

    /// The game's id of a state (`APPVIEW`, `MODESELECTOR`, ...).
    pub fn state_name(&self, state: AppState) -> &str {
        match state.index() {
            Some(i) => self.layout.state_name(i).unwrap_or("(unknown)"),
            None if state == AppState::NONE => "(none)",
            None => "(unknown)",
        }
    }

    /// A save is loaded: player and simulation data are meaningful.
    pub fn in_game(&self) -> bool {
        IN_GAME_STATES.contains(&self.state_name(self.state()))
    }

    fn member(&self, offset: u32) -> Option<usize> {
        self.data().map(|d| d + offset as usize)
    }

    pub fn game_state(&self) -> Option<usize> {
        self.member(self.layout.members.game_state)
    }

    pub fn simulation(&self) -> Option<usize> {
        self.member(self.layout.members.simulation)
    }

    pub fn reality(&self) -> Option<usize> {
        self.member(self.layout.members.reality)
    }

    pub fn hud(&self) -> Option<usize> {
        self.member(self.layout.members.hud)
    }

    pub fn frontend(&self) -> Option<usize> {
        self.member(self.layout.members.frontend)
    }

    /// The input controller object, re-read on every call.
    pub fn input_controller(&self) -> Option<usize> {
        mem::read_ptr(self.member(self.layout.members.input_controller)?)
    }
}

// ---- lifecycle hook (main thread, every frame) ------------------------------------------------

type UpdateFn = unsafe extern "system" fn(u64) -> u64;

static ORIGINAL: AtomicUsize = AtomicUsize::new(0);
static INSTALLED: AtomicBool = AtomicBool::new(false);
/// Tells the housekeeping thread to end (the hook could not be written).
static STOP: AtomicBool = AtomicBool::new(false);
static ENABLED: AtomicBool = AtomicBool::new(false);
static IN_TICK: AtomicBool = AtomicBool::new(false);
static FRAME: AtomicU64 = AtomicU64::new(0);
static GENERATION: AtomicU64 = AtomicU64::new(0);
static LAST_STATE_OBJ: AtomicUsize = AtomicUsize::new(0);
static MAIN_THREAD: AtomicU32 = AtomicU32::new(0);
static BOOTED: AtomicBool = AtomicBool::new(false);
/// The state seen after the last update, packed (`AppState::NONE` before the first frame).
static CURRENT: AtomicU32 = AtomicU32::new(u16::MAX as u32);
/// The game entered its shutdown state: ticks and posted jobs stop.
static SHUTTING_DOWN: AtomicBool = AtomicBool::new(false);
static CLOCK: OnceLock<Instant> = OnceLock::new();
static LAST_FRAME_NS: AtomicU64 = AtomicU64::new(0);

const RING: usize = 64;
const FLAG_BOOTED: u32 = 1;
const FLAG_FIRST: u32 = 2;

/// One state change, written by the main thread and read by the housekeeping thread.
struct Event {
    frame: AtomicU64,
    states: AtomicU32,
    flags: AtomicU32,
    data: AtomicUsize,
    state_obj: AtomicUsize,
}

impl Event {
    const fn new() -> Self {
        Self {
            frame: AtomicU64::new(0),
            states: AtomicU32::new(0),
            flags: AtomicU32::new(0),
            data: AtomicUsize::new(0),
            state_obj: AtomicUsize::new(0),
        }
    }
}

static EVENTS: [Event; RING] = [const { Event::new() }; RING];
static WRITTEN: AtomicU64 = AtomicU64::new(0);

/// Frames seen since the hook went in.
pub fn frame() -> u64 {
    FRAME.load(Ordering::Relaxed)
}

/// +1 on every observed state change. Mods that cache game pointers compare it to drop the cache.
pub fn generation() -> u64 {
    GENERATION.load(Ordering::Relaxed)
}

/// The game's main thread id, known after the first frame (0 before).
pub fn main_thread() -> u32 {
    MAIN_THREAD.load(Ordering::Relaxed)
}

/// True on the game's main thread (after the first frame).
pub fn is_main_thread() -> bool {
    let id = main_thread();
    // SAFETY: plain Win32 call without arguments.
    id != 0 && id == unsafe { GetCurrentThreadId() }
}

fn push_event(frame: u64, from: AppState, to: AppState, flags: u32, data: usize, state_obj: usize) {
    let i = WRITTEN.load(Ordering::Relaxed);
    let Some(e) = EVENTS.get((i % RING as u64) as usize) else { return };
    // Pairs with the reader's acquire fence: a reader that sees these field stores also sees the
    // `WRITTEN` value that tells it whether the slot was being rewritten.
    fence(Ordering::Release);
    e.frame.store(frame, Ordering::Relaxed);
    e.states.store((from.pack() << 16) | to.pack(), Ordering::Relaxed);
    e.flags.store(flags, Ordering::Relaxed);
    e.data.store(data, Ordering::Relaxed);
    e.state_obj.store(state_obj, Ordering::Relaxed);
    WRITTEN.store(i + 1, Ordering::Release);
}

/// A state change seen after the game's Update, and whether it is the boot.
struct Seen {
    change: StateChange,
    booted: bool,
}

/// Work after the game's Update returned. No locks, no allocation, no panics.
fn after_update(game: &Game, frame: u64) -> Option<Seen> {
    // A field of the static `cGcApplication` in the exe's data, checked readable in `install_lifecycle`.
    // SAFETY: the address is inside the loaded image for the life of the process.
    let state_obj = unsafe { std::ptr::read_volatile((game.app_obj() + FSM_CURRENT_STATE) as *const usize) };
    let last = LAST_STATE_OBJ.load(Ordering::Relaxed);
    if state_obj == last {
        return None;
    }
    LAST_STATE_OBJ.store(state_obj, Ordering::Relaxed);
    let generation = GENERATION.fetch_add(1, Ordering::Relaxed) + 1;
    let from = game.state_of(last);
    let to = game.state_of(state_obj);
    CURRENT.store(to.pack(), Ordering::Relaxed);
    let name = game.state_name(to);
    if name == SHUTDOWN_STATE {
        SHUTTING_DOWN.store(true, Ordering::Relaxed);
    }
    let first = last == 0;
    let mut flags = if first { FLAG_FIRST } else { 0 };
    let mut booted = false;
    if !BOOTED.load(Ordering::Relaxed) {
        let late = first && to.index().is_some() && !BOOT_STATES.contains(&name);
        if name == BOOTED_STATE || late {
            BOOTED.store(true, Ordering::Relaxed);
            flags |= FLAG_BOOTED;
            booted = true;
        }
    }
    push_event(frame, from, to, flags, game.data().unwrap_or(0), state_obj);
    let change = StateChange { size: std::mem::size_of::<StateChange>() as u32, from, to, frame, generation };
    Some(Seen { change, booted })
}

fn tick(phase: TickPhase, frame: u64, dt: f32) -> Tick {
    Tick {
        size: std::mem::size_of::<Tick>() as u32,
        phase,
        frame,
        dt,
        state: AppState::unpack(CURRENT.load(Ordering::Relaxed)),
        generation: GENERATION.load(Ordering::Relaxed),
    }
}

/// Seconds since the previous frame, from our own monotonic clock.
fn frame_dt() -> f32 {
    let now = CLOCK.get_or_init(Instant::now).elapsed().as_nanos().min(u64::MAX as u128) as u64;
    let last = LAST_FRAME_NS.swap(now, Ordering::Relaxed);
    if last == 0 {
        0.0
    } else {
        now.saturating_sub(last) as f32 / 1e9
    }
}

unsafe extern "system" fn update_detour(this: u64) -> u64 {
    let original = ORIGINAL.load(Ordering::Acquire);
    if original == 0 {
        // Unreachable: `jump::install` publishes the trampoline before it writes the jump. If it
        // ever ran, this frame's game update would be skipped.
        return 0;
    }
    // SAFETY: ORIGINAL holds the trampoline for a function with this exact signature
    // (`void cGcApplication::Update(this)`, `rax` passed through).
    let original: UpdateFn = std::mem::transmute::<usize, UpdateFn>(original);
    if !ENABLED.load(Ordering::Relaxed) || IN_TICK.swap(true, Ordering::Acquire) {
        return original(this);
    }
    let frame = FRAME.fetch_add(1, Ordering::Relaxed) + 1;
    if frame == 1 {
        MAIN_THREAD.store(GetCurrentThreadId(), Ordering::Relaxed);
    }
    let dt = frame_dt();
    let ticking = !SHUTTING_DOWN.load(Ordering::Relaxed);
    if ticking {
        dispatch::run_ticks(TickPhase::Pre, &tick(TickPhase::Pre, frame, dt));
    }
    let ret = original(this);
    if let Some(game) = GAME.get() {
        let post = tick(TickPhase::Post, frame, dt);
        if let Some(seen) = after_update(game, frame) {
            dispatch::run_state_change(&seen.change);
            if seen.booted {
                dispatch::run_booted(true, &post);
            }
        }
        if BOOTED.load(Ordering::Relaxed) {
            dispatch::run_booted(false, &post);
        }
        if !SHUTTING_DOWN.load(Ordering::Relaxed) {
            dispatch::run_posted();
            dispatch::run_ticks(TickPhase::Post, &tick(TickPhase::Post, frame, dt));
        }
    }
    IN_TICK.store(false, Ordering::Release);
    ret
}

/// Hook `cGcApplication::Update` once (never unhooked) and start the housekeeping thread that logs
/// state changes and the frame rate. Call as early as possible on the boot thread.
pub fn install_lifecycle(image: &ProcessImage, game: &'static Game, log: Arc<Log>) -> Result<(), String> {
    if INSTALLED.swap(true, Ordering::AcqRel) {
        return Ok(());
    }
    if !mem::readable(game.app_obj() + FSM_CURRENT_STATE, std::mem::size_of::<usize>()) {
        return Err("the application object is not readable".into());
    }
    // Start the frame clock here, not on the main thread's first frame.
    let _ = CLOCK.get_or_init(Instant::now);
    // The log thread first: if it cannot start, no hook goes in.
    let thread_log = log.clone();
    std::thread::Builder::new()
        .name("nms-rs lifecycle".into())
        .spawn(move || housekeeping(game, thread_log))
        .map_err(|e| format!("lifecycle log thread: {e}"))?;
    // SAFETY: `layout.update` is the first byte of `cGcApplication::Update` (unique pattern,
    // cross-checked in `resolve`); `UPDATE_PROLOGUE` is two whole instructions without a RIP operand
    // (checked against the live bytes by `jump::install`); the detour declares the only argument
    // (`this`) and passes `rax` through.
    let installed = unsafe {
        jump::install(image, game.layout.update, &sigs::UPDATE_PROLOGUE, update_detour as *const () as usize, &ORIGINAL)
    };
    if let Err(e) = installed {
        STOP.store(true, Ordering::Release);
        return Err(format!("cGcApplication::Update: {e}"));
    }
    ENABLED.store(true, Ordering::Release);
    log.info(&format!("lifecycle: hooked cGcApplication::Update at RVA {:#x} (threads paused for the write)", game.layout.update));
    Ok(())
}

const POLL: Duration = Duration::from_millis(200);
/// Frame-rate lines: one a minute for the first ten minutes, then one every half hour.
const RATE_EVERY: Duration = Duration::from_secs(60);
const RATE_EARLY_LINES: u32 = 10;
const RATE_LATER: Duration = Duration::from_secs(30 * 60);
/// Slow-callback lines: at most one per callback per 10 s.
const REPORT_EVERY: Duration = Duration::from_secs(10);

/// Writes what the main thread recorded. Runs for the life of the process (the core is never unloaded).
fn housekeeping(game: &'static Game, log: Arc<Log>) {
    let mut read = 0u64;
    let mut thread_logged = false;
    let mut rate_at = Instant::now();
    let mut rate_frames = frame();
    let mut rate_lines = 0u32;
    // The producer may be rewriting slot `written % RING` (the oldest unread one when the reader is a
    // full ring behind), so at most RING - 1 unread events are trusted.
    let keep = RING as u64 - 1;
    let mut report_at = Instant::now();
    loop {
        std::thread::sleep(POLL);
        if STOP.load(Ordering::Acquire) {
            return;
        }
        if !thread_logged && main_thread() != 0 {
            thread_logged = true;
            log.info(&format!("lifecycle: first frame on thread {}", main_thread()));
        }
        let written = WRITTEN.load(Ordering::Acquire);
        if written - read > keep {
            log.info(&format!("lifecycle: {} state changes not logged (log fell behind)", written - read - keep));
            read = written - keep;
        }
        while read < written {
            let Some(e) = EVENTS.get((read % RING as u64) as usize) else { break };
            let (frame, states, flags, data, state_obj) = (
                e.frame.load(Ordering::Relaxed),
                e.states.load(Ordering::Relaxed),
                e.flags.load(Ordering::Relaxed),
                e.data.load(Ordering::Relaxed),
                e.state_obj.load(Ordering::Relaxed),
            );
            fence(Ordering::Acquire);
            let overwritten = WRITTEN.load(Ordering::Relaxed) - read > keep;
            read += 1;
            if overwritten {
                continue;
            }
            let from = AppState::unpack(states >> 16);
            let to = AppState::unpack(states & 0xFFFF);
            let mut line = format!(
                "state: {} -> {} (frame {frame}, data {data:#x})",
                game.state_name(from),
                game.state_name(to)
            );
            if to == AppState::UNKNOWN {
                line.push_str(&format!(", state object {state_obj:#x} is not in the table"));
            }
            if flags & FLAG_FIRST != 0 {
                line.push_str(", first state seen");
            }
            log.info(&line);
            if flags & FLAG_BOOTED != 0 {
                let late = flags & FLAG_FIRST != 0 && game.state_name(to) != BOOTED_STATE;
                log.info(&format!(
                    "lifecycle: fully booted (state {}, frame {frame}{})",
                    game.state_name(to),
                    if late { ", hook landed after boot" } else { "" }
                ));
                log.info(&dispatch::summary());
            }
        }
        if report_at.elapsed() >= REPORT_EVERY {
            report_at = Instant::now();
            for line in dispatch::overrun_report() {
                log.info(&line);
            }
        }
        let every = if rate_lines < RATE_EARLY_LINES { RATE_EVERY } else { RATE_LATER };
        if rate_at.elapsed() >= every {
            rate_lines += 1;
            let now = frame();
            let secs = rate_at.elapsed().as_secs_f64();
            log.info(&format!(
                "lifecycle: {} frames in {:.0} s ({:.1} per second), state {}",
                now - rate_frames,
                secs,
                (now - rate_frames) as f64 / secs,
                game.state_name(game.state())
            ));
            rate_frames = now;
            rate_at = Instant::now();
        }
    }
}
