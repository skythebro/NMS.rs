-- AMUMSS script: add a Mods row to the pause-menu Options list.
-- The row is a copy of the Difficulty button placed after it. The NMS.rs loader draws the Mods page
-- and handles the click (BUTTON_MODS, see rust/crates/nms-mods/src/common/options_menu.rs).
-- Without NMS.rs the row would do nothing: remove this folder to take the row away.

NMS_MOD_DEFINITION_CONTAINER = {
  MOD_FILENAME = "NMSrsMenu.pak",
  MOD_AUTHOR = "skythebro",
  LUA_AUTHOR = "skythebro",
  MOD_DESCRIPTION = "Adds a Mods row to the pause-menu Options list for the NMS.rs in-game Mods page.",
  NMS_VERSION = "6.x",
  MODIFICATIONS = {
    {
      MBIN_CHANGE_TABLE = {
        {
          MBIN_FILE_SOURCE = "UI/SWITCHERPAGENEW.MBIN",
          EXML_CHANGE_TABLE = {
            {
              SPECIAL_KEY_WORDS = {"ID", "BUTTON_DIF"},
              SEC_SAVE_TO = "mods_btn",
            },
            {
              SEC_EDIT = "mods_btn",
              VALUE_CHANGE_TABLE = {
                {"ID", "BUTTON_MODS"},
              },
            },
            {
              SEC_EDIT = "mods_btn",
              SPECIAL_KEY_WORDS = {"Text", "UI_OPTIONS_DIFFICULTY_L"},
              VALUE_CHANGE_TABLE = {
                {"Text", "Mods"},
              },
            },
            {
              SPECIAL_KEY_WORDS = {"ID", "BUTTON_DIF"},
              SEC_ADD_NAMED = "mods_btn",
            },
          },
        },
      },
    },
  },
}
