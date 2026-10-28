//! PickerTweaks: search the item pickers and type an amount.
//!
//! One hotkey (default Ctrl+F) opens the game's own text box (the one that renames a storage
//! container) for whatever is active, in this order:
//! 1. `typed_quantity`: a held stack (stack split, machine slots, Transfer picker amount).
//! 2. `picker_search`: an open item picker (refiner, Nutrient Processor, Transfer Items, fossil) or
//!    the inventory craft list (empty slot, Craft).
//! 3. `typed_quantity`: the refiner amount.
//!
//! The hotkey is polled in the after-hook of the shared text box `Update` (`text_input_update`,
//! once per frame on the game thread), not from the picker popup `0x1406C8B80`: that popup and its slot
//! filler are hooked by CraftFromStorage as plain hooks, and the picker is detected through the
//! predicate this mod hooks anyway (it runs every frame while a picker is open).
//!
//! While our box is open over a page with no popup, the page ignores the mouse (`[page+0x14C84] = 0`);
//! the byte is put back when the box has closed and game input is back (300 ms, as the Mods
//! page does). With a popup up (the item picker) the byte is left alone: the page host hides the
//! popup for good when it reads 0 (`search::popup_up`).
//!
//! While our box is open and for a hold after it (until the end keys are up), the page and the
//! picker do not see button presses: `shield` clears the frame's button states and consumes the
//! action table's presses after the box has read them. That keeps pad A and B (`Fe_Select` and
//! `Fe_Back`, not in the action set the box switches off) from also clicking a tab or closing the
//! picker. The hold also keeps action set 1 off, but never sets the controller's "text entry active"
//! byte (`ctl+0x40`): that byte belongs to an open text box. The "Pad guard" switch turns the
//! shield and the hold's action-set switch off live (to bisect). Up to build 181442 the frontend
//! snapped its cursor to the mouse device position when the box opened and closed (183791 no longer does, so `cursor`
//! does not find its anchor there and stays off); `cursor` saves the frontend cursor
//! and the device position at box open and writes them back after the box (both input modes), the
//! device through its own setter, and GLFW by its cursor mode: its virtual cursor in disabled mode,
//! else `_glfwPlatformSetCursorPos`, which moves the OS cursor with `SetCursorPos`. Nothing
//! else here moves the OS cursor.
//!
//! With no box of ours open and no hold running, every hook here is a pure pass-through: nothing is
//! written and nothing is called on the game (only reads, for the hotkey and an input-mode log line).
//! The one code patch, the craft list's three recipe-add calls (`craft`), exists only while
//! "Search item pickers" is on: applied and removed on the game thread, original bytes back when off;
//! with no craft search set its filter calls the original at once.
//!
//! All boxes open empty (the search box too: each Ctrl+F is a new search). An empty Accept (Enter,
//! left click or pad A seen on the closing frame) uses the largest amount the control allows, or
//! clears the search; Cancel (Esc, right click, pad B, or no key seen) changes nothing
//! (`text_box::KeyWatch`).
//!
//! Both switches are live and off by default. A missing signature leaves that part vanilla.

mod amount;
mod craft;
mod cursor;
mod search;
mod shield;
mod text_box;

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use nms_core::hook::Hook;
use nms_core::log::Log;
use nms_core::mem::{read_i32, read_u8, MIN_ADDR};
use nms_core::process::ProcessImage;
use nms_core::{paths, Code, GameMod, Kind, Pattern, Range, SettingSpec, Settings};
use nms_sdk::sigs::picker_tweaks as sigs;
use serde_json::{json, Value};

use crate::common::hooks::text_input_update;
use crate::common::input::{async_key_down, face_button_a_xinput};

const NAME: &str = "PickerTweaks";

/// Live switches, read by the detours.
pub(crate) static PICKER_SEARCH: AtomicBool = AtomicBool::new(false);
pub(crate) static TYPED_QTY: AtomicBool = AtomicBool::new(false);
pub(crate) static LOG_NAMES: AtomicBool = AtomicBool::new(false);
/// "Pad guard": the shield and the hold's action-set switch (on by default).
static SHIELD_ON: AtomicBool = AtomicBool::new(true);
static CURSOR_READY: AtomicBool = AtomicBool::new(false);
/// Index into [`KEYS`].
static KEY: AtomicU8 = AtomicU8::new(0);
/// Which parts found their game code.
static SEARCH_READY: AtomicBool = AtomicBool::new(false);
static SPLIT_READY: AtomicBool = AtomicBool::new(false);
static REFINER_READY: AtomicBool = AtomicBool::new(false);
static SHIELD_READY: AtomicBool = AtomicBool::new(false);
static LOG: OnceLock<Arc<Log>> = OnceLock::new();
/// Game language for the box titles.
static LANG: OnceLock<&'static str> = OnceLock::new();

