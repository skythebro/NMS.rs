//! Keep the page and its popup from seeing the buttons that end our text box (pad A and B, mouse
//! buttons, Enter, Esc) and anything pressed while the box is open.
//!
//! Per frame, in the page host
//! `0x140905610`: (1) the frontend input builder `0x1401EAF20` turns the action table into button
//! states (8 dwords at `state+0x34`, 0 none, 1 released, 2 pressed, 3 held; every page reads them
//! through the pointer `G` the host stores at `+0x371`); (2) text `Update` (our after-hook);
//! (3) the page and the item picker popup, whose layer also asks the action table directly
//! (`Fe_Select` and Back, pressed this frame). The game's own "switch the action set off" only stops
//! action set 1 from registering new presses; `Fe_Select`/`Fe_Back` are not in it, so with a pad the
//! same press that ended the box also clicked a tab or closed the picker.
//!
//! So, from the after-hook, while our box is open and for the hold after it:
//! - the frame's button states are cleared (no click, back, or hold for any page this frame);
//! - every action pressed this frame is consumed exactly as the game's own query consumes it
//!   (`row[0] = row[4] - 1`, `0x142C29B3F`), so "pressed this frame" asks after us return false;
//! - after the box (hold frames) the context's reset frame is moved to now, as the game does when it
//!   switches input context (`0x142C26A39`): "held" and "long press" asks then ignore a press that
//!   started before, so a pad A still held after the box does not count as a hold either.
//!
//! The box's own reads all happen inside `Update`, before this runs. Nothing here is called on the
//! game; only the action table rows (ints the input code rewrites every frame) and the button
//! states are written, and only while the game reports our box open (state 1) or our hold runs,
//! with the "Pad guard" switch on. Any unreadable table ends the shield and the hold for that
//! box. `input_state` and `kbm_active` are reads for the log and the cursor's pad check.

use std::sync::atomic::{AtomicUsize, Ordering};

use nms_core::mem::{read_bytes, read_i32, read_ptr, MIN_ADDR};
use nms_core::process::ProcessImage;
use nms_core::{Code, Pattern};
use nms_sdk::sigs::picker_tweaks as sigs;

/// Action ids (`GcInputActionInfo` order).
pub const FE_SELECT: usize = 0x3B;
pub const FE_BACK: usize = 0x3E;
pub const FE_EXIT_MENU: usize = 0x4C;
pub const CHAT_OPEN_CLOSE: usize = 0xF8;
pub const CHAT_SEND: usize = 0xF9;

/// Contexts: `controller + 0x4C0 + index * 0x410`, index at `controller + 0x18` (the query
/// `0x142C29A70` with `ctxIndex = -1`; index 4 and above 5 mean "no context").
const CONTEXTS: usize = 0x4C0;
const CONTEXT_SIZE: usize = 0x410;
const CONTEXT_INDEX: usize = 0x18;
/// In a context: rows pointer, row count, current frame, reset frame.
const ROWS: usize = 0x10;
const ROW_COUNT: usize = 0x18;
const FRAME: usize = 0x20;
const RESET_FRAME: usize = 0x24;
/// One action row: press frame `+0`, last frame down `+4`.
pub const ROW_SIZE: usize = 0x1C;
const ROWS_MAX: usize = 2048;
/// Button states in the frontend input state: 8 dwords.
const BUTTONS: usize = 0x34;
const BUTTON_COUNT: usize = 8;

/// Address of the global pointer to the frame's frontend input state.
static STATE_PTR: AtomicUsize = AtomicUsize::new(0);

fn find(image: &ProcessImage, name: &str, pattern: &str) -> Result<usize, String> {
    let pat = Pattern::parse(pattern).map_err(|e| format!("{name}: {e}"))?;
    image.find_unique(&pat).unique().ok_or_else(|| format!("{name} not found (game update?)"))
}

fn rel_target(image: &ProcessImage, at: usize, len: usize) -> Option<usize> {
    let b = image.read(at + len - 4, 4)?;
    let rel = i32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    Some((at as i64 + len as i64 + i64::from(rel)) as usize)
}

