//! ToolModeBinds: Mining Laser, Boltcaster and Terrain Manipulator rows in the Quick Menu weapon submenu.
//!
//! Every address below was re-read in the disassembly of build 183791 (89,491,528 bytes, 2026-10-28). No
//! hooks: two code sites are rewritten (a jump into a cave, a call retarget) and four functions are located.

use crate::{ModSigs, Sig, SigUse};

/// Head of the submenu mode loop (`0x141559e50`): `mov rdi,[rip+G]; mov eax,esi; shr rax,5;
/// test [rdi+rax*4+0xE68], r14d; je skip; mov rdx,[rdi+0x2F8]`. `esi` is the mode (0..20), `r14d` its bit.
/// The `test`/`je` pair is the filter that keeps only secondary modes. 1 hit.
pub const FILTER_LOOP: &str =
    "48 8B 3D ? ? ? ? 8B C6 48 C1 E8 05 44 85 B4 87 68 0E 00 00 0F 84 ? ? ? ? 48 8B 97 F8 02 00 00";
/// Offset of the global's `disp32` in `FILTER_LOOP` (instruction ends 4 bytes later).
pub const FILTER_G_DISP: usize = 3;
/// Offset of the filter (`test` + `je`, 14 bytes) in `FILTER_LOOP`.
pub const FILTER_AT: usize = 13;
/// The 14 bytes replaced by a jump to the cave. `je +0xD8` lands on the loop tail `0x141559f43` (build 183791;
/// the loop body shrank, it was `je +0x103` before).
pub const FILTER_OLD: [u8; 14] = [0x44, 0x85, 0xB4, 0x87, 0x68, 0x0E, 0x00, 0x00, 0x0F, 0x84, 0xD8, 0x00, 0x00, 0x00];
/// Loop tail at the `je` target: `inc esi; rol r14d,1; add rbx,0x150; cmp esi,0x15; jl <loop head>`.
/// Proves the skip target, the 21-mode bound (so the bit index never leaves the first mask dword) and
/// that the loop head is the start of `FILTER_LOOP`. Followed by the `jl` rel32.
pub const LOOP_TAIL: [u8; 17] =
    [0xFF, 0xC6, 0x41, 0xD1, 0xC6, 0x48, 0x81, 0xC3, 0x50, 0x01, 0x00, 0x00, 0x83, 0xFE, 0x15, 0x0F, 0x8C];

/// Quick Menu action case `0x27` (Change Secondary Weapon row, also replayed by hotbar binds):
/// `mov edx,[rsi+0x50]; lea rcx,[rax+0x25FC40]; call select; lea rcx,[rip+..]` at `0x1415667a5`. 1 hit.
/// The weapon object moved from `+0x25FB40` to `+0x25FC40` in build 183791.
pub const ACTION_SITE: &str = "8B 56 50 48 8D 88 40 FC 25 00 E8 ? ? ? ? 48 8D 0D";
/// Offset of the `E8` in `ACTION_SITE` (the call at `0x1415667af`).
pub const ACTION_CALL_AT: usize = 10;

/// Secondary select `0x141435af0(W, mode) -> bool`: refuses a mode whose `G+0xE68` bit is clear.
/// Includes the `mov ebp,[rcx+0xF78]` (secondary slot). 1 hit. Located only (its address is taken from the action call, checked against this).
pub const SELECT: &str =
    "48 89 5C 24 08 48 89 6C 24 10 48 89 74 24 18 57 48 83 EC 20 48 8B 35 ? ? ? ? 48 8B D9 8B A9 78 0F 00 00";
/// Offset of the global's `disp32` in `SELECT`.
pub const SELECT_G_DISP: usize = 0x17;
/// In `SELECT` at this offset: `mov eax,[rsi+rax*4+0xE68]; bt eax,r8d`, the secondary mask read.
pub const SELECT_MASK_AT: usize = 0x3B;
pub const SELECT_MASK_CODE: [u8; 11] = [0x8B, 0x84, 0x86, 0x68, 0x0E, 0x00, 0x00, 0x44, 0x0F, 0xA3, 0xC0];

/// SetPrimary `0x141430990(W, mode)`: runs CanEquip, parks the mode in `[W+0xF6C]` while the player is
/// busy, else tears down the TerrainEdit / FishLaser / Gravity helpers, writes `[W+0xF74]`, saves it and
/// plays the switch sound. 7 callers (the vanilla weapon-switch key among them). 1 hit.
pub const SET_PRIMARY: &str = "48 89 5C 24 20 57 48 83 EC 60 48 63 DA 48 8B F9 8B D3 E8 ? ? ? ? 84 C0 0F 84";
/// Offset of the `E8` to CanEquip in `SET_PRIMARY`.
pub const SET_PRIMARY_CAN_CALL: usize = 0x12;
/// CanEquip `0x141430da0(W, mode) -> bool`: Melee always; refuses a secondary while the Fishing Rig or
/// Gravity tool is held; then the owned byte. Includes `mov ecx,[rcx+0xF74]`. 1 hit.
pub const CAN_EQUIP: &str = "40 57 48 83 EC 20 48 63 FA 83 FF 0A 75 08 B0 01 48 83 C4 20 5F C3 8B 89 74 0F 00 00";

/// Secondary-weapon mask inside the global (`G+0xE68`): bit set = secondary mode.
pub const MASK_OFFSET: usize = 0xE68;
/// Weapon fields read for the log only: pending primary, primary, secondary.
pub const W_PENDING: usize = 0xF6C;
pub const W_PRIMARY: usize = 0xF74;
pub const W_SECONDARY: usize = 0xF78;

pub const TABLE: ModSigs = ModSigs {
    owner: "ToolModeBinds",
    sigs: &[
        Sig { name: "FILTER_LOOP", pattern: FILTER_LOOP, kind: SigUse::Locate },
        Sig { name: "ACTION_SITE", pattern: ACTION_SITE, kind: SigUse::Locate },
        Sig { name: "SELECT", pattern: SELECT, kind: SigUse::Locate },
        Sig { name: "SET_PRIMARY", pattern: SET_PRIMARY, kind: SigUse::Locate },
        Sig { name: "CAN_EQUIP", pattern: CAN_EQUIP, kind: SigUse::Locate },
    ],
    patches: &[],
};
