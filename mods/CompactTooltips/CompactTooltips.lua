-- AMUMSS script for Compact Tooltips. The numbered sizes live in the UI files
-- in this folder; this script names those files so the loader can combine them
-- with another script that edits the same tooltip.

NMS_MOD_DEFINITION_CONTAINER = {
  MOD_FILENAME = "CompactTooltips.pak",
  MOD_AUTHOR = "skythebro",
  LUA_AUTHOR = "skythebro",
  MOD_DESCRIPTION = "A tighter item tooltip. Shared action rows and the value section are private copies.",
  NMS_VERSION = "6.x",
  MODIFICATIONS = {
    {
      MBIN_CHANGE_TABLE = {
        {
          MBIN_FILE_SOURCE = "UI/POPUP_TECHNOLOGY.MBIN",
          EXML_CHANGE_TABLE = {},
        },
        {
          MBIN_FILE_SOURCE = {
            {"UI/COMPONENTS/ACTIONBUTTON.MBIN", "UI/BIGGERITEMGRIDS/TOOLTIP/ACTIONBUTTON.MBIN"},
          },
          EXML_CHANGE_TABLE = {},
        },
        {
          MBIN_FILE_SOURCE = {
            {"UI/COMPONENTS/TECH/SUBSTANCEAMOUNTSECTION.MBIN", "UI/BIGGERITEMGRIDS/TOOLTIP/SUBSTANCEAMOUNTSECTION.MBIN"},
          },
          EXML_CHANGE_TABLE = {},
        },
      },
    },
  },
}
