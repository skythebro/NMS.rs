//! From `mods/QuickRecharge/quick_recharge.py`.

use crate::{CodePatch, ModSigs, Sig, SigUse};

/// rcx=menu, rdx=entry, r8b=commit. `@manual_hook("QmTriggerAction", ...)`.
pub const TRIGGER: &str = "44 88 44 24 ? 48 89 4C 24 ? 55 53 56 57 41 54 41 57";
/// Vanilla ChargeMenu one-tech fill (kind + id).
pub const CHARGE1: &str = "48 89 5C 24 10 48 89 6C 24 18 56 57 41 57 48 81 EC 90 00 00 00 44 8B D2";
/// ChargeMenu site: mov edx,[rsi+0x40] / lea r8,[rsi+0x44] / lea rcx,[rax+0xAAD0].
pub const CHARGE_SITE: &str = "8B 56 40 4C 8D 46 44 48 8D 88 D0 AA 00 00";
pub const WALLET: &str = "48 8B 05 ? ? ? ? 48 05 70 0E 00 00 C3";
/// Vanilla charge sound: PostEvent wrapper.
pub const POST: &str = "48 83 EC 48 33 C9 4C 8B D2 89 4C 24 38 49 8B C0 48 89 4C 24 30 45 33 C9 89 4C 24 28 45 33 C0 48 89 4C 24 20 48 8B D0 41 8B 4A 08";
/// Start of TriggerAction: mov rdi,[rip+G] / ... / mov rdi,[rdi+disp].
pub const OBJ: &str = "48 8B 3D ? ? ? ? 45 0F B6 E0 48 8B F2 4C 8B F9 48 8B BF ? ? ? ?";
/// The game's own Charge row (QM action 0x17).
pub const HEAD: &str = "8B 56 40 4C 8D 85 ? ? ? ? 48 8D 88 D0 AA 00 00 E8 ? ? ? ? 48 8D 0D ? ? ? ? E8 ? ? ? ? \
8B 56 40 49 C7 C0 FF FF FF FF 48 8D 88 D0 B3 00 00 E8 ? ? ? ? 48 8D 4E 44 48 89 4C 24 40 \
48 8B D1 48 8B C8 E8 ? ? ? ?";
/// The same row's loop ending in the real charge call. `mov rax,[rbp+0x4218]` is the parent card kept in
/// TriggerAction's frame (`0x41D8` before build 183791, which grew the frame by 0x40).
pub const LOOP: &str = "48 8D 0D ? ? ? ? E8 ? ? ? ? 48 8B 54 24 40 4C 8D 4E 18 48 8D 88 D0 AA 00 00 8B 46 20 \
89 44 24 28 8B 06 89 44 24 20 48 8B 85 18 42 00 00 44 8B 40 40 E8 ? ? ? ?";
pub const DTOR: &str = "40 53 48 83 EC 20 83 79 04 00 48 8B D9 76 07 C7 41 04 00 00 00 00 48 8B 51 08 48 85 D2 74 1F 41 B8 FF FF FF";
/// Quick Menu name lookup for an item (product, substance, tech, GetText).
pub const NAME: &str = "40 53 48 83 EC 20 80 79 08 00 48 8B D9 74 06 48 83 C3 10 EB 2B 48 8B 0D ? ? ? ? \
41 B8 FF FF FF FF 8B 53 40 48 81 C1 40 C2 00 00 E8 ? ? ? ? 48 8D 53 44 48 8B C8 E8 ? ? ? ? \
48 8B D8 48 89 7C 24 30 48 85 DB 0F 84 ? ? ? ? 48 8B 0D ? ? ? ? 41 B0 01 48 83 C1 60 \
48 8B D3 E8 ? ? ? ? 48 8B F8 48 85 C0 74 21 E8 ? ? ? ?";
/// Corner notice: AddTimedMessage plus cleanup (same site as Quick Transfer All).
pub const TOAST: &str = "48 8B 1D ? ? ? ? 48 8D 4C 24 78 C7 44 24 78 00 00 00 00";

pub const TABLE: ModSigs = ModSigs {
    owner: "QuickRecharge",
    sigs: &[
        Sig { name: "TRIGGER", pattern: TRIGGER, kind: SigUse::Hook },
        Sig { name: "CHARGE1", pattern: CHARGE1, kind: SigUse::Locate },
        Sig { name: "CHARGE_SITE", pattern: CHARGE_SITE, kind: SigUse::Locate },
        Sig { name: "WALLET", pattern: WALLET, kind: SigUse::Locate },
        Sig { name: "POST", pattern: POST, kind: SigUse::Locate },
        Sig { name: "OBJ", pattern: OBJ, kind: SigUse::Locate },
        Sig { name: "HEAD", pattern: HEAD, kind: SigUse::Locate },
        Sig { name: "LOOP", pattern: LOOP, kind: SigUse::Locate },
        Sig { name: "DTOR", pattern: DTOR, kind: SigUse::Locate },
        Sig { name: "NAME", pattern: NAME, kind: SigUse::Locate },
        Sig { name: "TOAST", pattern: TOAST, kind: SigUse::Locate },
    ],
    patches: &[] as &[CodePatch],
};
