//! Put the game's own cursor back where it was when our text box opened.
//!
//! The cursor a menu page draws and clicks
//! with is the frontend cursor in the page state (`[G]+0x0C`/`+0x10`, UI units; `+0x14`/`+0x18`
//! its velocity, `+0x28`/`+0x2C` the last mouse position it saw). The input builder moves it with the
//! stick in pad mode and sets it from the mouse device in keyboard/mouse mode. On the frame the text
//! box comes up or goes away (`[mgr+0xAF4] && [mgr+0x670] != 0` changes, `0x1401EC576`) the builder
//! takes the mouse device position as the cursor, whatever the mode: that is the jump. In pad mode the
//! mouse device position is stale (where the mouse was last), so the cursor lands somewhere else.
//!
//! So at box open the frontend fields and the mouse device position (input controller `+0x10`,
//! `+0x200`/`+0x210`, what GLFW reports) are saved; after the box they are written back: the frontend
//! fields directly, the device position through the device's own setter (`0x142C3ADB0`, vtable
//! `+0x38`), which also sets the "previous" position, so the game sees no mouse motion and stays in
//! pad mode. GLFW is written by its real cursor mode: in disabled mode its virtual cursor
//! (`window+0x1E8`), else `_glfwPlatformSetCursorPos` (GLFW's last position plus `SetCursorPos`,
//! client to screen), because `glfwSetCursorPos` did not take from our hook. Written once after the box closes (if the cursor moved) and again only for a jump
//! within the settle window, at most [`MAX_RESTORES`] times per box (a stick moves it gradually).

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::OnceLock;

use nms_core::mem::{read_bytes, read_ptr, read_u64, MIN_ADDR};
use nms_core::process::ProcessImage;
use nms_core::{Code, Pattern};
use nms_sdk::sigs::picker_tweaks as sigs;

/// The mouse device inside the input controller.
const CTL_MOUSE: usize = 0x10;
const VTBL_SET_POS: usize = 0x38;
const DEV_X: usize = 0x200;
const DEV_Y: usize = 0x210;
/// Frontend cursor fields in the page state.
const FE_POS: usize = 0x0C;
const FE_VEL: usize = 0x14;
const FE_LAST_MOUSE: usize = 0x28;

/// The first write: within this long after the box closed, if the cursor is this far off.
pub const FIRST_MS: u64 = 200;
const FIRST_UNITS: f32 = 3.0;
/// A later write needs a one-frame jump this big (UI units; a stick moves far less per frame).
const JUMP_UNITS: f32 = 48.0;
pub const MAX_RESTORES: u8 = 3;

static DEV_SET_POS: AtomicUsize = AtomicUsize::new(0);
static PLATFORM: AtomicUsize = AtomicUsize::new(0);
static PLATFORM_SET: AtomicUsize = AtomicUsize::new(0);
type PlatformFn = unsafe extern "system" fn() -> usize;
type PlatformSetFn = unsafe extern "system" fn(usize, f64, f64);
/// GLFW window in the platform object; in the window: cursor mode (0x34001 normal, 0x34002 hidden,
/// 0x34003 disabled: then the game's position is GLFW's virtual one and the OS cursor is parked),
/// virtual cursor, Win32 handle.
const PLATFORM_WINDOW: usize = 0x1E0;
const WIN_CURSOR_MODE: usize = 0x78;
const WIN_VIRTUAL: usize = 0x1E8;
const WIN_HANDLE: usize = 0x358;
pub const MODE_NORMAL: u32 = 0x34001;
pub const MODE_DISABLED: u32 = 0x34003;
static SNAP_SEEN: AtomicBool = AtomicBool::new(false);

type DevSetPosFn = unsafe extern "system" fn(usize, f32, f32);

fn find(image: &ProcessImage, name: &str, pattern: &str) -> Result<usize, String> {
    let pat = Pattern::parse(pattern).map_err(|e| format!("{name}: {e}"))?;
    image.find_unique(&pat).unique().ok_or_else(|| format!("{name} not found (game update?)"))
}

fn call_target(image: &ProcessImage, at: usize) -> Option<usize> {
    let b = image.read(at, 5)?;
    if b[0] != 0xE8 {
        return None;
    }
    let rel = i32::from_le_bytes([b[1], b[2], b[3], b[4]]);
    Some((at as i64 + 5 + i64::from(rel)) as usize)
}

fn bytes_at(image: &ProcessImage, at: usize, want: &[u8]) -> bool {
    image.read(at, want.len()).is_some_and(|b| b[..] == *want)
}

