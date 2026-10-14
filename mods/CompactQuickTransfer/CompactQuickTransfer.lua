-- AMUMSS script for Compact Quick Transfer. Row sizes live in the UI files
-- in this folder; this script names those files so the loader can combine them
-- with another script that edits the same popup.

NMS_MOD_DEFINITION_CONTAINER = {
  MOD_FILENAME = "CompactQuickTransfer.pak",
  MOD_AUTHOR = "skythebro",
  LUA_AUTHOR = "skythebro",
  MOD_DESCRIPTION = "A shorter Quick Transfer list. The row is a private copy.",
  NMS_VERSION = "6.x",
  MODIFICATIONS = {
    {
      MBIN_CHANGE_TABLE = {
        {
          MBIN_FILE_SOURCE = "UI/POPUP_TRANSFERSEND.MBIN",
          EXML_CHANGE_TABLE = {},
        },
        {
          MBIN_FILE_SOURCE = {
            {"UI/COMPONENTS/TRANSFERBUTTON.MBIN", "UI/BIGGERITEMGRIDS/QUICKTRANSFER/TRANSFERBUTTON.MBIN"},
          },
          EXML_CHANGE_TABLE = {},
        },
      },
    },
  },
}
