//! RefinerGuard: refuse to move or delete a refiner that still holds items.
//!
//! Measured in game (2026-10-13): moving a loaded refiner (base and
//! corvette) through `RemovePart` skips the game's refund and the slot 28 clear wipes the machine, so the
//! items are lost.
//!
//! One hook, on the entry of `RemovePart(tool, handle, mode) -> bool`. When the part being moved or deleted
//! is a refiner (or Nutrient Processor) whose machine holds any stack, the original is not called and the
//! hook does exactly what the game's own "refiner is running" refusal does (`0x1410D7FB0`): one
//! `AddTimedMessage` on-screen notice (not modal) and `return 0`. The two callers it refuses write nothing
//! on 0, so the build tool stays where it was, as after a vanilla refusal.
//!
//! The first in-game test soft-locked the build tool: that build opened a `ShowPopup` from inside the tool.
//! The popup was never drawn or answered there and kept the input. No popup is opened any more.
//!
//! Hard safety: the move is refused only when `RemovePart` was called by one of the two callers whose
//! handling of 0 was read in the disassembly (command 9 Move and command 2 `DeleteSelected`; both skip).
//! A naked entry stub passes the return address. Any other caller, or any read that fails, lets the vanilla
//! call run (logged).
//!
//! Finding the machine uses only the game's own pure lookups, the ones `RemovePart` makes itself:
//! `MaintLookup(handle, &comp)` (the call right before the slot 28 clear, reached through its call site),
//! then the refiner component in the same entity table, `COMP_GET(refiner type, [comp+8])`. The slot list is
//! the machine's store (`unit+0x50`: count `+0xDC`, data `+0xE0`, element `0x30`, amount `+0x18`, type
//! `+0x24`). Only substance and product stacks count: technology slots (type 1) are the machine's own, such
//! as the Portable Refiner's Carbon fuel charge `MAINT_FUEL1`, which made an empty Portable Refiner look
//! loaded in the second in-game test.
//!
//! The switch is live and on by default. Any missing signature or failed byte check leaves the game vanilla.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use nms_core::hook::Hook;
use nms_core::log::Log;
use nms_core::mem::{read_bytes, read_i32, read_ptr, read_u32, readable, MIN_ADDR};
use nms_core::process::ProcessImage;
use nms_core::{paths, Code, GameMod, Kind, Pattern, SettingSpec, Settings};
use nms_sdk::sigs::refiner_guard as sigs;
use serde_json::{json, Value};

const NAME: &str = "RefinerGuard";

/// Live switch, read by the detour.
static GUARD: AtomicBool = AtomicBool::new(true);
/// Set once every address below was found and checked.
static READY: AtomicBool = AtomicBool::new(false);
static ORIG_REMOVE: AtomicUsize = AtomicUsize::new(0);
static MAINT_LOOKUP: AtomicUsize = AtomicUsize::new(0);
static COMP_GET: AtomicUsize = AtomicUsize::new(0);
/// Address of the game's pointer to its application object.
static APP_GLOBAL: AtomicUsize = AtomicUsize::new(0);
/// Address of the refiner component type word (i16, -1 until the game fills it at start).
static TYPE_WORD: AtomicUsize = AtomicUsize::new(0);
/// The vetted callers: return addresses of the Move call and of the command loop's `DeleteSelected` call.
static MOVE_RET: AtomicUsize = AtomicUsize::new(0);
static DELETE_RET: AtomicUsize = AtomicUsize::new(0);
/// The vanilla refusal notice: `AddTimedMessage`, its release call, the message list offset in the
/// application object, the colour pointer, the audio id and the duration (f32 bits).
static NOTICE_ADD: AtomicUsize = AtomicUsize::new(0);
static NOTICE_RELEASE: AtomicUsize = AtomicUsize::new(0);
static NOTICE_OBJECT: AtomicUsize = AtomicUsize::new(0);
static NOTICE_COLOUR: AtomicUsize = AtomicUsize::new(0);
static NOTICE_AUDIO: AtomicUsize = AtomicUsize::new(0);
static NOTICE_DURATION: AtomicUsize = AtomicUsize::new(0);
static LOG: OnceLock<Arc<Log>> = OnceLock::new();
/// Game language for the notice.
static LANG: OnceLock<&'static str> = OnceLock::new();

type RemoveFn = unsafe extern "system" fn(usize, u32, i32) -> u8;
type LookupFn = unsafe extern "system" fn(u32, *mut usize) -> u32;
type CompGetFn = unsafe extern "system" fn(i32, usize) -> usize;
/// `AddTimedMessage(list, text, duration, colour, audio, timer, unused, f32, u8, u8, u8)` `0x1409D3FA0`: the
/// eleven arguments of the vanilla refusal call (and of QuickRecharge's notice). Build 183791 kept every
/// register and stack argument (`[rbp+0x260..0x290]`) where it was.
type AddMessageFn = unsafe extern "system" fn(usize, *const u8, f32, usize, u32, *mut u32, usize, f32, u8, u8, u8);
type ReleaseFn = unsafe extern "system" fn(*mut u32);

