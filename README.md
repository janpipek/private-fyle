# fyle

A minimal two-panel file manager in the spirit of Krusader / Midnight Commander,
built with [GPUI Kit](https://gpui-kit.com) (GPUI + gpui-component's `DataTable`).
Targets Linux and Windows (macOS later, see `TODO.md`).

```sh
cargo run --release -- [LEFT_DIR] [RIGHT_DIR]
```

Defaults: left = current directory, right = home directory.

## Keys

The menu bar (File, Edit, View, Go, Tools, Help) lists the commands with
their shortcuts; unavailable ones are greyed out with an explanation, and
toggles show a check. Opening Tools discovers actions for the active folder
and marked entries; actions are grouped by purpose and missing programs are
shown on disabled rows. F2 opens the searchable version of the same tools.
Right-click an entry in either panel for file actions. Clicking a marked
entry keeps the marked group; clicking another entry clears the marks and
targets that entry. Escape closes the menu and returns focus to that panel.
Keyboard shortcuts are available in the menus, F1 help and the Ctrl-P command
palette. The bottom area shows status messages and running jobs.

| Key                 | Action                                    |
| ------------------- | ----------------------------------------- |
| Up / Down, PgUp/Dn  | Move cursor (icon view: one row / a page of rows) |
| Home / End          | First / last entry                        |
| Enter / dbl-click   | Enter directory or archive (zip, tar, tar.gz/tgz: browsed like a folder, read-only), or open the file (first matching `[[opener]]`, else the system default) |
| Shift-Enter         | Open with…: matching openers, system default, built-in editor |
| Backspace           | Parent directory                          |
| Alt-Left / Alt-Right | Back / forward through visited directories (per panel) |
| Tab                 | Switch active panel                       |
| Space / Insert      | Mark entry                                |
| Ctrl-A / * / +      | Select all / invert selection / select by pattern (glob or substring) |
| / or Ctrl-F         | Filter as you type (Enter keeps the filter, Esc clears it) |
| Esc                 | Clear the filter, else the marks          |
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
| F2                  | Tools for the selection / directory (see [`docs/tools.md`](docs/tools.md)) |
| Alt-F7 / Ctrl-Shift-F | Find files by name (glob or substring) and/or containing text (literal or regex; hits show the matching line, and F3 preview scrolls to it), below the panel directory; results fill the panel (Enter shows a hit in its folder, Backspace returns, Esc stops) |
| Ctrl-G / Alt-F1     | Locations: drives/mounted volumes, desktop places (home, user folders, GTK/KDE bookmarks), fyle bookmarks and frequent directories |
| Ctrl-L              | Edit or paste the active panel's full path |
| Ctrl-D              | Bookmark the current directory (saved in `config.toml`) |
| F9                  | Open a terminal in the panel directory (`[terminal]` in `config.toml`) |
| F3                  | Toggle quick preview (replaces the other panel, follows the cursor) |
| F4                  | Edit the cursor file (UTF-8 text, up to 8 MiB) in place of the other panel; Ctrl-S saves, Esc/Ctrl-W closes (with unsaved changes it asks: Save, Discard or Cancel); saving asks before overwriting a file changed on disk. `[editor] command` in `config.toml` uses an external editor instead |
| Ctrl-P              | Command palette: all commands with their keys, filtered by typing |
| Ctrl-K Ctrl-T       | Theme picker: arrows preview live, Enter keeps and saves to `config.toml`, Esc restores |
| Right / Left        | Tree: expand / collapse (or go to parent); in icon view: next / previous cell |
| Ctrl-1 / Ctrl-2     | List view / icon view for the active panel (View menu; saved as `left_view` / `right_view`) |
| Alt-= / Alt--       | Larger / smaller icons in icon view (saved as `icon_size`) |
| Ctrl-R              | Refresh (also forgets computed folder sizes) |
| Ctrl-Shift-S        | Calculate Sizes (View menu, palette, right-click): recursive size of the marked folders, else the folder under the cursor, else (cursor on `..` or a file) of every folder listed. Runs in the background, shows in the Size column (`… 1.2 G` while running, `≥ 1.2 G` when incomplete: unreadable entries or stopped); Esc stops. Symlinks are not followed, hard-linked files count once, other filesystems are skipped; not available inside archives. Sorting by size uses the results; the footer sums marked entries (`≥` and "n folders not measured" when unknown). Never runs automatically |
| Ctrl-+ / Ctrl-- / Ctrl-0 | Zoom in / out / reset (also Ctrl-wheel); saved as `font_size` in `config.toml` |
| Ctrl-Shift-C        | Cancel running copy/move/delete jobs (also a Cancel button per job) |
| F10 / Ctrl-Q        | Quit                                      |

Symbolic links keep their target's file icon with a small badge: a curved
arrow for a working link, a crossed circle (and a muted name) for a broken
one (missing target or a loop). Hover the name for `→ target` and the
resolved path, or `Broken link (…) → target`; F3 shows the same in its header
(for a broken link, in place of the preview). Links inside tar archives show
the badge too (zip does not store the target when only listing).
A link to a folder is entered like a folder: the panel path, breadcrumbs and
history keep the link's path, not the resolved one.

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
tiff up to 64 MiB) show a thumbnail instead of the icon once it is loaded, in
the background and for the visible cells only. Thumbnails live in the shared
freedesktop cache (`$XDG_CACHE_HOME/thumbnails`, size `normal`/`large`/
`x-large`/`xx-large` picked from the icon size and the display scale), so
Nautilus, Dolphin and fyle reuse each other's; a cached one is used only if
its recorded modification time matches, otherwise it is regenerated. Files
that fail to decode are recorded under `fail/fyle/` and not retried until they
change. Files inside archives keep their icons.

