//! Galaxy map filters. Offsets re-read in the disassembly 2026-10-12 (cycle poll, gate, list builder, colour apply).

use crate::{CodePatch, ModSigs, Sig, SigUse};

/// `cGcGalaxyMapUI::SolarInfoPanel::UpdatePanelUI(panel)`: draws the hovered-system card.
/// Takes only `rcx`; the descriptor flags (`+0xF80..+0xF83`) are filled before it runs.
pub const UPDATE_PANEL_UI: &str = "48 89 4C 24 ? 55 53 41 55 41 56 48 8D AC 24 ? ? ? ? B8 18 10 00 00";

/// `ApplyFilterColours(data, packed_voxel: u64, filter: u32)`: bakes star colours for one voxel.
pub const APPLY_COLOURS: &str = "48 89 54 24 ? 55 53 57 41 54 48 8D AC 24 ? ? ? ? 48 81 EC 48 07 00 00";
/// `HasFilterScanner(_, filter: u32) -> bool`: false hides and skips a filter index.
pub const FILTER_GATE: &str = "48 89 4C 24 ? 53 55 56 57 41 56 48 83 EC ? 48 8B 0D ? ? ? ? BE 04 00 00 00";
/// Per-frame Previous/Next Filter poll `(data)`. Cycles `data+0x20` (0..3) with a mod-4 wrap.
pub const FILTER_POLL: &str = "48 8B C4 55 48 81 EC 40 04 00 00 48 89 58 08 48 8B E9 48 89 70 10 BA C1 00 00 00";
/// Filter setter `(data, index)`: writes `data+0x20` and re-bakes every voxel.
pub const FILTER_SETTER: &str = "40 55 56 48 83 EC 48 8B EA 48 8B F1 3B 51 20";
/// `LookupVoxelStars(voxel_cache, &packed_voxel) -> blob`: count `+0x254`, UA array `+0x258`, nodes `+0x10`.
pub const LOOKUP_STARS: &str = "48 89 5C 24 ? 57 48 83 EC ? 48 8B DA 48 8B F9 33 D2 E8 ? ? ? ? 48 8B 47 58";
/// `ClassifyStarSystem(ua, &attrs)` fills `cGcGalaxyStarAttributesData` (size `0x6B0`).
pub const CLASSIFY_STAR: &str = "48 89 54 24 10 55 53 56 57 41 54 41 55 41 56 48 8B EC 48 81 EC 80 00 00 00";
/// `cGcGalaxyStarAttributesData::SetDefaults(&attrs)`.
pub const SET_DEFAULTS: &str = "48 89 5C 24 08 48 89 6C 24 10 48 89 74 24 18 48 89 7C 24 20 41 56 48 83 EC ? 48 8B D9 48 8D 41";
/// Filter list builder: `lea rsi,[loc ids]` (disp32 at +7), ids are 0x20 apart.
pub const LIST_IDS: &str = "48 63 57 04 48 8D 35 ? ? ? ? 48 C1 E2 05 48 03 D6";
/// Filter list builder loop end: `cmp ebx, 4` (imm8 at +8).
pub const LIST_BOUND: &str = "FF C3 48 83 C6 20 83 FB ? 0F 8C ? ? ? ?";

/// The wrap and try count of the Previous Filter step, from 4 to 32 entries.
pub const POLL_PREV: CodePatch = CodePatch {
    label: "Galaxy map: Previous Filter wraps over 32 entries",
    sig: "41 8D 76 03 8B DE 81 E3 03 00 00 80 7D 07 FF CB 83 CB FC FF C3 8B D3 E8",
    at: 3,
    old: &[0x03, 0x8B, 0xDE, 0x81, 0xE3, 0x03, 0x00, 0x00, 0x80, 0x7D, 0x07, 0xFF, 0xCB, 0x83, 0xCB, 0xFC],
    new: &[0x1F, 0x8B, 0xDE, 0x81, 0xE3, 0x1F, 0x00, 0x00, 0x80, 0x7D, 0x07, 0xFF, 0xCB, 0x83, 0xCB, 0xE0],
};
pub const POLL_PREV_TRIES: CodePatch = CodePatch {
    label: "Galaxy map: Previous Filter tries 32 entries",
    sig: "FF C7 FF CE 83 FF 04 7C ? EB",
    at: 6,
    old: &[0x04],
    new: &[0x20],
};
pub const POLL_NEXT: CodePatch = CodePatch {
    label: "Galaxy map: Next Filter wraps over 32 entries",
    sig: "42 8D 1C 37 81 E3 03 00 00 80 7D 07 FF CB 83 CB FC FF C3 8B D3 E8",
    at: 6,
    old: &[0x03, 0x00, 0x00, 0x80, 0x7D, 0x07, 0xFF, 0xCB, 0x83, 0xCB, 0xFC],
    new: &[0x1F, 0x00, 0x00, 0x80, 0x7D, 0x07, 0xFF, 0xCB, 0x83, 0xCB, 0xE0],
};
pub const POLL_NEXT_TRIES: CodePatch = CodePatch {
    label: "Galaxy map: Next Filter tries 32 entries",
    sig: "FF C7 83 FF 04 7C ? EB 03 44 8B F3",
    at: 4,
    old: &[0x04],
    new: &[0x20],
};

pub const TABLE: ModSigs = ModSigs {
    owner: "GalaxyMapFilters",
    sigs: &[
        Sig { name: "UPDATE_PANEL_UI", pattern: UPDATE_PANEL_UI, kind: SigUse::Hook },
        Sig { name: "APPLY_COLOURS", pattern: APPLY_COLOURS, kind: SigUse::Hook },
        Sig { name: "FILTER_GATE", pattern: FILTER_GATE, kind: SigUse::Hook },
        Sig { name: "FILTER_POLL", pattern: FILTER_POLL, kind: SigUse::Hook },
        Sig { name: "FILTER_SETTER", pattern: FILTER_SETTER, kind: SigUse::Locate },
        Sig { name: "LOOKUP_STARS", pattern: LOOKUP_STARS, kind: SigUse::Locate },
        Sig { name: "CLASSIFY_STAR", pattern: CLASSIFY_STAR, kind: SigUse::Locate },
        Sig { name: "SET_DEFAULTS", pattern: SET_DEFAULTS, kind: SigUse::Locate },
        Sig { name: "LIST_IDS", pattern: LIST_IDS, kind: SigUse::Locate },
        Sig { name: "LIST_BOUND", pattern: LIST_BOUND, kind: SigUse::Locate },
    ],
    patches: &[POLL_PREV, POLL_PREV_TRIES, POLL_NEXT, POLL_NEXT_TRIES],
};