/// Find the input state pointer and check the frame order the shield relies on: input builder,
/// then text `Update` (`update_rva`), then the page. Nothing is written here.
pub fn locate(image: &ProcessImage, update_rva: usize) -> Result<(), String> {
    let host = find(image, "page host", sigs::PAGE_HOST)?;
    let changed = || "page host changed (game update?)".to_string();
    // text Update called from the host on the real manager.
    let at = host + sigs::HOST_UPDATE_CALL_AT;
    let lea = image.read(at, sigs::HOST_UPDATE_CALL.len()).ok_or_else(changed)?;
    let call = image.read(at + 7, 1).ok_or_else(changed)?;
    if lea[..] != sigs::HOST_UPDATE_CALL || call[0] != 0xE8 || rel_target(image, at + 7, 5) != Some(update_rva) {
        return Err(changed());
    }
    // The input builder, called before it.
    let at = host + sigs::HOST_BUILD_CALL_AT;
    if image.read(at, 1).map(|b| b[0]) != Some(0xE8) {
        return Err(changed());
    }
    let builder = rel_target(image, at, 5).ok_or_else(changed)?;
    let select = image.read(builder + sigs::BUILD_SELECT_AT, sigs::BUILD_SELECT.len());
    let last = image.read(builder + sigs::BUILD_LAST_AT, sigs::BUILD_LAST.len());
    if select.as_deref() != Some(&sigs::BUILD_SELECT[..]) || last.as_deref() != Some(&sigs::BUILD_LAST[..]) {
        return Err("frontend input builder changed (game update?)".into());
    }
    // mov [rip+G], r12
    let at = host + sigs::HOST_STATE_STORE_AT;
    let store = image.read(at, 3).ok_or_else(changed)?;
    if store[..] != sigs::HOST_STATE_STORE || at >= host + sigs::HOST_UPDATE_CALL_AT {
        return Err(changed());
    }
    let global = rel_target(image, at, 7).ok_or_else(changed)?;
    STATE_PTR.store(image.va(global), Ordering::Release);
    Ok(())
}

/// The frame's frontend input state (`[G]`, the page host's `rdi+0x2248`), when the shield located it.
pub fn frontend_state() -> Option<usize> {
    let global = STATE_PTR.load(Ordering::Acquire);
    (global != 0).then(|| read_ptr(global)).flatten().filter(|&s| s >= MIN_ADDR)
}

/// The current input context of `controller`, as the action query picks it.
pub fn context(controller: usize) -> Option<usize> {
    let index = read_i32(controller + CONTEXT_INDEX)?;
    (matches!(index, 0..=3 | 5)).then(|| controller + CONTEXTS + index as usize * CONTEXT_SIZE)
}

/// `(frame, rows pointer, rows bytes)` of a context.
fn rows(ctx: usize) -> Option<(i32, usize, Vec<u8>)> {
    let frame = read_i32(ctx + FRAME)?;
    let count = usize::try_from(read_i32(ctx + ROW_COUNT)?).ok()?.min(ROWS_MAX);
    let ptr = read_ptr(ctx + ROWS)?;
    Some((frame, ptr, read_bytes(ptr, count * ROW_SIZE)?))
}

/// "Keyboard and mouse active" in a context (`ctx+0xA2`, vtable `+0x30` of the controller); 0 means
/// pad mode. Set by the mouse device on mouse motion (`0x142C3A4D5`), cleared on pad input
/// (`0x142C3777A`).
const CTX_KBM: usize = 0xA2;
/// The controller's "text entry active" byte (`ctl+0x40`): 1 from the game's text `Start` until its
/// close code (`0x1406DF855` / `0x1406E06A3`). The handler loop reads it every frame (`0x142C314D7`).
const CTL_TEXT_ACTIVE: usize = 0x40;
/// The action-set group the text box switches (`ctl+0x1500`): entry count `+0x3C`, entries `+0x40`;
/// an entry has its set id at `+0` and "enabled" at `+0x49` (`0x142C27E00`).
const CTL_SETS: usize = 0x1500;
const SETS_COUNT: usize = 0x3C;
const SETS_ENTRIES: usize = 0x40;
const SET_ENABLED: usize = 0x49;

static KBM_READY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Byte check for the pad-mode read (`KBM_QUERY` must read `ctx+0xA2`). Nothing is written.
pub fn locate_mode(image: &ProcessImage) -> Result<(), String> {
    find(image, "input mode query", sigs::KBM_QUERY)?;
    KBM_READY.store(true, Ordering::Release);
    Ok(())
}

/// `Some(true)` keyboard and mouse, `Some(false)` pad, `None` unknown. Read only.
pub fn kbm_active(controller: Option<usize>) -> Option<bool> {
    if !KBM_READY.load(Ordering::Acquire) {
        return None;
    }
    let ctx = controller.and_then(context)?;
    nms_core::mem::read_u8(ctx + CTX_KBM).map(|b| b != 0)
}

