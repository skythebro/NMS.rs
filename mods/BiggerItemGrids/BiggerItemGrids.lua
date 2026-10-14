-- AMUMSS script for Bigger Item Grids. The card count and window size live in
-- the UI files in this folder (see bigger_item_grids_preset.json). This script
-- names those files so the loader can combine them with another script that
-- edits the same popups.

NMS_MOD_DEFINITION_CONTAINER = {
  MOD_FILENAME = "BiggerItemGrids.pak",
  MOD_AUTHOR = "skythebro",
  LUA_AUTHOR = "skythebro",
  MOD_DESCRIPTION = "A bigger card grid on Transfer Items and Craft Product.",
  NMS_VERSION = "6.x",
  MODIFICATIONS = {
    {
      MBIN_CHANGE_TABLE = {
        {
          MBIN_FILE_SOURCE = "UI/POPUP_INVENTORY.MBIN",
          EXML_CHANGE_TABLE = {},
        },
        {
          MBIN_FILE_SOURCE = "UI/POPUP_TRANSFER.MBIN",
          EXML_CHANGE_TABLE = {},
        },
        {
          MBIN_FILE_SOURCE = "UI/POPUP_BUILD.MBIN",
          EXML_CHANGE_TABLE = {},
        },
      },
    },
  },
}
