//! From `mods/QuickTransferAll/quick_transfer_all.py`.

use crate::{CodePatch, ModSigs, Sig, SigUse};

pub const TRANSFER: &str = "48 89 5C 24 ? 48 89 74 24 ? 55 41 56 41 57 48 8B EC 48 81 EC ? ? ? ?  \
48 8B DA 4C 8B F9 49 8B D1 49 8B C8 49 8B F1 4D 8B F0 E8";
pub const SORT: &str = "48 8B C4 55 41 57 48 8D 68 A8 48 81 EC 48 01 00 00 48 89 58 10 48 89 70 18 8B F2";
pub const ADD: &str = "48 89 5C 24 ? 48 89 74 24 ? 55 57 41 56 48 8D 6C 24 ? 48 81 EC ? ? ? ? 41 0F 10 00";
pub const TAKE: &str = "48 83 EC 48 44 8B D2 41 BB FF FF FF FF 41 83 EA 04 74";
/// `AWARD_TOAST_AT`: toggles five-byte `E8` call to NOP (original rel32 captured at runtime).
pub const AWARD_UNITS: &str = "48 89 5C 24 ? 57 48 83 EC ? 44 8B 81 ? ? ? ? 48 8B D9 66 0F 6F 05 ? ? ? ?  \
41 8B C0 F7 D0 48 C7 44 24 ? ? ? ? ? 3B D0 8B FA F3 0F 7F 44 24 ? 0F 42 C2  \
45 33 C9 44 03 C0 48 8D 54 24 ? 44 89 81 ? ? ? ? 48 8B 0D ? ? ? ?  \
48 81 C1 ? ? ? ? E8 ? ? ? ? 85 FF 7E ? 48 8B 0D";
/// Same voice site as `AWARD_UNITS` (`AWARD_TOAST_AT`).
pub const AWARD_NANITES: &str = "48 89 5C 24 ? 57 48 83 EC ? 44 8B 81 ? ? ? ? 48 8B D9 66 0F 6F 05 ? ? ? ?  \
41 8B C0 F7 D0 48 C7 44 24 ? ? ? ? ? 3B D0 8B FA F3 0F 7F 44 24 ? 0F 42 C2  \
45 33 C9 44 03 C0 48 8D 54 24 ? 44 89 81 ? ? ? ? 48 8B 0D ? ? ? ?  \
48 81 C1 ? ? ? ? E8 ? ? ? ? 85 FF 7E ? 4C 8B 15";
pub const SELL_DEMAND: &str = "48 89 5C 24 08 48 89 6C 24 10 48 89 74 24 18 48 89 7C 24 20  \
41 56 48 83 EC 50 80 3D";

pub const BASE: &str = "4C 8B 0D ? ? ? ? 8B 05 ? ? ? ? 4C 89 7D 68 4C 89 7D 70 44 88 7D 90 4D 8D 14 41 4D 3B CA 74 ?  \
41 BB ? ? ? ? 41 0F B7 01 48 8D 8D E0 02 00 00 66 89 85 E0 02 00 00 E8 ? ? ? ? 83 F8 10 77 06  \
41 0F A3 C3 72 0B 49 83 C1 02 4D 3B CA 75 D5 EB 1F 41 0F B7 01 4C 8D 45 68 48 8D 95 E0 02 00 00  \
66 89 85 E0 02 00 00 E8";
pub const CHOICES: &str = "89 54 24 10 48 89 4C 24 08 55 53 56 57 41 57 48 8B EC 48 81 EC 80 00 00 00  \
49 8B 40 08 49 8D 70 08 49 8B D8 41 0F B6 F9";
pub const CONFIRM: &str = "48 89 6C 24 20 41 54 41 56 41 57 48 83 EC 40 80 B9 44 4C 01 00 00";
pub const CONFIRM_SLOW_LOAD: &str = "83 F8 02 75 0A F3 0F 10 35 ? ? ? ?";
pub const DEPOSIT_LOOKUP: &str = "49 8B 8E 78 44 01 00 48 8D 55 B0 C7 44 24 7C 00 00 00 00 45 33 C0 0F 28 44 24 70  \
66 0F 7F 45 B0 E8 ? ? ? ? 48 85 C0 0F 84 ? ? ? ? 45 33 C0 66 0F 7F 7D B0 48 8D 55 B0  \
48 8B C8 E8 ? ? ? ? 48 8B F8 48 85 C0 0F 84 ? ? ? ? 45 33 C0 48 8D 15 ? ? ? ?  \
48 8D 4D B0 E8 ? ? ? ? 48 8B D0 45 33 C0 48 8B CF E8 ? ? ? ? 48 85 C0 74 35  \
48 8B 48 48 40 84 F6 74 28 C6 41 61 00 48 8B C8 E8 ? ? ? ?";
/// Patch head `DEPOSIT_PATCH_BYTES`; new bytes are `E9` rel32 to a cave (runtime).
pub const DEPOSIT_PATCH: &str = "48 C7 45 90 FF FF FF FF 48 C7 45 88 FF FF FF FF 4D 85 FF 74 24 41 B1 01";
/// Lock dim steals `FILTER_STEAL` at match start; new bytes are `E9` rel32 to a cave when lock mode is on.
pub const FILTER: &str = "41 8B 44 24 24 32 DB 44 8B B7 ? ? ? ? 85 C0 75 11 41 83 FE 01 0F 86 E3 00 00 00 B3 01 E9 DC 00 00 00  \
83 F8 01 75 11 45 85 F6 0F 84 CE 00 00 00 0F B6 D8 E9 C6 00 00 00 83 F8 02 0F 85 BD 00 00 00  \
48 8B 0D ? ? ? ? 49 8B D4 48 83 C1 60 E8 ? ? ? ? 41 83 FE 01 75 09 41 0F B6 DE E9 9B 00 00 00  \
48 85 C0 74 6E 41 83 FE 02 75 25 81 B8 94 01 00 00 A0 86 01 00 0F 8D 80 00 00 00  \
38 98 F4 02 00 00 75 78 44 39 B0 E8 01 00 00 74 6F B3 01 EB 6B 41 83 FE 03 75 3D  \
48 83 B8 10 01 00 00 00 75 2F 48 83 B8 18 01 00 00 00 75 25 83 B8 E8 01 00 00 01 74 48  \
48 8B 0D ? ? ? ? 48 8D 90 50 01 00 00 48 83 C1 60 E8 ? ? ? ?";
/// Patch head `NOTIFY_PATCH_BYTES`; new bytes are `E9` rel32 to a cave (runtime).
pub const NOTIFY_PATCH: &str = "44 8B 7C 24 74 4C 8B A5 98 04 00 00";
pub const NOTIFY_TOAST: &str = "48 8B 1D ? ? ? ? 48 8D 4C 24 78 C7 44 24 78 00 00 00 00";
pub const SAVE_ID: &str = "49 8B ? 80 89 00 00 49 89 ? E8 87 01 00";
/// `SELL_TOAST_AT`: toggles five-byte `E8` call to NOP (original rel32 captured at runtime).
pub const SELL_TOAST: &str = "45 33 C9 4C 89 6C 24 20 45 8B C4 48 8D 95 B0 03 00 00 48 8D 8B 10 6A 30 00 E8";
/// `SELL_VOICE_AT`: toggles five-byte `E8` call to NOP (original rel32 captured at runtime).
pub const SELL_VOICE: &str = "48 8D 55 80 66 0F 7F 45 80 48 8B CF E8";
pub const VIEW: &str = "89 54 24 10 48 83 EC 28 48 8B 01 4C 8B 08 49 63 81 18 0B 00 00 83 F8 FF 74 36 4C 8B D0";
pub const WALLET: &str = "48 8B 05 ? ? ? ? 48 05 70 0E 00 00 C3";

