//! The core's own signatures: game context and the lifecycle hook (`nms_core::game`).

use crate::{CodePatch, ModSigs, Sig, SigUse};
use nms_core::game::sigs as g;

pub const OWNER: &str = "NMS.rs core";

/// Boot step 11 (`CheckModWarning` in `cGcApplicationGameModeSelectorState::Update`) asks
/// `cTkFileSystem::IsModded()`. The patch turns `mov rcx,rax; call IsModded` into
/// `xor eax,eax; jmp +4`, so the step takes the game's own skip path to step 13. The "modded" flag
/// itself stays set for every other reader (crash dumps, the crash dialog, the system log), and mod
/// loading never runs this code. Research: `RESEARCH-mod-warning.md` (private repo), candidate A.
pub const MOD_WARNING: CodePatch = CodePatch {
    label: "Hide mod warning",
    sig: "E8 ? ? ? ? 48 8B C8 E8 ? ? ? ? E9 ? ? ? ? E8 ? ? ? ? 48 8D 97 ? ? ? ? 48 8D 48 18",
    at: 0x05,
    old: &[0x48, 0x8B, 0xC8, 0xE8],
    new: &[0x31, 0xC0, 0xEB, 0x04],
};

pub const TABLE: ModSigs = ModSigs {
    owner: OWNER,
    sigs: &[
        Sig { name: "UPDATE", pattern: g::UPDATE, kind: SigUse::Hook },
        Sig { name: "UPDATE_TAIL", pattern: g::UPDATE_TAIL, kind: SigUse::Locate },
        Sig { name: "FSM_UPDATE", pattern: g::FSM_UPDATE, kind: SigUse::Locate },
        Sig { name: "FSM_CONSTRUCT", pattern: g::FSM_CONSTRUCT, kind: SigUse::Locate },
        Sig { name: "FSM_CONSTRUCT_CALL", pattern: g::FSM_CONSTRUCT_CALL, kind: SigUse::Locate },
        Sig { name: "APP_GETTERS", pattern: g::APP_GETTERS, kind: SigUse::Locate },
    ],
    patches: &[MOD_WARNING],
};
