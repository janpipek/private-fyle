---
title: "File management and archives"
weight: 30
---

Mark one or more entries, then use {{< key "F5" >}} to copy them to the other panel or
{{< key "F6" >}} to move them there. {{< key "F7" >}} creates a folder; {{< key "Shift" "F4" >}} creates a file
and opens it for editing. {{< key "Shift" "F6" >}} renames the cursor entry, or opens bulk
rename when several entries are marked. {{< key "F8" >}} or {{< key "Delete" >}} moves entries to the
trash when the filesystem supports it. {{< key "Shift" "F8" >}} or {{< key "Shift" "Delete" >}} deletes
permanently, after confirmation.

{{< shot src="images/fyle-copy-dialog.png" alt="Copy dialog asking to copy project-brief.md from Source to Destination" caption="Check the file and destination before confirming the copy." >}}

Transfers run in the background. Existing destinations are never overwritten
silently: choose Skip, Keep both, Replace or, for two directories, Merge.
{{< key "Ctrl" "Shift" "C" >}} cancels running copy, move or delete jobs.

For local files, {{< key "Ctrl" "C" >}} or {{< key "Ctrl" "X" >}} copies or cuts the selected paths;
{{< key "Ctrl" "V" >}} pastes them into the active folder as a background copy or move.
The paths also go to the system clipboard.

ZIP, TAR and TAR.GZ/TGZ archives can be browsed like folders. Archive contents
are read-only. {{< key "F5" >}} copies selected archive entries into the other panel;
{{< key "Shift" "F5" >}} extracts the archive under the cursor into a new folder there.
Archive transfers use the normal conflict handling. Nested archive browsing and
archive write support are not available.

{{< key "Ctrl" "R" >}} refreshes the active panel and clears computed folder sizes and
cached Open With results.