/// `(setting value, label)`; the setting stores the key.
const KEYS: &[(&str, &str)] = &[("ctrl_f", "Ctrl+F"), ("f3", "F3"), ("f4", "F4"), ("none", "None")];
const VK_CONTROL: i32 = 0x11;
const VK_F: i32 = 0x46;
const VK_F3: i32 = 0x72;
const VK_F4: i32 = 0x73;
const VK_LBUTTON: i32 = 0x01;
const VK_RBUTTON: i32 = 0x02;
const VK_RETURN: i32 = 0x0D;
const VK_ESCAPE: i32 = 0x1B;

/// The picker object's "accepts input" byte: hover and click tests bail out while it is 0 (`Confirm`
/// `0x140901CF0` and the popup host `0x140901604` read it; `0x14C44` before build 183791).
const PAGE_INPUT: usize = 0x14C84;
/// A control counts as on screen when its hook ran this recently.
const ACTIVE_MS: u64 = 300;
/// Game input stays off this long after our box closed, so the key that closed it does not also
/// reach the page (Esc closing the picker, Enter on a card).
const HOLD_MS: u64 = 300;
/// The hold never lasts longer than this, even with a key that reads as stuck.
const HOLD_MAX_MS: u64 = 3000;
/// The cursor is watched (and logged every frame) this long after the hold, for a late snap.
const CURSOR_SETTLE_MS: u64 = 800;
/// Cursor log lines in the settle window: on change only, at most one per this many ms.
const CURSOR_LOG_GAP_MS: u64 = 50;
/// The game's input mode is sampled (read only) this often for the log.
const INPUT_SAMPLE_MS: u64 = 1000;
/// A box left open this long is closed for the player (the page went away under it).
const BOX_MAX_MS: u64 = 600_000;

const SPECS: &[SettingSpec] = &[
    SettingSpec { key: "status", label: "Status", kind: Kind::Status },
    SettingSpec { key: "picker_search", label: "Search item pickers", kind: Kind::Bool },
    SettingSpec { key: "typed_quantity", label: "Type an amount", kind: Kind::Bool },
    SettingSpec { key: "search_key", label: "Hotkey", kind: Kind::Enum },
    SettingSpec { key: "shield", label: "Pad guard under the box", kind: Kind::Bool },
    SettingSpec { key: "log_names", label: "Log item names", kind: Kind::Bool },
    SettingSpec { key: "write_logs", label: "Write log", kind: Kind::Bool },
];

pub(crate) fn log_line(msg: &str) {
    if let Some(log) = LOG.get() {
        log.info(msg);
    }
}

pub(crate) fn now_ms() -> u64 {
    static CLOCK: OnceLock<std::time::Instant> = OnceLock::new();
    CLOCK.get_or_init(std::time::Instant::now).elapsed().as_millis() as u64 + 1
}

/// Box titles in the game's language, English when the string is not there yet.
fn title(id: &str, english: &str) -> String {
    let lang = LANG.get().copied().unwrap_or("en");
    let text = crate::common::menu_text::ui(lang, id);
    if text.is_empty() { english.to_string() } else { text.to_string() }
}

fn key_down(index: u8) -> bool {
    match index {
        0 => async_key_down(VK_CONTROL) && async_key_down(VK_F),
        1 => async_key_down(VK_F3),
        2 => async_key_down(VK_F4),
        _ => false,
    }
}

// ---- the text box and the hotkey (main thread, once per frame) -------------------------------

#[derive(Clone, Copy, Debug)]
enum Target {
    Search { popup: usize, kind: i32, mode: i32 },
    Split { page: usize, inv: usize, x: i32, y: i32 },
    Refiner { unit: usize },
}

struct OpenBox {
    target: Target,
    started_ms: u64,
    /// The page whose input byte we cleared, and the value it had.
    page: usize,
    input_saved: Option<u8>,
}

struct Runtime {
    open: Option<OpenBox>,
    key_was_down: bool,
    /// Hold after the box: at least until `hold_until`, at most until `hold_cap`. 0 when none.
    hold_until: u64,
    hold_cap: u64,
    /// Input byte to put back once the hold is over: `(page, value)`.
    restore: Option<(usize, u8)>,
    /// Keys that end the box, sampled while it is open.
    keys: text_box::KeyWatch,
    /// The game's cursor at box open, put back after the box until `cursor_until` (0: off).
    cursor: cursor::Settle,
    cursor_saved: Option<cursor::Snapshot>,
    cursor_seen: Option<cursor::Snapshot>,
    cursor_until: u64,
    cursor_log_ms: u64,
    /// We switched action set 1 off for the hold and still owe switching it on.
    sets_off: bool,
    /// The shield failed for this box (unreadable table): it stays off until the next box.
    shield_failed: bool,
    /// Values the shield wrote for the current box and its hold (for the log).
    shield_writes: usize,
    /// Read-only input-mode sampling for the log.
    input_seen: Option<shield::InputState>,
    input_sample_ms: u64,
}

