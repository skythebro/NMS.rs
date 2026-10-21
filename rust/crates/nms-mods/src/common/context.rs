//! The game context from the core (`nms_core::game`), for mods that used to find the application
//! data themselves. Each accessor falls back to the mod's own anchor (the global it read from game
//! code) when the core context is off, so a mod never loses a feature because of it.

use nms_core::game::live;
use nms_core::mem::read_ptr;

/// The input controller offset in the application data when the core context is off (build 180836).
pub const CONTROLLER_OFFSET_FALLBACK: u32 = 0x933B78;

/// Offset of the input controller pointer in the application data, read from the game's getter by
/// the core (it moved to 0x933BC8 on build 181442).
pub fn controller_offset() -> u32 {
    live::get().map_or(CONTROLLER_OFFSET_FALLBACK, |g| g.layout().members.input_controller)
}

/// `cGcApplication::Data` (older code calls it "app"). `fallback_global` is the address of the
/// global holding it, as the mod found it in game code; 0 if unknown.
pub fn data(fallback_global: usize) -> Option<usize> {
    match live::get() {
        Some(g) => g.data(),
        None if fallback_global != 0 => read_ptr(fallback_global),
        None => None,
    }
}

/// The input controller object.
pub fn input_controller(fallback_global: usize) -> Option<usize> {
    match live::get() {
        Some(g) => g.input_controller(),
        None => read_ptr(data(fallback_global)? + CONTROLLER_OFFSET_FALLBACK as usize),
    }
}

/// The close code at the end of the text popup's `Update` loads the controller with
/// `mov rcx,[rax+disp32]`; its disp32 must be the controller offset for this build.
pub fn controller_disp_matches(disp: &[u8]) -> bool {
    disp == controller_offset().to_le_bytes()
}
