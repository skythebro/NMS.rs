-- AMUMSS script: add the Quick Deposit and Exosuit lock-mode buttons.
-- The NMS.py loader merges this with other scripts that edit the same file.
-- Click behaviour stays in quick_transfer_all.py.
-- Do not edit SQU_SLOT here: an extra LOCK child with an empty DataFilename
-- froze the game when the Exosuit inventory opened.

NMS_MOD_DEFINITION_CONTAINER = {
  MOD_FILENAME = "QuickDeposit.pak",
  MOD_AUTHOR = "skythebro",
  LUA_AUTHOR = "skythebro",
  MOD_DESCRIPTION = "Adds the Quick Deposit and Exosuit lock-mode buttons to the inventory side bar.",
  NMS_VERSION = "6.x",
  MODIFICATIONS = {
    {
      MBIN_CHANGE_TABLE = {
        {
          MBIN_FILE_SOURCE = "UI/COMPONENTS/PAGESELECTBARSIDE.MBIN",
          EXML_CHANGE_TABLE = {
            {
              SPECIAL_KEY_WORDS = {"ID", "SORTBYTYPE"},
              SEC_SAVE_TO = "deposit_btn",
            },
            {
              SEC_EDIT = "deposit_btn",
              VALUE_CHANGE_TABLE = {
                {"ID", "DEPOSIT"},
                {"Position Y", "68.000000"},
              },
            },
            {
              SEC_EDIT = "deposit_btn",
              SPECIAL_KEY_WORDS = {"ID", "BG"},
              VALUE_CHANGE_TABLE = {
                {"Width", "58.000000"},
                {"Height", "58.000000"},
              },
            },
            {
              SEC_EDIT = "deposit_btn",
              VALUE_CHANGE_TABLE = {
                {"Image", "TEXTURES/UI/FRONTEND/ICONS/CATEGORIES/QUICKDEPOSIT.DDS"},
              },
              REPLACE_TYPE = "ALL",
              SPECIAL_KEY_WORDS = {"Image", "TEXTURES/UI/FRONTEND/ICONS/CATEGORIES/CATAGORY.SORTITEM.DDS"},
            },
            {
              SEC_EDIT = "deposit_btn",
              SPECIAL_KEY_WORDS = {"Image", "TEXTURES/UI/FRONTEND/ICONS/CATEGORIES/QUICKDEPOSIT.DDS"},
              VALUE_CHANGE_TABLE = {
                {"Width", "52.000000"},
                {"Height", "52.000000"},
              },
            },
            {
              SEC_EDIT = "deposit_btn",
              VALUE_CHANGE_TABLE = {
                {"Image", "TEXTURES/UI/FRONTEND/ICONS/CATEGORIES/QUICKDEPOSITBG.DDS"},
              },
              REPLACE_TYPE = "ALL",
              SPECIAL_KEY_WORDS = {"Image", "TEXTURES/UI/FRONTEND/ICONS/CATEGORIES/CATAGORY.SORTBG.DDS"},
            },
            {
              SPECIAL_KEY_WORDS = {"ID", "SORTBYTYPE"},
              SEC_ADD_NAMED = "deposit_btn",
            },
            {
              SPECIAL_KEY_WORDS = {"ID", "TECH_EXPAND"},
              VALUE_CHANGE_TABLE = {{"Position Y", "350.000000"}},
            },
            {
              SPECIAL_KEY_WORDS = {"ID", "ITEM_EXPAND"},
              VALUE_CHANGE_TABLE = {{"Position Y", "420.000000"}},
            },
            {
              SPECIAL_KEY_WORDS = {"ID", "SORTBYTYPE"},
              SEC_SAVE_TO = "lock_btn",
            },
            {
              SEC_EDIT = "lock_btn",
              VALUE_CHANGE_TABLE = {
                {"ID", "LOCKMODE"},
                {"Position X", "32.631579"},
                {"Position Y", "68.000000"},
              },
            },
            {
              SEC_EDIT = "lock_btn",
              SPECIAL_KEY_WORDS = {"ID", "BG"},
              VALUE_CHANGE_TABLE = {
                {"Width", "58.000000"},
                {"Height", "58.000000"},
              },
            },
            {
              SEC_EDIT = "lock_btn",
              VALUE_CHANGE_TABLE = {
                {"Image", "TEXTURES/UI/FRONTEND/ICONS/CATEGORIES/SUITLOCK.DDS"},
              },
              REPLACE_TYPE = "ALL",
              SPECIAL_KEY_WORDS = {"Image", "TEXTURES/UI/FRONTEND/ICONS/CATEGORIES/CATAGORY.SORTITEM.DDS"},
            },
            {
              SEC_EDIT = "lock_btn",
              SPECIAL_KEY_WORDS = {"Image", "TEXTURES/UI/FRONTEND/ICONS/CATEGORIES/SUITLOCK.DDS"},
              VALUE_CHANGE_TABLE = {
                {"Width", "36.000000"},
                {"Height", "36.000000"},
                {"Horizontal", "Center"},
                {"Vertical", "Middle"},
              },
            },
            {
              SEC_EDIT = "lock_btn",
              VALUE_CHANGE_TABLE = {
                {"Image", "TEXTURES/UI/FRONTEND/ICONS/CATEGORIES/QUICKDEPOSITBG.DDS"},
              },
              REPLACE_TYPE = "ALL",
              SPECIAL_KEY_WORDS = {"Image", "TEXTURES/UI/FRONTEND/ICONS/CATEGORIES/CATAGORY.SORTBG.DDS"},
            },
            {
              SPECIAL_KEY_WORDS = {"ID", "SORTBYTYPE"},
              SEC_ADD_NAMED = "lock_btn",
            },
          },
        },
      },
    },
  },
}
