//! Game functions more than one mod hooks. Each mod subscribes; there is one detour per function.

nms_core::shared_hook! {
    /// `cGcFrontendPageFunctions::DoInventorySlots` (nmspy signature, `scroll_offset` on the stack).
    pub mod do_inventory_slots = "DoInventorySlots": fn(
        page: u64,
        layer: u64,
        inventory: u64,
        empty_slot_actions: u64,
        slots_wide: i32,
        popup_actions: i32,
        disabled: u8,
        min_index: u64,
        slot_width: i32,
        slot_height: i32,
        can_hide_slots: u8,
        render_width: i32,
        render_height: i32,
        compare_special: u8,
        scroll_offset: f32,
    ) -> u64;
}

nms_core::shared_hook! {
    /// Quick Transfer move to the picked destination (CraftFromStorage `QUICK_PICK`, QuickTransferAll
    /// `TRANSFER`: two patterns for one function). `amount` is an `i32` in a stack slot and the return
    /// is a `bool` in `al`; both are declared full width so the detour passes them through untouched.
    /// Read `amount` with `as i32` and write it back sign-extended.
    pub mod quick_transfer_pick = "QuickTransferPick": fn(
        dest: u64,
        page: u64,
        store: u64,
        index: u64,
        amount: i64,
        moved: u64,
    ) -> u64;
}

nms_core::shared_hook! {
    /// `cGcOptionsPageUI::FinalizePage(page, bool, bool)`. The bools are declared full width so the
    /// detour passes them through untouched.
    pub mod options_finalize_page = "OptionsFinalizePage": fn(page: u64, a: u64, b: u64) -> u64;
}

nms_core::shared_hook! {
    /// `cGcOptionsPageUI::EndPage(page)`, called once per frame after every Options page is drawn.
    pub mod options_end_page = "OptionsEndPage": fn(page: u64) -> u64;
}

nms_core::shared_hook! {
    /// `cGcFrontendPageOptions::DoGameSwitcher(this)`: the pause-menu Options list.
    pub mod options_game_switcher = "OptionsGameSwitcher": fn(page: u64) -> u64;
}

nms_core::shared_hook! {
    /// `cGcFrontendTextInput::Update(this)`: the text-entry popup's per-frame step (the game's rename box).
    pub mod text_input_update = "TextInputUpdate": fn(this: u64) -> u64;
}

nms_core::shared_hook! {
    /// Sets the Options page title text from the page id (no arguments).
    pub mod options_page_title = "OptionsPageTitle": fn() -> u64;
}

nms_core::shared_hook! {
    /// `cGcInventoryStore::Sort`.
    pub mod inventory_sort = "InventorySort": fn(inventory: u64, order: i32) -> u64;
}
