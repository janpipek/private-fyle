# Behavior reference

Detailed behavior and limits, moved from the project README. For everyday
tasks, start with the [user guide](../site/content/_index.md).

## Keys

The menu bar (File, Edit, View, Go, Tools, Help) lists the commands with
their shortcuts; unavailable ones are greyed out with an explanation, and
toggles show a check. Opening Tools discovers actions for the active folder
and marked entries; actions are grouped by purpose and missing programs are
shown on disabled rows. F2 opens the searchable version of the same tools.
Right-click an entry in either panel for file actions, a Selection submenu and
contextual Tools for Selection. Copy Name copies the entry under the cursor;
Copy Full Path copies a local absolute path or a standard URL for supported
configured remotes. It is unavailable for archive members and providers
without a standard URL. Clicking a marked
entry keeps the marked group; clicking another entry clears the marks and
targets that entry. Escape closes the menu and returns focus to that panel.
Keyboard shortcuts are available in the menus, F1 help and the Ctrl-P command
palette. The bottom area shows status messages and running jobs.

| Key                 | Action                                    |
| ------------------- | ----------------------------------------- |
| Up / Down, PgUp/Dn  | Move cursor (icon view: one row / a page of rows) |
| Home / End          | First / last entry                        |
| Enter / dbl-click   | Enter directory or archive (zip, tar, tar.gz/tgz: browsed like a folder, read-only), or open the file (first matching `[[opener]]`, else the system default) |
| Shift-Enter         | Open with… for a file or directory: matching `[[opener]]`s, then the system's default application (marked "(default)" when launchable) and other applications registered for its type (Linux and Windows); files also offer the built-in editor |
| Backspace           | Parent directory                          |
| Alt-Left / Alt-Right | Back / forward through visited directories (per panel) |
| Tab                 | Switch active panel                       |
| Space / Insert      | Mark entry                                |
| Ctrl-A / * / +      | Select all / invert selection / select by pattern (glob or substring) |
| / or Ctrl-F         | Filter as you type (Enter keeps the filter, Esc clears it) |
| Esc                 | Clear the filter, else the marks          |
| (no key)            | Clear Selection (Edit menu, palette, F1): unmark everything in the active panel, leaving the filter; unavailable when nothing is marked |
| Shift-F4            | New file: create and edit it |
| Ctrl-C / Ctrl-X / Ctrl-V | Yank / cut / paste entries (paths also go to the system clipboard); paste runs as a background copy/move |
| F5                  | Copy marked (or current) to other panel; inside an archive, extract them there (existing files are asked about) |
| Shift-F5            | Extract the archive under the cursor to a folder named after it in the other panel |
| F6                  | Move marked (or current) to other panel   |
| Shift-F6            | Rename; with several marked, edit one name per line (bulk rename, swaps allowed) |
| F7                  | New directory                             |
| F8 / Delete         | Move to trash (with confirmation)         |
| Shift-F8 / Shift-Delete | Delete permanently (with confirmation) |
| Ctrl-H              | Toggle hidden files                       |
| Ctrl-T              | Toggle tree mode (per panel)              |
| F1                  | Help: all commands with their keys (runnable, like Ctrl-P) |
| F2                  | Tools for the selection / directory (see [`docs/tools.md`](tools.md)) |
| Alt-F7 / Ctrl-Shift-F | Find files by name (glob or substring) and/or containing text (literal or regex; hits show the matching line, and F3 preview scrolls to it), below the panel directory; results fill the panel (Enter shows a hit in its folder, Backspace returns, Esc stops) |
| Ctrl-G / Alt-F1     | Locations: drives/mounted volumes, desktop places (home, user folders, GTK/KDE bookmarks), fyle bookmarks and frequent directories |
| Ctrl-L              | Edit or paste the active panel's full path |
| Ctrl-U              | Swap Panels: exchange the two panels' content (typed local, remote, archive or search location), cursor, marks, filter, sort, tree state, history and list/icon view; the active side stays active. Unavailable with an explanation while a folder is loading or a search / size scan is running (Esc cancels it) |
| Ctrl-Left / Ctrl-Right | Go to Other Panel's Folder: the active panel shows the other panel's folder (local, remote or archive location kept; search results lead to their root; a navigation still loading there is followed to its destination; unavailable when the other panel shows Places) |
| Ctrl-D              | Add or remove the current directory's bookmark (saved in `config.toml`); the star in the location bar shows and toggles it |
| F9                  | Open a terminal in the panel directory (`[terminal]` in `config.toml`) |
| F3                  | Toggle Preview in the inspector; it follows the active panel's cursor and does not replace the inactive folder panel |
| Alt-Enter           | Open Properties for the cursor entry in the inspector |
| F4                  | Open the cursor file (UTF-8 text, up to 8 MiB) in its own editor window; each file gets its own window. Ctrl-S saves, Esc/Ctrl-W closes (with unsaved changes it asks: Save, Discard or Cancel); saving asks before overwriting a file changed on disk. `[editor] command` in `config.toml` uses an external editor instead |
| Ctrl-P              | Command palette: all commands with their keys, filtered by typing |
| Ctrl-K Ctrl-T       | Theme picker: arrows preview live, Enter keeps and saves to `config.toml`, Esc restores |
| Right / Left        | Tree: expand / collapse (or go to parent); in icon view: next / previous cell |
| Ctrl-1 / Ctrl-2     | List view / icon view for the active panel (View menu; saved as `left_view` / `right_view`) |
| Alt-= / Alt--       | Larger / smaller icons in icon view (saved as `icon_size`) |
| Ctrl-R              | Refresh (also forgets computed folder sizes) |
| Ctrl-Shift-S        | Calculate Sizes (View menu, palette, right-click): recursive size of the marked folders, else the folder under the cursor, else (cursor on `..` or a file) of every folder listed. Runs in the background, shows in the Size column (`… 1.2 G` while running, `≥ 1.2 G` when incomplete: unreadable entries or stopped); Esc stops. Symlinks are not followed, hard-linked files count once, other filesystems are skipped; not available inside archives. Sorting by size uses the results; the footer sums marked entries (`≥` and "n folders not measured" when unknown). Never runs automatically |
| Ctrl-+ / Ctrl-- / Ctrl-0 | Zoom in / out / reset (also Ctrl-wheel throughout the panels and preview, including scrollable content); saved as `font_size` in `config.toml` |
| Ctrl-Shift-C        | Cancel running copy/move/delete jobs (also a Cancel button per job) |
| F10 / Ctrl-Q        | Quit                                      |