/// Build tool: the selected part's handle (the one Move and Delete pass), and three state words logged on
/// a refusal so a test can see the tool was left as vanilla leaves it.
const TOOL_SELECTED: usize = 0x7C8;
const TOOL_MODE: usize = 0x400;
const TOOL_PLACING: usize = 0x424;
const TOOL_STATE: usize = 0xB5BC;
/// Machine (refiner unit) layout.
const UNIT_TABLE: usize = 0x08;
const UNIT_COUNT: usize = 0xDC;
const UNIT_DATA: usize = 0xE0;
const ELEMENT_SIZE: usize = 0x30;
const ELEMENT_AMOUNT: usize = 0x18;
/// `cGcInventoryElement.Type`: 0 substance, 1 technology, 2 product.
const ELEMENT_TYPE: usize = 0x24;
/// The machine's own technology slots (`MAINT_REFINER`, `MAINT_COOKER`, and the Portable Refiner's fuel
/// charge `MAINT_FUEL1`, "Charge equipment with Carbon") are type 1. They never hold the player's items.
const TYPE_TECHNOLOGY: i32 = 1;
const MAX_SLOTS: i32 = 256;
/// The message list fields `AddTimedMessage` reads: `+0xD8`, `+0x2A4` count, `+0x2A8` data, and since build
/// 183791 a byte at `+0x8F8` that makes it return early (was `+0x28C` / `+0x290`, span `0x298`, before).
const LIST_SPAN: usize = 0x900;
/// The game copies the text into a 0x200-byte fixed string (`0x1401F7BE0`, `cmp rax,0x200`).
const TEXT_LEN: usize = 0x200;
const NOTICE_EN: &str = "Empty this refiner first: moving it deletes its contents.";

const SPECS: &[SettingSpec] = &[
    SettingSpec { key: "status", label: "Status", kind: Kind::Status },
    SettingSpec { key: "guard", label: "Guard loaded refiners", kind: Kind::Bool },
    SettingSpec { key: "write_logs", label: "Write log", kind: Kind::Bool },
];

fn log_line(msg: &str) {
    if let Some(log) = LOG.get() {
        log.info(msg);
    }
}

// ---- pure logic (unit tested) -----------------------------------------------------------------

/// A live entity handle: generation bits set and not the "none" index (the game's own test).
fn handle_ok(h: u32) -> bool {
    (h >> 19) != 0 && (h & 0x7FFFF) != 0x7FFFF
}

/// Item stacks (substance or product) that hold something, from `(amount, type)` pairs. Unused recipe
/// slots read -1. Technology slots are the machine's own: an empty base refiner has `MAINT_REFINER` amount
/// 0, a Portable Refiner also has its fuel charge `MAINT_FUEL1` with amount = the charge (95 as read
/// in game). Moving the machine loses no items there, so they never count.
fn held_stacks(slots: impl IntoIterator<Item = (i32, i32)>) -> usize {
    slots.into_iter().filter(|&(amount, kind)| amount > 0 && kind != TYPE_TECHNOLOGY).count()
}

/// The call is the Move (or Delete) of the selected part, the one place the guard looks.
fn guarded_call(guard_on: bool, mode: i32, handle: u32, selected: Option<u32>) -> bool {
    guard_on && mode == 0 && handle_ok(handle) && selected == Some(handle)
}

/// A caller whose handling of a 0 result was read in the disassembly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Caller {
    Move,
    Delete,
}

/// Which vetted caller a return address belongs to. A site that was not found (0) never matches.
fn caller_of(ret: usize, move_ret: usize, delete_ret: usize) -> Option<Caller> {
    if ret == 0 {
        None
    } else if ret == move_ret {
        Some(Caller::Move)
    } else if ret == delete_ret {
        Some(Caller::Delete)
    } else {
        None
    }
}

/// What the detour does with the selected part's machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Verdict {
    /// Nothing held: vanilla.
    Pass,
    /// Loaded, but the caller's handling of 0 was not read: vanilla, logged.
    PassUnvetted,
    /// Loaded and a vetted caller: notice and return 0.
    Refuse(Caller),
}

fn verdict(held: usize, caller: Option<Caller>) -> Verdict {
    match (held, caller) {
        (0, _) => Verdict::Pass,
        (_, Some(c)) => Verdict::Refuse(c),
        (_, None) => Verdict::PassUnvetted,
    }
}

// UTF-8 text in a zero-filled buffer, cut at a character boundary, always NUL-terminated.
use crate::common::menu_text::fixed;

