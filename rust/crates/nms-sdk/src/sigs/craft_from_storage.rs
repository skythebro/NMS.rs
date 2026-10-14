//! From `mods/CraftFromStorage/craft_from_storage.py`.

use crate::{CodePatch, ModSigs, Sig, SigUse};

pub const CHOICES: &str = "89 54 24 10 48 89 4C 24 08 55 53 56 57 41 57 48 8B EC 48 81 EC 80 00 00 00 \
49 8B 40 08 49 8D 70 08 49 8B D8 41 0F B6 F9";
pub const BUILD_AFFORD: &str =
    "48 8B C4 55 56 48 8D A8 D8 FA FF FF 48 81 EC 18 06 00 00 48 89 58 08 41 B9 04 00 00";
pub const BUILD_PAY: &str =
    "48 8B C4 89 50 10 55 57 41 57 48 8D A8 C8 FA FF FF 48 81 EC 20 06 00 00 48 89 58 08 41 B9 04 00 00 00";
pub const COUNT: &str =
    "40 53 48 83 EC 50 4C 8B 4A 08 4C 8D 91 00 09 00 00 8B 42 04 0F 57 C0 49 8B D8 0F 11";
pub const REMOVE: &str =
    "4C 8B DC 49 89 6B 10 49 89 73 18 49 89 7B 20 41 54 41 56 41 57 48 81 EC B0 00 00 00";
pub const CHARGE: &str = "48 8B C4 48 89 50 10 48 89 48 08 55 48 8D A8 B8 F8 FF FF 48 81 EC 40 08 00 00 \
48 89 58 F0 48 89 70 E8 4C 89 60 D8 4C 8B E2 4C 89 68 D0 4C 8B E9";
pub const SOURCES: &str = "40 55 57 41 54 41 55 41 57 48 81 EC B0 00 00 00 4D 8B F8 48 C7 02 00 00 00 00 \
48 8B FA 48 C7 42 08 00 00 00 00 48 8B E9 E8";
pub const OWNED_LISTS: &str = "48 89 5C 24 10 48 89 4C 24 08 57 48 83 EC 20 48 8B 1D ? ? ? ? 48 8B 3D ? ? ? ? 48 81 C3 ? ? ? ? \
48 89 5C 24 30 48 39 7B 20 74 ? B2 01 48 8D 4C 24 30 E8 ? ? ? ? 88 43 18 48 89 7B 20 80 7B 18 00 \
48 8D 0D ? ? ? ? 48 8B 5C 24 38 48 8D 05 ? ? ? ? 48 0F 45 C1";
pub const BASE: &str = "4C 8B 0D ? ? ? ? 8B 05 ? ? ? ? 4C 89 7D 68 4C 89 7D 70 44 88 7D 90 4D 8D 14 41 4D 3B CA 74 ? \
41 BB ? ? ? ? 41 0F B7 01 48 8D 8D E0 02 00 00 66 89 85 E0 02 00 00 E8 ? ? ? ? 83 F8 10 77 06 \
41 0F A3 C3 72 0B 49 83 C1 02 4D 3B CA 75 D5 EB 1F 41 0F B7 01 4C 8D 45 68 48 8D 95 E0 02 00 00 \
66 89 85 E0 02 00 00 E8";
pub const POPUP: &str = "48 8B C4 48 89 50 10 48 89 48 08 55 53 56 57 41 54 41 55 41 56 41 57 48 8D A8 38 FD FF FF \
48 81 EC 88 03 00 00";
pub const SLOTS: &str = "4C 89 4C 24 20 44 89 44 24 18 48 89 54 24 10 48 89 4C 24 08 55 48 8D AC 24 10 EF FF FF \
B8 F0 11 00 00";
pub const VIEW: &str =
    "89 54 24 10 48 83 EC 28 48 8B 01 4C 8B 08 49 63 81 18 0B 00 00 83 F8 FF 74 36 4C 8B D0";
