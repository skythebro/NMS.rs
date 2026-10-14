-- AMUMSS script: make the galaxy map Filter list tall enough for the extra filters.
-- The Filter and Navigation dropdowns live inside the MODES layer of UI/GALAXYBAR.MBIN, which is
-- 200 units high and clips its children. That fits about seven list lines, so the list was cut off
-- once several extra filters were on. This makes MODES 600 units high; nothing else moves.
-- Remove this script to go back to the vanilla size. The game reads this file at start.

NMS_MOD_DEFINITION_CONTAINER = {
  MOD_FILENAME = "GalaxyMapFilters.pak",
  MOD_AUTHOR = "skythebro",
  LUA_AUTHOR = "skythebro",
  MOD_DESCRIPTION = "Taller galaxy map Filter list for the extra filters.",
  NMS_VERSION = "6.x",
  MODIFICATIONS = {
    {
      MBIN_CHANGE_TABLE = {
        {
          MBIN_FILE_SOURCE = "UI/GALAXYBAR.MBIN",
          EXML_CHANGE_TABLE = {
            {
              SPECIAL_KEY_WORDS = {"ID", "MODES", "Height", "200.000000"},
              VALUE_CHANGE_TABLE = {
                {"Height", "600.000000"},
              },
            },
          },
        },
      },
    },
  },
}
