Craft from Storage
==================

Crafting, building and charging also use the Storage Containers in the base
you are at, and the ones on your freighter while it is in range.

Needs the NMS.rs loader. Goes in No Man's Sky\GAMEDATA\MODS\CraftFromStorage.
Park it in the manager (NMS.rs\nms-rs.exe), or delete the folder, to turn it off.

In the game
-----------

The CraftFromStorage section of the in-game Mods page (Options, then Mods)
shows the status and every switch below.

"Anywhere (cheat)" uses all ten Storage Containers wherever you are. It applies
straight away and is saved in craft_from_storage.json.

"Fossil bones from storage" (on by default) lets the Fossil Assembly bone
picker also list the Storage Containers in reach. Untested for placing bones
from a container: back up your save first.

"All tab: inventory order" (on by default) shows the All tab of the item
pickers in inventory order: exosuit first, slots in order. The game lists it
backwards, last container first. Turn it off for the game's own order.

"Refiner: storage tabs" (on by default) adds the Freighter and Storage tabs to
the refiner's Transfer Items window.

"Cooker: inventory tabs" (on by default) gives the Nutrient Processor's Transfer
Items window the same tabs as the other pickers (Exosuit, Starship, Exocraft,
Raw Ingredient Storage, Cold Storage, Freighter, Storage) instead of
Simple/Medium/Complex. Freighter and Storage need "Refiner: storage tabs".

"Quick Transfer: each box" (on by default) lists every Storage Container in
reach as its own row in the Quick Transfer window, with its own name and free
slots, including from the freighter core or a freighter container. Clicking a
row moves the item into that container only. The container you are transferring
from is left out. With more than eight containers the list slides when the
cursor reaches its top or bottom row.

When the Exo-Skiff hold exists and the item is a fish, Quick Transfer lists
Cold Storage just after Exosuit, Starship, Freighter and Exocraft, above All
Containers. Ingredient Storage sits with it when the item is food. Those rows
are not general boxes. Both use the same crate icon as the storage rows.
Both still show when each box is off (one combined storage row under them).

"QT: All Containers row" (on by default) keeps the game's combined storage
row after Exosuit, Starship, Freighter and Exocraft, names it All Containers,
then lists each named box under it. Clicking All Containers sends the item to
a container the game picks, the same as vanilla's one row. Turn it off and
the first storage row is a named box again. With each box off it still names
the one combined row.

"Log frigate feed" (off by default) only writes to the log. Turn it on, open
the Feed menu of a living frigate, and send the log file to the author.

"Cooker: elements on Raw only" (off by default) hides elements such as Salt
and Carbon from the Nutrient Processor's Processed tab.

Shop slider and Buy/Sell list memory are a separate mod: Trade UI Tweaks.

These switches change the game's code in memory and take effect straight away,
turning off puts the game's own behaviour back.

"Write log" (off) turns craft_from_storage.log on or off.

After a game update the mod checks the game code again at every start. If it
moved, crafting stays vanilla (the log says why) and the rest of the game
works as usual.