static RT: Mutex<Runtime> = Mutex::new(Runtime {
    open: None,
    key_was_down: false,
    hold_until: 0,
    hold_cap: 0,
    restore: None,
    keys: text_box::KEY_WATCH_IDLE,
    cursor: cursor::Settle::IDLE,
    cursor_saved: None,
    cursor_seen: None,
    cursor_until: 0,
    cursor_log_ms: 0,
    sets_off: false,
    shield_failed: false,
    shield_writes: 0,
    input_seen: None,
    input_sample_ms: 0,
});

fn shield_active() -> bool {
    SHIELD_ON.load(Ordering::Acquire) && SHIELD_READY.load(Ordering::Acquire)
}

/// The hold is over: its minimum has passed and no end key is down, or its cap has passed.
fn hold_over(now: u64, until: u64, cap: u64, end_key_down: bool) -> bool {
    now >= cap || (now >= until && !end_key_down)
}

/// An end key is still down: Enter, Esc, a mouse button, pad A (XInput) or any end action in the
/// game's action table (that also covers Steam Input pads).
fn end_key_down(table: shield::EndKeys) -> bool {
    table.down
        || [VK_RETURN, VK_ESCAPE, VK_LBUTTON, VK_RBUTTON].iter().any(|&vk| async_key_down(vk))
        || face_button_a_xinput()
}

fn cursor_text(s: &cursor::Snapshot) -> String {
    format!(
        "ui {:.1},{:.1} vel {:.1},{:.1} last mouse {:.1},{:.1} device {:.1},{:.1} {}",
        s.ui.0, s.ui.1, s.vel.0, s.vel.1, s.last_mouse.0, s.last_mouse.1, s.dev.0, s.dev.1, s.glfw
    )
}

fn restore_input_byte(restore: Option<(usize, u8)>) {
    if let Some((page, value)) = restore {
        if read_u8(page + PAGE_INPUT) == Some(0) {
            // SAFETY: the page's own input byte; we only put back the value it had before our box.
            unsafe {
                let _ = nms_core::mem::write(page + PAGE_INPUT, value);
            }
        }
    }
}

impl Runtime {
    fn release_if_due(&mut self, now: u64, end_down: bool) {
        if self.hold_until != 0 && hold_over(now, self.hold_until, self.hold_cap, end_down) {
            self.end_hold("hold over");
        }
        if self.hold_until == 0 && self.restore.is_some() {
            restore_input_byte(self.restore.take());
        }
    }

    /// End the hold now and give back everything it took: action set 1 and the page input byte.
    /// Safe to call at any time; it is the one exit for every error path too.
    fn end_hold(&mut self, why: &str) {
        let was_holding = self.hold_until != 0;
        self.hold_until = 0;
        self.hold_cap = 0;
        if self.sets_off {
            self.sets_off = false;
            text_box::set_action_set(true);
        }
        restore_input_byte(self.restore.take());
        if was_holding {
            let state = shield::input_state(text_box::controller());
            log_line(&format!("input given back ({why}); shield wrote {} values; {state}", self.shield_writes));
        }
    }

    /// Our box is open or its hold is running.
    fn guarding(&self) -> bool {
        self.open.is_some() || self.hold_until != 0
    }

    /// From box open until `CURSOR_SETTLE_MS` after the hold: log the cursor (on change while the
    /// box is open, every frame after it), and write the box-open cursor back once after the box
    /// closes and again for a one-frame jump (`cursor::Settle`). Both input modes.
    fn watch_cursor(&mut self, now: u64, controller: Option<usize>) {
        let Some(saved) = self.cursor_saved else { return };
        if self.guarding() {
            self.cursor_until = now + CURSOR_SETTLE_MS;
        }
        if now > self.cursor_until || !CURSOR_READY.load(Ordering::Acquire) {
            log_line(&format!("cursor settle over: {} write(s)", self.cursor.writes()));
            self.cursor_until = 0;
            self.cursor_saved = None;
            self.cursor.reset();
            return;
        }
        let Some(snap) = cursor::snapshot(controller) else { return };
        let box_open = self.open.is_some();
        let changed = self.cursor_seen != Some(snap);
        if self.cursor_seen.map(|s| s.glfw.mode) != Some(snap.glfw.mode) {
            log_line(&format!("glfw cursor mode now {}", cursor::mode_name(snap.glfw.mode)));
        }
        // Verified 2026-10-13: only changes now, at most one line per 50 ms.
        if changed && now.saturating_sub(self.cursor_log_ms) >= CURSOR_LOG_GAP_MS {
            self.cursor_log_ms = now;
            log_line(&format!(
                "cursor{}{} {}",
                if box_open { " (box open)" } else { "" },
                if self.hold_until != 0 { " (hold)" } else { "" },
                cursor_text(&snap),
            ));
        }
        self.cursor_seen = Some(snap);
        if self.cursor.step(now, snap.ui, box_open) {
            let how = cursor::restore(controller, &saved);
            log_line(&format!(
                "cursor written back ({}, glfw part {how:?}): ui {:.1},{:.1} device {:.1},{:.1}",
                cursor::mode_name(snap.glfw.mode),
                saved.ui.0,
                saved.ui.1,
                saved.dev.0,
                saved.dev.1
            ));
            self.cursor_seen = cursor::snapshot(controller);
            if let Some(after) = self.cursor_seen {
                log_line(&format!("cursor after write: {}", cursor_text(&after)));
            }
        }
    }

