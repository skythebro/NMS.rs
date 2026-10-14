-- AMUMSS script: extra product-category filter buttons on the inventory side bar.
-- FILTER18 (Foods, filter id 17) and FILTER19 (Custom, filter id 18) sit in row 5 of the block,
-- beside FILTER5. Both stay hidden until their switch is on in the Mods page.
-- FILTER11 (Treasures), FILTER14 (Creature eggs), FILTER16 (Bones), FILTER18 and FILTER19 use the
-- mod's own glyphs in TEXTURES/UI/FRONTEND/ICONS/CATEGORIES in this folder.
-- Slot size and private box copies stay in the UI files in this folder.
-- The NMS.py loader merges this with QuickDeposit.lua (and any other script
-- that edits PAGESELECTBARSIDE) so this folder no longer has to own that button.

NMS_MOD_DEFINITION_CONTAINER = {
  MOD_FILENAME = "SmallerInventorySlots.pak",
  MOD_AUTHOR = "skythebro",
  LUA_AUTHOR = "skythebro",
  MOD_DESCRIPTION = "Adds eight product-category filter buttons, plus Foods and Custom, left of the vanilla five.",
  NMS_VERSION = "6.x",
  MODIFICATIONS = {
    {
      MBIN_CHANGE_TABLE = {
        {
          MBIN_FILE_SOURCE = "UI/COMPONENTS/PAGESELECTBARSIDE.MBIN",
          EXML_CHANGE_TABLE = {
            {
              SPECIAL_KEY_WORDS = {"ID", "SORTBYTYPE"},
              MATH_OPERATION = "+",
              VALUE_CHANGE_TABLE = {{"Position X", "65.263158"}},
            },
            {
              SPECIAL_KEY_WORDS = {"ID", "SORTBYCOLOUR"},
              MATH_OPERATION = "+",
              VALUE_CHANGE_TABLE = {{"Position X", "65.263158"}},
            },
            {
              SPECIAL_KEY_WORDS = {"ID", "SORTBYNAME"},
              MATH_OPERATION = "+",
              VALUE_CHANGE_TABLE = {{"Position X", "65.263158"}},
            },
            {
              SPECIAL_KEY_WORDS = {"ID", "SORTBYVALUE"},
              MATH_OPERATION = "+",
              VALUE_CHANGE_TABLE = {{"Position X", "65.263158"}},
            },
            {
              SPECIAL_KEY_WORDS = {"ID", "FILTERS"},
              MATH_OPERATION = "+",
              VALUE_CHANGE_TABLE = {{"Width", "124"}},
            },
            {
              SPECIAL_KEY_WORDS = {"ID", "FILTER1"},
              MATH_OPERATION = "+",
              VALUE_CHANGE_TABLE = {{"Position X", "124"}},
            },
            {
              SPECIAL_KEY_WORDS = {"ID", "FILTER2"},
              MATH_OPERATION = "+",
              VALUE_CHANGE_TABLE = {{"Position X", "124"}},
            },
            {
              SPECIAL_KEY_WORDS = {"ID", "FILTER3"},
              MATH_OPERATION = "+",
              VALUE_CHANGE_TABLE = {{"Position X", "124"}},
            },
            {
              SPECIAL_KEY_WORDS = {"ID", "FILTER4"},
              MATH_OPERATION = "+",
              VALUE_CHANGE_TABLE = {{"Position X", "124"}},
            },
            {
              SPECIAL_KEY_WORDS = {"ID", "FILTER5"},
              MATH_OPERATION = "+",
              VALUE_CHANGE_TABLE = {{"Position X", "124"}},
            },
            {
              SPECIAL_KEY_WORDS = {"ID", "TECH_EXPAND"},
              MATH_OPERATION = "+",
              VALUE_CHANGE_TABLE = {{"Position X", "124"}},
            },
            {
              SPECIAL_KEY_WORDS = {"ID", "ITEM_EXPAND"},
              MATH_OPERATION = "+",
              VALUE_CHANGE_TABLE = {{"Position X", "124"}},
            },
            {
              SPECIAL_KEY_WORDS = {"ID", "FILTER5"},
              SEC_SAVE_TO = "filter_btn",
            },
            {
              SEC_EDIT = "filter_btn",
              VALUE_CHANGE_TABLE = {
                {"ID", "FILTER6"},
                {"Position X", "0.000000"},
                {"Position Y", "0.000000"},
              },
            },
            {
              SEC_EDIT = "filter_btn",
              SPECIAL_KEY_WORDS = {"Image", "TEXTURES/UI/FRONTEND/ICONS/CATEGORIES/BUILDCATAGORY.ALL.DDS"},
              REPLACE_TYPE = "ALL",
              VALUE_CHANGE_TABLE = {{"Image", "TEXTURES/UI/FRONTEND/ICONS/WIKI/WIKI.CRAFTEDPROD.OFF.DDS"}},
            },
            {
              SPECIAL_KEY_WORDS = {"ID", "FILTER5"},
              SEC_ADD_NAMED = "filter_btn",
            },
            {
              SPECIAL_KEY_WORDS = {"ID", "FILTER5"},
              SEC_SAVE_TO = "filter_btn",
            },
            {
              SEC_EDIT = "filter_btn",
              VALUE_CHANGE_TABLE = {
                {"ID", "FILTER8"},
                {"Position X", "62.000000"},
                {"Position Y", "0.000000"},
              },
            },
            {
              SEC_EDIT = "filter_btn",
              SPECIAL_KEY_WORDS = {"Image", "TEXTURES/UI/FRONTEND/ICONS/CATEGORIES/BUILDCATAGORY.ALL.DDS"},
              REPLACE_TYPE = "ALL",
              VALUE_CHANGE_TABLE = {{"Image", "TEXTURES/UI/FRONTEND/ICONS/WIKI/WIKI.TRADEPRODS.OFF.DDS"}},
            },
            {
              SPECIAL_KEY_WORDS = {"ID", "FILTER5"},
              SEC_ADD_NAMED = "filter_btn",
            },
            {
              SPECIAL_KEY_WORDS = {"ID", "FILTER5"},
              SEC_SAVE_TO = "filter_btn",
            },
            {
              SEC_EDIT = "filter_btn",
              VALUE_CHANGE_TABLE = {
                {"ID", "FILTER9"},
                {"Position X", "0.000000"},
                {"Position Y", "70.000000"},
              },
            },
            {
              SEC_EDIT = "filter_btn",
              SPECIAL_KEY_WORDS = {"Image", "TEXTURES/UI/FRONTEND/ICONS/CATEGORIES/BUILDCATAGORY.ALL.DDS"},
              REPLACE_TYPE = "ALL",
              VALUE_CHANGE_TABLE = {{"Image", "TEXTURES/UI/FRONTEND/ICONS/WIKI/WIKI.CURIO.OFF.DDS"}},
            },
            {
              SPECIAL_KEY_WORDS = {"ID", "FILTER5"},
              SEC_ADD_NAMED = "filter_btn",
            },
            {
              SPECIAL_KEY_WORDS = {"ID", "FILTER5"},
              SEC_SAVE_TO = "filter_btn",
            },
            {
              SEC_EDIT = "filter_btn",
              VALUE_CHANGE_TABLE = {
                {"ID", "FILTER11"},
                {"Position X", "62.000000"},
                {"Position Y", "70.000000"},
              },
            },
            {
              SEC_EDIT = "filter_btn",
              SPECIAL_KEY_WORDS = {"Image", "TEXTURES/UI/FRONTEND/ICONS/CATEGORIES/BUILDCATAGORY.ALL.DDS"},
              REPLACE_TYPE = "ALL",
              VALUE_CHANGE_TABLE = {{"Image", "TEXTURES/UI/FRONTEND/ICONS/CATEGORIES/FILTERTREASURE.DDS"}},
            },
            {
              SPECIAL_KEY_WORDS = {"ID", "FILTER5"},
              SEC_ADD_NAMED = "filter_btn",
            },
            {
              SPECIAL_KEY_WORDS = {"ID", "FILTER5"},
              SEC_SAVE_TO = "filter_btn",
            },
            {
              SEC_EDIT = "filter_btn",
              VALUE_CHANGE_TABLE = {
                {"ID", "FILTER14"},
                {"Position X", "0.000000"},
                {"Position Y", "140.000000"},
              },
            },
            {
              SEC_EDIT = "filter_btn",
              SPECIAL_KEY_WORDS = {"Image", "TEXTURES/UI/FRONTEND/ICONS/CATEGORIES/BUILDCATAGORY.ALL.DDS"},
              REPLACE_TYPE = "ALL",
              VALUE_CHANGE_TABLE = {{"Image", "TEXTURES/UI/FRONTEND/ICONS/CATEGORIES/FILTEREGG.DDS"}},
            },
            {
              SPECIAL_KEY_WORDS = {"ID", "FILTER5"},
              SEC_ADD_NAMED = "filter_btn",
            },
            {
              SPECIAL_KEY_WORDS = {"ID", "FILTER5"},
              SEC_SAVE_TO = "filter_btn",
            },
            {
              SEC_EDIT = "filter_btn",
              VALUE_CHANGE_TABLE = {
                {"ID", "FILTER15"},
                {"Position X", "62.000000"},
                {"Position Y", "140.000000"},
              },
            },
            {
              SEC_EDIT = "filter_btn",
              SPECIAL_KEY_WORDS = {"Image", "TEXTURES/UI/FRONTEND/ICONS/CATEGORIES/BUILDCATAGORY.ALL.DDS"},
              REPLACE_TYPE = "ALL",
              VALUE_CHANGE_TABLE = {{"Image", "TEXTURES/UI/FRONTEND/ICONS/MISSIONS/MISSION.FISHROUND.OFF.DDS"}},
            },
            {
              SPECIAL_KEY_WORDS = {"ID", "FILTER5"},
              SEC_ADD_NAMED = "filter_btn",
            },
            {
              SPECIAL_KEY_WORDS = {"ID", "FILTER5"},
              SEC_SAVE_TO = "filter_btn",
            },
            {
              SEC_EDIT = "filter_btn",
              VALUE_CHANGE_TABLE = {
                {"ID", "FILTER16"},
                {"Position X", "0.000000"},
                {"Position Y", "210.000000"},
              },
            },
            {
              SEC_EDIT = "filter_btn",
              SPECIAL_KEY_WORDS = {"Image", "TEXTURES/UI/FRONTEND/ICONS/CATEGORIES/BUILDCATAGORY.ALL.DDS"},
              REPLACE_TYPE = "ALL",
              VALUE_CHANGE_TABLE = {{"Image", "TEXTURES/UI/FRONTEND/ICONS/CATEGORIES/FILTERBONE.DDS"}},
            },
            {
              SPECIAL_KEY_WORDS = {"ID", "FILTER5"},
              SEC_ADD_NAMED = "filter_btn",
            },
            {
              SPECIAL_KEY_WORDS = {"ID", "FILTER5"},
              SEC_SAVE_TO = "filter_btn",
            },
            {
              SEC_EDIT = "filter_btn",
              VALUE_CHANGE_TABLE = {
                {"ID", "FILTER17"},
                {"Position X", "62.000000"},
                {"Position Y", "210.000000"},
              },
            },
            {
              SEC_EDIT = "filter_btn",
              SPECIAL_KEY_WORDS = {"Image", "TEXTURES/UI/FRONTEND/ICONS/CATEGORIES/BUILDCATAGORY.ALL.DDS"},
              REPLACE_TYPE = "ALL",
              VALUE_CHANGE_TABLE = {{"Image", "TEXTURES/UI/FRONTEND/ICONS/MISSIONS/MISSION.SPACEHULK.OFF.DDS"}},
            },
            {
              SPECIAL_KEY_WORDS = {"ID", "FILTER5"},
              SEC_ADD_NAMED = "filter_btn",
            },
            {
              SPECIAL_KEY_WORDS = {"ID", "FILTER5"},
              SEC_SAVE_TO = "filter_btn",
            },
            {
              SEC_EDIT = "filter_btn",
              VALUE_CHANGE_TABLE = {
                {"ID", "FILTER18"},
                {"Position X", "0.000000"},
                {"Position Y", "280.000000"},
              },
            },
            {
              SEC_EDIT = "filter_btn",
              SPECIAL_KEY_WORDS = {"Image", "TEXTURES/UI/FRONTEND/ICONS/CATEGORIES/BUILDCATAGORY.ALL.DDS"},
              REPLACE_TYPE = "ALL",
              VALUE_CHANGE_TABLE = {{"Image", "TEXTURES/UI/FRONTEND/ICONS/CATEGORIES/FILTERFOODS.DDS"}},
            },
            {
              SPECIAL_KEY_WORDS = {"ID", "FILTER5"},
              SEC_ADD_NAMED = "filter_btn",
            },
            {
              SPECIAL_KEY_WORDS = {"ID", "FILTER5"},
              SEC_SAVE_TO = "filter_btn",
            },
            {
              SEC_EDIT = "filter_btn",
              VALUE_CHANGE_TABLE = {
                {"ID", "FILTER19"},
                {"Position X", "62.000000"},
                {"Position Y", "280.000000"},
              },
            },
            {
              SEC_EDIT = "filter_btn",
              SPECIAL_KEY_WORDS = {"Image", "TEXTURES/UI/FRONTEND/ICONS/CATEGORIES/BUILDCATAGORY.ALL.DDS"},
              REPLACE_TYPE = "ALL",
              VALUE_CHANGE_TABLE = {{"Image", "TEXTURES/UI/FRONTEND/ICONS/CATEGORIES/FILTERCUSTOM.DDS"}},
            },
            {
              SPECIAL_KEY_WORDS = {"ID", "FILTER5"},
              SEC_ADD_NAMED = "filter_btn",
            },
          },
        },
      },
    },
  },
}