The View menu offers three workspace layouts: both folder panels, both panels
with an inspector, or the active panel with an inspector. The inspector has
View and Properties tabs. F3 opens/selects View, closes the inspector when View
is already selected, and switches from Properties to View. Alt-Enter and File
› Properties open the inspector on Properties; the same command is available
from the file context menu. The active tab stays selected when the layout
changes and for the current app session. In the one-panel layout, the other
panel remains the destination for copy and move commands and is identified by
its destination label. The selected layout is saved in `config.toml` and restored at startup; set
`[workspace] layout` to `"two_panels"`, `"two_panels_inspector"` or
`"one_panel_inspector"` to choose it directly. This is the only startup
setting for the inspector.

Properties shows source listing facts for local, remote and archive entries.
For local entries, a single Filesystem group combines source identity with
fresh native attributes; symlink target facts appear in a separate subgroup.
On Unix, fresh filesystem facts show owner and group names when found in the
local account files, alongside their numeric UID and GID. Symlink objects and
their targets each show their own ownership. Numeric IDs remain visible when
a name cannot be resolved.
On Linux, Extended Attributes shows the selected local object's sorted attribute
names, sizes and values using the shared `xattr` native reader. Symlinks show
attributes of the link itself, including dangling links; targets are not followed.
Values use UTF-8 text when valid and free of non-text control characters,
otherwise hexadecimal with a Binary label. Empty values are explicit. Long values
show a byte-counted prefix and truncation status; omitted values and individual
read failures stay visible alongside readable attributes. Names preserve native
bytes for lookups and escape control/non-UTF-8 bytes for display. Inspection caps
the name list at 64 KiB, names at 255 bytes, count at 64, individual value reads
at 64 KiB and total successful value reads at 256 KiB. Display retains at most
1,024 source bytes per value and 8 KiB overall, respecting UTF-8 boundaries.
Size/read races get at most three attempts; cancellation and a two-second deadline
are checked between native calls. Empty/unsupported attribute sets are hidden;
list failures do not hide other Properties facts. Remote/archive Properties stays
listing-only. The Xattrs column still shows names and SELinux keeps its existing
context formatting, now through the same bounded native primitives.