pub const COOKER_TAB: &str = "83 7B 24 02 0F 85 ? ? ? ? 48 8B 0D ? ? ? ? 48 8B D3 48 83 C1 60 E8 ? ? ? ? 48 85 C0 0F 84 ? ? ? ? \
F3 0F 10 88 A0 01 00 00";
pub const COOKER_STRIP: &str = "45 84 F6 0F 84 83 00 00 00 48 8B 8D D0 02 00 00";
pub const COOKER_FILTER: &str = "8B 83 1C 0B 00 00 85 C0 75 0C C7 45 78 04";
pub const ORDER: &str = "4C 8B 46 08 41 B3 01 8B 56 04";
pub const COOK_ITEM: &str = "48 83 EC 28 8B 41 24 48 8B D1 85 C0 75 21 48 8B 0D ? ? ? ? 48 83 C1 60 E8 ? ? ? ? \
48 85 C0 74 32 0F B6 80 94 01 00 00";
pub const QUICK_ROW: &str =
    "48 89 5C 24 08 57 48 81 EC B0 00 00 00 48 8B 79 08 48 8B D9 8B 49 04 4C 8B CA 3B 0B 73 32";
pub const QUICK_FREIGHTER: &str = "39 75 B4 0F 84 FD 00 00 00";
pub const QUICK_TEXT: &str = "48 63 0C 38 48 8B C1 C7 85 C4 01 00 00 3B 00 00 00 48 C1 E0 05 C7 85 C8 01 00 00 3E 00 00 00 \
0F 10 44 05 40 0F 10 4C 05 50 8B 84 8D A0 01 00 00";
pub const QUICK_DEST: &str = "48 89 5C 24 08 57 48 81 EC B0 00 00 00 48 8B 79 08 48 8B D9 8B 49 04 \
4C 8B CA 3B 0B 73 2A 41 8B 01 0F 57 C0 48 6B D1 70 41 0F B6 08 48 03 D7 \
89 02 88 4A 08 C6 42 30 00 0F 11 42 10 0F 11 42 20 FF 43 04 E9 A5 00 00 00 \
48 8D 05 ? ? ? ? 4C 89 84 24 80 00 00 00 48 89 44 24 70 44 8D 41 01 \
48 8D 44 24 70 4C 89 4C 24 78 48 89 84 24 A8 00 00 00 48 8D 15 ? ? ? ? \
33 C0 4C 8B CF 48 89 44 24 68 48 89 7C 24 60 C7 44 24 58 FF FF FF FF \
48 C7 44 24 50 08 00 00 00 48 C7 44 24 48 70 00 00 00 48 89 44 24 40 \
48 89 44 24 38 48 8D 44 24 70 48 C7 44 24 30 01 00 00 00 48 89 44 24 28 \
48 89 4C 24 20 48 8B CB E8 ? ? ? ? 48 8B 8C 24 A8 00 00 00 48 8B F8 \
48 85 C9 74 11 48 8D 44 24 70 48 3B C8 48 8B 01 0F 95 C2 FF 50 20 \
8B 43 04 FF C8 48 89 7B 08 48 8B 9C 24 C0 00 00 00 48 6B C0 70 48 03 C7 \
48 81 C4 B0 00 00 00 5F C3 CC CC \
48 89 5C 24 08 57 48 81 EC B0 00 00 00 48 8B 79 08 48 8B D9 8B 49 04 \
4C 8B CA 3B 0B 73 31";
pub const QUICK_FREE: &str = "48 89 5C 24 18 48 89 74 24 20 55 57 41 54 41 55 41 56 48 8D 6C 24 C9 48 81 EC C0 00 00 00 \
44 8B 01 45 33 E4 4C 8B EA";
pub const QUICK_PICK: &str = "48 89 5C 24 18 48 89 74 24 20 55 41 56 41 57 48 8B EC 48 81 EC 80 00 00 00 48 8B DA \
4C 8B F9 49 8B D1 49 8B C8 49 8B F1";
pub const OBJECT_CHESTS: &str =
    "4C 8B DC 49 89 4B 08 53 57 48 81 EC C8 00 00 00 0F B7 0A B8 FF FF 00 00";
pub const QUICK_BUTTON: &str = "48 8B C4 48 89 58 08 44 88 48 20 55 56 57 41 54 41 55 41 56 41 57 48 81 EC 40 01 00 00 \
0F 29 70 B8 48 8B F9 0F 29 78 A8";

pub const REFINER_TAB: CodePatch = CodePatch {
    label: "Refiner: storage tabs",
    sig: "8B 90 88 44 01 00 83 FA 35 74 ? 8D 42 C6 83 F8 02 76 ? 8D 42 B2 83 F8 01 76 ?",
    at: 0x0B,
    old: &[0x8D, 0x42, 0xC6, 0x83, 0xF8, 0x02],
    new: &[0x8D, 0x42, 0xC5, 0x83, 0xF8, 0x01],
};