    /// Read-only: log the game's input mode when it changes (once a second at most), so a log shows
    /// whether the game ever goes to pad mode and what the text-entry byte says.
    fn sample_input(&mut self, now: u64, controller: Option<usize>) {
        if now < self.input_sample_ms {
            return;
        }
        self.input_sample_ms = now + INPUT_SAMPLE_MS;
        let state = shield::input_state(controller);
        if self.input_seen != Some(state) {
            log_line(&format!("game input: {state}{}", if self.guarding() { " (our box or hold)" } else { "" }));
            self.input_seen = Some(state);
        }
    }

    fn hotkey(&mut self, mgr: usize, now: u64) {
        let fresh = |seen: u64| now.saturating_sub(seen) <= ACTIVE_MS;
        let typed = TYPED_QTY.load(Ordering::Acquire);
        // 1. A held stack.
        if typed && SPLIT_READY.load(Ordering::Acquire) && fresh(amount::HELD_SEEN_MS.load(Ordering::Acquire)) {
            let page = amount::HELD_PAGE.load(Ordering::Acquire);
            if let Some(h) = amount::held(page) {
                if let Some(max) = amount::element_amount(h.inv, h.x, h.y).filter(|&m| m > 0) {
                    let text = title("picker_amount_title", "Amount (max {n})").replace("{n}", &max.to_string());
                    let target = Target::Split { page, inv: h.inv, x: h.x, y: h.y };
                    self.open_box(mgr, now, &text, "", target, page);
                    return;
                }
            }
        }
        // 2. An open picker: its predicate ran recently and the page says the picker popup is up
        // and shown now (the last call of a closed picker still looks fresh).
        if PICKER_SEARCH.load(Ordering::Acquire)
            && SEARCH_READY.load(Ordering::Acquire)
            && fresh(search::SEEN_MS.load(Ordering::Acquire))
        {
            let popup = search::SEEN_POPUP.load(Ordering::Acquire);
            if let Some(kind) = search::searchable_kind(popup) {
                if search::is_craft(kind) && !craft::READY.load(Ordering::Acquire) {
                    log_line("hotkey: the craft list cannot be searched in this game version (see the start of the log)");
                    return;
                }
                let mode = read_i32(popup + search::POPUP_MODE).unwrap_or(-1);
                let text = title("picker_search_title", "Search (empty clears)");
                // Always empty: each Ctrl+F is a new search.
                self.open_box(mgr, now, &text, "", Target::Search { popup, kind, mode }, popup);
                return;
            }
        }
        // 3. The refiner amount.
        if typed && REFINER_READY.load(Ordering::Acquire) && fresh(amount::REFINER_SEEN_MS.load(Ordering::Acquire)) {
            let unit = amount::REFINER_UNIT.load(Ordering::Acquire);
            let page = amount::REFINER_PAGE.load(Ordering::Acquire);
            if amount::refiner_amount(unit).is_some_and(|a| a > 0) {
                let text = title("picker_refiner_title", "Amount");
                self.open_box(mgr, now, &text, "", Target::Refiner { unit }, page);
                return;
            }
        }
        log_line("hotkey: no item picker or amount on screen");
    }

    fn open_box(&mut self, mgr: usize, now: u64, title: &str, initial: &str, target: Target, page: usize) {
        if !text_box::idle(mgr) {
            log_line("text box not opened: the game's text box is busy");
            return;
        }
        // The game's cursor before anything moves it (the builder snaps it on the next frame).
        let controller = text_box::controller();
        let snap = CURSOR_READY.load(Ordering::Acquire).then(|| cursor::snapshot(controller)).flatten();
        if !text_box::start(mgr, title, initial) {
            log_line("text box not opened: the game did not open it");
            return;
        }
        // Never with a popup up: the host would hide it for good (see the module docs).
        let input_saved = (page >= MIN_ADDR && !search::popup_up(page)).then(|| read_u8(page + PAGE_INPUT)).flatten();
        if input_saved.is_some_and(|v| v != 0) {
            // SAFETY: the page's own input byte; restored when the box has closed.
            unsafe {
                let _ = nms_core::mem::write(page + PAGE_INPUT, 0u8);
            }
        }
        self.cursor.reset();
        self.cursor_saved = snap;
        if let Some(s) = snap {
            self.cursor.open(s.ui);
            self.cursor_until = now + CURSOR_SETTLE_MS;
            self.cursor_seen = None;
        }
        log_line(&format!(
            "text box opened for {target:?}{}; cursor {}; {}",
            if input_saved.is_some() { "" } else { " (page input left on)" },
            snap.map_or("unknown".to_string(), |s| cursor_text(&s)),
            shield::input_state(controller)
        ));
        self.keys = text_box::KeyWatch::default();
        self.open = Some(OpenBox { target, started_ms: now, page, input_saved });
        self.shield_failed = false;
        self.shield_writes = 0;
    }