fn jmp_target(image: &ProcessImage, at: usize) -> Option<usize> {
    let b = image.read(at, 5)?;
    if b[0] != 0xE9 {
        return None;
    }
    let rel = i32::from_le_bytes([b[1], b[2], b[3], b[4]]);
    Some((at as i64 + 5 + i64::from(rel)) as usize)
}

/// Find the device setter (byte-checked: both position stores and its call into GLFW's
/// `glfwSetCursorPos`), GLFW's virtual-cursor store and `_glfwPlatformSetCursorPos` behind it, the
/// platform accessor (GLFW window), and check the builder's text-box snap and its cursor store (the
/// frontend field offsets). Nothing is called or written here.
pub fn locate(image: &ProcessImage) -> Result<(), String> {
    let changed = |what: &str| format!("{what} changed (game update?)");
    let set = find(image, "mouse device set position", sigs::DEVICE_SET_POS)?;
    if !(bytes_at(image, set + sigs::DEV_SET_X_AT, &sigs::DEV_SET_X) && bytes_at(image, set + sigs::DEV_SET_Y_AT, &sigs::DEV_SET_Y)) {
        return Err(changed("mouse device set position"));
    }
    let glfw = call_target(image, set + sigs::DEV_SET_GLFW_CALL_AT).ok_or_else(|| changed("mouse device set position"))?;
    if !(bytes_at(image, glfw, &sigs::SET_POS_HEAD)
        && bytes_at(image, glfw + sigs::SET_POS_HANDLE_AT, &sigs::SET_POS_HANDLE)
        && bytes_at(image, glfw + sigs::SET_POS_MODE_AT, &sigs::SET_POS_MODE)
        && bytes_at(image, glfw + sigs::SET_POS_VIRTUAL_AT, &sigs::SET_POS_VIRTUAL))
    {
        return Err(changed("glfwSetCursorPos"));
    }
    let platform_set = jmp_target(image, glfw + sigs::SET_POS_PLATFORM_JMP_AT).ok_or_else(|| changed("glfwSetCursorPos"))?;
    if !(bytes_at(image, platform_set, &sigs::PLATFORM_SET_HEAD)
        && bytes_at(image, platform_set + sigs::PLATFORM_SET_LAST_AT, &sigs::PLATFORM_SET_LAST))
    {
        return Err(changed("_glfwPlatformSetCursorPos"));
    }
    let platform = call_target(image, set + sigs::DEV_SET_PLATFORM_CALL_AT)
        .filter(|_| bytes_at(image, set + sigs::DEV_SET_WINDOW_AT, &sigs::DEV_SET_WINDOW))
        .ok_or_else(|| changed("mouse device set position"))?;
    let snap = find(image, "frontend text-box snap", sigs::FRONTEND_TEXT_SNAP)?;
    if !bytes_at(image, snap + sigs::SNAP_CURSOR_STORE_AT, &sigs::SNAP_CURSOR_STORE) {
        return Err(changed("frontend cursor store"));
    }
    PLATFORM.store(image.va(platform), Ordering::Release);
    PLATFORM_SET.store(image.va(platform_set), Ordering::Release);
    SNAP_SEEN.store(true, Ordering::Release);
    DEV_SET_POS.store(image.va(set), Ordering::Release);
    Ok(())
}

/// The game's GLFW window, checked: a window handle and a known cursor mode.
fn window() -> Option<usize> {
    let platform = PLATFORM.load(Ordering::Acquire);
    if platform == 0 {
        return None;
    }
    // SAFETY: the game's own accessor of a function-local static, called by the mouse device every
    // frame on the game thread (where our `Update` after-hook runs).
    let p = unsafe { std::mem::transmute::<usize, PlatformFn>(platform)() };
    let win = read_ptr(p.checked_add(PLATFORM_WINDOW)?).filter(|&w| w >= MIN_ADDR)?;
    let ok = read_ptr(win + WIN_HANDLE).is_some_and(|h| h != 0)
        && nms_core::mem::read_u32(win + WIN_CURSOR_MODE).is_some_and(|m| (MODE_NORMAL..=MODE_DISABLED).contains(&m));
    ok.then_some(win)
}

/// GLFW cursor mode (`0x34001` normal, `0x34002` hidden, `0x34003` disabled). Read only.
pub fn glfw_mode() -> Option<u32> {
    nms_core::mem::read_u32(window()? + WIN_CURSOR_MODE)
}

pub fn mode_name(mode: Option<u32>) -> &'static str {
    match mode {
        Some(MODE_NORMAL) => "normal",
        Some(0x34002) => "hidden",
        Some(MODE_DISABLED) => "disabled",
        _ => "?",
    }
}

