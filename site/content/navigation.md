---
title: "Navigation and selection"
weight: 20
---

Each panel has its own location, cursor, marks, filter, sort order, view and
navigation history. {{< key "Tab" >}} switches panels. {{< key "Backspace" >}} goes to the parent;
{{< key "Alt" "Left" >}} and {{< key "Alt" "Right" >}} move through that panel's history. {{< key "Ctrl" "L" >}} edits
the active location. {{< key "Ctrl" "G" >}} opens Locations for drives, places, bookmarks,
frequent folders and configured remotes. {{< key "Ctrl" "D" >}} adds or removes a bookmark.

In list or icon view, {{< key "Space" >}} or {{< key "Insert" >}} toggles a mark. {{< key "Ctrl" "A" >}} selects
all, {{< key "∗" >}} inverts marks and {{< key "+" >}} selects by glob or substring pattern. {{< key "Esc" >}}
clears the current filter first, then marks. Use {{< key "Ctrl" "U" >}} to swap panel
contents, including their locations and panel state; use {{< key "Ctrl" "Left" >}} or
{{< key "Ctrl" "Right" >}} to show the other panel's folder in the active panel.

Press {{< key "/" >}} or {{< key "Ctrl" "F" >}} to filter the current listing as you type. {{< key "Enter" >}} keeps
the filter; {{< key "Esc" >}} clears it. Press {{< key "Ctrl" "H" >}} to show or hide hidden files.
Breadcrumbs and the location controls in each panel header can also be clicked.

{{< shot src="images/fyle-locations.png" alt="Locations picker with a search field for finding folders and bookmarks" caption="Open Locations and type to narrow the choices." >}}

Tree mode is per panel and works with list view. {{< key "Ctrl" "T" >}} toggles it; in the
tree, {{< key "Left" >}} collapses and {{< key "Right" >}} expands a folder. {{< key "Ctrl" "1" >}} selects list view and
{{< key "Ctrl" "2" >}} selects icon view.
