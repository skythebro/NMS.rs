//! From `mods/SphereSizedMining/sphere_sized_mining.py`.

use crate::{CodePatch, ModSigs, Sig, SigUse};

/// cGcTerrainEditorBeam::ApplyTerrainEditStroke(this, sTerrainEditData (1 byte), impact) -> i64.
pub const STROKE: &str = "4C 89 44 24 ? 55 53 56 57 41 54 41 55 41 56 48 8D AC 24 ? ? ? ? 48 81 EC";
/// The function giving the resources of a dig; each cap is read by a cmovge r8d,[rip+cap].
pub const AWARD: &str = "48 89 5C 24 ? 48 89 74 24 ? 55 57 41 55 41 56 41 57 48 8D 6C 24 ? 48 81 EC ? ? ? ? \
4C 8B F1 48 8B F2 48 8B 0D ? ? ? ? 49 8D 50 30";

/// The stroke's resource award (rcx = captured edit state, rdx = 11 removed-voxel counts; returns
/// the total in eax). STROKE calls it directly and also hands it to the voxel edit as a functor
/// (thunk `add rcx,0x10; jmp`), so it may run after STROKE returns. Every cap read of a
/// Terrain Manipulator dig happens inside it (inline cmov reads and the AWARD call).
pub const STROKE_AWARD: &str = "40 55 53 56 57 48 8D AC 24 ? ? ? ? 48 81 EC ? ? ? ? 48 8B D9 48 8B F2 \
B9 0B 00 00 00 33 FF 48 8B C2 03 38 48 8D 40 04 48 83 E9 01 75 F4";

pub const TABLE: ModSigs = ModSigs {
    owner: "SphereSizedMining",
    sigs: &[
        Sig { name: "STROKE", pattern: STROKE, kind: SigUse::Hook },
        Sig { name: "STROKE_AWARD", pattern: STROKE_AWARD, kind: SigUse::Hook },
        Sig { name: "AWARD", pattern: AWARD, kind: SigUse::Locate },
    ],
    patches: &[] as &[CodePatch],
};
