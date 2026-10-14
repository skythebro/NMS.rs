
local NMS_VERSION = "6.45.1"
local MOD_VERSION = "0"
local BUILDING_UPGRADE_TIME = 300

NMS_MOD_DEFINITION_CONTAINER = {
    ["MOD_FILENAME"] = string.format("Accelerated Settlements %s.%s", NMS_VERSION, MOD_VERSION),
    ["NMS_VERSION"] = NMS_VERSION,
    ["MODIFICATIONS"] = {
        {
            ["MBIN_CHANGE_TABLE"] = {
                {
                    ["MBIN_FILE_SOURCE"] = "GCSETTLEMENTGLOBALS.MBIN",
                    ["MXML_CHANGE_TABLE"] = {
                        {
                            ["VALUE_CHANGE_TABLE"] = {
                                {"BuildingUpgradeTimeInSeconds", BUILDING_UPGRADE_TIME},
                            }
                        },
                    }
                }
            }
        }
    }
}
