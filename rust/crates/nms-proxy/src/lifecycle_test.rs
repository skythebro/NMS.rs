//! Dev-only (`selftest` feature): an automatic in-game check of the lifecycle callbacks (L1.3).
//! Nothing to click: it registers every kind of callback at boot, exercises a live toggle, a
//! deliberately slow callback and a posted job, then writes one verdict line to `core.log`.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use nms_core::game::dispatch::{self, SlotRef, StateChange, Tick, TickPhase};
use nms_core::game::live;
use nms_core::log::Log;

/// Frames after fully booted at which each step runs.
const SLOW_AT: u64 = 300;
const SLOW_FRAMES: u64 = 5;
const TOGGLE_AT: u64 = 600;
const CHECK_AT: u64 = 1200;

static PRE: AtomicU64 = AtomicU64::new(0);
static POST: AtomicU64 = AtomicU64::new(0);
static STATES: AtomicU64 = AtomicU64::new(0);
static STATES_OFF_MAIN: AtomicU64 = AtomicU64::new(0);
static BOOTS: AtomicU64 = AtomicU64::new(0);
static BOOT_FRAME: AtomicU64 = AtomicU64::new(0);
static BOOT_ON_MAIN: AtomicBool = AtomicBool::new(false);
static JOB_FRAME: AtomicU64 = AtomicU64::new(0);
static JOB_ON_MAIN: AtomicBool = AtomicBool::new(false);
static PRE_AT_TOGGLE: AtomicU64 = AtomicU64::new(0);
static PRE_AT_CHECK: AtomicU64 = AtomicU64::new(u64::MAX);
static DT_BAD: AtomicU64 = AtomicU64::new(0);
static PRE_SLOT: OnceLock<SlotRef> = OnceLock::new();

fn pre(_: &Tick) {
    PRE.fetch_add(1, Ordering::Relaxed);
}

fn post(t: &Tick) {
    POST.fetch_add(1, Ordering::Relaxed);
    let boot = BOOT_FRAME.load(Ordering::Relaxed);
    if boot == 0 {
        return; // loading frames can be long; only judge frame times after boot
    }
    if !(0.0..30.0).contains(&t.dt) {
        DT_BAD.fetch_add(1, Ordering::Relaxed);
    }
    let since = t.frame.saturating_sub(boot);
    if (SLOW_AT..SLOW_AT + SLOW_FRAMES).contains(&since) {
        let start = Instant::now();
        while start.elapsed() < Duration::from_millis(3) {
            std::hint::spin_loop();
        }
    }
    if since == TOGGLE_AT {
        if let Some(s) = PRE_SLOT.get() {
            s.set_enabled(false);
            PRE_AT_TOGGLE.store(PRE.load(Ordering::Relaxed), Ordering::Relaxed);
        }
    }
    if since == CHECK_AT {
        PRE_AT_CHECK.store(PRE.load(Ordering::Relaxed), Ordering::Relaxed);
    }
}

fn state(_: &StateChange) {
    STATES.fetch_add(1, Ordering::Relaxed);
    if !live::is_main_thread() {
        STATES_OFF_MAIN.fetch_add(1, Ordering::Relaxed);
    }
}

fn booted(t: &Tick) {
    BOOTS.fetch_add(1, Ordering::Relaxed);
    BOOT_FRAME.store(t.frame.max(1), Ordering::Relaxed);
    BOOT_ON_MAIN.store(live::is_main_thread(), Ordering::Relaxed);
}

/// Register the callbacks and start the reporter. Call after the lifecycle hook is installed.
pub fn start(log: Arc<Log>) {
    let reg = (|| -> Result<(), String> {
        let _ = PRE_SLOT.set(dispatch::on_tick("lifecycle self-test", TickPhase::Pre, pre)?);
        dispatch::on_tick("lifecycle self-test", TickPhase::Post, post)?;
        dispatch::on_state_change("lifecycle self-test", state)?;
        dispatch::on_fully_booted("lifecycle self-test", booted)?;
        Ok(())
    })();
    if let Err(e) = reg {
        log.info(&format!("lifecycle self-test: could not register: {e}"));
        return;
    }
    dispatch::post(
        "lifecycle self-test",
        Box::new(|| {
            JOB_FRAME.store(live::frame(), Ordering::Relaxed);
            JOB_ON_MAIN.store(live::is_main_thread(), Ordering::Relaxed);
        }),
    );
    log.info(&format!("lifecycle self-test: registered (verdict {CHECK_AT} frames after the main menu appears)"));
    let _ = std::thread::Builder::new().name("nms-rs lifecycle self-test".into()).spawn(move || report(log));
}

fn report(log: Arc<Log>) {
    let started = Instant::now();
    loop {
        std::thread::sleep(Duration::from_millis(500));
        if PRE_AT_CHECK.load(Ordering::Relaxed) != u64::MAX {
            break;
        }
        // Only give up if frames stopped coming; a slow machine just takes longer.
        if started.elapsed() > Duration::from_secs(3600) {
            log.info(&format!(
                "lifecycle self-test: no verdict after an hour (frame {}, booted at {})",
                live::frame(),
                BOOT_FRAME.load(Ordering::Relaxed)
            ));
            return;
        }
    }
    let toggle_held = PRE_AT_CHECK.load(Ordering::Relaxed) == PRE_AT_TOGGLE.load(Ordering::Relaxed);
    let checks = [
        ("ticks before and after the update", PRE.load(Ordering::Relaxed) > 0 && POST.load(Ordering::Relaxed) > 0),
        ("state changes on the main thread", STATES.load(Ordering::Relaxed) > 0 && STATES_OFF_MAIN.load(Ordering::Relaxed) == 0),
        ("fully booted once, on the main thread", BOOTS.load(Ordering::Relaxed) == 1 && BOOT_ON_MAIN.load(Ordering::Relaxed)),
        ("posted job ran on the main thread", JOB_FRAME.load(Ordering::Relaxed) > 0 && JOB_ON_MAIN.load(Ordering::Relaxed)),
        ("live toggle stops a callback", toggle_held),
        ("frame times sane", DT_BAD.load(Ordering::Relaxed) == 0),
    ];
    let failed: Vec<&str> = checks.iter().filter(|(_, ok)| !ok).map(|(name, _)| *name).collect();
    log.info(&format!(
        "lifecycle self-test: {} (pre {}, post {}, state changes {}, booted at frame {}, job at frame {}){}",
        if failed.is_empty() { "PASS" } else { "FAIL" },
        PRE.load(Ordering::Relaxed),
        POST.load(Ordering::Relaxed),
        STATES.load(Ordering::Relaxed),
        BOOT_FRAME.load(Ordering::Relaxed),
        JOB_FRAME.load(Ordering::Relaxed),
        if failed.is_empty() { String::new() } else { format!("; failed: {}", failed.join(", ")) }
    ));
    log.info("lifecycle self-test: expect one 'took up to ... ms' line for the deliberately slow callback");
}