Click a column's sort icon to sort (directories always stay on top); drag
header borders to resize. Columns… (in the command palette) picks the
columns from a grouped catalog: file, owner and permissions, Git status,
extended attributes, and EXIF fields of photos (date taken, camera, lens,
exposure, aperture, ISO, focal length, dimensions, GPS). With the Ext column
shown, names leave out their extension (`.gitignore` and other dotfiles stay
whole).

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

## Settings

`config.toml` in the platform config directory:

- Linux: `~/.config/fyle/config.toml` (respects `$XDG_CONFIG_HOME`)
- Windows: `%APPDATA%\fyle\config.toml`
- macOS: `~/Library/Application Support/fyle/config.toml`

All keys are optional; see [`config.example.toml`](config.example.toml) for
every key with its default. Errors (including unknown keys) are shown in the
status line and the defaults are used. Settings are read at startup.

### Keys

`[[keymap]]` entries add or override bindings (`keys`, `action` such as
`"Copy"` or `"none"` to unbind, `context` = `panel` / `global` / `editor`);
see `config.example.toml`.

### Themes

Ctrl-K Ctrl-T (or View › Theme…) opens a picker; the highlighted
theme is applied immediately as a preview, Enter keeps it and writes it to
`config.toml` (other keys and comments are preserved), Esc restores the
previous theme.

`theme = "Tokyo Night"` selects a theme by name (case-insensitive);
`"system"`, `"light"` and `"dark"` use the built-in themes. 17 themes from
GPUI Kit are bundled (Ayu, Catppuccin, Everforest, Flexoki, Gruvbox, Solarized,
Tokyo Night, …; full list and attribution in [`themes/README.md`](themes/README.md)).
More can be added as gpui-component theme files (`*.json`) in a `themes`
folder next to `config.toml`; a user theme with the same name as a bundled
one replaces it.

## Build dependencies

- Linux (Debian/Ubuntu): `libxkbcommon-dev libxkbcommon-x11-dev libfontconfig1-dev libxcb1-dev libwayland-dev libssl-dev pkg-config`,
  plus a Vulkan driver at runtime.
- Windows: MSVC toolchain.

CI builds and tests on Linux (Windows job disabled for now) (`.github/workflows/ci.yml`).

## Remote locations

Remote folders have paths like `proton:/Reports`, `gdrive:/Docs` or
`nas:/photos`; the root is `<remote>:/`. Only the provider layer and the
model exist so far; it is not wired into the UI yet. Providers:

- Proton Drive through the official `proton-drive` CLI (run
  `proton-drive auth login` first). Supports upload, move to trash, new
  folder, rename and move. Docs and Sheets are listed but the CLI skips them
  on download.
- Every remote set up with `rclone config` (Google Drive, OneDrive, ...);
  credentials stay with rclone.