/// The mouse device of `controller`, checked: its vtable `+0x38` is the located setter.
fn device(controller: Option<usize>) -> Option<usize> {
    let set = DEV_SET_POS.load(Ordering::Acquire);
    let dev = controller?.checked_add(CTL_MOUSE)?;
    let vtbl = read_ptr(dev).filter(|&v| v >= MIN_ADDR)?;
    (set != 0 && read_ptr(vtbl + VTBL_SET_POS) == Some(set)).then_some(dev)
}

fn f32_pair(addr: usize) -> Option<(f32, f32)> {
    let b = read_bytes(addr, 8)?;
    let p = (f32::from_le_bytes([b[0], b[1], b[2], b[3]]), f32::from_le_bytes([b[4], b[5], b[6], b[7]]));
    (p.0.is_finite() && p.1.is_finite()).then_some(p)
}

fn f64_pair(addr: usize) -> Option<(f64, f64)> {
    let p = (f64::from_bits(read_u64(addr)?), f64::from_bits(read_u64(addr + 8)?));
    (p.0.is_finite() && p.1.is_finite()).then_some(p)
}

/// GLFW and OS state of the cursor (all reads), for the log and the restore decision.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Glfw {
    pub mode: Option<u32>,
    /// GLFW's virtual cursor (`window+0x1E8/+0x1F0`, client pixels; what the game reads in disabled mode).
    pub virt: Option<(f64, f64)>,
    /// The OS cursor and the window's client centre, in screen pixels.
    pub os: Option<(i32, i32)>,
    pub centre: Option<(i32, i32)>,
    /// The thread we run on is the window's thread; the window is the active one for this thread.
    pub window_thread: Option<bool>,
    pub active: Option<bool>,
}

impl std::fmt::Display for Glfw {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let p = |v: Option<(i32, i32)>| v.map_or("?".to_string(), |(x, y)| format!("{x},{y}"));
        write!(
            f,
            "glfw {} virtual {} os {} centre {} window thread {:?} active {:?}",
            mode_name(self.mode),
            self.virt.map_or("?".to_string(), |(x, y)| format!("{x:.1},{y:.1}")),
            p(self.os),
            p(self.centre),
            self.window_thread,
            self.active
        )
    }
}

pub fn glfw_state() -> Glfw {
    let win = window();
    let handle = win.and_then(|w| read_ptr(w + WIN_HANDLE));
    let u = user32();
    let os = u.and_then(|u| {
        let mut p = [0i32; 2];
        // SAFETY: an out pointer to a POINT.
        (unsafe { (u.get_cursor_pos)(p.as_mut_ptr()) } != 0).then_some((p[0], p[1]))
    });
    let centre = u.zip(handle).and_then(|(u, h)| {
        let mut r = [0i32; 4];
        // SAFETY: a window handle and out pointers to a RECT and a POINT.
        unsafe {
            if (u.client_rect)(h, r.as_mut_ptr()) == 0 {
                return None;
            }
            let mut p = [r[2] / 2, r[3] / 2];
            ((u.client_to_screen)(h, p.as_mut_ptr()) != 0).then_some((p[0], p[1]))
        }
    });
    let window_thread = u.zip(handle).map(|(u, h)| {
        // SAFETY: plain thread ids.
        unsafe { (u.window_thread)(h, std::ptr::null_mut()) == (u.current_thread)() }
    });
    // SAFETY: no arguments.
    let active = u.zip(handle).map(|(u, h)| unsafe { (u.active_window)() } == h);
    Glfw {
        mode: win.and_then(|w| nms_core::mem::read_u32(w + WIN_CURSOR_MODE)),
        virt: win.and_then(|w| f64_pair(w + WIN_VIRTUAL)),
        os,
        centre,
        window_thread,
        active,
    }
}

type GetCursorPosFn = unsafe extern "system" fn(*mut i32) -> i32;
type ClientRectFn = unsafe extern "system" fn(usize, *mut i32) -> i32;
type ClientToScreenFn = unsafe extern "system" fn(usize, *mut i32) -> i32;
type WindowThreadFn = unsafe extern "system" fn(usize, *mut u32) -> u32;
type CurrentThreadFn = unsafe extern "system" fn() -> u32;
type ActiveWindowFn = unsafe extern "system" fn() -> usize;

struct User32 {
    get_cursor_pos: GetCursorPosFn,
    client_rect: ClientRectFn,
    client_to_screen: ClientToScreenFn,
    window_thread: WindowThreadFn,
    current_thread: CurrentThreadFn,
    active_window: ActiveWindowFn,
}