/// Every `movsx ecx, word [rip+disp32]` target in `code` (which starts at `rva`).
fn type_word_targets(code: &[u8], rva: usize) -> Vec<usize> {
    let mut out = Vec::new();
    let mut i = 0;
    while i + 7 <= code.len() {
        if code[i..i + 3] == [0x0F, 0xBF, 0x0D] {
            let disp = i32::from_le_bytes([code[i + 3], code[i + 4], code[i + 5], code[i + 6]]);
            if let Ok(t) = usize::try_from((rva + i + 7) as i64 + i64::from(disp)) {
                out.push(t);
            }
            i += 7;
        } else {
            i += 1;
        }
    }
    out
}

/// RVAs of every `E8 rel32` in `code` (which starts at `rva`) whose target is `target`.
fn calls_to(code: &[u8], rva: usize, target: usize) -> Vec<usize> {
    (0..code.len().saturating_sub(4))
        .filter(|&i| code[i] == 0xE8)
        .filter(|&i| {
            let disp = i32::from_le_bytes([code[i + 1], code[i + 2], code[i + 3], code[i + 4]]);
            (rva + i + 5) as i64 + i64::from(disp) == target as i64
        })
        .map(|i| rva + i)
        .collect()
}

/// The notice in the game's language; English when the string is missing.
fn notice(lang: &str) -> String {
    let text = crate::common::menu_text::ui(lang, "refiner_guard_notice");
    if text.is_empty() { NOTICE_EN.to_string() } else { text.to_string() }
}

// ---- game reads ---------------------------------------------------------------------------------

/// The refiner unit of the entity and how many of its stacks hold items. `None`: no refiner there, or
/// something did not read (the guard then lets the game go on).
fn loaded_refiner(handle: u32) -> Option<(usize, usize)> {
    let (lookup, get, tw) =
        (MAINT_LOOKUP.load(Ordering::Acquire), COMP_GET.load(Ordering::Acquire), TYPE_WORD.load(Ordering::Acquire));
    if lookup == 0 || get == 0 || tw == 0 {
        return None;
    }
    let w = read_bytes(tw, 2)?;
    let ty = i16::from_le_bytes([w[0], w[1]]);
    if ty < 0 {
        return None;
    }
    let mut comp: usize = 0;
    // SAFETY: the game's own lookup, called by `RemovePart` with the same handle a moment later. Pure:
    // handle table reads, a hash find and the component getter; only `comp` and its stack are written.
    unsafe { std::mem::transmute::<usize, LookupFn>(lookup)(handle, &mut comp) };
    if comp < MIN_ADDR || !readable(comp, UNIT_TABLE + 8) {
        return None;
    }
    let table = read_ptr(comp + UNIT_TABLE).filter(|&t| readable(t, 0x40))?;
    // SAFETY: `0x142D95FC0(type, table)`: reads the table's bitset and component list, no stores. The game
    // calls it with `[unit+8]` of a maintenance component the same way (the maintenance getter
    // `0x141048CE0`, the refiner update at `0x1410D9FD7`).
    let unit = unsafe { std::mem::transmute::<usize, CompGetFn>(get)(i32::from(ty), table) };
    if unit < MIN_ADDR || !readable(unit, UNIT_DATA + 8) {
        return None;
    }
    let count = read_i32(unit + UNIT_COUNT).filter(|c| (0..=MAX_SLOTS).contains(c))?;
    if count == 0 {
        return Some((unit, 0));
    }
    let data = read_ptr(unit + UNIT_DATA)?;
    if !readable(data, count as usize * ELEMENT_SIZE) {
        return None;
    }
    // A type that does not read counts as an item (refusing is the safe side for the player's items).
    let slots = (0..count as usize).map(|i| {
        let e = data + i * ELEMENT_SIZE;
        (read_i32(e + ELEMENT_AMOUNT).unwrap_or(0), read_i32(e + ELEMENT_TYPE).unwrap_or(0))
    });
    Some((unit, held_stacks(slots)))
}

/// The game's on-screen message, called exactly as its "refiner is running" refusal calls it, with
/// our text. Not modal: it only appends to the message list. False when something did not read.
fn show_notice(text: &str) -> bool {
    let (add, release, global, object, colour) = (
        NOTICE_ADD.load(Ordering::Acquire),
        NOTICE_RELEASE.load(Ordering::Acquire),
        APP_GLOBAL.load(Ordering::Acquire),
        NOTICE_OBJECT.load(Ordering::Acquire),
        NOTICE_COLOUR.load(Ordering::Acquire),
    );
    if add == 0 || release == 0 || global == 0 || object == 0 || colour == 0 {
        return false;
    }
    let Some(app) = read_ptr(global).filter(|&a| a >= MIN_ADDR) else { return false };
    let list = app + object;
    if !readable(list, LIST_SPAN) || !readable(colour, 16) {
        return false;
    }
    let audio = NOTICE_AUDIO.load(Ordering::Acquire) as u32;
    let duration = f32::from_bits(NOTICE_DURATION.load(Ordering::Acquire) as u32);
    let body: [u8; TEXT_LEN] = fixed(text);
    let mut timer = [0u32; 4];
    // SAFETY: the vanilla refusal's own call: message list `app+object`, a NUL-terminated text the game
    // copies (at most 0x200 bytes), the duration and colour it passes, its audio id, a zeroed timer handle
    // that the release call then reads, 0.0 and three zero bytes. Vanilla's `0x1401AE230` calls around it
    // are `ret 0` and are left out.
    unsafe {
        let f = std::mem::transmute::<usize, AddMessageFn>(add);
        f(list, body.as_ptr(), duration, colour, audio, timer.as_mut_ptr(), 0, 0.0, 0, 0, 0);
        std::mem::transmute::<usize, ReleaseFn>(release)(timer.as_mut_ptr());
    }
    true
}