If native stat fails, listing facts remain visible with a status. The view also
shows supported EXIF fields for regular local files. The metadata-only reader
checks at most 8 MiB for ordinary images or 64 MiB for supported RAW files,
limits each displayed field to 4,096 characters, and can be cancelled. Empty
or inapplicable metadata groups are hidden; independent failures and truncated
results remain visible. For remote and archive entries, Properties does not download or
extract data; only facts already present in the listing are shown. A Properties
inspection does not depend on View or on
optional preview programs. Local regular Parquet files show schema paths,
physical and logical types, definition and repetition levels, file row count,
and per-row-group row counts and compressed/uncompressed sizes. This footer-only
reader caps serialized metadata at 8 MiB, schema output at 256 leaf columns,
row groups at 128, and displayed text at 1,024 characters. Footer decoding
cannot be interrupted and decoded metadata may use more memory than its
serialized size. Symlinks to regular local Parquet files show target metadata
while Filesystem continues to describe the link object. Local regular SQLite
databases show user table names only. SQLite inspection is read-only,
cancellable and limited to two seconds, 128 table names and 1,024 characters
per name. Symlinks to regular databases inspect their targets. Local
regular RPM and DEB packages show a Package group with name, version,
architecture, summary/description, available maintainer/vendor/packager,
homepage, section/group, declared license when present, and installed size in
bytes. RPM release and epoch are separate fields; DEB keeps its complete version
string. Debian license data is commonly outside the control file and is then
omitted. Native `formats/package` readers inspect RPM headers or Debian control
archives (plain, gzip, XZ or Zstd), without package-manager programs, extraction,
script execution, installed-package queries or signature verification. RPM
signature/header data is capped at 8 MiB each, 4,096 index entries per header
and 128 translations per text tag;
RPM descriptions use the first stored translation. Debian control archives are
capped at 8 MiB compressed and decompressed, 32 ar members, 128 tar entries and
128 fields in a 256 KiB control file; XZ/Zstd decoder dictionaries/windows are
limited to 32 MiB. Text values retain at most 4,096 characters and report
truncation/replacement. Reads/decompression check cancellation and a two-second
deadline. Symlinks inspect regular-file targets, failures leave filesystem facts
visible, and remote/archive Properties remains listing-only. This adds metadata
inspection rather than a new package-content viewer. Local
regular audio and video files show Container, Video,
Audio and Subtitles groups from `ffprobe`; unavailable `ffprobe` and probe
failures appear in Properties without hiding filesystem facts. The shared
reader caps process output at 1 MiB, error output at 8 KiB, streams at 32,
and displayed values at 1,024 characters, with a ten-second deadline and
inspection cancellation. The stream analysis requests a 10 MB probe size
and five-second analysis window. Video View still extracts a frame, while
audio View shows format and tag details. One native `builtin.media` definition
binds the existing Duration, Properties and Preview adapters; they retain their
independent request lifecycles and share no cross-consumer cache.

Local entries in a Git working tree also show a Git Properties group with the
repository root, queried panel directory, repository-relative selected path and
status. The Git column and Properties share bounded snapshots for the same
query scope and refresh epoch; directory marks keep their existing aggregation.
“No changes reported” means Git returned no record for that path, including
when it groups an ignored or untracked parent directory. Ctrl-R, listing reloads
and operations refresh repository context even if the selected file is unchanged;
there is no continuous Git watcher. Git remains an optional executable; failed
queries appear independently in Properties. Remote and archive items never run
these queries.

On Linux, explicitly enabled [reader packages](extensions.md) can add
Properties sections and custom View layouts for local regular files. Sections
retain independent field failures and partial/lower-bound measurements. Package
viewers follow specialized built-ins and precede generic text/binary; they offer
selectable text, typed tables and bounded native path graphs. Remote/archive
copies do not authorize package inspection. The [sensor example](../examples/extensions/sensor/README.md)
demonstrates Properties, a display-only column and a custom graph without
rebuilding fyle; restart after editing the package.

Text in Properties, including labels, paths, values, table contents and errors,
can be selected with the mouse and copied with Ctrl-C. When Properties text is
selected, Ctrl-C copies it instead of yanking panel files. Changing the
inspected item or leaving Properties clears the text selection.

Symbolic links keep their target's file icon with a small badge: a curved
arrow for a working link, a crossed circle (and a muted name) for a broken
one (missing target or a loop). Hover the name for `→ target` and the
resolved path, or `Broken link (…) → target`; F3 shows the same in its header
(for a broken link, in place of the preview). Links inside tar archives show
the badge too (zip does not store the target when only listing).
A link to a folder is entered like a folder: the panel path, breadcrumbs and
history keep the link's path, not the resolved one.

F3 previews Parquet files as a table using the native reader. The sample is
limited to 500 rows and 40 columns, with cells capped at 1,024 characters,
footer metadata at 8 MiB, and selected column chunks have a 64 MiB decoding
budget. Uncompressed, Snappy, gzip, Brotli, LZ4 and Zstandard chunks are
supported. Files with
repeated, list or map columns show an unsupported-format message.
One native `builtin.parquet` definition binds Properties metadata and the
Preview row sample to the checked footer reader; footer inspection and row
sampling retain separate demand and presentation limits.