fn user32() -> Option<&'static User32> {
    static LIB: OnceLock<Option<User32>> = OnceLock::new();
    LIB.get_or_init(|| {
        use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
        let get = |dll: &str, name: &std::ffi::CStr| -> Option<usize> {
            let wide: Vec<u16> = dll.encode_utf16().chain(Some(0)).collect();
            // SAFETY: NUL-terminated names of system DLLs the game has already loaded.
            let module = unsafe { LoadLibraryW(wide.as_ptr()) };
            if module.is_null() {
                return None;
            }
            // SAFETY: NUL-terminated export name.
            unsafe { GetProcAddress(module, name.as_ptr().cast()) }.map(|f| f as usize)
        };
        let (a, b, c, d, e, f) = (
            get("user32.dll", c"GetCursorPos")?,
            get("user32.dll", c"GetClientRect")?,
            get("user32.dll", c"ClientToScreen")?,
            get("user32.dll", c"GetWindowThreadProcessId")?,
            get("kernel32.dll", c"GetCurrentThreadId")?,
            get("user32.dll", c"GetActiveWindow")?,
        );
        // SAFETY: the documented signatures of these exports.
        unsafe {
            Some(User32 {
                get_cursor_pos: std::mem::transmute::<usize, GetCursorPosFn>(a),
                client_rect: std::mem::transmute::<usize, ClientRectFn>(b),
                client_to_screen: std::mem::transmute::<usize, ClientToScreenFn>(c),
                window_thread: std::mem::transmute::<usize, WindowThreadFn>(d),
                current_thread: std::mem::transmute::<usize, CurrentThreadFn>(e),
                active_window: std::mem::transmute::<usize, ActiveWindowFn>(f),
            })
        }
    })
    .as_ref()
}

/// Everything the cursor is made of, at one moment.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Snapshot {
    /// Frontend cursor (UI units), its velocity, the last mouse position it saw.
    pub ui: (f32, f32),
    pub vel: (f32, f32),
    pub last_mouse: (f32, f32),
    /// Mouse device position (client pixels, what GLFW reports).
    pub dev: (f64, f64),
    pub glfw: Glfw,
}

pub fn snapshot(controller: Option<usize>) -> Option<Snapshot> {
    let state = super::shield::frontend_state()?;
    let dev = device(controller)?;
    let (x, y) = (f64::from_bits(read_u64(dev + DEV_X)?), f64::from_bits(read_u64(dev + DEV_Y)?));
    (x.is_finite() && y.is_finite()).then_some(())?;
    Some(Snapshot {
        ui: f32_pair(state + FE_POS)?,
        vel: f32_pair(state + FE_VEL)?,
        last_mouse: f32_pair(state + FE_LAST_MOUSE)?,
        dev: (x, y),
        glfw: glfw_state(),
    })
}

/// How the GLFW side was written back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GlfwWrite {
    /// Disabled mode: GLFW's virtual cursor written (what `glfwSetCursorPos` writes there).
    Virtual,
    /// Normal or hidden: `_glfwPlatformSetCursorPos` (lastCursorPos, ClientToScreen, SetCursorPos).
    OsCursor,
    None,
}

/// Write `saved` back: the device through its own setter (position and previous alike, so no mouse
/// motion), GLFW by the real cursor mode, then the frontend fields. `glfwSetCursorPos` (called by
/// the device setter) returns early unless `GetActiveWindow()` of the calling thread is the game
/// window; in a test log the device read the window centre again the frame after our write, so it
/// did not take. The GLFW part is therefore written here directly, as it would have.
pub fn restore(controller: Option<usize>, saved: &Snapshot) -> GlfwWrite {
    let (Some(state), Some(dev)) = (super::shield::frontend_state(), device(controller)) else { return GlfwWrite::None };
    let set = DEV_SET_POS.load(Ordering::Acquire);
    // SAFETY: the device's own `SetCursorPos(device, x, y)` (rcx, xmm1, xmm2), its vtable entry
    // checked against the located function.
    unsafe { std::mem::transmute::<usize, DevSetPosFn>(set)(dev, saved.dev.0 as f32, saved.dev.1 as f32) };
    let (x, y) = (saved.dev.0.round(), saved.dev.1.round());
    let how = match (window(), glfw_mode()) {
        (Some(win), Some(MODE_DISABLED)) => {
            // SAFETY: GLFW's virtual cursor doubles, exactly what `glfwSetCursorPos` stores in
            // disabled mode (`SET_POS_VIRTUAL`, byte-checked); GLFW adds mouse deltas to it.
            unsafe {
                let _ = nms_core::mem::write(win + WIN_VIRTUAL, (x, y));
            }
            GlfwWrite::Virtual
        }
        (Some(win), Some(_)) => {
            let platform_set = PLATFORM_SET.load(Ordering::Acquire);
            // SAFETY: `_glfwPlatformSetCursorPos(window, x, y)` (rcx, xmm1, xmm2), what
            // `glfwSetCursorPos` jumps to outside disabled mode (byte-checked): it records the
            // position as GLFW's last one and moves the OS cursor there (client to screen).
            unsafe { std::mem::transmute::<usize, PlatformSetFn>(platform_set)(win, x, y) };
            GlfwWrite::OsCursor
        }
        _ => GlfwWrite::None,
    };
    // SAFETY: plain floats of the frame's frontend state, which the builder rewrites every frame.
    unsafe {
        let _ = nms_core::mem::write(state + FE_POS, saved.ui);
        let _ = nms_core::mem::write(state + FE_VEL, (0f32, 0f32));
        let _ = nms_core::mem::write(state + FE_LAST_MOUSE, saved.last_mouse);
    }
    how
}


