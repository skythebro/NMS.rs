Trade UI Tweaks
===============

Tweaks for the station / galactic trade UI. More can land here later.
Crafting and Quick Transfer stay in Craft from Storage.

Needs the NMS.rs loader. Goes in No Man's Sky\GAMEDATA\MODS\TradeUITweaks.
Park it in the manager (NMS.rs\nms-rs.exe), or delete the folder, to turn it off.

In the game
-----------

The TradeUITweaks section of the in-game Mods page (Options, then Mods)
shows the status and every switch below.

"Shop: Q to affordable" (on by default) is the buy slider. Left/right still
jump to 1 or to the stock max. Q from 1 still jumps to max when you can pay
for all of it. If that max costs more than your units, Q writes the most you
can pay at the price the shop charges (the Galactic Trade Network tooltip,
not the listed Buy for). If even 1 is too much, Q writes 1. Sell lists stay
vanilla. Turn it off for the game's own 1-to-max wrap.

"Shop: remember Buy/Sell list" (on by default) keeps the item-list step
for Buy and for Sell while the trade window is open, so each button
returns to where you left it. Closing the trade window clears both and
starts at the top next time. Turn it off for the game's shared step.

"Shop: more inventories" (on by default) extends Switch and the C key
past Exosuit, Starship and Freighter. Starship is included when that
inventory exists, including on a landing pad. When those exist it
also walks a nearby vehicle, the Storage Containers at the base you
are in (or on a freighter in range), and corvette parts. Boxes you own
somewhere else stay out of the list. Ingredient Storage and Cold
Storage stay off this switch: those holds only take food or fish, not
trade goods. Buy still lists the vendor stock. Sell lists the
inventory you picked. The title line follows the inventory: Exosuit,
Starship and Freighter keep the game's own names and icons. A Storage
Container uses the name you gave it and the same crate icon as Quick
Transfer, or Storage Container if it has no name. Vehicle and corvette
parts use those names. Scroll stays on the list. Turn it off for the game's three
inventories.

These switches change the game's code in memory and take effect straight away,
turning off puts the game's own behaviour back.

"Write log" (off) turns trade_ui_tweaks.log on or off.

After a game update the mod checks the game code again at every start. If it
moved, the shop stays vanilla (the log says why) and the rest of the game
works as usual.
