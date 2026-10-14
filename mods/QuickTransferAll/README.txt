Quick Transfer All
==================

Hover an item, press Quick Transfer while holding Shift, and pick where it
goes as usual. That item goes first, then every other item in the same
inventory that the side filter lights up (Raw Materials, High-Value Items,
Consumables, Installable Technology, or everything when no filter is on) goes
to the same place, row by row from the top left.

- What does not fit stays where it is. The game moves as much of a stack as
  fits and shows its usual "no room" message for the rest.
- Technology installed in slots is never moved.

Controller
----------

The game's menu layout does not use the left trigger, so the game never
receives it. Give it the game's spare menu action "Unbound" once, and it
works as the hold button. The game does nothing with that action, and the
left trigger keeps its normal use outside menus.

Shift+Tab should open the Steam overlay, not the inventory. From there
open Controller Layout, then Edit Layout (Hardware, View controller
settings). If Shift+Tab opens the exosuit, Steam is not catching that
key: in the Steam library, open No Man's Sky, then the controller icon,
or Manage, Controller layout.

The game's own Controls menu will still show nothing on the left
trigger. That is normal. Do not bind the keyboard Shift key to the
trigger. The game then treats you as being on keyboard, and the other
inventory pane goes away.

1. Open the controller layout as above.
2. Switch to the Menus action set.
3. Under Left Trigger, click "Add command" next to Full Pull, open Game
   Actions, and pick "Unbound".

If Steam does not keep the command (the slot still says "Add command"), first
switch to the game's own layout for your controller (for an Xbox pad, Hello
Games' "Xbox One" layout) and edit that one instead.

Then press Y on an item, hold the left trigger, and confirm the destination
with A. The setting is kept in your Steam layout, so game updates do not undo
it. Any other button bound to "Unbound" in the Menus set works the same way,
and so does "Hold to Paste (Text Chat)".

Each move is the game's own Quick Transfer, so stack limits and what a
destination accepts are the game's rules.

Deposit
-------

With the inventory open at a base or on a freighter that has a Storage
Container in reach, click the button in the gap above the sort icons (arrow
into a crate, hexagon frame). A mouse click or the controller Confirm (Menus) / A button both
count. Hovering it says what it will do. A click plays the same select
sound and fading blue notice as the sort buttons. The notice says how
many items moved, or that nothing could be deposited. It is a quick stack: each lit
item goes into the container that already holds that item. If that container
is full, another one holding it is tried; if none has room the item stays
where it is. Installed technology stays put.

"Same item only" is on by default. Turn it off and items no container holds
yet also go to the storage (the game picks the container). Items a container
already holds still go to that container.

The button only shows where the filter and sort buttons show. On the storage
screen itself it does nothing: it will not move a container into itself.

Smaller Inventory Slots' extra filters count the same way: with Foods
picked only the lit food items move. With its Custom filter picked, Quick
Transfer and Deposit move only the marked items: only the marked stacks when "Custom
marks" is "Clicked stack" (its default), every stack of a marked item when it
is "Every stack of the item".

The button is added by QuickDeposit.lua in this folder. The loader writes
it into GAMEDATA\MODS\z_Combined Mods, and merges it with Smaller Inventory
Slots' extra filters when that mod is installed too. You do not need
Smaller Inventory Slots for the button. The QuickTransferAll section of
the in-game Mods page has
"Deposit" (on) and "Same item only" (on). They apply at once. There is no
deposit key. Deposit and Lock use the same hold ring as Sort when the
game needs a hold. Instant Actions and Instant UI stay a tap.

Exosuit lock
------------

On by default. The Mods page has "Exosuit lock mode". When it
is on, a lock button sits one filter-column left of Deposit on the
Exosuit side bar only. Starship, storage, and the other inventories do
not show it. Click
it on the Exosuit page to turn lock mode on or off. That click uses the
same select sound and fading blue notice as sort, and the notice says
whether lock is on or off. In lock mode, a click on an occupied Exosuit
slot pins that item and count. The stack stays in the slot. Click it
again to unpin. Empty slots do nothing. Only the item grid counts, not
the tech slots. Tech slots still pick up. While lock mode is on, pinned
cargo stays lit and the rest of the Exosuit (including tech) looks like
a filter miss. Lock off puts the usual filter look back. Sort and filter
clicks wait until lock mode is off. Closing the inventory or leaving
the Exosuit turns lock mode off. Pins stay per save and are written as
soon as you pin or unpin, not when you save the game. If you reload
without saving, the items go back and the locks stay. Unpin a slot if
that leftover lock is in the way. Opening the Exosuit refills each
pinned slot from nearby Storage Containers up to the reserved count,
using the item saved at pin time. A different item in that slot is
left alone. A world pickup or harvest of a different item skips an
empty pin and lands in the next free cell. The reserved item can
still land on that pin (mine Carbon onto an emptied Carbon pin, then
restock fills the rest). Ship, freighter cargo, and exocraft are not sources. A
fading notice says Restocked N only when something moved. Sort leaves
pinned Exosuit slots where they are. Quick Transfer and Deposit move
only the count above the reserved amount on a pinned Exosuit slot.
Selling that item to a shop or trader only takes the surplus. The
slider still shows the full stack. Confirm leaves the reserved count
on the pin. The shop pays for what left. A fading notice says
Sold N surplus, or Sale blocked when nothing left.
The pin stays on that slot. A stack moved to
an unlocked slot can leave in full. "Lock follows item" (off) moves
that pin when you move the reserved stack to another Exosuit cargo
cell, including a swap. The reserved count stays. A split that
leaves some on the old cell keeps the pin there. Moving the
whole reserved stack off the suit (Starship, freighter, boxes)
clears the pin. Quick Transfer from the pinned
slot, including with the stack in hand, still leaves the reserve.
Unpinned slots still move the whole stack. There is no padlock
drawn on the slot itself.

Needs the NMS.rs loader (4.0). Goes in
No Man's Sky\GAMEDATA\MODS\QuickTransferAll. Park it in NMS.rs\nms-rs.exe,
or delete the folder, to remove it.

The Deposit button (an arrow into an open crate) and the lock button (a slot
grid with a padlock) draw their own icons from this folder's TEXTURES.

Language: the button tooltips and the fading notices follow the game's
language, which the mod reads once when the game starts. After you change the
game's language, restart the game to see them in the new language.

Settings
--------

The QuickTransferAll section of the in-game Mods page (Options, then Mods).
Every switch applies at once. "Hold key" (Shift, Control or Alt). The controller button above always counts as well.
"Deposit" and "Same item only" are the storage button above.
"Exosuit lock mode" is the pin button (on).
"Lock follows item" (off) moves a pin with that reserved stack inside
the Exosuit.
"Write log" (off) turns quick_transfer_all.log on or off. The switches
are saved in quick_transfer_all.json in this folder.

After a game update the mod checks that the game code it uses is the same. If
not, it stays off (the log says why) and Quick Transfer works as in vanilla.
