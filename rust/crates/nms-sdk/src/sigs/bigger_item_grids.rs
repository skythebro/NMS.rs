//! From `mods/BiggerItemGrids/bigger_item_grids.py` and `bigger_item_grids_sigscan.py`.
//! Page-size sites are found at runtime from string anchors (`TRANSFER_RECEIVE_DESC`, `PRODUCTS_ALL`, `SLOT%02d`).

use crate::{CodePatch, ModSigs, Sig, SigUse};

/// Next page arrow in the slot filler: `shl ecx, 3` / `add eax, 8` / clamp (`_NEXT_PAGE` in sigscan).
pub const NEXT_PAGE: &str = "C1 E1 03 83 C0 08 3B C8 0F 4C C1";

// `_PREV_PAGE` (`83 C1 F8`, `add ecx, -8`) is not listed: it is only searched inside the slot
// filler's body, where it must occur once. It matches many times across the whole exe.

// Runtime-only patches (not `CodePatch`): `redirect_rip_float` rel32 to the post-function cave;
// `next_new` / `prev_new` immediates from `items_per_page` in `plan_slot_filler` and `plan_scale_redirect`.

pub const TABLE: ModSigs = ModSigs {
    owner: "BiggerItemGrids",
    sigs: &[Sig { name: "NEXT_PAGE", pattern: NEXT_PAGE, kind: SigUse::Locate }],
    patches: &[] as &[CodePatch],
};
