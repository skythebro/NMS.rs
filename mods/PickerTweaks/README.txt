PickerTweaks
============

Two helpers for the item pickers and amount controls, both driven by one
hotkey (Ctrl+F by default) and the game's own text box.

- Search item pickers: with a picker open (refiner, Nutrient Processor,
  Transfer Items, fossil exhibit, Install Technology) or the craft list of an
  inventory (click an empty slot, Craft), press the hotkey, type part of an item name and press
  Enter. Only matching items stay in the list, and the picker stays open
  behind the box. The box opens empty every time (a new search). Accept an
  empty box to show everything again; Esc, right click or the controller's B
  keeps the search as it was. The search also ends when the picker closes.
  The hotkey does nothing when no picker is open. In the craft list only the
  matching recipes are listed while the search is set.
- Type an amount: while you hold a stack (stack split, refiner and cooker
  slots, Transfer Items), or on a refiner with an amount set, press the
  hotkey, type a number and press Enter. The box opens empty. Enter on an
  empty box picks the most the game allows; Esc or right click changes
  nothing. A held stack takes any number from 1 to the amount in that stack
  (the arrows may step in bigger amounts); the refiner amount stays inside
  the range of its own arrows.
  Craft amounts and shop amounts are not covered (TradeUITweaks has the
  shop).
- An empty box counts as accepted only when Enter, a left click or the
  controller's A button closed it.
- While the box is open, and until the key that closed it is let go, the
  picker and the page behind it ignore buttons, so A or B on a controller only
  answers the box. The mouse cursor stays where it was.

Goes in No Man's Sky\GAMEDATA\MODS\PickerTweaks. Needs NMS.rs 4.0.

Switches (in-game Mods page, PickerTweaks)
------------------------------------------

- Search item pickers (off).
- Type an amount (off).
- Hotkey: Ctrl+F, F3, F4 or None.
- Pad guard under the box (on): keeps controller A and B from also reaching
  the picker behind the box. Switch it off if the controller misbehaves after
  using the box; the box itself works the same, only that protection is gone.
- Log item names (off): writes each picker item's name key and the text the
  game shows for it to picker_tweaks.log. Changes nothing.
- Write log (off).

Order when several things are on screen: a held stack first, then an open
picker, then the refiner amount.

After a game update the mod checks that the game code it uses is the same. If
not, that part stays off (the log says why) and the game works as in vanilla.
