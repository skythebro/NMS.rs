//! From `mods/SmallerInventorySlots/smaller_inventory_slots.py`.
//!
//! Runtime cave jumps (rel32): `install_dim_patch` at FILTER + DIM_AT (DIM_BYTES, also the Foods test);
//! `install_custom_patch` at FILTER + 0x0E (8 bytes `85 C0 75 11 41 83 FE 01`, Custom filter; FILTER
//! + 0..5 stay for QuickTransferAll's lock dim, + 0x0A is the filter field both mods read);
//! `install_button_patch` TIPS_AT, ICON_SITES, and COUNT_AT (cmp r13d, 5 -> 19, fixed byte but past
//! BUTTONS sig span); `install_skip_patch` at BUTTONS + SKIP_AT; `install_hide_patch` at HIDE + HIDE_AT.

use crate::{CodePatch, ModSigs, Sig, SigUse};

pub const FILTER: &str = "41 8B 44 24 24 32 DB 44 8B B7 ? ? ? ? 85 C0 75 11 41 83 FE 01 0F 86 E3 00 00 00 B3 01 E9 DC 00 00 00 \
83 F8 01 75 11 45 85 F6 0F 84 CE 00 00 00 0F B6 D8 E9 C6 00 00 00 83 F8 02 0F 85 BD 00 00 00 \
48 8B 0D ? ? ? ? 49 8B D4 48 83 C1 60 E8 ? ? ? ? 41 83 FE 01 75 09 41 0F B6 DE E9 9B 00 00 00 \
48 85 C0 74 6E 41 83 FE 02 75 25 81 B8 94 01 00 00 A0 86 01 00 0F 8D 80 00 00 00 \
38 98 F4 02 00 00 75 78 44 39 B0 E8 01 00 00 74 6F B3 01 EB 6B 41 83 FE 03 75 3D \
48 83 B8 10 01 00 00 00 75 2F 48 83 B8 18 01 00 00 00 75 25 83 B8 E8 01 00 00 01 74 48 \
48 8B 0D ? ? ? ? 48 8D 90 50 01 00 00 48 83 C1 60 E8 ? ? ? ?";

pub const BUTTONS: &str = "4C 8D 25 ? ? ? ? 66 0F 6F 35 ? ? ? ? 44 8B EB 48 89 5D 88 0F 57 C0 45 8D 45 01 \
48 8D 15 ? ? ? ? 48 8D 4D B0 0F 11 45 B0 E8 ? ? ? ? 49 8B BE ? ? ? ?";

pub const HIDE: &str = "0F 57 C0 4C 8D 2D ? ? ? ? 0F 11 44 24 70 48 8B D3";

pub const HIDE_LOOKUP: &str = "49 8B 8E 78 44 01 00 48 8D 55 B0 C7 44 24 7C 00 00 00 00 45 33 C0 0F 28 44 24 70 \
66 0F 7F 45 B0 E8 ? ? ? ? 48 85 C0 0F 84 ? ? ? ? 45 33 C0 66 0F 7F 7D B0 48 8D 55 B0 \
48 8B C8 E8 ? ? ? ? 48 8B F8 48 85 C0 0F 84 ? ? ? ? 45 33 C0 48 8D 15 ? ? ? ? \
48 8D 4D B0 E8 ? ? ? ? 48 8B D0 45 33 C0 48 8B CF E8 ? ? ? ? 48 85 C0 74 35 \
48 8B 48 48 40 84 F6 74 28 C6 41 61 00 48 8B C8 E8 ? ? ? ?";

pub const SORT: &str = "48 8B C4 55 41 57 48 8D 68 A8 48 81 EC 48 01 00 00 48 89 58 10 48 89 70 18 8B F2";

pub const NMSPY_DO_INVENTORY_SLOTS: &str = "48 85 D2 0F 84 ? ? ? ? 48 8B C4 4C 89 48 ? 4C 89 40";

pub const TABLE: ModSigs = ModSigs {
    owner: "SmallerInventorySlots",
    sigs: &[
        Sig { name: "FILTER", pattern: FILTER, kind: SigUse::Locate },
        Sig { name: "BUTTONS", pattern: BUTTONS, kind: SigUse::Locate },
        Sig { name: "HIDE", pattern: HIDE, kind: SigUse::Locate },
        Sig { name: "HIDE_LOOKUP", pattern: HIDE_LOOKUP, kind: SigUse::Locate },
        Sig { name: "SORT", pattern: SORT, kind: SigUse::Hook },
        Sig { name: "NMSPY_DoInventorySlots", pattern: NMSPY_DO_INVENTORY_SLOTS, kind: SigUse::Hook },
    ],
    patches: &[] as &[CodePatch],
};
