Sphere-Sized Mining
===================

In vanilla, every dig of the Terrain Manipulator gives at most 8 of a
deposit's main resource, 2 of its rarer ones and 8 Silicate. The medium and
large spheres dig out 8 and 64 times as much as the small one, yet hit the
same cap.

With this mod the cap follows the sphere's volume: the small sphere is
unchanged, the medium one can give up to 8 times as much per dig and the large
one up to 64 times. You still only get what the sphere cuts out of the
deposit, so in practice the large sphere mines about twice as fast (about 40
to 55 units a second against about 24). Tested with the multi-tool Terrain
Manipulator. The Exocraft terrain tool is not tested and may take another
code path that keeps the vanilla caps.

Only the game's memory is changed; no game file or save data is touched.

Needs the NMS.rs loader (installed by nms-rs.exe). Its settings are on the
in-game Mods page (Options, then Mods), and are saved in
No Man's Sky\GAMEDATA\MODS\SphereSizedMining\sphere_sized_mining.json.

Asteroids: the scaled caps are only in place while a Terrain Manipulator dig
is paid out, so mining an asteroid with the ship (or anything else that uses
the same caps) gets the vanilla amount.

Settings (all apply at once, no restart):
- Status: On, or why the mod is off.
- Scale mining yield (on): off puts the vanilla caps back straight away; on
  scales them again from the next dig.
- Yield grows with (Volume): Volume gives the medium and large spheres 8 and
  64 times the vanilla cap, Area 4 and 16 times, Linear 2 and 4 times. Off
  keeps the vanilla caps.
- Yield multiplier (1.0, 0.25 to 4.0): multiplies the factor of the medium and
  large spheres. The small sphere always keeps the vanilla cap, and no cap
  ever drops below vanilla, so 0.25 makes the big spheres closer to vanilla,
  not worse than it.
- Largest factor (64, 1 to 512): upper limit on the factor, after the
  multiplier. With the defaults the large sphere sits exactly at 64.
- Write log (off): turns sphere_sized_mining.log on or off.

Each cap is the vanilla cap times the factor, rounded, and at least 1.

After a game update the mod checks the game code again; if it moved, the mod
stays off, the game keeps its vanilla caps and the log says why.