- Servers in `remotes.toml` (next to `config.toml`; see `remotes.example.toml`)
  over SFTP, FTP, FTPS, WebDAV and S3, through the `remotefs` crates. SMB is
  not supported. Removing there is permanent. The file is never created
  automatically.

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
`~/.ssh/known_hosts` and refuses unknown or changed ones; connect once with `ssh`
to accept a new host.

Building needs the OpenSSL development files (WebDAV); see Build
dependencies (`openssl-devel` on Fedora).

## Development sandbox

A Fedora 44 container (rootless Podman, or Docker) with the Rust toolchain,
the build dependencies, Claude Code and Codex CLI, so an agent can work on the repo
autonomously with the container as the safety boundary. The repo is
bind-mounted read-write at its host path (sharing `target/`), and the
container runs as your UID, so files stay yours.

```sh
just sandbox-build              # build the image (also done on first run)
just sandbox                    # bash shell
just sandbox --claude           # Claude Code with permissions skipped
just sandbox --codex            # interactive Codex (default permissions)
just sandbox --gui --mount ~/   # Wayland + GPU, extra dirs read-only
just sandbox -- cargo test      # one-shot command
```

`.devcontainer/` uses the same image for VS Code, JetBrains and Zed.

After adding Codex to an existing setup, run `just sandbox-build` (or rebuild
your IDE's dev container). Sign in once with
`just sandbox -- codex login --device-auth`, then run `just sandbox --codex`.
Follow the browser link and enter the code; device code login must be enabled
in your ChatGPT security settings or workspace permissions
([OpenAI authentication docs](https://developers.openai.com/codex/auth#login-on-headless-devices)).
The `fyle-codex` named volume keeps Codex credentials, configuration and sessions
across container exits and rebuilds, and is shared by the CLI launcher and IDE
dev container.

## Architecture

Model first: `src/model.rs` holds each panel's state (`Folder`: path, rows,
cursor, marks, sort, hidden files, tree expansion) with no UI code and its own unit tests.
`src/panel.rs` is a `DataTable` view on top of a `Folder`; it only renders the
model and forwards cursor moves, sorting and double-clicks to it.
`src/commander.rs` owns the two models and their views and implements the
commands. `src/fs_ops.rs` contains the file system operations.
`src/tools/` is the model for context-dependent tools (run ruff on Python
files, uv commands in uv projects, convert images, …); see
[`docs/tools.md`](docs/tools.md). F2 opens them; your own go in
`<config dir>/fyle/tools.toml`, or in Lua plugins in `<config dir>/fyle/plugins/*.lua`.
`src/editor.rs` loads/saves edited files (UTF-8 only, atomic save keeping
permissions); `src/editor_view.rs` is the editor pane.
`src/dirsize.rs` measures directories recursively (on demand, cancellable, UI-independent); `Folder` caches the results per path until navigation or refresh. A hard-linked file counts once per measured folder, so each size stands alone. Other reloads (after file operations, saves) keep the sizes.
`src/search.rs` finds files and searches contents with ripgrep's libraries
(`ignore`, `grep-searcher`); results are a mode of the `Folder` model.
`src/transfer.rs` copies and moves with conflict resolution (merging
directories, unique names, never writing through aliases of the source);
`src/conflict_view.rs` formats the conflict details.
`src/icons.rs` maps names/extensions to semantic icon kinds; the table view
picks the glyph for each kind. `src/links.rs` holds the UI-independent
symlink state (`LinkState`: not a link / link with target / broken with
reason), probed in `fs_ops::read_dir` from the metadata already fetched (one
`read_link` per shown link; the resolved path is computed for tooltips only);
`src/link_badge.rs` draws the badge over an icon.
`src/breadcrumbs.rs` derives panel path segments, parses edited paths and guards
background navigation results. `src/places.rs` discovers grouped desktop places,
Linux mounts, bookmarks and frequent paths; `src/location_view.rs` renders
them as the locations menu and the searchable picker. Path loading and place discovery run in the background.

`src/archive.rs` reads zip, tar and tar.gz archives into an `Index` (a tree of
nodes) and extracts entries. Inside an archive a panel's path is the archive
file joined with the inner path (shown as `file.zip › inner/dir`); `Folder`
mounts the index for such paths, so navigation, marks and sorting work as
for directories, and preview loads an entry by extracting it to the preview
cache. The first read of an archive runs in the background
(`commander/archives.rs`; the panel shows the location with "Reading archive…",
Esc cancels, navigating elsewhere drops the result); once cached (by path, size
and modification time) navigation inside it is instant. Archives are
read-only. Extraction (`jobs::run_extract`) writes to a
staging folder in the destination, then moves the result with the usual
conflict resolution (the dialog names the archive entry, `file.zip › inner/name`,
as the source; the status counts extracted files); names with `..` or absolute paths are skipped, links
must stay inside the destination and nothing is written through a link.

Where things live (`src/`):

| Module | Role |
|---|---|
| `main.rs` | Startup, settings, start directories, window |
| `commander.rs`, `commander/` | Main window: the two panels, actions, dialogs, jobs bar, tools orchestration. Split by area into `commander/` (keys, navigation, search, files, jobs, view, archives, tools, menus, appearance); new logic goes into its own module, only wiring here. |
| `menus.rs` | `Cmd`: every user-visible command with labels, availability (with an explanation) and toggle state, plus the menu bar and context menu layouts. Menus, palette and F1 help are built from it; `commander::keys::action_for` maps each `Cmd` to its action. |
| `model.rs` | `Folder`: entries, cursor, marks, filter, sort, tree, search results, archive mount, directory size cache |
| `panel.rs` | `FolderView` (DataTable delegate `FileTable`), header with breadcrumbs, filter bar; shows the list or the icon grid |
| `thumbnails.rs`, `thumb_queue.rs` | Icon view thumbnails: the freedesktop cache (URI and MD5 names, validity by `Thumb::MTime`, atomic writes, failure records) and the load scheduler with its LRU, both UI-independent and tested |
| `grid.rs`, `icon_view.rs`, `glyphs.rs`, `context_menu.rs` | Icon view: `ViewMode`, icon sizes, `Layout` (columns for a width) and cursor moves (UI-independent, tested); the lazy `IconGrid` view; the `FileIcon` → glyph table and the right-click menu shared with the list |
| `columns.rs` | `ColumnKind` catalog, grouped, ids like `exif.iso`. A new column is a variant here and in `ColumnKind::ALL`, plus a renderer in `panel.rs`. |
| `fs_ops.rs`, `transfer.rs`, `jobs.rs` | `Entry` and file operations; copy/move with conflict resolution; background jobs with progress and cancel |
| `remote.rs`, `remote/` | Remote providers: `Provider` trait, `Location` paths, Proton Drive (`proton.rs`), rclone (`rclone.rs`), shared CLI runner (`process.rs`), `remotes.toml` parsing (`connections.rs`), SFTP host key check (`hostkey.rs`), SFTP/FTP/WebDAV/S3 (`remotefs.rs`); `model/remote.rs` holds cached remote listings |
| `archive.rs`, `dirsize.rs`, `links.rs` | Archive index and safe extraction; recursive sizes; symlink state |
| `preview/`, `preview_view.rs` | `Previewer`s and their `Registry` (order: directory, image, pdf, media, archive, markdown, table, text, binary); one render arm per `Content` |
| `editor.rs`, `editor_view.rs` | Text document model (UTF-8, ≤ 8 MiB, atomic save) and the editor pane |
| `tools/`, `tools_view.rs` | Tool, Action and Registry model; `defaults.toml`; built-ins; Lua plugins; `exec.rs` runs them |
| `search.rs`, `places.rs`, `breadcrumbs.rs`, `rename.rs`, `openers.rs`, `palette.rs`, `themes.rs`, `icons.rs`, `settings.rs`, `zoom.rs` | Further UI-independent models, each with tests |
| `*_view.rs`, `menu_bar.rs`, `link_badge.rs`, `theme_picker.rs` | Thin views |

Previews follow the same split. `src/preview/` is the model: a `Registry`
of `Previewer`s (directory, image with EXIF, PDF via poppler, audio/video via ffmpeg, archive, Markdown,
CSV table, text, binary fallback) turns a path into
plain `Content` data; the first previewer whose `matches` accepts the file's
`Probe` (name, extension, size, first 8 KiB) wins. `src/preview_view.rs` loads
in the background and has one render arm per `Content` variant. A new file
type is a new `Previewer` registered before the fallbacks; only a new kind of
display needs a new `Content` variant and render arm.

## License

fyle is licensed under the GNU General Public License v3.0 or later
([`LICENSE`](LICENSE)). The bundled themes in `themes/` are from GPUI Kit under
the Apache License 2.0 (see [`themes/README.md`](themes/README.md)).
