//! Pass-through to the real `xinput9_1_0.dll` in the system folder.

#![allow(non_snake_case)]

use std::ffi::c_void;
use std::sync::OnceLock;

use windows_sys::Win32::Foundation::HMODULE;
use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW;

const ERROR_DEVICE_NOT_CONNECTED: u32 = 1167;

fn real() -> HMODULE {
    static REAL: OnceLock<usize> = OnceLock::new();
    *REAL.get_or_init(|| {
        let mut buf = [0u16; 260];
        // SAFETY: buffer length passed matches the buffer.
        let n = unsafe { GetSystemDirectoryW(buf.as_mut_ptr(), buf.len() as u32) } as usize;
        if n == 0 || n >= buf.len() {
            return 0;
        }
        let mut path: Vec<u16> = buf[..n].to_vec();
        path.extend("\\xinput9_1_0.dll".encode_utf16());
        path.push(0);
        // SAFETY: NUL-terminated absolute path, so this never loads ourselves again.
        unsafe { LoadLibraryW(path.as_ptr()) as usize }
    }) as HMODULE
}

fn proc(name: &[u8]) -> Option<usize> {
    let module = real();
    if module.is_null() {
        return None;
    }
    // SAFETY: name is NUL-terminated.
    unsafe { GetProcAddress(module, name.as_ptr()) }.map(|f| f as usize)
}

macro_rules! forward {
    ($name:ident, $cname:literal, ($($arg:ident: $ty:ty),*)) => {
        /// XInput pass-through.
        ///
        /// # Safety
        /// Same contract as the system XInput function of this name.
        #[no_mangle]
        pub unsafe extern "system" fn $name($($arg: $ty),*) -> u32 {
            static FN: OnceLock<Option<usize>> = OnceLock::new();
            match *FN.get_or_init(|| proc($cname)) {
                // SAFETY: same signature as the documented XInput export.
                Some(f) => {
                    let f: unsafe extern "system" fn($($ty),*) -> u32 = std::mem::transmute(f);
                    f($($arg),*)
                }
                None => ERROR_DEVICE_NOT_CONNECTED,
            }
        }
    };
}

forward!(XInputGetState, b"XInputGetState\0", (user: u32, state: *mut c_void));
forward!(XInputSetState, b"XInputSetState\0", (user: u32, vibration: *mut c_void));
forward!(XInputGetCapabilities, b"XInputGetCapabilities\0", (user: u32, flags: u32, caps: *mut c_void));
forward!(XInputGetDSoundAudioDeviceGuids, b"XInputGetDSoundAudioDeviceGuids\0", (user: u32, render: *mut c_void, capture: *mut c_void));