F3 previews local GPX and FIT files with available name, type, sport, start,
duration, distance, ascent/descent and track/lap counts, plus an offline route
outline when GPS points exist. GPX track segments stay separate, so gaps do not
count as traveled distance or appear as connecting lines. The outline uses the
active theme's foreground and background colors for a wide track stroke, and
its success and danger colors for the start and end markers; it updates when
the theme changes. FIT activity session
totals are used when present; other FIT types show the fields they contain,
including files with no GPS track. Properties reads the same format models
independently for local regular files; remote and archive Properties continue
to show listing facts only. Readers cap source files at 16 MiB and route outlines
at 100,000 points. The FIT decoder validates the file CRC; its bounded decode
call cannot be interrupted mid-file.
Invalid duration or distance values show a field-level error while other
readable facts and the route outline remain available.

F3 previews XLSX, XLSM, XLS, XLSB and ODS workbooks as tables using the pure-Rust
`calamine` reader (no external programs). One sheet shows at a time with
sheet-name tabs; other sheets are read on demand. Columns are headed by letters
(the sheet's used range keeps its absolute letters and a row-number column),
formulas show their cached values,
dates appear as ISO text and errors as codes. The sample is limited to 500
rows and 40 columns with cells capped at 1,024 characters; files over 32 MiB or
declaring over 256 MiB uncompressed are refused. Corrupt or password-protected
files show the reader's error. XLSX, XLSM and XLSB cells are streamed into the
bounded sample, including sparse sheets with distant cells. XLS and ODS still
use calamine's in-memory sheet range, which can be large for sparse sheets.

F3 previews Word (`.docx`, `.docm`, `.dotx`, `.dotm`) and OpenDocument text
(`.odt`, `.ott`) documents as formatted text: headings, bold and italic,
nested lists, tables, page breaks and `[image: name]` placeholders, with a word
count. Layout, fonts, footnotes, headers and underline are not reproduced and
deleted tracked changes are skipped. Pure Rust, bounded to 8 MiB of
uncompressed XML, 5000 blocks and 300000 characters (then marked truncated).
Legacy `.doc` and encrypted files show an explanatory message or the hex dump.
Markdown previews (including document and notebook text) use an installed font
with distinct bold and italic faces when the Linux theme font resolves to a
variable Cantarell or Noto Sans face.

F3 previews `.ipynb` notebooks as a lazy, read-only list of Markdown and
highlighted code cells with their saved text, error and PNG/JPEG outputs. The
Rust nbformat 4 reader bounds notebooks to 16 MiB and 500 cells, text items to
512 KiB, outputs to 100 per cell, images to 4 megapixels each and 64 MiB total.
It never starts a kernel. Raster data takes precedence in mixed MIME bundles;
HTML-only outputs render only when the fragment is a single image with a
base64 PNG or JPEG data URI. Other HTML outputs, HTML in Markdown or raw cells,
SVG, math, attachments, widgets, remote Markdown images and unknown output
types show explicit placeholders.

F3 previews `.pptx` and `.pptm` presentations as a scrollable list of slides in
presentation order. It extracts slide text and labels embedded images with
placeholders; layout and speaker notes are not reproduced. The reader is pure
Rust and caps the ZIP file at 32 MiB, XML at 2 MiB per entry and 16 MiB total,
slides at 300 and text at 300,000 bytes.

F3 and icon view can use embedded JPEG previews from camera RAW files with
extensions `.raw`, `.cr2`, `.cr3`, `.nef`, `.nrw`, `.arw`, `.sr2`, `.srf`,
`.raf`, `.rw2`, `.orf`, `.pef` and `.dng`. This is best-effort JPEG extraction;
files with only bitmap/JPEG XL previews or no embedded JPEG show the file icon
or a preview error. Extraction scans at most 64 MiB and limits JPEG dimensions,
decode memory and candidate count; it does not decode RAW sensor data. RAW
thumbnails use the original file's URI and modification time in the shared
thumbnail cache. View shows images and video frames without metadata overlays;
use Properties for EXIF details. Audio keeps its standalone format and tag
details in View. Properties and audio View share the bounded `formats/media.rs`
reader, including decoded ID3/common tags: title, artist, album, album artist,
composer, date, genre, track, disc, comment and copyright. This requires
`ffprobe`; raw ID3 frames and embedded artwork are not displayed. Ogg Vorbis
uses Vorbis comments rather than ID3. Missing container tags fall back to the
first audio stream among the first 32 streams, including common Vorbis tag names
such as `ALBUMARTIST`, `TRACKNUMBER`, `DISCNUMBER` and `DESCRIPTION`.

File recognition is shared by Open With, panel icons and Preview through the
UI-independent `recognition` model and its swappable content `Detector` interface.
The native detector examines at most 8 KiB: known binary signatures (including
SQLite), complete XML roots (including XMP), JSON, simple YAML mappings and
GLSL/WGSL content clues. It also recognizes guarded BMP, TIFF, 7z, RAR, xz,
bzip2, zstd and RIFF AVI prefixes. These signatures are bounded clues, not
format validation; generic containers carry no preview subtype. The Properties
perspective exposes ranked MIME candidates with evidence labels for local
regular files, retaining filename clues and reporting a bounded-prefix read
failure explicitly. Known TIFF-based RAW filename types and matching compressed
tar subtypes keep their more specific names; mismatched content signatures remain
ranked above the filename clue. Properties uses native filename hints, while
Open With also canonicalizes names through the system MIME database, so its
candidate vocabulary may differ. Candidates retain evidence and deterministic likelihood
order: signatures, filename hints, text heuristics, then generic text/binary.
Ambiguous text is inferred conservatively; heuristic matches are not validation.
Generic ZIP signatures preserve named Office/OpenDocument subtypes. Preview keeps
its existing viewer order and reader limits; inferred text highlighting applies
to extensionless files. Icons refine only visible local rows through the bounded,
generation-checked metadata queue. No render reads files. Remote/archive rows use
names; Preview may refine a copy only after the existing download/extraction path.
Open With still supports local entries only. Unknown signatures and text patterns
fall back to existing behavior; there is no `file` executable dependency.

In tree mode, clicking a folder's chevron also expands/collapses it; Enter
still makes the folder the panel's root.

Icon view (View › Icon View, per panel): a grid of large file-type icons with
the name below (two lines, then `…`; the tooltip shows the full name, size
and date, plus the target of a link). It shows the same entries as the list
(directories first, same sort, filter, hidden files, search results and
archives; `..` is a cell too) and keeps the cursor when you switch. The
cursor is the highlighted cell; marked cells get a coloured outline and a
check, so marks do not rely on colour. Arrows move by cell and row, PgUp/PgDn
by visible rows, Home/End to the ends; click, double-click, right-click,
Space and all other panel commands work as in the list. Tree mode only
applies to the list (switching to icons turns it off). Larger / Smaller Icons
step through 48, 64, 96, 128 and 160 px. Images (png, jpeg, gif, webp, bmp,
tiff up to 64 MiB, and supported RAW files using an embedded JPEG preview)
show a thumbnail instead of the icon once it is loaded, in
the background and for the visible cells only. Thumbnails live in the shared
freedesktop cache (`$XDG_CACHE_HOME/thumbnails`, size `normal`/`large`/
`x-large`/`xx-large` picked from the icon size and the display scale), so
Nautilus, Dolphin and fyle reuse each other's; a cached one is used only if
its recorded modification time matches, otherwise it is regenerated. Files
that fail to decode are recorded under `fail/fyle/` and not retried until they
change. Files inside archives keep their icons.

In either panel, click to select an item, Ctrl-click another item to add it to
the current selection (then Ctrl-click to toggle marks), and Shift-click to
mark a range (Ctrl-Shift-click adds a range). Drag a selected
group between panels or onto a folder to choose Copy or Move; the transfer
uses the usual progress and conflict dialogs. Archive contents can be copied
to a local folder. Local files can also be dragged out to another desktop
application, and local files from another application can be dropped into a
panel. Remote and archive entries do not yet offer outgoing desktop file data.

Click a column's sort icon to sort (directories always stay on top); drag
header borders to resize. The small **+** at the right of each table header
opens **Edit columns…**; right-click a header for **Remove column** or
**Edit columns…**. Name is required and cannot be removed. Column choices
apply to both panels and are saved in order in the background.
Right-click a header and choose **Filter column…** to match its displayed values:
text columns use a case-insensitive substring, while categorical columns offer
multi-select values. Rules combine across columns, and each panel shows active
filter chips with per-column removal and a Clear all action. Facets are collected
from the whole listed folder in cancellable background batches; unsupported and
failed metadata reads stay distinct from blank values. Filters clear on navigation
and a new search, and survive refreshes and panel swaps.
Unknown file sizes show as `—`. Edit columns… (also in the command palette)
picks the columns from a grouped catalog: file, owner and permissions, Git status,
media (optional duration for local audio and video files), extended attributes,
and EXIF fields of photos (date taken, camera, lens, exposure, aperture, ISO,
focal length, dimensions, GPS). These fields also load from supported camera
RAW files when their metadata or embedded JPEG carries EXIF data. Duration
uses the bounded media reader and is loaded for visible local rows in the
background; remote and archive rows stay blank. With the Ext column shown,
names leave out their extension
(`.gitignore` and other dotfiles stay whole). EXIF, owner, extended-attribute
and SELinux values load for visible
local rows in the background; remote and archive locations are never treated
as native paths for these fields.
Saved column IDs that are unavailable in the current build remain in settings
and show blank cells. Edit columns… lists them with their ID and an unavailable
explanation; changing other columns preserves them, while unchecking one removes
it. On Linux, explicitly enabled [reader packages](extensions.md) can add
columns for local regular files. Partial values show `…` and lower bounds `≥`;
hover explains partial values and blank failed/unavailable cells. These columns
are display-only. Built-in sorting and default columns are unchanged.

Windows: ".." from a drive root lists all drives; files with the hidden
attribute count as hidden.

Drag the divider between the panels to resize them; double-click it to
split evenly again.

Each panel header shows the current location as keyboard-focusable, clickable
breadcrumbs. The map button opens a menu of drives and mounted volumes,
desktop places, fyle bookmarks and frequent paths; groups with more than six
entries open as submenus, and "Search Locations…" (Ctrl-G) opens the same
places as a searchable list. Volumes are limited to the root, `/home`,
removable media (`/media`, `/run/media`), `/mnt` and network shares, so
snaps and system mounts stay out. The adjacent buttons edit or copy the full
path. Button tooltips show the panel's configured keyboard shortcuts in both
list and icon view. Long breadcrumbs scroll with
the current segment kept at the visible end. Archive files are labelled
`[archive]`, and their internal path uses indented segments.

Copy and move never overwrite silently. When a target exists, a dialog shows
both entries (path, type, size, modified) and offers Skip, Keep both (a
unique name), Replace, or Merge for two directories, and can apply the
choice to the remaining conflicts of the same kind or cancel the operation.
Copy, move, trash and delete run in the background with progress in a jobs
bar.

## Open with

The File and right-click menus offer Open With as a submenu; Shift-Enter opens
the same choices in a popup. They list, in this order: matching `[[opener]]`
rules; the desktop's default application for the file's type (or
`inode/directory` for folders), marked "(default)"; other applications
registered for it. Files the built-in editor can open (UTF-8 text up to 8 MiB)
also offer it; binary and PDF files do not. This check runs in the background.
The `..` entry is excluded.
A system entry that runs the same command as a configured opener is dropped,
and the opener takes the "(default)" mark. Entries whose program is missing
are greyed out with the reason. On Linux, desktop entries obey `OnlyShowIn`
and `NotShowIn` for the current desktop. Enter is unchanged: it runs the first
matching opener, or `xdg-open` (through the `open` crate).

On Linux the types and applications come from the desktop's files, read
directly (no `xdg-mime` or `gio`): the type from shared-mime-info's
`globs2` by file name (weight, then longest glob), refined by shared bounded
content recognition for the selected local file, with `subclasses` and `aliases` (so `text/x-rust` also offers `text/plain`
applications); desktop entries from `applications/` in `XDG_DATA_HOME` and
`XDG_DATA_DIRS` (`MimeType=`, `Hidden`, `NoDisplay`, `TryExec`, localized
names); `mimeapps.list` in the order of the spec, including the
`$XDG_CURRENT_DESKTOP` variants, Default, Added and Removed. Without an
explicit default the first associated application is marked. `Terminal=true`
applications start in the configured terminal. The lookup runs in the
background and is kept for 60 seconds (F5 drops it). On Windows the same
chooser reads extension defaults and registered applications from the registry
(including the current user's OpenWith list). For directories it offers Explorer
and registered folder open handlers. The System default choice uses
Windows itself for associations without a launchable command, including some
Store apps. macOS associations are not read yet.

## Settings

`config.toml` in the platform config directory:

- Linux: `~/.config/fyle/config.toml` (respects `$XDG_CONFIG_HOME`)
- Windows: `%APPDATA%\fyle\config.toml`
- macOS: `~/Library/Application Support/fyle/config.toml`

All keys are optional; see [`config.example.toml`](../config.example.toml) for
every key with its default. Errors (including unknown keys) are shown in the
status line and the defaults are used. Settings are read at startup.
Linux reader packages live in the config directory's `extensions/<id>/` folders.
`[extensions] enabled = []` is the default; adding IDs authorizes automatic
read-time execution for selected columns, Properties and View. Package and enablement changes need
a restart. See the [package example and execution contract](extensions.md).
The View menu's workspace layout choice is saved automatically in
`[workspace] layout`.
View › Custom Title Bar saves `window.custom_titlebar` (default `true`) for
the next start on Windows and Linux. The current windows keep their startup
decoration until Fyle restarts; the command is unavailable on macOS. On Linux,
`false` requests native decorations; client-only compositors still use the
themed title bar so their window controls remain available.

Bookmarks still accept ordinary path strings in `config.toml`; newly saved
bookmarks use a versioned location value so native path bytes and control
characters round-trip. Bookmarks can also be remote folders (`proton:/a/b`),
stored the same way; one whose remote is no longer configured stays in the
list and fails with a status-line message when opened. Frequent-directory
history reads the existing `count<TAB>time<TAB>path` format and writes typed,
versioned locations, keeping local, remote and archive addresses distinct.
History snapshots save in one background writer; while a save is active,
later visits coalesce into the newest pending snapshot.

### Keys

Help › Edit Keyboard Shortcuts edits existing fyle commands, checks shortcut
conflicts across overlapping contexts, and saves changes to `config.toml`.
Reset restores a command's default; Disable removes its shortcut. The
underlying `[[keymap]]` entries add or override bindings (`keys`, `action` such
as `"Copy"` or `"none"` to unbind, `context` = `panel` / `global` / `editor`);
see `config.example.toml`.

### Themes

Ctrl-K Ctrl-T (or View › Theme…) opens a picker; the highlighted
theme is applied immediately as a preview, Enter keeps it and writes it to
`config.toml` (other keys and comments are preserved), Esc restores the
previous theme.

`theme = "Tokyo Night"` selects a theme by name (case-insensitive);
`"system"`, `"light"` and `"dark"` use the built-in themes. 17 themes from
GPUI Kit are bundled (Ayu, Catppuccin, Everforest, Flexoki, Gruvbox, Solarized,
Tokyo Night, …; full list and attribution in [`themes/README.md`](../themes/README.md)).
More can be added as gpui-component theme files (`*.json`) in a `themes`
folder next to `config.toml`; a user theme with the same name as a bundled
one replaces it.

For the classic Midnight Commander colours, copy
[`examples/themes/midnight-commander.json`](../examples/themes/midnight-commander.json)
into that user `themes` folder, restart fyle, and select **Midnight Commander**
in the picker (or set `theme = "Midnight Commander"`). On Linux the folder is
`${XDG_CONFIG_HOME:-~/.config}/fyle/themes`; on Windows it is
`%APPDATA%\fyle\themes`. This optional user theme uses the classic ANSI palette:
blue panels, light-gray text, cyan cursor rows, white directories, yellow marks
and headers, gray popovers, and square corners. Its palette follows MC's
[default skin](https://github.com/MidnightCommander/mc/blob/master/misc/skins/default.ini);
actual terminal RGB colours vary. Fyle keeps each file's text colour on the
cursor row, so it cannot reproduce MC's black cursor text through a theme alone.
Marks share the danger colour in fyle, so danger indicators are yellow too;
destructive buttons retain red backgrounds.

### Window title bars

On Windows and Linux, the main window can combine its menus and themed title
bar in one row. This is on by default and can be changed in View › Custom Title
Bar; the change applies after a restart. When enabled, drag the title or empty
space to move the window and double-click to maximize or restore. Minimize,
maximize, close, and edge resizing use the platform window controls. Linux
requests client decorations from the window manager and keeps the existing menu
row if the manager selects server decorations. If native decorations were
requested but the compositor only supports client decorations, Fyle keeps the
themed title bar and its controls. Editor windows use a matching title bar, and
closing a modified document still asks whether to save it. macOS retains its
existing window decorations, and the command is disabled there.

## Remote locations

Remote folders have paths like `proton:/Reports`, `gdrive:/Docs` or
`nas:/photos`; the root is `<remote>:/`.

Using them:

- Remotes are found in the background at startup and whenever Locations
  opens (Ctrl-G or the map button); each shows in the "Remotes" group, labelled
  with its service ("Proton Drive", ...). Problems (e.g. a bad `remotes.toml`
  entry) appear once on the status line.
- Type `gdrive:/Docs` in the address bar (Ctrl-L) to go there, or pass it on
  the command line (`fyle proton:/ /tmp`); the panel shows your home folder
  until the remotes are found, or stays there with an error if the remote is
  unknown. Visited remote folders also appear under Frequent in Locations. Breadcrumbs start
  at the remote's root, named after the service. `..` at a remote root returns
  to the local folder you came from.
- Folders are listed in the background (Esc cancels; the panel shows "Loading…"
  and stays where it was if listing fails). The last 32 listings per panel are
  kept, so Back, Forward and Up are instant; Refresh (Ctrl-R) lists again.
- F5 copies between a local and a remote panel (upload or download) as a
  background job with progress and Cancel. Providers never overwrite, so names
  that already exist at the destination are listed first and can be skipped.
  F6 moves within one remote when it supports it. Moving or copying between
  local and remote (F6) or between two remotes is not supported yet.
- F7 creates a folder, Shift-F6 renames (one item), F8 removes: into the
  service's trash (Proton Drive) or permanently (`remotes.toml` servers), as the
  confirmation says.
- Enter on a remote file downloads it to `$XDG_CACHE_HOME/fyle/remote/<remote>/<path>`
  and opens that copy; edits are not uploaded back. The quick view (F3) shows
  such a copy for files up to 8 MiB.
- File > Open in Browser (also in the command palette) opens the item under the
  cursor, or the current folder on `..`, in the service's web view, which needs
  your own login; fyle never creates public or share links. Supported for
  Proton Drive (Docs and Sheets open in their own apps) and for rclone remotes
  of type `drive`; the address is looked up in the background. For Proton
  files and folders the address uses a share ID taken from the CLI; if it
  cannot be learnt, the Drive start page opens and the status line says so.
  SFTP, FTP, S3 and WebDAV servers have no web view.
- Everything else is unavailable on remote folders and says so: search, tools,
  archives, folder sizes, thumbnails, terminal, editing, tree mode,
  cut/copy/paste. Remote paths are never read from the local disk.

Providers:

- Proton Drive through the official `proton-drive` CLI (run
  `proton-drive auth login` first). Supports upload, move to trash, new
  folder, rename and move. Docs and Sheets are listed but the CLI skips them
  on download.
- Every remote set up with `rclone config` (Google Drive, OneDrive, ...);
  credentials stay with rclone.

Google Drive through rclone:

See [`HOWTO.md`](../HOWTO.md) for installing rclone, configuring OAuth access and
opening a Drive remote in fyle.

- Drive allows several items with one name. Only the first of each name is
  listed; the status line warns (e.g. "gdrive:/X: 2 items share a name with
  another item and are hidden; run rclone dedupe").
- Native Google Docs, Sheets and Slides have unknown size (shown as `—`, not
  zero). The marked total treats them as unknown bytes; quick view waits until
  a size is available instead of starting an unbounded download. Downloading
  one exports it with rclone's default export formats (docx, xlsx, pptx, svg
  for drawings), so a Doc is listed and saved as `Report.docx`.
- Every Drive remote also gets a read-only `<name>.shared` remote,
  "Google Drive (name) – Shared with me" in Locations (it runs rclone with
  `--drive-shared-with-me`): browse, download and open in the browser only.
- Shared drives (team drives) are separate rclone remotes: create one with
  `rclone config` and a team drive, and it appears here automatically.

Other providers:

- Servers in `remotes.toml` (next to `config.toml`; see `remotes.example.toml`)
  over SFTP, FTP, FTPS, WebDAV and S3, through the `remotefs` crates. Manage
  these entries from Go or Tools › Manage Remotes, also available in both
  Locations pickers. The dialog has a keyboard-navigable saved-remote list and
  shows only fields for the selected protocol; it supports add, edit, remove,
  test and connect;
  rclone remotes still use `rclone config`, and Proton Drive authentication
  uses `proton-drive auth login`. SMB is not supported. Removing a remote
  configuration never deletes server files. The file is never created
  automatically until the first remote is saved.

```toml
[[remote]]
name = "nas"                         # panel prefix nas:/
protocol = "sftp"                    # sftp | ftp | ftps | webdav | s3
host = "nas.local"                   # webdav: url = "https://..."
port = 22                            # optional
user = "jan"                         # optional
root = "/srv/data"                   # optional folder that nas:/ shows
password_command = "pass show nas"   # optional
```

S3 uses `bucket`, `region`, `profile` and `endpoint` instead of `host`; its
credentials come from the AWS environment or profile. SFTP uses ssh-agent and
the `IdentityFile` entries of `~/.ssh/config`. Passwords are never stored:
`password_command` is split into words (quotes group a word; no shell, pipes
or expansion), run when connecting, and its output is the password. Bad
entries are reported without hiding the good ones. SFTP verifies host keys against
`~/.ssh/known_hosts`. Opening a remote or testing its connection offers explicit
confirmation for an unknown key, showing the resolved host, port, algorithm and
SHA256 fingerprint. Accept saves that exact key in the background and retries;
cancel leaves it untrusted. Changed keys remain refused, including conflicts
added while confirmation is open. Keys can also be accepted first using `ssh`. If `~/.ssh/config` maps the configured `host` to another
`HostName` or `Port`, the key is checked for that server.

Building needs the OpenSSL development files (WebDAV); see Build
dependencies (`openssl-devel` on Fedora). Each protocol is its own Cargo
feature (`sftp`, `ftp`, `webdav`, `s3`; SFTP and FTP on by default) pulling in its own
SDK; building with `--no-default-features --features ftp,webdav` (for
example) drops the others and their crates. A remote configured for a
disabled protocol fails to connect with a clear message instead of failing
to parse.
