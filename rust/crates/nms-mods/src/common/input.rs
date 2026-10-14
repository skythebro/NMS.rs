//! Steam Input and XInput helpers shared by SmallerInventorySlots and QuickTransferAll.

use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;

const PAD_A: u16 = 0x1000;
const MAX_CONTROLLERS: usize = 16;
/// XInput's own "pressed" point for a trigger. Below this a resting finger does not count.
const TRIGGER_HELD: u8 = 30;

type XInputGetStateFn = unsafe extern "system" fn(u32, *mut XInputState) -> u32;

#[repr(C)]
struct XInputState {
    packet: u32,
    buttons: u16,
    left_trigger: u8,
    right_trigger: u8,
    thumb_lx: i16,
    thumb_ly: i16,
    thumb_rx: i16,
    thumb_ry: i16,
}

pub fn face_button_a_xinput() -> bool {
    static LIB: std::sync::OnceLock<Option<XInputGetStateFn>> = std::sync::OnceLock::new();
    let Some(func) = LIB.get_or_init(load_xinput).as_ref() else {
        return false;
    };
    let mut user = 0u32;
    while user < 4 {
        let mut state = XInputState {
            packet: 0,
            buttons: 0,
            left_trigger: 0,
            right_trigger: 0,
            thumb_lx: 0,
            thumb_ly: 0,
            thumb_rx: 0,
            thumb_ry: 0,
        };
        let ok = unsafe { func(user, &mut state) };
        if ok == 0 && state.buttons & PAD_A != 0 {
            return true;
        }
        user = user.wrapping_add(1);
    }
    false
}

/// True when any connected Xbox-style pad has the left trigger pulled in.
///
/// Only pads outside Steam Input show up here; Steam hides the ones it handles.
pub fn left_trigger_xinput() -> bool {
    static LIB: std::sync::OnceLock<Option<XInputGetStateFn>> = std::sync::OnceLock::new();
    let Some(func) = LIB.get_or_init(load_xinput).as_ref() else {
        return false;
    };
    let mut user = 0u32;
    while user < 4 {
        let mut state = XInputState {
            packet: 0,
            buttons: 0,
            left_trigger: 0,
            right_trigger: 0,
            thumb_lx: 0,
            thumb_ly: 0,
            thumb_rx: 0,
            thumb_ry: 0,
        };
        let ok = unsafe { func(user, &mut state) };
        if ok == 0 && state.left_trigger >= TRIGGER_HELD {
            return true;
        }
        user = user.wrapping_add(1);
    }
    false
}

/// `GetAsyncKeyState` high bit, the same test Python uses for Shift and the left mouse button.
pub fn async_key_down(vk: i32) -> bool {
    (unsafe { GetAsyncKeyState(vk) } as u16) & 0x8000 != 0
}

fn load_xinput() -> Option<XInputGetStateFn> {
    for name in ["xinput1_4.dll\0", "xinput1_3.dll\0", "xinput9_1_0.dll\0"] {
        if let Some(func) = load_proc(name, b"XInputGetState\0") {
            return Some(unsafe { std::mem::transmute::<usize, XInputGetStateFn>(func) });
        }
    }
    None
}

type SteamV006 = unsafe extern "C" fn() -> usize;
type SteamControllers = unsafe extern "C" fn(usize, *mut u64) -> i32;
type SteamDigital = unsafe extern "C" fn(usize, *const u8) -> u64;
type SteamDigitalData = unsafe extern "C" fn(usize, u64, u64) -> u16;

pub struct SteamInput {
    v006: SteamV006,
    get_controllers: SteamControllers,
    get_digital: SteamDigital,
    get_data: SteamDigitalData,
    iface: usize,
}

impl SteamInput {
    pub fn new() -> Result<Self, String> {
        let module = load_module("steam_api64.dll\0").ok_or_else(|| "steam_api64 missing".to_string())?;
        let v006 = proc(module, b"SteamAPI_SteamInput_v006\0").ok_or_else(|| "SteamInput v006 missing".to_string())?;
        let get_controllers = proc(module, b"SteamAPI_ISteamInput_GetConnectedControllers\0")
            .ok_or_else(|| "GetConnectedControllers missing".to_string())?;
        let get_digital = proc(module, b"SteamAPI_ISteamInput_GetDigitalActionHandle\0")
            .ok_or_else(|| "GetDigitalActionHandle missing".to_string())?;
        let get_data = proc(module, b"SteamAPI_ISteamInput_GetDigitalActionData\0")
            .ok_or_else(|| "GetDigitalActionData missing".to_string())?;
        let v006: SteamV006 = unsafe { std::mem::transmute::<usize, SteamV006>(v006) };
        let iface = unsafe { v006() };
        Ok(Self {
            v006,
            get_controllers: unsafe { std::mem::transmute::<usize, SteamControllers>(get_controllers) },
            get_digital: unsafe { std::mem::transmute::<usize, SteamDigital>(get_digital) },
            get_data: unsafe { std::mem::transmute::<usize, SteamDigitalData>(get_data) },
            iface,
        })
    }

    fn ensure_iface(&mut self) -> bool {
        if self.iface != 0 {
            return true;
        }
        self.iface = unsafe { (self.v006)() };
        self.iface != 0
    }

    pub fn controllers(&mut self) -> Vec<u64> {
        if !self.ensure_iface() {
            return Vec::new();
        }
        let mut handles = [0u64; MAX_CONTROLLERS];
        let count = unsafe { (self.get_controllers)(self.iface, handles.as_mut_ptr()) };
        let n = count.max(0) as usize;
        let n = n.min(MAX_CONTROLLERS);
        handles[..n].to_vec()
    }

    pub fn digital(&mut self, name: &str) -> u64 {
        if !self.ensure_iface() {
            return 0;
        }
        let mut buf = name.as_bytes().to_vec();
        buf.push(0);
        unsafe { (self.get_digital)(self.iface, buf.as_ptr()) }
    }

    pub fn down(&self, controller: u64, action: u64) -> bool {
        if self.iface == 0 {
            return false;
        }
        let value = unsafe { (self.get_data)(self.iface, controller, action) };
        (value & 0xFF) != 0 && (value >> 8) != 0
    }
}

fn load_module(name: &str) -> Option<windows_sys::Win32::Foundation::HMODULE> {
    let mut wide: Vec<u16> = name.encode_utf16().collect();
    if wide.last() != Some(&0) {
        wide.push(0);
    }
    let handle = unsafe { windows_sys::Win32::System::LibraryLoader::LoadLibraryW(wide.as_ptr()) };
    if handle.is_null() {
        None
    } else {
        Some(handle)
    }
}

fn proc(module: windows_sys::Win32::Foundation::HMODULE, name: &[u8]) -> Option<usize> {
    let p = unsafe { windows_sys::Win32::System::LibraryLoader::GetProcAddress(module, name.as_ptr()) };
    p.map(|f| f as usize)
}

fn load_proc(dll: &str, name: &[u8]) -> Option<usize> {
    proc(load_module(dll)?, name)
}
