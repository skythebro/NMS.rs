//! From `mods/HideEmptyHover/hide_empty_hover.py`.

use crate::{CodePatch, ModSigs, Sig, SigUse};

/// Same pattern as nmspy `DoEmptySlotPopup`. Readiness check only; hooks use NMSPY entries.
pub const POPUP: &str = "4C 89 4C 24 ? 4C 89 44 24 ? 48 89 54 24 ? 48 89 4C 24 ? \
55 53 56 57 41 55 41 56 41 57 48 8D AC 24";
/// `SetPopupBasics` (11 args). Build 183791 changed the prologue (no `mov rax, rsp`, r8 homed instead of r9);
/// the body still starts by hashing stack arg 11 (`[rbp+0x87]`). Readiness check only.
pub const BASICS: &str = "48 89 5C 24 ? 48 89 74 24 ? 48 89 7C 24 ? 4C 89 44 24 ? 55 41 54 41 55 41 56 41 57 \
48 8D 6C 24 ? 48 81 EC ? ? ? ? 48 8B 85 ? ? ? ? 4D 8B E1 4C 8B FA 4C 8B E9 48 83 38 00";
pub const HASH: &str = "4C 8B D9 45 8B D0 0F 57 C0 0F 11 01 0F 10 02 0F 11 01 4C 8B 0A";
pub const LOOKUP: &str = "40 55 57 41 57 48 83 EC 20 4C 8B 89 50 01 00 00";

pub const NMSPY_DO_EMPTY_SLOT_POPUP: &str = "4C 89 4C 24 ? 4C 89 44 24 ? 48 89 54 24 ? 48 89 4C 24 ? \
55 53 56 57 41 55 41 56 41 57 48 8D AC 24";
/// Same as `BASICS` (the old nmspy pattern no longer matches build 183791).
pub const NMSPY_SET_POPUP_BASICS: &str = "48 89 5C 24 ? 48 89 74 24 ? 48 89 7C 24 ? 4C 89 44 24 ? 55 41 54 41 55 41 56 41 57 \
48 8D 6C 24 ? 48 81 EC ? ? ? ? 48 8B 85 ? ? ? ? 4D 8B E1 4C 8B FA 4C 8B E9 48 83 38 00";

pub const TABLE: ModSigs = ModSigs {
    owner: "HideEmptyHover",
    sigs: &[
        Sig { name: "POPUP", pattern: POPUP, kind: SigUse::Locate },
        Sig { name: "BASICS", pattern: BASICS, kind: SigUse::Locate },
        Sig { name: "HASH", pattern: HASH, kind: SigUse::Locate },
        Sig { name: "LOOKUP", pattern: LOOKUP, kind: SigUse::Locate },
        Sig { name: "NMSPY_DoEmptySlotPopup", pattern: NMSPY_DO_EMPTY_SLOT_POPUP, kind: SigUse::Hook },
        Sig { name: "NMSPY_SetPopupBasics", pattern: NMSPY_SET_POPUP_BASICS, kind: SigUse::Hook },
    ],
    patches: &[] as &[CodePatch],
};
