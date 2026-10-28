//! From `mods/PulseTweaks/pulse_tweaks.py`.

use crate::{CodePatch, ModSigs, Sig, SigUse};

pub const ATMO: &str = "85 FF 0F 84 58 03 00 00 45 84 FF 0F 84 42 03 00 00 83 FF 07 \
0F 8D 39 03 00 00 48 8B C7";
pub const MODE8: &str = "0F 57 C9 C7 86 9C 00 00 00 08 00 00 00 48 8B CE";
/// `@manual_hook("UpdatePulseDrive", ...)`.
pub const UPD: &str = "F3 0F 11 4C 24 10 55 53 56 41 54 41 55 48 8D AC 24 B0 A5 FF FF";
/// `@manual_hook("FindPulseDest", ...)` / `@manual_hook("FindPulseDestAfter", ...)`.
pub const FIND: &str = "48 89 5C 24 10 55 56 57 41 54 41 55 41 56 41 57 48 8D 6C 24 B0 \
48 81 EC 50 01 00 00 8B 9D B0 00";
pub const GATE: &str = "45 84 FF 0F 84 FB 00 00 00 44 39 B6 9C 00 00 00 0F 85 EE 00 00 00";
pub const OBS: &str = "48 8B D7 48 8B CE 0F 11 4F 30 E8 E3 03 00 00 84 C0 75 38 \
66 0F 6F 05 E7 A9 38 03 48 8D 95 80 1A 00 00 44 89 A6 A8 00 00 00";
/// The call and jne targets are wildcarded; `[r14+0x6258]` after them pins the site. The patch still
/// checks the exact old bytes, so a moved call target leaves the game vanilla.
pub const CHARGE_OBS: &str = "48 8B CE E8 ? ? ? ? 84 C0 0F 85 ? ? ? ? 49 8B BE 58 62 00 00 E8";
pub const CRASH: &str = "C6 86 F0 00 00 00 00 0F 84 7A 02 00 00 80 BD 80 5A 00 00 00 \
0F 84 6D 02 00 00";
pub const GEO: &str = "83 E0 07 3C 07 0F 84 31 01 00 00 41 0F 28 C4 41 0F 28 CC \
0F 57 C3 0F 5C D0 0F 55 CA 0F C2 CC 02 0F 50 C1 83 E0 07 3C 07 0F 84 0E 01 00 00";
pub const DEST: &str = "4C 8B 86 90 00 00 00 33 FF 4D 85 C0 74 41";
/// `addss xmm4, [rip+disp]` at +10; the jbe and the displacement move with every build.
pub const SURFACE: &str = "41 0F 2F C8 0F 86 ? ? ? ? F3 0F 58 25 ? ? ? ? 0F 59 DB";
pub const ABORT: &str = "80 7C 24 70 00 44 0F 28 AC 24 B0 5A 00 00 75 0D \
83 BD 20 03 00 00 FF 0F 85 05 18 00 00";
pub const ORBIT: &str = "85 C0 0F 84 31 06 00 00 F3 0F 10 55";
pub const FLAG: &str = "80 7C 24 70 00 44 0F 28 AC 24 B0 5A 00 00 75 0D \
83 BD 20 03 00 00 FF 0F 85 05 18 00 00";
pub const PROX: &str = "84 C0 75 05 0F 2F C8 76 19 C6 44 24 70 01 C6 86 74 01 00 00 00";
/// Starts at the `84` of the first `test`: its register (and REX prefix) changes between builds
/// (r13b in 181442, dil in 183791).
pub const NAME: &str = "84 ? 74 08 84 D2 75 04 84 C9 74 05 C6 44 24 70 01";

pub const PATCH_ATMO_TOAST: CodePatch = CodePatch {
    label: "Pulse in atmosphere (toast)",
    sig: ATMO,
    at: 8,
    old: &[0x45, 0x84, 0xFF, 0x0F, 0x84, 0x42, 0x03, 0x00, 0x00],
    // Old je target plus 0xD: skips the `xor r13b; xor r15b; mov [rbp+..],r15b` block.
    new: &[0x83, 0xFF, 0x04, 0x0F, 0x84, 0x4F, 0x03, 0x00, 0x00],
};

pub const PATCH_GATE: CodePatch = CodePatch {
    label: "Pulse in atmosphere (charge gate)",
    sig: GATE,
    at: 16,
    old: &[0x0F, 0x85, 0xEE, 0x00, 0x00, 0x00],
    new: &[0x90, 0x90, 0x90, 0x90, 0x90, 0x90],
};

pub const PATCH_OBS: CodePatch = CodePatch {
    label: "Pulse in atmosphere (path obstruct)",
    sig: OBS,
    at: 10,
    old: &[0xE8, 0xE3, 0x03, 0x00, 0x00, 0x84, 0xC0, 0x75, 0x38],
    new: &[0x31, 0xC0, 0x90, 0x90, 0x90, 0x84, 0xC0, 0x90, 0x90],
};

pub const PATCH_CHARGE_OBS: CodePatch = CodePatch {
    label: "Pulse in atmosphere (charge obstruct)",
    sig: CHARGE_OBS,
    at: 3,
    old: &[
        0xE8, 0x76, 0x17, 0x00, 0x00, 0x84, 0xC0, 0x0F, 0x85, 0x9E, 0x0F, 0x00, 0x00,
    ],
    new: &[0x31, 0xC0, 0x90, 0x90, 0x90, 0x84, 0xC0, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90],
};

