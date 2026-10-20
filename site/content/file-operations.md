---
title: "File management and archives"
weight: 30
---

Mark one or more entries, then use **F5** to copy them to the other panel or
**F6** to move them there. **F7** creates a folder; **Shift-F4** creates a file
and opens it for editing. **Shift-F6** renames the cursor entry, or opens bulk
rename when several entries are marked. **F8** or Delete moves entries to the
trash when the filesystem supports it. **Shift-F8** or **Shift-Delete** deletes
permanently, after confirmation.

Transfers run in the background. Existing destinations are never overwritten
silently: choose Skip, Keep both, Replace or, for two directories, Merge.
**Ctrl-Shift-C** cancels running copy, move or delete jobs.

For local files, **Ctrl-C** or **Ctrl-X** copies or cuts the selected paths;
**Ctrl-V** pastes them into the active folder as a background copy or move.
The paths also go to the system clipboard.

ZIP, TAR and TAR.GZ/TGZ archives can be browsed like folders. Archive contents
are read-only. **F5** copies selected archive entries into the other panel;
**Shift-F5** extracts the archive under the cursor into a new folder there.
Archive transfers use the normal conflict handling. Nested archive browsing and
archive write support are not available.

**Ctrl-R** refreshes the active panel and clears computed folder sizes and
cached Open With results.
