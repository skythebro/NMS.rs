Smaller Inventory Slots
=======================

Smaller slots in the Technology and Cargo grids of the inventory page
(Exosuit, Starship, Multi-tool, Exocraft, Freighter), so more rows show
without scrolling, and a filter button for each item type.

  slot size       tech rows   cargo rows
  100% (vanilla)  3           5
  80%             3           6
  70%             4           7

Goes in No Man's Sky\GAMEDATA\MODS\SmallerInventorySlots. The slot size is
fixed by the UI files in this folder (see smaller_inventory_slots_preset.json).
To change it, close the game and open NMS.rs\nms-rs.exe. Sizes (click it to
expand) and Apply sizes rebuild this folder from your installed game. Slot
size is 50% to 100% (100% is vanilla). Extra filter buttons are optional.

Updating: Update keeps the UI files Apply sizes built, so a side bar built by
an older version has no Foods and Custom buttons and the old icons for
Treasures, Creature eggs and Bones. The manager and "nms-rs mods status" then
say "Press Apply sizes to add the new filter buttons ... and the new icons":
press Apply sizes once.

Filter buttons
--------------

Eight buttons, two columns of four left of the game's own five: Components,
Trade goods, Curiosities, Procedural, Creature eggs, Fish, Bones and Salvage
(left to right, top to bottom; Procedural is named Treasures in game: the
items you dig up). Treasures (a gem on a dug mound with a spade), Creature
eggs (a cracked egg) and Bones (a bone) have icons of their own, shipped in
this folder's TEXTURES; the others use the game's icons. Click one to light
only that item type and dim
the rest; click it again, or the top vanilla button, for All. A mouse click
or the controller Confirm (Menus) / A button both count. Hovering a
button names it in the game's language. They hide on the same inventory
tabs as the sort stack and Quick Deposit. The inventory panel sits a little
right of where vanilla has it to make room.

- Consumables keep the vanilla button. Building parts, emotes and
  customisation parts have none: they live in the build, expedition and
  Quicksilver menus, not the inventory.
- The categories are the items' own, so Trade goods can light a different set
  than the vanilla "high-value items" button.
- With Quick Transfer All, holding Shift moves only the lit items.

Two more buttons, each with its own switch (off by default). They sit in the
empty cells beside the fifth vanilla button and have their own icons (a
cooking pot for Foods, a bookmark with a check mark for Custom), shipped in
this folder's TEXTURES:

- Foods: lights cooking ingredients, dishes, fish and creature feed (Creature
  Pellets and the other bait). Ammo, fuel and upgrade modules stay dim, so it
  is narrower than the vanilla Consumables button.
- Custom: while it is selected, click items to mark them. "Custom marks"
  picks what a click marks:
    Clicked stack (default): only that stack lights, and Quick Transfer and
    Deposit move only that stack. The mark stays with the slot and goes when
    that slot no longer holds the item (moved, used up, swapped).
    Every stack of the item: all stacks of that item light, in every
    inventory.
  Changing it clears the marks. Marks also clear when you pick another
  filter on the tab where you picked Custom (changing tabs does not count),
  when you switch Custom off, and on every game restart. "Keep Custom marks"
  (off) keeps them until the game restarts.
  While you carry an item (picked up on another tab), a click on a Custom
  tab drops it as usual instead of marking.

Per-tab memory and containers
-----------------------------

Three switches on the in-game Mods page (all on by default):

- Keep filter per tab: the filter you picked stays when you change between
  Exosuit, Starship, Multi-tool, Exocraft and the Freighter containers.
- Keep scroll per tab: each tab has its own scroll position.
- Keep freighter tab: the Freighter opens on the Core or container tab (Q/E)
  you last used, until you close the game.

Storage Containers (base and ship) and the Freighter's container tabs use the
smaller slots too. UI\ROCKETLOCKERPAGE.MBIN is the trade rocket screen, and
it is replaced so the exosuit side of that screen uses the smaller slots.
UI\SWAPPAGE.MBIN is the Storage Container screen, and it is replaced to add
the sort buttons. Another mod that replaces either file conflicts with this
one.

Sort storage
------------

"Sort storage" on the in-game Mods page (on by default). On a
storage container, four sort buttons sit between the two grids near the
bottom: type, colour, name, value, top to bottom, as on the exosuit page.
A click sorts the container only; sort the exosuit on its own page. The
container screen is the game's transfer page (UI\SWAPPAGE.MBIN), so the
buttons also show on other transfer screens that use it. The trade rocket
has no buttons. A click uses the game's own sort on the grid it is drawing. The game's sort call is not rewritten.
The exosuit's own sort buttons are unchanged. Switch it off and the buttons
hide, and a container is not sorted. The buttons are in the 70% and 80% UI
files; the 100% files do not rebuild this page.

Needs the NMS.rs loader
-----------------------

The UI files draw the smaller slots and the buttons on their own. The NMS.rs
loader (4.0) also makes each grid use all the rows the page now has room for,
and makes the new buttons filter. Without the loader the new buttons do
nothing. The SmallerInventorySlots section of the in-game Mods page (Options,
then Mods) has every switch above, live, and its Status row shows whether
each part is on. "Write log" (off) turns smaller_inventory_slots.log on or
off. Settings are saved in smaller_inventory_slots.json in this folder.

This folder includes SmallerInventorySlots.lua. The loader uses it when
another installed mod also edits the side bar (Quick Transfer All's deposit
button, or anyone else's AMUMSS script), and writes the combined file to
GAMEDATA\MODS\z_Combined Mods. The side bar in this folder does not include
that deposit button.

After a game update the mod checks that the game code it uses is the same. If
not, that part stays off (the log says why) and the inventory works as in
vanilla.

The grids cannot get more columns: the game lays every row out as ten slot
positions and hides the unused ones. With smaller slots a grid can scroll a
little past its last row, because the game works out the scroll range for
vanilla-sized slots. Every row stays reachable.

The slot and box files are private copies (UI\BIGGERITEMGRIDS\INVENTORY), so
the ship shop and other screens keep the vanilla look. UI\INVENTORYPAGE.MBIN
and UI\COMPONENTS\PAGESELECTBARSIDE.MBIN replace the game's own. Another
mod that ships a .lua for the same file is merged by the loader; a finished
MBIN with no script still conflicts. A custom build with the extra filters
off does not replace the side bar.