pub const PATCH_CRASH: CodePatch = CodePatch {
    label: "Pulse in atmosphere (planet crash)",
    sig: CRASH,
    at: 7,
    old: &[
        0x0F, 0x84, 0x7A, 0x02, 0x00, 0x00, 0x80, 0xBD, 0x80, 0x5A, 0x00, 0x00, 0x00, 0x0F, 0x84,
        0x6D, 0x02, 0x00, 0x00,
    ],
    new: &[
        0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x80, 0xBD, 0x80, 0x5A, 0x00, 0x00, 0x00, 0x90, 0x90,
        0x90, 0x90, 0x90, 0x90,
    ],
};

pub const PATCH_GEO: CodePatch = CodePatch {
    label: "Pulse in atmosphere (terrain crash)",
    sig: GEO,
    at: 5,
    old: &[
        0x0F, 0x84, 0x31, 0x01, 0x00, 0x00, 0x41, 0x0F, 0x28, 0xC4, 0x41, 0x0F, 0x28, 0xCC, 0x0F,
        0x57, 0xC3, 0x0F, 0x5C, 0xD0, 0x0F, 0x55, 0xCA, 0x0F, 0xC2, 0xCC, 0x02, 0x0F, 0x50, 0xC1,
        0x83, 0xE0, 0x07, 0x3C, 0x07, 0x0F, 0x84, 0x0E, 0x01, 0x00, 0x00,
    ],
    new: &[
        0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x41, 0x0F, 0x28, 0xC4, 0x41, 0x0F, 0x28, 0xCC, 0x0F,
        0x57, 0xC3, 0x0F, 0x5C, 0xD0, 0x0F, 0x55, 0xCA, 0x0F, 0xC2, 0xCC, 0x02, 0x0F, 0x50, 0xC1,
        0x83, 0xE0, 0x07, 0x3C, 0x07, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90,
    ],
};

pub const PATCH_DEST: CodePatch = CodePatch {
    label: "Pulse in atmosphere (end audio)",
    sig: DEST,
    at: 12,
    old: &[0x74, 0x41],
    new: &[0xEB, 0x41],
};

pub const PATCH_ORBIT: CodePatch = CodePatch {
    label: "Pulse in atmosphere (orbit abort)",
    sig: ORBIT,
    at: 0,
    old: &[0x85, 0xC0, 0x0F, 0x84, 0x31, 0x06, 0x00, 0x00],
    new: &[0x3C, 0x01, 0x0F, 0x86, 0x31, 0x06, 0x00, 0x00],
};

pub const PATCH_SURFACE: CodePatch = CodePatch {
    label: "Pulse in atmosphere (sphere stop)",
    sig: SURFACE,
    at: 10,
    // Jump over the 8-byte addss so the bytes checked never include its displacement.
    old: &[0xF3, 0x0F],
    new: &[0xEB, 0x06],
};

pub const PATCH_FLAG: CodePatch = CodePatch {
    label: "Pulse in atmosphere (flag consumer)",
    sig: FLAG,
    at: 14,
    old: &[0x75, 0x0D],
    new: &[0x90, 0x90],
};

pub const PATCH_PROX: CodePatch = CodePatch {
    label: "Pulse in atmosphere (prox abort)",
    sig: PROX,
    at: 2,
    old: &[0x75, 0x05],
    new: &[0xEB, 0x1E],
};

pub const PATCH_NAME: CodePatch = CodePatch {
    label: "Pulse in atmosphere (name abort)",
    sig: NAME,
    at: 4,
    old: &[0x84, 0xD2, 0x75, 0x04],
    new: &[0x90, 0x90, 0x90, 0x90],
};

pub const TABLE: ModSigs = ModSigs {
    owner: "PulseTweaks",
    sigs: &[
        Sig { name: "ATMO", pattern: ATMO, kind: SigUse::Locate },
        Sig { name: "MODE8", pattern: MODE8, kind: SigUse::Locate },
        Sig { name: "UPD", pattern: UPD, kind: SigUse::Hook },
        Sig { name: "FIND", pattern: FIND, kind: SigUse::Hook },
        Sig { name: "GATE", pattern: GATE, kind: SigUse::Locate },
        Sig { name: "OBS", pattern: OBS, kind: SigUse::Locate },
        Sig { name: "CHARGE_OBS", pattern: CHARGE_OBS, kind: SigUse::Locate },
        Sig { name: "CRASH", pattern: CRASH, kind: SigUse::Locate },
        Sig { name: "GEO", pattern: GEO, kind: SigUse::Locate },
        Sig { name: "DEST", pattern: DEST, kind: SigUse::Locate },
        Sig { name: "SURFACE", pattern: SURFACE, kind: SigUse::Locate },
        Sig { name: "ABORT", pattern: ABORT, kind: SigUse::Locate },
        Sig { name: "ORBIT", pattern: ORBIT, kind: SigUse::Locate },
        Sig { name: "FLAG", pattern: FLAG, kind: SigUse::Locate },
        Sig { name: "PROX", pattern: PROX, kind: SigUse::Locate },
        Sig { name: "NAME", pattern: NAME, kind: SigUse::Locate },
    ],
    patches: &[
        PATCH_ATMO_TOAST,
        PATCH_GATE,
        PATCH_OBS,
        PATCH_CHARGE_OBS,
        PATCH_CRASH,
        PATCH_GEO,
        PATCH_DEST,
        PATCH_ORBIT,
        PATCH_SURFACE,
        PATCH_FLAG,
        PATCH_PROX,
        PATCH_NAME,
    ],
};
