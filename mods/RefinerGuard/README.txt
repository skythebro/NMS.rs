RefinerGuard
============

Moving a refiner (or Nutrient Processor) that still holds items deletes those
items: the game wipes the machine and nothing comes back to your inventory.
This happens at bases and on the corvette.

RefinerGuard stops that move. When you pick Move (or Delete) on a refiner
that holds anything, nothing happens to the refiner and the game shows an
on-screen message, the same way it tells you a running refiner cannot be
picked up: "Empty this refiner first: moving it deletes its contents." The
build tool keeps working. Take the items out, then move it as usual. Empty
refiners and every other part move as in vanilla. Only real item stacks
count: a Portable Refiner's Carbon fuel charge does not, so an empty but
charged Portable Refiner moves as usual.

Goes in No Man's Sky\GAMEDATA\MODS\RefinerGuard. Needs NMS.rs 4.0.

Switches (in-game Mods page, RefinerGuard)
------------------------------------------

- Guard loaded refiners (on). Off: moves work as in vanilla again.
- Write log (off): writes each refused move to refiner_guard.log.

After a game update the mod checks that the game code it uses is the same. If
not, it stays off (the log says why) and the game works as in vanilla.