/// What the game's input state says, for the log (all reads).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct InputState {
    /// `ctl+0x40`.
    pub text_active: Option<u8>,
    /// Action set 1 in `ctl+0x1500`: enabled.
    pub set1_on: Option<bool>,
    /// Context index `ctl+0x18`.
    pub ctx: Option<i32>,
    /// Keyboard and mouse (`Some(true)`) or pad (`Some(false)`).
    pub kbm: Option<bool>,
}

pub fn input_state(controller: Option<usize>) -> InputState {
    let Some(ctl) = controller else { return InputState::default() };
    let set1_on = (|| {
        let group = ctl + CTL_SETS;
        let count = read_i32(group + SETS_COUNT).and_then(|c| usize::try_from(c).ok())?.min(64);
        let entries = read_ptr(group + SETS_ENTRIES)?;
        let mut found = None;
        for i in 0..count {
            let e = read_ptr(entries + i * 8)?;
            if read_i32(e) == Some(1) {
                let on = nms_core::mem::read_u8(e + SET_ENABLED)? != 0;
                found = Some(found.unwrap_or(true) && on);
            }
        }
        found
    })();
    InputState {
        text_active: nms_core::mem::read_u8(ctl + CTL_TEXT_ACTIVE),
        set1_on,
        ctx: read_i32(ctl + CONTEXT_INDEX),
        kbm: kbm_active(controller),
    }
}

impl std::fmt::Display for InputState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Not a reliable keyboard/mouse versus pad flag (it read 0 for a whole mouse-only session);
        // logged raw, used for nothing.
        let flag = match self.kbm {
            Some(true) => "1",
            Some(false) => "0",
            None => "?",
        };
        write!(f, "ctx+0xA2 {flag}, text flag {:?}, action set 1 on {:?}, context {:?}", self.text_active, self.set1_on, self.ctx)
    }
}

/// One action row: `(press frame, last frame down)`.
fn row(bytes: &[u8], action: usize) -> Option<(i32, i32)> {
    let b = bytes.get(action * ROW_SIZE..action * ROW_SIZE + 8)?;
    Some((i32::from_le_bytes([b[0], b[1], b[2], b[3]]), i32::from_le_bytes([b[4], b[5], b[6], b[7]])))
}

/// Pressed this frame (query mode 1 without the consume).
pub fn pressed(bytes: &[u8], frame: i32, action: usize) -> bool {
    row(bytes, action).is_some_and(|(press, _)| press == frame)
}

/// Down this frame.
pub fn held(bytes: &[u8], frame: i32, action: usize) -> bool {
    row(bytes, action).is_some_and(|(_, down)| down == frame)
}

/// Released since the last frame (query mode 3).
pub fn released(bytes: &[u8], frame: i32, action: usize) -> bool {
    row(bytes, action).is_some_and(|(_, down)| down == frame.wrapping_sub(1))
}

/// Rows pressed this frame and the press frame that consumes them, as the game's query writes it.
pub fn consume_edges(bytes: &[u8], frame: i32) -> Vec<(usize, i32)> {
    bytes
        .as_chunks::<ROW_SIZE>()
        .0
        .iter()
        .enumerate()
        .filter_map(|(i, r)| {
            let press = i32::from_le_bytes([r[0], r[1], r[2], r[3]]);
            let down = i32::from_le_bytes([r[4], r[5], r[6], r[7]]);
            (press == frame).then(|| (i, down.wrapping_sub(1)))
        })
        .collect()
}

/// Button states to clear: indices of the 8 dwords that say released, pressed or held.
pub fn active_buttons(bytes: &[u8]) -> Vec<usize> {
    bytes
        .as_chunks::<4>()
        .0
        .iter()
        .take(BUTTON_COUNT)
        .enumerate()
        .filter(|(_, b)| (1..=3).contains(&i32::from_le_bytes(**b)))
        .map(|(i, _)| i)
        .collect()
}

/// The end keys as the action table has them this frame (any device, Steam Input included).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EndKeys {
    pub accept: bool,
    pub cancel: bool,
    pub down: bool,
}

pub fn end_keys(controller: Option<usize>) -> EndKeys {
    let Some((frame, _, bytes)) = controller.and_then(context).and_then(rows) else { return EndKeys::default() };
    EndKeys {
        accept: pressed(&bytes, frame, FE_SELECT) || pressed(&bytes, frame, CHAT_SEND),
        cancel: pressed(&bytes, frame, FE_BACK)
            || pressed(&bytes, frame, FE_EXIT_MENU)
            || released(&bytes, frame, CHAT_OPEN_CLOSE),
        down: [FE_SELECT, FE_BACK, FE_EXIT_MENU, CHAT_OPEN_CLOSE, CHAT_SEND].iter().any(|&a| held(&bytes, frame, a)),
    }
}