    fn poll(&mut self, mgr: usize, now: u64) {
        let Some(open) = self.open.as_ref() else { return };
        match text_box::poll(mgr) {
            text_box::Poll::Waiting => {
                if now.saturating_sub(open.started_ms) > BOX_MAX_MS {
                    text_box::close(mgr);
                    self.finish(now, false);
                    log_line("text box closed: open too long");
                }
            }
            text_box::Poll::Gone => {
                self.finish(now, false);
                log_line("text box ended without a result");
            }
            text_box::Poll::Done(text) => {
                let target = open.target;
                let accepted = self.keys.empty_was_accept(now);
                self.finish(now, true);
                handle(target, ended(text, accepted));
            }
        }
    }

    /// Forget the box. With `hold`, run the hold a moment (shield, action set 1 off with the Pad
    /// guard on) and put the page's input byte back afterwards; otherwise put it back now.
    fn finish(&mut self, now: u64, hold: bool) {
        let Some(open) = self.open.take() else { return };
        let restore = open.input_saved.filter(|&v| v != 0).map(|v| (open.page, v));
        if hold {
            // Never `ctl+0x40` (text entry active): only the action set, and only with the guard on.
            self.sets_off = shield_active() && !self.shield_failed && text_box::set_action_set(false);
            self.hold_until = now + HOLD_MS;
            self.hold_cap = now + HOLD_MAX_MS;
            self.restore = restore;
        } else {
            self.end_hold("box ended");
            restore_input_byte(restore);
        }
    }
}

/// How the box ended, with an empty result split into Accept and Cancel.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Ended {
    Refused,
    Cancelled,
    Text(String),
}

fn ended(text: Option<String>, empty_was_accept: bool) -> Ended {
    match text {
        None => Ended::Refused,
        Some(t) if t.is_empty() && !empty_was_accept => Ended::Cancelled,
        Some(t) => Ended::Text(t),
    }
}

/// The amount to queue for a typed text: a number, or the control's maximum for an empty Accept
/// (`u32::MAX`, cut to the stack or refiner bounds when written). `None` leaves it unchanged.
fn typed_value(text: &str) -> Option<u32> {
    if text.is_empty() {
        Some(u32::MAX)
    } else {
        amount::parse(text)
    }
}

fn handle(target: Target, end: Ended) {
    let text = match end {
        Ended::Refused => {
            log_line("text box: the game refused the text");
            return;
        }
        Ended::Cancelled => {
            log_line("text box cancelled: nothing changed");
            return;
        }
        Ended::Text(text) => text,
    };
    match target {
        Target::Search { popup, kind, mode } => {
            if PICKER_SEARCH.load(Ordering::Acquire) {
                search::set(&text, popup, kind, mode);
            }
        }
        Target::Split { .. } | Target::Refiner { .. } if !TYPED_QTY.load(Ordering::Acquire) => {
            log_line("amount not used: Type an amount was switched off");
        }
        Target::Split { page, inv, x, y } => match typed_value(&text) {
            Some(value) => {
                amount::queue(amount::Write::Split { page, inv, x, y, value, until: now_ms() + amount::WRITE_WAIT_MS })
            }
            None => log_line(&format!("amount unchanged: \"{text}\" is not a whole number above 0")),
        },
        Target::Refiner { unit } => match typed_value(&text) {
            Some(value) => amount::queue(amount::Write::Refiner { unit, value, until: now_ms() + amount::WRITE_WAIT_MS }),
            None => log_line(&format!("amount unchanged: \"{text}\" is not a whole number above 0")),
        },
    }
}