/// nmspy `cGcFrontendPageFunctions.DoInventorySlots` (before/after hooks).
pub const NMSPY_DO_INVENTORY_SLOTS: &str = "48 85 D2 0F 84 ? ? ? ? 48 8B C4 4C 89 48 ? 4C 89 40";

pub const TABLE: ModSigs = ModSigs {
    owner: "QuickTransferAll",
    sigs: &[
        Sig { name: "TRANSFER", pattern: TRANSFER, kind: SigUse::Hook },
        Sig { name: "SORT", pattern: SORT, kind: SigUse::Hook },
        Sig { name: "ADD", pattern: ADD, kind: SigUse::Hook },
        Sig { name: "TAKE", pattern: TAKE, kind: SigUse::Hook },
        Sig { name: "AWARD_UNITS", pattern: AWARD_UNITS, kind: SigUse::Hook },
        Sig { name: "AWARD_NANITES", pattern: AWARD_NANITES, kind: SigUse::Hook },
        Sig { name: "SELL_DEMAND", pattern: SELL_DEMAND, kind: SigUse::Hook },
        Sig { name: "BASE", pattern: BASE, kind: SigUse::Locate },
        Sig { name: "CHOICES", pattern: CHOICES, kind: SigUse::Locate },
        Sig { name: "CONFIRM", pattern: CONFIRM, kind: SigUse::Locate },
        Sig { name: "CONFIRM_SLOW_LOAD", pattern: CONFIRM_SLOW_LOAD, kind: SigUse::Locate },
        Sig { name: "DEPOSIT_LOOKUP", pattern: DEPOSIT_LOOKUP, kind: SigUse::Locate },
        Sig { name: "DEPOSIT_PATCH", pattern: DEPOSIT_PATCH, kind: SigUse::Locate },
        Sig { name: "FILTER", pattern: FILTER, kind: SigUse::Locate },
        Sig { name: "NOTIFY_PATCH", pattern: NOTIFY_PATCH, kind: SigUse::Locate },
        Sig { name: "NOTIFY_TOAST", pattern: NOTIFY_TOAST, kind: SigUse::Locate },
        Sig { name: "SAVE_ID", pattern: SAVE_ID, kind: SigUse::Locate },
        Sig { name: "SELL_TOAST", pattern: SELL_TOAST, kind: SigUse::Locate },
        Sig { name: "SELL_VOICE", pattern: SELL_VOICE, kind: SigUse::Locate },
        Sig { name: "VIEW", pattern: VIEW, kind: SigUse::Locate },
        Sig { name: "WALLET", pattern: WALLET, kind: SigUse::Locate },
        Sig { name: "NMSPY_DoInventorySlots", pattern: NMSPY_DO_INVENTORY_SLOTS, kind: SigUse::Hook },
    ],
    patches: &[] as &[CodePatch],
};
