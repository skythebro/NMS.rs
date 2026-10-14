
function GetNewSpecial(ID)
    return ID
end

NEW_SPECIAL = {}
table.insert(NEW_SPECIAL, GetNewSpecial("X"))

NMS_MOD_DEFINITION_CONTAINER =
{
["MOD_FILENAME"] = "~Consumerism",
["MODIFICATIONS"] =
    {
        {
            ["MBIN_CHANGE_TABLE"] =
            {
                {
                    ["MBIN_FILE_SOURCE"] = { "METADATA\\REALITY\\TABLES\\PURCHASEABLESPECIALS.MBIN"},
                    ["MXML_CHANGE_TABLE"] = NEW_SPECIAL
                },
            }
        },
    }
}