/// Once per frame, after the game's text box step: finish our box or watch the hotkey.
fn on_update(args: &text_input_update::Args, _ret: u64) {
    let mgr = args.this as usize;
    if text_box::real_mgr() != Some(mgr) {
        return;
    }
    let now = now_ms();
    // The craft-list patch follows the switch; applied here, on the game thread, outside the list.
    craft::WANT.store(PICKER_SEARCH.load(Ordering::Acquire) && SEARCH_READY.load(Ordering::Acquire), Ordering::Release);
    craft::sync();
    search::expire(now);
    amount::expire(now);
    let Ok(mut rt) = RT.try_lock() else { return };
    let controller = text_box::controller();
    rt.sample_input(now, controller);
    // The Pad guard was switched off live: give back what the hold took, now.
    if rt.sets_off && !shield_active() {
        rt.sets_off = false;
        text_box::set_action_set(true);
        log_line("Pad guard off: action set 1 given back");
    }
    // The end keys as the game's action table has them this frame, read before the shield consumes
    // anything (the box has already asked for them inside `Update`).
    let table = if rt.guarding() { shield::end_keys(controller) } else { shield::EndKeys::default() };
    let end_down = rt.hold_until != 0 && end_key_down(table);
    rt.release_if_due(now, end_down);
    let key = KEY.load(Ordering::Acquire);
    let any = PICKER_SEARCH.load(Ordering::Acquire) || TYPED_QTY.load(Ordering::Acquire);
    let down = any && key_down(key);
    let edge = down && !rt.key_was_down;
    rt.key_was_down = down;
    if rt.open.is_some() {
        // Before GetResult: which end key is down on the closing frame (or was, for Esc, which
        // ends the box one frame after its release).
        let accept =
            table.accept || async_key_down(VK_RETURN) || async_key_down(VK_LBUTTON) || face_button_a_xinput();
        let cancel = table.cancel || async_key_down(VK_ESCAPE) || async_key_down(VK_RBUTTON);
        rt.keys.sample(now, accept, cancel);
        rt.poll(mgr, now);
    } else if edge && rt.hold_until == 0 {
        rt.hotkey(mgr, now);
    }
    // After the box has read its keys: hide every press from the page while OUR box is open (the
    // game reports it open, state 1) and during its hold. Never otherwise.
    let our_box_open = rt.open.is_some() && text_box::state(mgr) == Some(1);
    if shield_active() && !rt.shield_failed && (our_box_open || rt.hold_until != 0) {
        match shield::apply(controller, !our_box_open) {
            Ok(n) => rt.shield_writes = rt.shield_writes.saturating_add(n),
            Err(e) => {
                rt.shield_failed = true;
                log_line(&format!("shield stopped for this box: {e}"));
                rt.end_hold("shield error");
            }
        }
    }
    rt.watch_cursor(now, controller);
}

// ---- install ----------------------------------------------------------------------------------

fn find(image: &ProcessImage, name: &str, pattern: &str) -> Result<usize, String> {
    let pat = Pattern::parse(pattern).map_err(|e| format!("{name}: {e}"))?;
    image.find_unique(&pat).unique().ok_or_else(|| format!("{name} not found (game update?)"))
}

