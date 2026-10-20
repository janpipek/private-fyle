---
title: "Workspace, columns and filters"
weight: 40
---

The workspace can show two folder panels, two panels with an inspector, or one
panel with an inspector. Choose a layout from the View menu or command palette.
The inspector has View and Properties tabs. **F3** opens View; using it again
while View is selected closes the inspector. **Alt-Enter** opens Properties.

Each panel can use list or icon view. The two views show the same entries and
keep the cursor when switching. In icon view, arrows move by cell or row; use
Alt-= to increase icon size and Alt-- to decrease it.

Sort by clicking a column's sort control. Drag header borders to resize. Use the
**+** at the end of a header to edit columns; right-click a header to remove it
or open the column editor. Name is required. Column choices are shared by both
panels and saved in order. Available columns include file details, permissions
and owner, Git status, media duration, extended attributes and EXIF fields.
Some values are available only for local files.

Folder sizes are calculated on demand. Press **Ctrl-Shift-S** to measure marked
folders, the folder under the cursor, or all folders shown when the cursor is
on a file or `..`. Results appear in the Size column; **Esc** cancels the scan.
Refresh with **Ctrl-R** to clear calculated sizes.

Right-click a column header and choose **Filter column** to filter displayed
values. Text columns use case-insensitive substring matching; categorical
columns let you choose values. Rules on different columns combine, and each
panel shows chips for active rules. Filters clear on navigation and new search,
and survive refresh and panel swaps.
