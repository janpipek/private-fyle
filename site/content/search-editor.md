---
title: "Search and editing"
weight: 60
---

Press {{< key "Alt" "F7" >}} or {{< key "Ctrl" "Shift" "F" >}} to find files beneath the active panel's
folder. Search by name pattern and/or by text content, using a literal string
or regular expression. Search results fill the panel; {{< key "Enter" >}} opens a result in
its containing folder, {{< key "Backspace" >}} returns to the previous location, and {{< key "Esc" >}}
stops a running search. {{< key "F3" >}} can preview a text match and scroll to its line.

{{< shot src="images/fyle-find-files.png" alt="Find files dialog searching Markdown names for the text project" caption="Combine a file-name pattern with containing text, or leave either field empty." >}}

{{< shot src="images/fyle-search-results.png" alt="Search results in the source panel showing project-brief.md and its matching line" caption="Results replace the active listing and show the matching text line." >}}

Search and content filtering are different: **/** or {{< key "Ctrl" "F" >}} filters the
current listing, while Find Files walks below the current folder. Search is
unavailable in remote folders and archives.

Press {{< key "F4" >}} to edit a local UTF-8 text file up to 8 MiB in Fyle's editor. Each
file opens in its own window. {{< key "Ctrl" "S" >}} saves; {{< key "Esc" >}} or {{< key "Ctrl" "W" >}} closes the window.
If there are unsaved changes, choose Save, Discard or Cancel. Before saving,
Fyle checks whether the file changed on disk. Set `[editor].command` to use an
external editor instead. {{< key "Shift" "F4" >}} creates a new file and opens it.

Remote editing is unavailable. Opening a remote file downloads a cached copy;
editing that copy does not upload changes to the remote.