/// Install everything; each part reports whether it is ready. Returns the hooks to keep.
fn install(image: &ProcessImage, log: &Log) -> Result<(Vec<Hook>, Vec<&'static str>), String> {
    text_box::locate(image)?;
    match find(image, "text popup update", nms_sdk::sigs::ingame_options::TEXT_UPDATE).and_then(|u| shield::locate(image, u)) {
        Ok(()) => SHIELD_READY.store(true, Ordering::Release),
        Err(e) => log.info(&format!("input shield not ready, pad buttons may reach the page under the box: {e}")),
    }
    if let Err(e) = shield::locate_mode(image) {
        log.info(&format!("input mode not readable (log and pad check off): {e}"));
    }
    match cursor::locate(image) {
        Ok(()) => CURSOR_READY.store(true, Ordering::Release),
        Err(e) => log.info(&format!("cursor not put back after the box: {e}")),
    }
    // First, so a failure here leaves nothing installed (a dropped hook would unhook live).
    text_input_update::subscribe(image, nms_sdk::sigs::ingame_options::TEXT_UPDATE, None, Some(on_update))?;
    let mut hooks = Vec::new();
    let mut ready = Vec::new();
    let mut put = |name: &'static str, rva: usize, detour: *const (), orig: &std::sync::atomic::AtomicUsize| -> Result<(), String> {
        // SAFETY: each pattern is a function start and each detour declares the game's arguments
        // (predicate: 4 registers; held tick: rcx and xmm1; refiner page: rcx, rdx; no stack ones).
        hooks.push(unsafe { Hook::install(name, image.va(rva), detour, orig) }?);
        Ok(())
    };

    // Search: the predicate, plus the name lookups (optional).
    let search_part = || -> Result<usize, String> { find(image, "picker predicate", sigs::PREDICATE) };
    let held_part = || -> Result<usize, String> { find(image, "held stack tick", sigs::HELD_TICK) };
    match search_part() {
        Ok(rva) => match put("PickerPredicate", rva, search::pred_detour as *const (), &search::ORIG_PRED) {
            Ok(()) => {
                SEARCH_READY.store(true, Ordering::Release);
                ready.push("search");
                // Located here, but nothing is written: `craft::sync` applies the patch on the
                // game thread while "Search item pickers" is on, and puts the bytes back when
                // it is off.
                if let Err(e) = craft::init(image) {
                    log.info(&format!("craft list search not available, the hotkey does nothing there: {e}"));
                }
                let lookups = || -> Result<search::Lookups, String> {
                    Ok(search::Lookups {
                        product: image.va(find(image, "product lookup", sigs::PRODUCT_LOOKUP)?),
                        substance: image.va(find(image, "substance lookup", sigs::SUBSTANCE_LOOKUP)?),
                        loc: image.va(find(image, "loc lookup", sigs::LOC_LOOKUP)?),
                        loc_table: image.va(find(image, "loc table", sigs::LOC_TABLE)?),
                        tech: find(image, "tech lookup", sigs::TECH_LOOKUP).map(|r| image.va(r)).unwrap_or(0),
                    })
                };
                match lookups() {
                    Ok(l) => search::set_lookups(l),
                    Err(e) => log.info(&format!("search matches item ids only: {e}")),
                }
            }
            Err(e) => log.info(&format!("search not installed: {e}")),
        },
        Err(e) => log.info(&format!("search not installed: {e}")),
    }
    // Typed amount: the held stack.
    match held_part().and_then(|rva| put("HeldStackTick", rva, amount::held_detour as *const (), &amount::ORIG_HELD)) {
        Ok(()) => {
            SPLIT_READY.store(true, Ordering::Release);
            ready.push("split amount");
        }
        Err(e) => log.info(&format!("split amount not installed: {e}")),
    }

    // Typed amount: the refiner amount.
    let refiner = || -> Result<usize, String> {
        let page = find(image, "refiner page", sigs::REFINER_PAGE)?;
        let mov = image.read(page + sigs::GETTER_MOV_AT, 3).ok_or("refiner page unreadable")?;
        let call = image.read(page + sigs::GETTER_CALL_AT, 5).ok_or("refiner page unreadable")?;
        if mov[..] != sigs::GETTER_MOV || call.first() != Some(&0xE8) || call.len() < 5 {
            return Err("refiner page changed (game update?)".into());
        }
        let disp = i32::from_le_bytes([call[1], call[2], call[3], call[4]]);
        let getter = (image.va(page + sigs::GETTER_CALL_AT + 5) as i64 + i64::from(disp)) as usize;
        amount::set_refiner_fns(amount::RefinerFns {
            getter,
            min: image.va(find(image, "refiner min", sigs::REFINER_MIN)?),
            extra: image.va(find(image, "refiner max", sigs::REFINER_MAX)?),
            setter: image.va(find(image, "refiner setter", sigs::REFINER_SETTER)?),
        });
        Ok(page)
    };
    match refiner().and_then(|rva| put("RefinerPage", rva, amount::refiner_detour as *const (), &amount::ORIG_REFINER)) {
        Ok(()) => {
            REFINER_READY.store(true, Ordering::Release);
            ready.push("refiner amount");
        }
        Err(e) => log.info(&format!("refiner amount not installed: {e}")),
    }

    if ready.is_empty() {
        return Err("no part found its game code".into());
    }
    Ok((hooks, ready))
}

// ---- the mod ----------------------------------------------------------------------------------

pub struct PickerTweaks {
    settings: Arc<Mutex<Settings>>,
    status: Arc<Mutex<String>>,
    log: Arc<Log>,
    ready: bool,
    _hooks: Vec<Hook>,
}

fn key_index(value: &str) -> Option<u8> {
    KEYS.iter().position(|(k, _)| *k == value).map(|i| i as u8)
}

impl PickerTweaks {
    pub fn new(ctx: &super::Ctx) -> Self {
        let dir = paths::mod_dir(&ctx.root, NAME);
        let settings = Settings::load(
            &dir.join("picker_tweaks.json"),
            json!({
                "picker_search": false,
                "typed_quantity": false,
                "search_key": "ctrl_f",
                "shield": true,
                "log_names": false,
                "write_logs": false,
            }),
        );
        PICKER_SEARCH.store(settings.is_true("picker_search"), Ordering::Release);
        SHIELD_ON.store(settings.is_not_false("shield"), Ordering::Release);
        TYPED_QTY.store(settings.is_true("typed_quantity"), Ordering::Release);
        LOG_NAMES.store(settings.is_true("log_names"), Ordering::Release);
        let key = settings.get("search_key").and_then(Value::as_str).and_then(key_index).unwrap_or(0);
        KEY.store(key, Ordering::Release);
        let log = Arc::new(Log::open(
            &dir.join("picker_tweaks.log"),
            settings.is_true("write_logs") || settings.is_true("log_names"),
        ));
        let _ = LOG.set(log.clone());
        let _ = LANG.set(crate::common::lang::detect(&ctx.image, &log));

        let (hooks, status, ready) = match install(&ctx.image, &log) {
            Ok((hooks, parts)) => {
                let line = format!("On: {} ready", parts.join(", "));
                log.info(&format!("Picker tweaks: {line}"));
                (hooks, line, true)
            }
            Err(e) => {
                log.info(&format!("Picker tweaks not changed: {e}"));
                (Vec::new(), "Off, see log".to_string(), false)
            }
        };
        Self {
            settings: Arc::new(Mutex::new(settings)),
            status: Arc::new(Mutex::new(status)),
            log,
            ready,
            _hooks: hooks,
        }
    }

