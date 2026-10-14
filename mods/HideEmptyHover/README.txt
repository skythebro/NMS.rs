HideEmptyHover
==============

Hides the Modify panel that covers the inventory grid when the cursor slides
over an empty cargo or tech slot (craft preview, click-to-craft hint, and
Quick Transfer hint). Clicking the empty slot still opens Craft / Modify.
Tooltips on items you already have are unchanged.

Goes in No Man's Sky\GAMEDATA\MODS\HideEmptyHover. Needs the NMS.rs loader.

Switches (in-game Mods page, HideEmptyHover)
--------------------------------------------

- Hide inventory hover (default on): hide that hover on empty cargo slots of
  every inventory that shows it. Click and Quick Transfer still run. Switch
  off, no restart, and the hover is back.
- Hide tech hover (default on): the same for empty slots in tech grids (suit,
  ship, multi-tool). The two switches are independent. A tech grid is told
  apart by its supercharged slots.
- Write log (off): turns hide_empty_hover.log on or off.

After a game update the mod checks that the game code it uses is the same. If
not, it stays off (the log says why) and the game works as in vanilla.