fn near(a: (f32, f32), b: (f32, f32), units: f32) -> bool {
    (a.0 - b.0).abs() <= units && (a.1 - b.1).abs() <= units
}

/// When to write the cursor back after the box. Pure, for tests.
#[derive(Clone, Copy, Debug, Default)]
pub struct Settle {
    saved: Option<(f32, f32)>,
    closed_ms: u64,
    prev: Option<(f32, f32)>,
    done: u8,
}

impl Settle {
    pub const IDLE: Settle = Settle { saved: None, closed_ms: 0, prev: None, done: 0 };

    pub fn open(&mut self, ui: (f32, f32)) {
        *self = Settle { saved: Some(ui), ..Settle::IDLE };
    }

    pub fn reset(&mut self) {
        *self = Settle::IDLE;
    }

    #[cfg(test)]
    pub fn active(&self) -> bool {
        self.saved.is_some()
    }

    pub fn writes(&self) -> u8 {
        self.done
    }

    /// One frame. `box_open`: our box is still up (never write then). True: write the saved state.
    pub fn step(&mut self, now: u64, ui: (f32, f32), box_open: bool) -> bool {
        let Some(saved) = self.saved else { return false };
        if box_open {
            self.prev = Some(ui);
            return false;
        }
        if self.closed_ms == 0 {
            self.closed_ms = now;
        }
        let prev = self.prev.replace(ui);
        if self.done >= MAX_RESTORES || near(ui, saved, FIRST_UNITS) {
            return false;
        }
        let first = self.done == 0 && now.saturating_sub(self.closed_ms) <= FIRST_MS;
        let jump = prev.is_some_and(|p| !near(ui, p, JUMP_UNITS));
        if first || jump {
            self.done += 1;
            self.prev = Some(saved);
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn written_once_after_close_then_only_for_a_jump() {
        let mut s = Settle::default();
        assert!(!s.step(0, (1.0, 1.0), false), "nothing saved, nothing done");
        s.open((300.0, 200.0));
        // The box snaps the cursor while it is open: never written then.
        assert!(!s.step(10, (960.0, 540.0), true));
        // First frame after close, still off: written.
        assert!(s.step(100, (960.0, 540.0), false));
        // Back where it was: nothing.
        assert!(!s.step(116, (300.0, 200.0), false));
        // The stick moves it a little each frame: kept.
        assert!(!s.step(132, (310.0, 205.0), false));
        assert!(!s.step(148, (320.0, 210.0), false));
        // A late snap (one-frame jump): written again.
        assert!(s.step(400, (960.0, 540.0), false));
        assert!(s.step(416, (900.0, 100.0), false));
        // The cap.
        assert!(!s.step(432, (100.0, 900.0), false));
        assert_eq!(s.writes(), MAX_RESTORES);
    }

    #[test]
    fn unchanged_or_slow_moves_are_left_alone() {
        let mut s = Settle::default();
        s.open((300.0, 200.0));
        assert!(!s.step(100, (301.0, 200.0), false));
        // After the first window a slow drift is the player's.
        assert!(!s.step(400, (330.0, 200.0), false));
        assert!(!s.step(416, (360.0, 200.0), false));
        assert_eq!(s.writes(), 0);
        // Reset forgets the box.
        s.reset();
        assert!(!s.active());
        assert!(!s.step(500, (960.0, 540.0), false));
    }
}
