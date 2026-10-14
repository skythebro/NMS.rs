Compact Quick Transfer
======================

The Quick Transfer list (where an item can be sent) takes far less room: shorter
rows, a smaller icon and text, and a popup that shrinks to match, so a long list
needs much less scrolling. Nothing about what the rows do is changed.

Game files only; no loader code. Goes in
No Man's Sky\GAMEDATA\MODS\CompactQuickTransfer. The row it changes is a private
copy (UI\BIGGERITEMGRIDS\QUICKTRANSFER). CompactQuickTransfer.lua names the
files so the loader can merge them with another AMUMSS script. Park it in the
manager, or delete the folder, to remove it.

Works with Craft from Storage's Quick Transfer: each box. It changes only the look,
not the ten rows the game fills.

In this repository the UI files live in presets\CompactQuickTransfer\default
and are added to the zip by release.py.