// ---- the detour ---------------------------------------------------------------------------------

/// Hook entry. `RemovePart` takes three register arguments; the stub hands the caller's return address to
/// [`remove_detour`] as a fourth one (`r9` is volatile and `RemovePart` does not read it) and jumps, so the
/// stack is untouched and `remove_detour` returns straight to the game's caller.
#[unsafe(naked)]
unsafe extern "system" fn remove_entry(_tool: usize, _handle: u32, _mode: i32) -> u8 {
    std::arch::naked_asm!("mov r9, [rsp]", "jmp {detour}", detour = sym remove_detour)
}

/// `RemovePart(tool, handle, mode) -> bool`, plus the caller's return address from [`remove_entry`].
unsafe extern "system" fn remove_detour(tool: usize, handle: u32, mode: i32, ret: usize) -> u8 {
    let selected = if tool >= MIN_ADDR { read_u32(tool + TOOL_SELECTED) } else { None };
    if READY.load(Ordering::Acquire) && guarded_call(GUARD.load(Ordering::Acquire), mode, handle, selected) {
        if let Some((unit, held)) = loaded_refiner(handle) {
            let caller = caller_of(ret, MOVE_RET.load(Ordering::Acquire), DELETE_RET.load(Ordering::Acquire));
            match verdict(held, caller) {
                Verdict::Pass => {}
                Verdict::PassUnvetted => log_line(&format!(
                    "not refused: handle {handle:#x} refiner {unit:#x} holds {held} stack(s), but caller {ret:#x} \
                     is not the Move or Delete call; vanilla runs"
                )),
                Verdict::Refuse(c) => {
                    let shown = show_notice(&notice(LANG.get().copied().unwrap_or("en")));
                    let word = |off: usize| read_u32(tool + off).map_or_else(|| "?".to_string(), |v| v.to_string());
                    log_line(&format!(
                        "refused {c:?}: handle {handle:#x} refiner {unit:#x} holds {held} stack(s); on-screen notice \
                         {}; tool +0x400={} +0x424={} +0xB5BC={} (not changed)",
                        if shown { "shown" } else { "not shown" },
                        word(TOOL_MODE),
                        word(TOOL_PLACING),
                        word(TOOL_STATE),
                    ));
                    return 0;
                }
            }
        }
    }
    let original: RemoveFn = std::mem::transmute(ORIG_REMOVE.load(Ordering::Acquire));
    original(tool, handle, mode)
}

// ---- install ------------------------------------------------------------------------------------

fn find<C: Code>(image: &C, name: &str, pattern: &str) -> Result<usize, String> {
    let pat = Pattern::parse(pattern).map_err(|e| format!("{name}: {e}"))?;
    image.find_unique(&pat).unique().ok_or_else(|| format!("{name} not found (game update?)"))
}

fn starts_with<C: Code>(image: &C, rva: usize, want: &[u8]) -> bool {
    image.read(rva, want.len()).is_some_and(|b| b == want)
}

