---
title: "Workspace, columns and filters"
weight: 40
---

Choose a workspace layout from the View menu or command palette:

- Two folder panels.
- Two folder panels with an inspector.
- One folder panel with an inspector.

The inspector has View and Properties tabs. {{< key "F3" >}} opens View; using it again
while View is selected closes the inspector. {{< key "Alt" "Enter" >}} opens Properties.

Each panel can use list or icon view. The two views show the same entries and
keep the cursor when switching. In icon view, arrows move by cell or row; use
{{< key "Alt" "=" >}} to increase icon size and {{< key "Alt" "-" >}} to decrease it.

Use the column headers to adjust the listing:

- Click a column's sort control to sort.
- Drag header borders to resize columns.
- Click **+** at the end of a header to edit columns.
- Right-click a header to remove it or open the column editor.

Name is required. Column choices are shared by both
panels and saved in order. Available columns include file details, permissions
and owner, Git status, media duration, extended attributes and EXIF fields.
Some values are available only for local files.

{{< shot src="images/fyle-columns.png" alt="Columns chooser grouping file, media, permissions, Git, attribute and EXIF columns" caption="Choose the columns to display; Name is required." >}}

Folder sizes are calculated on demand. Press {{< key "Ctrl" "Shift" "S" >}} to measure marked
folders, the folder under the cursor, or all folders shown when the cursor is
on a file or `..`. Results appear in the Size column; {{< key "Esc" >}} cancels the scan.
Refresh with {{< key "Ctrl" "R" >}} to clear calculated sizes.

Right-click a column header and choose **Filter column** to filter displayed
values. Text columns use case-insensitive substring matching; categorical
columns let you choose values. Rules on different columns combine, and each
panel shows chips for active rules. Filters clear on navigation and new search,
and survive refresh and panel swaps.

{{< shot src="images/fyle-column-filter.png" alt="Filter column dialog showing text and values modes and extension choices" caption="Filter the Extension column by text or choose listed values, then Apply the rule." >}}
