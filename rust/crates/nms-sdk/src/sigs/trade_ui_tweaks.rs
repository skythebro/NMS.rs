//! From `mods/TradeUITweaks/trade_ui_tweaks.py`.

use crate::{CodePatch, ModSigs, Sig, SigUse};

pub const SHOP_Q: &str = "44 88 4C 24 20 4C 89 44 24 18 48 89 54 24 10 48 89 4C 24 08 \
55 53 56 57 41 54 41 55 41 56 41 57";
pub const SHOP_BUYFOR: &str = "41 B8 20 00 00 00 48 8D 95 50 01 00 00 41 8B CC E8 ? ? ? ? \
C6 85 60 0A 00 00 00";
pub const SHOP_LIST: &str = "48 8D 15 ? ? ? ? 44 8B 45 00 49 8B CF 41 B9 06 00 00 00";
pub const SHOP_PAGE: &str = "48 89 5C 24 08 48 89 6C 24 10 4C 89 44 24 18 56 57 41 56 \
48 83 EC 40 65 48 8B 04 25 58 00 00 00";
pub const SHOP_WALLET: &str = "48 8B 05 ? ? ? ? 48 05 70 0E 00 00 C3";
pub const SHOP_HAS_INV: &str = "48 83 EC 28 48 81 C1 00 09 00 00 41 B8 FF FF FF FF E8";
pub const SHOP_HAS_GLOBAL: &str = "48 8B 0D ? ? ? ? 33 D2 45 8B 67 0C 48 81 C1 40 B9 00 00 33 DB E8";
pub const SHOP_INV_CLEAR: &str = "40 57 48 83 EC 20 48 83 79 20 00 48 8B F9 74 40";
pub const SHOP_CONFIRM: &str = "48 89 6C 24 20 41 54 41 56 41 57 48 83 EC 40 80 B9 44 4C 01 00 00";
pub const SHOP_TOGGLE: &str = "48 8B 0D ? ? ? ? 41 B9 FF FF FF FF 41 B8 01 00 00 00 \
48 8B 89 ? ? ? ? 48 8B 11 4C 8B 52 20 BA D8 00 00 00";
pub const SHOP_CHOICES: &str = "89 54 24 10 48 89 4C 24 08 55 53 56 57 41 57 48 8B EC 48 81 EC 80 00 00 00 \
49 8B 40 08 49 8D 70 08 49 8B D8 41 0F B6 F9";
pub const SHOP_BASE: &str = "4C 8B 0D ? ? ? ? 8B 05 ? ? ? ? 4C 89 7D 68 4C 89 7D 70 44 88 7D 90 4D 8D 14 41 4D 3B CA 74 ? \
41 BB ? ? ? ? 41 0F B7 01 48 8D 8D E0 02 00 00 66 89 85 E0 02 00 00 E8 ? ? ? ? 83 F8 10 77 06 \
41 0F A3 C3 72 0B 49 83 C1 02 4D 3B CA 75 D5 EB 1F 41 0F B7 01 4C 8D 45 68 48 8D 95 E0 02 00 00 \
66 89 85 E0 02 00 00 E8";
pub const SHOP_VIEW: &str = "89 54 24 10 48 83 EC 28 48 8B 01 4C 8B 08 49 63 81 18 0B 00 00 83 F8 FF 74 36 4C 8B D0";

const INV_STORE_0_SIG: &str = "49 8D 8E D8 43 01 00 E8 ? ? ? ? 41 C7 47 0C 00 00 00 00";
const INV_STORE_1_SIG: &str = "49 8D 8E D8 43 01 00 E8 ? ? ? ? 41 C7 47 0C 04 00 00 00";
const INV_STORE_2_SIG: &str = "49 8D 8E D8 43 01 00 E8 ? ? ? ? 41 C7 47 0C 07 00 00 00";
const INV_STORE_3_SIG: &str = "48 81 C1 D8 43 01 00 E8 ? ? ? ? 48 8B 44 24 68 C7 40 0C 00 00 00 00";
const INV_STORE_4_SIG: &str = "48 81 C1 D8 43 01 00 E8 ? ? ? ? 48 8B 44 24 68 C7 40 0C 04 00 00 00";
const INV_STORE_5_SIG: &str = "48 81 C1 D8 43 01 00 E8 ? ? ? ? 48 8B 44 24 68 C7 40 0C 07 00 00 00";