    fn sync_log(&self) {
        let on = self.settings.lock().map(|s| s.is_true("write_logs") || s.is_true("log_names")).unwrap_or(false);
        self.log.set_enabled(on);
    }
}

fn is_switch(key: &str) -> bool {
    matches!(key, "picker_search" | "typed_quantity" | "shield" | "log_names" | "write_logs")
}

impl GameMod for PickerTweaks {
    fn name(&self) -> &'static str {
        NAME
    }

    fn specs(&self) -> &'static [SettingSpec] {
        SPECS
    }

    fn range(&self, key: &str) -> Option<Range> {
        (key == "search_key").then_some(Range::Enum { choices: KEYS, default: 0 })
    }

    fn get(&self, key: &str) -> Option<Value> {
        match key {
            "status" => Some(json!(self.status.lock().ok()?.clone())),
            "search_key" => Some(json!(KEYS.get(KEY.load(Ordering::Acquire) as usize).map_or("ctrl_f", |(k, _)| *k))),
            _ if is_switch(key) => Some(json!(self.settings.lock().ok()?.is_true(key))),
            _ => None,
        }
    }

    fn default_value(&self, key: &str) -> Option<Value> {
        match key {
            "status" => None,
            _ => self.settings.lock().ok()?.default(key).cloned(),
        }
    }

    fn set(&self, key: &str, value: &Value) -> Result<String, String> {
        let label = SPECS.iter().find(|s| s.key == key).map_or(key, |s| s.label);
        if key == "search_key" {
            let choice = value.as_str().ok_or_else(|| "expected a key name".to_string())?;
            let index = key_index(choice).ok_or_else(|| format!("unknown key {choice:?}"))?;
            self.settings.lock().map_err(|_| "settings lock poisoned")?.set(key, json!(choice))?;
            KEY.store(index, Ordering::Release);
            return Ok(format!("{label} {}", KEYS[index as usize].1));
        }
        if !is_switch(key) {
            return Err(format!("unknown setting {key:?}"));
        }
        let on = value.as_bool().ok_or_else(|| "expected true or false".to_string())?;
        self.settings.lock().map_err(|_| "settings lock poisoned")?.set(key, json!(on))?;
        match key {
            "picker_search" => {
                PICKER_SEARCH.store(on, Ordering::Release);
                if !on {
                    search::clear();
                }
            }
            "typed_quantity" => {
                TYPED_QTY.store(on, Ordering::Release);
                if !on {
                    amount::clear();
                }
            }
            "log_names" => LOG_NAMES.store(on, Ordering::Release),
            // Off: the next frame gives back anything the hold took (`on_update`).
            "shield" => SHIELD_ON.store(on, Ordering::Release),
            _ => {}
        }
        self.sync_log();
        let state = if on { "on" } else { "off" };
        if key == "shield" && on && !SHIELD_READY.load(Ordering::Acquire) {
            return Err(format!("{label} cannot apply: game code not found (see the log)"));
        }
        if on && !self.ready && key != "write_logs" && key != "log_names" {
            return Err(format!("{label} cannot apply: game code not found (see the log)"));
        }
        Ok(format!("{label} {state}"))
    }
}

pub fn register(ctx: &super::Ctx) -> Box<dyn GameMod> {
    Box::new(PickerTweaks::new(ctx))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_key_choice_has_an_index() {
        for (i, (k, _)) in KEYS.iter().enumerate() {
            assert_eq!(key_index(k), Some(i as u8));
        }
        assert_eq!(key_index("ctrl_g"), None);
    }

    #[test]
    fn empty_accept_means_max_and_cancel_means_nothing() {
        assert_eq!(ended(Some(String::new()), false), Ended::Cancelled);
        assert_eq!(ended(Some(String::new()), true), Ended::Text(String::new()));
        assert_eq!(ended(Some("50".into()), false), Ended::Text("50".into()));
        assert_eq!(ended(None, true), Ended::Refused);
        assert_eq!(typed_value(""), Some(u32::MAX));
        assert_eq!(typed_value("50"), Some(50));
        assert_eq!(typed_value("0"), None);
        assert_eq!(typed_value("abc"), None);
    }

    #[test]
    fn the_hold_waits_for_the_end_keys_up_within_its_cap() {
        // Minimum not reached.
        assert!(!hold_over(100, 300, 3000, false));
        // Minimum reached, keys up: over.
        assert!(hold_over(300, 300, 3000, false));
        // Pad A still down: keep holding.
        assert!(!hold_over(900, 300, 3000, true));
        // Cap reached: over even with a key that reads as stuck.
        assert!(hold_over(3000, 300, 3000, true));
    }
}