pub const COOKER_TAB_PATCH: CodePatch = CodePatch {
    label: "Cooker Processed tab",
    sig: COOKER_TAB,
    at: 0x06,
    old: &[0x82, 0x03, 0x00, 0x00],
    new: &[0x58, 0x04, 0x00, 0x00],
};

pub const COOKER_FILTER_PATCH: CodePatch = CodePatch {
    label: "Cooker filter",
    sig: COOKER_FILTER,
    at: 0x00,
    old: &[0x8B, 0x83, 0x1C, 0x0B, 0x00, 0x00],
    new: &[0xB8, 0xFF, 0xFF, 0xFF, 0xFF, 0x90],
};

pub const COOKER_STRIP_PATCH: CodePatch = CodePatch {
    label: "Cooker inventory tabs",
    sig: COOKER_STRIP,
    at: 0x03,
    old: &[0x0F, 0x84, 0x83, 0x00, 0x00, 0x00],
    new: &[0xE9, 0x84, 0x00, 0x00, 0x00, 0x90],
};

pub const ORDER_PATCH: CodePatch = CodePatch {
    label: "All tab order",
    sig: ORDER,
    at: 0x04,
    old: &[0x41, 0xB3, 0x01],
    new: &[0x41, 0xB3, 0x00],
};

// QUICK_TEXT: patch replaces movups with a rel32 jmp to QUICK_CAVE (allocated at runtime).
// QUICK_FREIGHTER: patch embeds rel32 call to QUICK_ROW and rel32 jmp to join (computed at runtime).

pub const TABLE: ModSigs = ModSigs {
    owner: "CraftFromStorage",
    sigs: &[
        Sig { name: "CHOICES", pattern: CHOICES, kind: SigUse::Hook },
        Sig { name: "BUILD_AFFORD", pattern: BUILD_AFFORD, kind: SigUse::Hook },
        Sig { name: "BUILD_PAY", pattern: BUILD_PAY, kind: SigUse::Hook },
        Sig { name: "COUNT", pattern: COUNT, kind: SigUse::Hook },
        Sig { name: "REMOVE", pattern: REMOVE, kind: SigUse::Hook },
        Sig { name: "CHARGE", pattern: CHARGE, kind: SigUse::Hook },
        Sig { name: "SOURCES", pattern: SOURCES, kind: SigUse::Hook },
        Sig { name: "OWNED_LISTS", pattern: OWNED_LISTS, kind: SigUse::Locate },
        Sig { name: "BASE", pattern: BASE, kind: SigUse::Locate },
        Sig { name: "POPUP", pattern: POPUP, kind: SigUse::Hook },
        Sig { name: "SLOTS", pattern: SLOTS, kind: SigUse::Hook },
        Sig { name: "VIEW", pattern: VIEW, kind: SigUse::Locate },
        Sig { name: "REFINER_TAB", pattern: REFINER_TAB.sig, kind: SigUse::Locate },
        Sig { name: "COOKER_TAB", pattern: COOKER_TAB, kind: SigUse::Locate },
        Sig { name: "COOKER_STRIP", pattern: COOKER_STRIP, kind: SigUse::Locate },
        Sig { name: "COOKER_FILTER", pattern: COOKER_FILTER, kind: SigUse::Locate },
        Sig { name: "ORDER", pattern: ORDER, kind: SigUse::Locate },
        Sig { name: "COOK_ITEM", pattern: COOK_ITEM, kind: SigUse::Hook },
        Sig { name: "QUICK_ROW", pattern: QUICK_ROW, kind: SigUse::Hook },
        Sig { name: "QUICK_FREIGHTER", pattern: QUICK_FREIGHTER, kind: SigUse::Locate },
        Sig { name: "QUICK_TEXT", pattern: QUICK_TEXT, kind: SigUse::Locate },
        Sig { name: "QUICK_DEST", pattern: QUICK_DEST, kind: SigUse::Hook },
        Sig { name: "QUICK_FREE", pattern: QUICK_FREE, kind: SigUse::Hook },
        Sig { name: "QUICK_PICK", pattern: QUICK_PICK, kind: SigUse::Hook },
        Sig { name: "OBJECT_CHESTS", pattern: OBJECT_CHESTS, kind: SigUse::Hook },
        Sig { name: "QUICK_BUTTON", pattern: QUICK_BUTTON, kind: SigUse::Hook },
    ],
    patches: &[REFINER_TAB, COOKER_TAB_PATCH, COOKER_FILTER_PATCH, COOKER_STRIP_PATCH, ORDER_PATCH],
};
