//! `xinput9_1_0.dll` placed in `Binaries\`. NMS.exe imports it at startup, so the mods load with
//! no injector. The four XInput functions pass straight through to the real System32 DLL.
//!
//! Proton: Wine prefers its builtin xinput, so the launch option needs
//! `WINEDLLOVERRIDES="xinput9_1_0=n,b" %command%`.
//! Kill switch: a file named `disabled` in `<game>\NMS.rs\` keeps the game vanilla.

#![cfg(windows)]

mod boot;
mod loader_settings;
#[cfg(feature = "selftest")]
mod lifecycle_test;
#[cfg(feature = "selftest")]
mod selftest;
mod xinput;

use windows_sys::Win32::Foundation::{BOOL, HMODULE, TRUE};
use windows_sys::Win32::System::LibraryLoader::DisableThreadLibraryCalls;
use windows_sys::Win32::System::SystemServices::DLL_PROCESS_ATTACH;

pub use xinput::{XInputGetCapabilities, XInputGetDSoundAudioDeviceGuids, XInputGetState, XInputSetState};

/// `nms-rs install` looks for these bytes to tell our proxy from another mod's xinput9_1_0.dll.
#[used]
#[no_mangle]
pub static NMS_RS_PROXY_MARKER: [u8; 20] = *b"NMS.rs proxy marker\0";

#[no_mangle]
#[allow(non_snake_case)]
extern "system" fn DllMain(module: HMODULE, reason: u32, _reserved: *mut core::ffi::c_void) -> BOOL {
    if reason == DLL_PROCESS_ATTACH {
        // SAFETY: valid module handle passed by the loader.
        unsafe { DisableThreadLibraryCalls(module) };
        // DllMain only starts the boot thread and returns. No wait, no CreateProcess, no
        // LoadLibrary under the loader lock. Merge and hooks run on that thread after this returns.
        let _ = std::thread::Builder::new().name("nms-rs boot".into()).spawn(boot::run);
    }
    TRUE
}
