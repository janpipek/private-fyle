---
title: "File management and archives"
weight: 30
---

Mark one or more entries to act on them. With no marks, file commands use the
entry under the cursor.

- {{< key "F5" >}} copies the entries to the other panel.
- {{< key "F6" >}} moves the entries to the other panel.
- {{< key "F7" >}} creates a folder.
- {{< key "Shift" "F4" >}} creates a file and opens it for editing.
- {{< key "Shift" "F6" >}} renames the cursor entry, or opens bulk rename when several entries are marked.
- {{< key "F8" >}} or {{< key "Delete" >}} moves entries to the trash when the filesystem supports it.
- {{< key "Shift" "F8" >}} or {{< key "Shift" "Delete" >}} deletes permanently, after confirmation.

{{< shot src="images/fyle-copy-dialog.png" alt="Copy dialog asking to copy project-brief.md from Source to Destination" caption="Check the file and destination before confirming the copy." >}}

Transfers run in the background. Existing destinations are never overwritten
silently: choose Skip, Keep both, Replace or, for two directories, Merge.
{{< key "Ctrl" "Shift" "C" >}} cancels running copy, move or delete jobs.

For local files, clipboard commands use the selection:

- {{< key "Ctrl" "C" >}} copies the selected paths.
- {{< key "Ctrl" "X" >}} cuts the selected paths.
- {{< key "Ctrl" "V" >}} pastes them into the active folder as a background copy or move.

The paths also go to the system clipboard.

ZIP, TAR and TAR.GZ/TGZ archives can be browsed like folders. Archive contents
are read-only.

- {{< key "F5" >}} copies selected archive entries into the other panel.
- {{< key "Shift" "F5" >}} extracts the archive under the cursor into a new folder there.

Archive transfers use the normal conflict handling. Nested archive browsing and
archive write support are not available.

{{< key "Ctrl" "R" >}} refreshes the active panel and clears computed folder sizes and
cached Open With results.