/// Once per frame from the `Update` after-hook, and only while our own box is open or its hold runs
/// (the caller checks both; nothing here runs otherwise). `after_box`: the box has closed (hold
/// frames), so held and long presses are reset too. Returns the number of values written, or an
/// error when the action table cannot be read (the caller then ends the shield).
pub fn apply(controller: Option<usize>, after_box: bool) -> Result<usize, &'static str> {
    let mut writes = 0;
    // 1. The frame's button states.
    let global = STATE_PTR.load(Ordering::Acquire);
    if let Some(state) = (global != 0).then(|| read_ptr(global)).flatten().filter(|&s| s >= MIN_ADDR) {
        if let Some(bytes) = read_bytes(state + BUTTONS, BUTTON_COUNT * 4) {
            for i in active_buttons(&bytes) {
                // SAFETY: a button state the input builder rewrites every frame; 0 is "nothing".
                if unsafe { nms_core::mem::write(state + BUTTONS + i * 4, 0i32) } {
                    writes += 1;
                }
            }
        }
    }
    // 2. The action table of the current context.
    let ctx = controller.and_then(context).ok_or("no input context")?;
    let (frame, ptr, bytes) = rows(ctx).ok_or("action table unreadable")?;
    if ptr < MIN_ADDR {
        return Err("action table unreadable");
    }
    for (i, press) in consume_edges(&bytes, frame) {
        // SAFETY: the row's press frame, written exactly as the game's own query consumes it.
        if unsafe { nms_core::mem::write(ptr + i * ROW_SIZE, press) } {
            writes += 1;
        }
    }
    if after_box && read_i32(ctx + RESET_FRAME).is_some_and(|r| r < frame) {
        // SAFETY: the context's reset frame, set to the current frame as the game does on a switch.
        if unsafe { nms_core::mem::write(ctx + RESET_FRAME, frame) } {
            writes += 1;
        }
    }
    Ok(writes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(rows: &[(i32, i32)]) -> Vec<u8> {
        let mut out = Vec::new();
        for &(press, down) in rows {
            let mut r = [0u8; ROW_SIZE];
            r[..4].copy_from_slice(&press.to_le_bytes());
            r[4..8].copy_from_slice(&down.to_le_bytes());
            out.extend_from_slice(&r);
        }
        out
    }

    #[test]
    fn only_this_frames_presses_are_consumed() {
        // Row 0 pressed now and held, row 1 pressed earlier and held, row 2 released, row 3 idle.
        let t = table(&[(100, 100), (90, 100), (95, 99), (-1, -1)]);
        assert_eq!(consume_edges(&t, 100), vec![(0, 99)]);
        assert!(pressed(&t, 100, 0) && !pressed(&t, 100, 1));
        assert!(held(&t, 100, 1) && !held(&t, 100, 2));
        assert!(released(&t, 100, 2) && !released(&t, 100, 3));
        // An action id past the table is never pressed.
        assert!(!pressed(&t, 100, 40));
    }

    #[test]
    fn end_keys_read_the_table() {
        let mut rows = vec![(-1, -1); 0x100];
        rows[FE_SELECT] = (50, 50);
        let t = table(&rows);
        assert!(pressed(&t, 50, FE_SELECT));
        rows[FE_SELECT] = (-1, -1);
        rows[CHAT_OPEN_CLOSE] = (40, 49);
        let t = table(&rows);
        assert!(released(&t, 50, CHAT_OPEN_CLOSE));
    }

    #[test]
    fn active_button_states_are_found() {
        let mut b = Vec::new();
        for v in [0i32, 1, 2, 3, 4, -1, 0, 2] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        assert_eq!(active_buttons(&b), vec![1, 2, 3, 7]);
    }

    #[test]
    fn field_offsets_match_the_disassembly() {
        // Query 0x142C29A70: rows [ctl+idx*0x410+0x4D0], count +0x4D8, frame +0x4E0, reset +0x4E4.
        assert_eq!(CONTEXTS + ROWS, 0x4D0);
        assert_eq!(CONTEXTS + ROW_COUNT, 0x4D8);
        assert_eq!(CONTEXTS + FRAME, 0x4E0);
        assert_eq!(CONTEXTS + RESET_FRAME, 0x4E4);
        // Builder: button states 0x34..0x50 inclusive.
        assert_eq!(BUTTONS + (BUTTON_COUNT - 1) * 4, 0x50);
    }
}