fn read_u32_rva<C: Code>(image: &C, rva: usize) -> Option<u32> {
    image.read(rva, 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

/// Everything the mod needs, as RVAs (plus three plain values), found and checked without calling or
/// writing anything. Generic so the test can run it on `Binaries\NMS.exe`.
#[derive(Debug)]
struct Found {
    remove: usize,
    lookup: usize,
    get: usize,
    app_global: usize,
    type_word: usize,
    move_ret: usize,
    delete_ret: usize,
    notice: Notice,
}

#[derive(Debug)]
struct Notice {
    add: usize,
    release: usize,
    colour: usize,
    object: u32,
    audio: u32,
    duration: f32,
}

/// The two vetted callers' return addresses (RVAs): the Move call and the command loop's call of
/// `DeleteSelected`, which tail-jumps into `RemovePart`.
fn locate_callers<C: Code>(image: &C, remove: usize) -> Result<(usize, usize), String> {
    let site = find(image, "Move call", sigs::MOVE_SITE)?;
    if image.rel32_target(site + sigs::MOVE_SITE_CALL + 1) != Some(remove) {
        return Err("the Move call does not call RemovePart (game update?)".into());
    }
    let move_ret = site + sigs::MOVE_SITE_CALL + 5;

    let delete = find(image, "DeleteSelected", sigs::DELETE_SELECTED)?;
    let tail = delete + sigs::DELETE_TAIL_AT;
    if !starts_with(image, tail, &sigs::DELETE_TAIL)
        || image.rel32_target(tail + sigs::DELETE_TAIL.len()) != Some(remove)
    {
        return Err("DeleteSelected changed (game update?)".into());
    }
    let from = site.checked_sub(sigs::DELETE_CALL_WINDOW).ok_or("Move call too low")?;
    let window = image.read(from, sigs::DELETE_CALL_WINDOW).ok_or("command loop unreadable")?;
    let delete_ret = match calls_to(&window, from, delete).as_slice() {
        [one] => one + 5,
        other => return Err(format!("command loop: {} calls to DeleteSelected, want 1 (game update?)", other.len())),
    };
    Ok((move_ret, delete_ret))
}

/// The vanilla refusal notice: function, release, message list offset, colour, audio, duration.
fn locate_notice<C: Code>(image: &C, app_global: usize) -> Result<Notice, String> {
    let at = find(image, "refusal notice", sigs::REFUSAL_NOTICE)?;
    if image.rel32_target(at + sigs::NOTICE_APP_DISP) != Some(app_global) {
        return Err("refusal notice uses another application pointer (game update?)".into());
    }
    let add = image.rel32_target(at + sigs::NOTICE_ADD_CALL + 1).ok_or("AddTimedMessage unreadable")?;
    let release = image.rel32_target(at + sigs::NOTICE_RELEASE_CALL + 1).ok_or("release unreadable")?;
    let colour = image.rel32_target(at + sigs::NOTICE_COLOUR_DISP).ok_or("notice colour unreadable")?;
    let duration_at = image.rel32_target(at + sigs::NOTICE_DURATION_DISP).ok_or("notice duration unreadable")?;
    let duration = read_u32_rva(image, duration_at).ok_or("notice duration unreadable")?;
    let object = read_u32_rva(image, at + sigs::NOTICE_OBJECT_DISP).ok_or("message list unreadable")?;
    let audio = read_u32_rva(image, at + sigs::NOTICE_AUDIO_IMM).ok_or("notice audio unreadable")?;
    let secs = f32::from_bits(duration);
    if !(0.1..=30.0).contains(&secs) || object == 0 || object >= 0x0100_0000 {
        return Err(format!("refusal notice looks wrong (duration {secs}, list {object:#x}; game update?)"));
    }
    Ok(Notice { add, release, colour, object, audio, duration: secs })
}

/// Find and check every address. Nothing is called or written in the game here.
fn locate<C: Code>(image: &C) -> Result<Found, String> {
    let remove = find(image, "RemovePart", sigs::REMOVE_PART)?;

    // The machine lookup, through RemovePart's own call to it.
    let site = find(image, "machine lookup call", sigs::MAINT_SITE)?;
    let lookup = image.rel32_target(site + sigs::MAINT_SITE_CALL + 1).ok_or("machine lookup unreadable")?;
    if !starts_with(image, lookup, &sigs::MAINT_LOOKUP_HEAD) || !starts_with(image, lookup + sigs::MAINT_LOOKUP_GETTER_CALL, &[0xE8]) {
        return Err("machine lookup changed (game update?)".into());
    }
    let getter = image.rel32_target(lookup + sigs::MAINT_LOOKUP_GETTER_CALL + 1).ok_or("machine getter unreadable")?;
    if !starts_with(image, getter, &sigs::MAINT_GETTER_HEAD) {
        return Err("machine getter changed (game update?)".into());
    }

    // Application global and the refiner type word.
    let anchor = find(image, "refiner anchor", sigs::REFINER_ANCHOR)?;
    let app_global = image.rel32_target(anchor + sigs::ANCHOR_APP_DISP).ok_or("application pointer unreadable")?;
    let type_word = image.rel32_target(anchor + sigs::ANCHOR_TYPE_DISP).ok_or("refiner type unreadable")?;
    let getter_code = image.read(getter, sigs::MAINT_GETTER_SPAN).ok_or("machine getter unreadable")?;
    if !type_word_targets(&getter_code, getter).contains(&type_word) {
        return Err("the machine getter does not look for refiners (game update?)".into());
    }
    let get = find(image, "component get", sigs::COMP_GET)?;
    let (move_ret, delete_ret) = locate_callers(image, remove)?;
    let notice = locate_notice(image, app_global)?;
    Ok(Found { remove, lookup, get, app_global, type_word, move_ret, delete_ret, notice })
}

fn install(image: &ProcessImage) -> Result<Hook, String> {
    let f = locate(image)?;
    MAINT_LOOKUP.store(image.va(f.lookup), Ordering::Release);
    COMP_GET.store(image.va(f.get), Ordering::Release);
    APP_GLOBAL.store(image.va(f.app_global), Ordering::Release);
    TYPE_WORD.store(image.va(f.type_word), Ordering::Release);
    MOVE_RET.store(image.va(f.move_ret), Ordering::Release);
    DELETE_RET.store(image.va(f.delete_ret), Ordering::Release);
    NOTICE_ADD.store(image.va(f.notice.add), Ordering::Release);
    NOTICE_RELEASE.store(image.va(f.notice.release), Ordering::Release);
    NOTICE_COLOUR.store(image.va(f.notice.colour), Ordering::Release);
    NOTICE_OBJECT.store(f.notice.object as usize, Ordering::Release);
    NOTICE_AUDIO.store(f.notice.audio as usize, Ordering::Release);
    NOTICE_DURATION.store(f.notice.duration.to_bits() as usize, Ordering::Release);
    // SAFETY: the pattern is RemovePart's first byte. The entry stub keeps the stack and the three register
    // arguments as they are and adds the return address in r9 for the detour.
    let hook = unsafe { Hook::install("RemovePart", image.va(f.remove), remove_entry as *const (), &ORIG_REMOVE) }?;
    READY.store(true, Ordering::Release);
    Ok(hook)
}

// ---- the mod ------------------------------------------------------------------------------------

pub struct RefinerGuard {
    settings: Arc<Mutex<Settings>>,
    status: Arc<Mutex<String>>,
    log: Arc<Log>,
    ready: bool,
    _hook: Option<Hook>,
}

impl RefinerGuard {
    pub fn new(ctx: &super::Ctx) -> Self {
        let dir = paths::mod_dir(&ctx.root, NAME);
        let settings = Settings::load(&dir.join("refiner_guard.json"), json!({ "guard": true, "write_logs": false }));
        GUARD.store(settings.is_not_false("guard"), Ordering::Release);
        let log = Arc::new(Log::open(&dir.join("refiner_guard.log"), settings.is_true("write_logs")));
        let _ = LOG.set(log.clone());
        let _ = LANG.set(crate::common::lang::detect(&ctx.image, &log));

        let (hook, status, ready) = match install(&ctx.image) {
            Ok(hook) => {
                log.info(&format!(
                    "Refiner guard: On (callers Move {:#x}, Delete {:#x}; notice {:#x})",
                    MOVE_RET.load(Ordering::Acquire),
                    DELETE_RET.load(Ordering::Acquire),
                    NOTICE_ADD.load(Ordering::Acquire),
                ));
                (Some(hook), "On".to_string(), true)
            }
            Err(e) => {
                log.info(&format!("Refiner guard not installed: {e}"));
                ctx.log.info(&format!("RefinerGuard not installed: {e}"));
                (None, "Off, see log".to_string(), false)
            }
        };
        Self { settings: Arc::new(Mutex::new(settings)), status: Arc::new(Mutex::new(status)), log, ready, _hook: hook }
    }
}

impl GameMod for RefinerGuard {
    fn name(&self) -> &'static str {
        NAME
    }

    fn specs(&self) -> &'static [SettingSpec] {
        SPECS
    }

    fn get(&self, key: &str) -> Option<Value> {
        match key {
            "status" => Some(json!(self.status.lock().ok()?.clone())),
            "guard" => Some(json!(GUARD.load(Ordering::Acquire))),
            "write_logs" => Some(json!(self.settings.lock().ok()?.is_true(key))),
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
        if !matches!(key, "guard" | "write_logs") {
            return Err(format!("unknown setting {key:?}"));
        }
        let label = SPECS.iter().find(|s| s.key == key).map_or(key, |s| s.label);
        let on = value.as_bool().ok_or_else(|| "expected true or false".to_string())?;
        self.settings.lock().map_err(|_| "settings lock poisoned")?.set(key, json!(on))?;
        match key {
            "guard" => GUARD.store(on, Ordering::Release),
            _ => self.log.set_enabled(on),
        }
        let state = if on { "on" } else { "off" };
        if key == "guard" && on && !self.ready {
            return Err(format!("{label} cannot apply: game code not found (see the log)"));
        }
        Ok(format!("{label} {state}"))
    }
}

pub fn register(ctx: &super::Ctx) -> Box<dyn GameMod> {
    Box::new(RefinerGuard::new(ctx))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handles_follow_the_game_test() {
        assert!(handle_ok(0x3B79C6));
        assert!(handle_ok(0x3843AE));
        assert!(!handle_ok(0x7FFFF));
        assert!(!handle_ok(0x1234));
        assert!(!handle_ok((1 << 19) | 0x7FFFF));
    }

    #[test]
    fn only_positive_amounts_count() {
        // Read in game: [FUEL2 amt -1 type 0] [FUEL1 amt 188 type 0] is a loaded refiner; [MAINT_REFINER amt 0
        // type 1] is empty; corvette [JELLY amt -1 type 2] [LAUNCHSUB amt 100 type 0] is loaded.
        assert_eq!(held_stacks([(-1, 0), (188, 0)]), 1);
        assert_eq!(held_stacks([(0, 1)]), 0);
        assert_eq!(held_stacks([(-1, 2), (100, 0)]), 1);
        assert_eq!(held_stacks([]), 0);
        assert_eq!(held_stacks([(5, 0), (0, 0), (7, 2)]), 2);
    }

    #[test]
    fn portable_refiner_fuel_charge_is_not_an_item() {
        // Read in game, portable refiner: [MAINT_FUEL1 x=0 amt=95 type=1] [MAINT_REFINER x=1 amt=0
        // type=1]. Charged with Carbon but empty: must move as in vanilla.
        assert_eq!(held_stacks([(95, TYPE_TECHNOLOGY), (0, TYPE_TECHNOLOGY)]), 0);
        // The same machine with an input (substance) and an output (product) stack.
        assert_eq!(held_stacks([(95, TYPE_TECHNOLOGY), (0, TYPE_TECHNOLOGY), (40, 0), (3, 2)]), 2);
        // Other machines' maintenance technology (MAINT_TECH*, amt 50) never counts either.
        assert_eq!(held_stacks([(50, 1), (50, 1)]), 0);
    }

    #[test]
    fn guards_only_the_selected_part_in_mode_0() {
        let h = 0x3B79C6;
        assert!(guarded_call(true, 0, h, Some(h)));
        assert!(!guarded_call(false, 0, h, Some(h)));
        assert!(!guarded_call(true, 1, h, Some(h)));
        assert!(!guarded_call(true, 0, h, Some(0x3843AE)));
        assert!(!guarded_call(true, 0, h, None));
        assert!(!guarded_call(true, 0, 0x7FFFF, Some(0x7FFFF)));
    }

    #[test]
    fn only_the_two_vetted_callers_are_recognised() {
        let (mv, del) = (0x1403BD851, 0x1403BD695);
        assert_eq!(caller_of(mv, mv, del), Some(Caller::Move));
        assert_eq!(caller_of(del, mv, del), Some(Caller::Delete));
        // The other RemovePart callers (`0x1403CC4C3` reacts to 0 with its own code, `0x1403D9383`).
        assert_eq!(caller_of(0x1403CC4C3, mv, del), None);
        assert_eq!(caller_of(0x1403D9383, mv, del), None);
        // Sites that were not found never match, not even a zero return address.
        assert_eq!(caller_of(0, 0, 0), None);
        assert_eq!(caller_of(mv, 0, 0), None);
    }

    #[test]
    fn refuses_only_a_loaded_machine_from_a_vetted_caller() {
        assert_eq!(verdict(0, Some(Caller::Move)), Verdict::Pass);
        assert_eq!(verdict(0, None), Verdict::Pass);
        assert_eq!(verdict(1, Some(Caller::Move)), Verdict::Refuse(Caller::Move));
        assert_eq!(verdict(3, Some(Caller::Delete)), Verdict::Refuse(Caller::Delete));
        // Unknown caller: never refuse, vanilla runs (the hard safety against a stuck tool).
        assert_eq!(verdict(1, None), Verdict::PassUnvetted);
    }

    #[test]
    fn finds_calls_to_a_target() {
        // `mov rcx,rdi; call 0x1403CC910; jmp ..` at 0x1403BD68D (the command loop's Delete case).
        let rva = 0x1403BD68D;
        let mut code = vec![0x48, 0x8B, 0xCF, 0xE8];
        code.extend_from_slice(&0xF27Bi32.to_le_bytes());
        code.extend_from_slice(&[0xE9, 0x01, 0x03, 0x00, 0x00]);
        assert_eq!(calls_to(&code, rva, 0x1403CC910), vec![rva + 3]);
        assert!(calls_to(&code, rva, 0x1403CD3E0).is_empty());
        assert!(calls_to(&[0xE8, 0, 0], rva, rva + 5).is_empty());
        // A backward call (negative rel32).
        let back = [0xE8, 0xFB, 0xFF, 0xFF, 0xFF];
        assert_eq!(calls_to(&back, 0x1000, 0x1000), vec![0x1000]);
    }

    #[test]
    fn fixed_buffers_are_terminated_and_cut_on_char_boundaries() {
        let long: [u8; 8] = fixed("abcdefghij");
        assert_eq!(&long, b"abcdefg\0");
        // "é" is two bytes: a cut inside it drops the whole character.
        let cut: [u8; 4] = fixed("abé");
        assert_eq!(&cut, b"ab\0\0");
        let body: [u8; TEXT_LEN] = fixed(NOTICE_EN);
        assert_eq!(&body[..NOTICE_EN.len()], NOTICE_EN.as_bytes());
        assert_eq!(body[NOTICE_EN.len()], 0);
    }

    #[test]
    fn notice_fits_the_message_in_every_language() {
        for code in ["en", "de", "fr", "it", "es", "es-419", "ru", "pl", "nl", "pt", "pt-BR", "ja", "zh", "zh-Hant", "ko"] {
            let text = notice(code);
            assert!(!text.is_empty() && text.len() < TEXT_LEN, "{code}");
            assert!(!text.contains('\0'), "{code}");
        }
        assert_eq!(notice("en"), NOTICE_EN);
        assert_eq!(notice("xx"), NOTICE_EN);
    }

    #[test]
    fn finds_type_words_in_the_getter() {
        // `0x141027FC0`: push rbx; sub rsp,0x20; mov rbx,rcx; movsx ecx, word [rip+0x4087B10]; mov rdx,rbx;
        // call ..; then a second movsx with disp 0x4087E28 at +0x1D.
        let mut code = vec![0x40, 0x53, 0x48, 0x83, 0xEC, 0x20, 0x48, 0x8B, 0xD9, 0x0F, 0xBF, 0x0D];
        code.extend_from_slice(&0x4087B10i32.to_le_bytes());
        code.extend_from_slice(&[0x48, 0x8B, 0xD3, 0xE8, 0, 0, 0, 0, 0x48, 0x85, 0xC0, 0x75, 0x78]);
        code.extend_from_slice(&[0x0F, 0xBF, 0x0D]);
        code.extend_from_slice(&0x4087E28i32.to_le_bytes());
        let rva = 0x1027FC0;
        let found = type_word_targets(&code, rva);
        assert_eq!(found, vec![0x1027FC9 + 7 + 0x4087B10, rva + 0x1D + 7 + 0x4087E28]);
        assert_eq!(found[0], 0x50AFAE0);
    }

    /// Runs the whole `locate` on `Binaries\NMS.exe` when it is there (skipped elsewhere). Values are the
    /// ones read in the disassembly of builds 88,560,712 and 183791; another build only has to locate cleanly.
    #[test]
    fn locates_everything_in_the_game_exe() {
        let exe = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().find_map(|d| {
            let p = d.join("Binaries").join("NMS.exe");
            p.is_file().then_some(p)
        });
        let Some(exe) = exe else { return };
        let image = nms_core::image::FileImage::load(&exe).expect("exe loads");
        let f = locate(&image).expect("every address found and checked");
        if std::fs::metadata(&exe).map(|m| m.len()).ok() == Some(88_560_712) {
            assert_eq!(f.remove, 0x3CD3E0);
            assert_eq!(f.move_ret, 0x3BD851);
            assert_eq!(f.delete_ret, 0x3BD695);
            assert_eq!(f.app_global, 0x6E8D708);
            assert_eq!(f.type_word, 0x50AFA58);
            assert_eq!(f.notice.add, 0x9BF7B0);
            assert_eq!(f.notice.release, 0x2D65A80);
            assert_eq!(f.notice.object, 0x847BB0);
            assert_eq!(f.notice.audio, 0x75AC6211);
            assert_eq!(f.notice.duration, 1.0);
        }
        // Build 183791 (89,491,528 bytes).
        if std::fs::metadata(&exe).map(|m| m.len()).ok() == Some(89_491_528) {
            assert_eq!(f.remove, 0x3D50C0);
            assert_eq!(f.lookup, 0x3E3300);
            assert_eq!(f.get, 0x2D95FC0);
            assert_eq!(f.move_ret, 0x3C5771);
            assert_eq!(f.delete_ret, 0x3C55B5);
            assert_eq!(f.app_global, 0x6F60828);
            assert_eq!(f.type_word, 0x517F8D0);
            assert_eq!(f.notice.add, 0x9D3FA0);
            assert_eq!(f.notice.release, 0x2DD6870);
            assert_eq!(f.notice.object, 0x86E3D0);
            assert_eq!(f.notice.audio, 0x75AC6211);
            assert_eq!(f.notice.duration, 1.0);
        }
    }

    #[test]
    fn delete_tail_ends_in_a_jump() {
        assert_eq!(sigs::DELETE_TAIL.last(), Some(&0xE9));
        assert_eq!(sigs::MOVE_SITE_CALL, 12);
    }

    #[test]
    fn settings_are_listed_with_the_guard_on_by_default() {
        assert!(SPECS.iter().any(|s| s.key == "guard" && matches!(s.kind, Kind::Bool)));
        assert!(SPECS.iter().any(|s| s.key == "status" && matches!(s.kind, Kind::Status)));
    }
}
