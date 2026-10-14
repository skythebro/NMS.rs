GalaxyMapFilters
================

Extra filters for the galaxy map. Each one is a new entry in the map's Filter
list (the one with No Filter, Economy, Conflict, Lifeform, changed with the
Previous / Next Filter buttons). Stars that match stay lit, every other star
is dimmed.

Goes in No Man's Sky\GAMEDATA\MODS\GalaxyMapFilters. Needs NMS.rs 4.0.

Switches (in-game Mods page, GalaxyMapFilters)
----------------------------------------------

All off by default. Turn on only the filters you want; only those appear in the
Filter list, so it stays short. They work on every star, visited or not.

- Gas Giants filter: systems that have a gas giant.
- Abandoned Systems filter.
- Pirate Systems filter.
- Yellow / Green / Blue / Red / Purple Stars filter: the star colour group.
- Debug paint (off): developer check. Paints matching stars green and every
  other star magenta instead of the normal colours, to see what a filter
  matches. Changes nothing else.
- Log hovered systems (off): developer probe. Writes one line per system card
  you hover to galaxy_map_filters.log. Changes nothing.
- Write log (off): turns the log on without the probe.

Turn a filter off while it is selected and the map goes back to No Filter.
With every filter off the map is vanilla, except that the Filter list box
stays a little taller (set by GalaxyMapFilters.lua; restart after installing).

After a game update the mod checks that the game code it uses is the same. If
not, it stays off (the log says why) and the game works as in vanilla.
