Bigger Item Grids
=================

A bigger card grid on Transfer Items and Craft Product. The game shows 4 by 2
cards per page; this shows more (8 by 3 in a 1.4 times larger window by
default).

Goes in No Man's Sky\GAMEDATA\MODS\BiggerItemGrids (the NMS.rs manager installs
it from NMS.rs\stock). The UI files work on their own; the NMS.rs loader adds
whole-page scrolling.

What is in this folder
----------------------

  UI\...                           the grid layout (game files, loaded by the game)
  BiggerItemGrids.lua              names those files so the loader can merge them
  bigger_item_grids_preset.json    the grid size those files were built for
  bigger_item_grids.json           your settings (made at first start)
  bigger_item_grids.log            what happened, when Write log is on

In this repository the default UI files live in
presets\BiggerItemGrids\8x3-window1.4 and are added to the zip by release.py.

The UI files draw and fill the whole grid on their own. The game still thinks
a page is 8 cards, so without the loader the arrows and page dots move 8 cards
at a time: one row on the 8-wide grids. The loader sets the page to the whole
grid, so the arrows turn a full page.

Other sizes
-----------

The size is fixed by the files in this folder. To change it, close the game
and open NMS.rs\nms-rs.exe. Sizes (click it to expand) and Apply sizes rebuild
this folder from your installed game. Window scale stays between 1.00 and
1.40, and columns times rows stays at 99 or below.

Why the window stops at 1.4: the popup opens to the left of the slot when
there is no room on the right, and past about 1.45 it covers the inventory
page's sort buttons, which then take the clicks meant for the first card.

In the game
-----------

The BiggerItemGrids section of the in-game Mods page (Options, then Mods)
shows the grid, the window size and whether the code patch applied. "Write
log" (off) turns bigger_item_grids.log on or off.

After a game update the mod checks the game code again at every start. If it
moved, the page size stays at 8, so the grid scrolls as it does without the
loader (the log says why), and the rest of the game works as usual.