pub const SHOP_INV_STORE_0: CodePatch = CodePatch {
    label: "Shop inv store 0",
    sig: INV_STORE_0_SIG,
    at: 12,
    old: &[0x41, 0xC7, 0x47, 0x0C, 0x00, 0x00, 0x00, 0x00],
    new: &[0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90],
};
pub const SHOP_INV_STORE_1: CodePatch = CodePatch {
    label: "Shop inv store 1",
    sig: INV_STORE_1_SIG,
    at: 12,
    old: &[0x41, 0xC7, 0x47, 0x0C, 0x04, 0x00, 0x00, 0x00],
    new: &[0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90],
};
pub const SHOP_INV_STORE_2: CodePatch = CodePatch {
    label: "Shop inv store 2",
    sig: INV_STORE_2_SIG,
    at: 12,
    old: &[0x41, 0xC7, 0x47, 0x0C, 0x07, 0x00, 0x00, 0x00],
    new: &[0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90],
};
pub const SHOP_INV_STORE_3: CodePatch = CodePatch {
    label: "Shop inv store 3",
    sig: INV_STORE_3_SIG,
    at: 17,
    old: &[0xC7, 0x40, 0x0C, 0x00, 0x00, 0x00, 0x00],
    new: &[0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90],
};
pub const SHOP_INV_STORE_4: CodePatch = CodePatch {
    label: "Shop inv store 4",
    sig: INV_STORE_4_SIG,
    at: 17,
    old: &[0xC7, 0x40, 0x0C, 0x04, 0x00, 0x00, 0x00],
    new: &[0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90],
};
pub const SHOP_INV_STORE_5: CodePatch = CodePatch {
    label: "Shop inv store 5",
    sig: INV_STORE_5_SIG,
    at: 17,
    old: &[0xC7, 0x40, 0x0C, 0x07, 0x00, 0x00, 0x00],
    new: &[0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90],
};

pub const TABLE: ModSigs = ModSigs {
    owner: "TradeUITweaks",
    sigs: &[
        Sig { name: "SHOP_Q", pattern: SHOP_Q, kind: SigUse::Hook },
        // SHOP_BUYFOR: rel32 E8 at SHOP_BUYFOR_AT jumps to a runtime cave; not a fixed CodePatch.
        Sig { name: "SHOP_BUYFOR", pattern: SHOP_BUYFOR, kind: SigUse::Locate },
        Sig { name: "SHOP_LIST", pattern: SHOP_LIST, kind: SigUse::Locate },
        Sig { name: "SHOP_PAGE", pattern: SHOP_PAGE, kind: SigUse::Hook },
        Sig { name: "SHOP_WALLET", pattern: SHOP_WALLET, kind: SigUse::Locate },
        Sig { name: "SHOP_HAS_INV", pattern: SHOP_HAS_INV, kind: SigUse::Locate },
        Sig { name: "SHOP_HAS_GLOBAL", pattern: SHOP_HAS_GLOBAL, kind: SigUse::Locate },
        Sig { name: "SHOP_INV_CLEAR", pattern: SHOP_INV_CLEAR, kind: SigUse::Locate },
        Sig { name: "SHOP_CONFIRM", pattern: SHOP_CONFIRM, kind: SigUse::Hook },
        Sig { name: "SHOP_TOGGLE", pattern: SHOP_TOGGLE, kind: SigUse::Locate },
        Sig { name: "SHOP_CHOICES", pattern: SHOP_CHOICES, kind: SigUse::Locate },
        Sig { name: "SHOP_BASE", pattern: SHOP_BASE, kind: SigUse::Locate },
        Sig { name: "SHOP_VIEW", pattern: SHOP_VIEW, kind: SigUse::Locate },
    ],
    patches: &[
        SHOP_INV_STORE_0,
        SHOP_INV_STORE_1,
        SHOP_INV_STORE_2,
        SHOP_INV_STORE_3,
        SHOP_INV_STORE_4,
        SHOP_INV_STORE_5,
    ],
};
