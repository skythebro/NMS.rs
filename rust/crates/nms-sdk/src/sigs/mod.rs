//! Per-mod signature tables. Add a module per mod and list it in `ALL`.

use crate::ModSigs;

pub mod bigger_item_grids;
pub mod craft_from_storage;
pub mod galaxy_map_filters;
pub mod hide_empty_hover;
pub mod ingame_options;
pub mod picker_tweaks;
pub mod pulse_tweaks;
pub mod refiner_guard;
pub mod quick_recharge;
pub mod quick_transfer_all;
pub mod smaller_inventory_slots;
pub mod sphere_sized_mining;
pub mod tool_mode_binds;
pub mod trade_ui_tweaks;

pub const ALL: &[ModSigs] = &[
    hide_empty_hover::TABLE,
    sphere_sized_mining::TABLE,
    smaller_inventory_slots::TABLE,
    bigger_item_grids::TABLE,
    trade_ui_tweaks::TABLE,
    quick_recharge::TABLE,
    quick_transfer_all::TABLE,
    craft_from_storage::TABLE,
    pulse_tweaks::TABLE,
    ingame_options::TABLE,
    galaxy_map_filters::TABLE,
    picker_tweaks::TABLE,
    refiner_guard::TABLE,
    tool_mode_binds::TABLE,
];
