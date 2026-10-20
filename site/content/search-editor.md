---
title: "Search and editing"
weight: 60
---

Press **Alt-F7** or **Ctrl-Shift-F** to find files beneath the active panel's
folder. Search by name pattern and/or by text content, using a literal string
or regular expression. Search results fill the panel; Enter opens a result in
its containing folder, Backspace returns to the previous location, and Esc
stops a running search. F3 can preview a text match and scroll to its line.

Search and content filtering are different: **/** or **Ctrl-F** filters the
current listing, while Find Files walks below the current folder. Search is
unavailable in remote folders and archives.

Press **F4** to edit a local UTF-8 text file up to 8 MiB in Fyle's editor. Each
file opens in its own window. **Ctrl-S** saves; Esc or Ctrl-W closes the window.
If there are unsaved changes, choose Save, Discard or Cancel. Before saving,
Fyle checks whether the file changed on disk. Set `[editor].command` to use an
external editor instead. **Shift-F4** creates a new file and opens it.

Remote editing is unavailable. Opening a remote file downloads a cached copy;
editing that copy does not upload changes to the remote.
