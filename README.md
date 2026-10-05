# fyle

A minimal two-panel file manager in the spirit of Krusader / Midnight Commander,
built with [GPUI Kit](https://gpui-kit.com) (GPUI + gpui-component's `DataTable`).
Targets Linux and Windows (macOS later, see `TODO.md`).

```sh
cargo run --release -- [LEFT_DIR] [RIGHT_DIR]
```

Defaults: left = current directory, right = home directory.

## Keys

| Key                 | Action                                    |
| ------------------- | ----------------------------------------- |
| Up / Down, PgUp/Dn  | Move cursor                               |
| Home / End          | First / last entry                        |
| Enter / dbl-click   | Enter directory, or open the file (first matching `[[opener]]`, else the system default) |
| Shift-Enter         | Open with…: matching openers, system default, built-in editor |
| Backspace           | Parent directory                          |
| Tab                 | Switch active panel                       |
| Space / Insert      | Mark entry                                |
| / or Ctrl-F         | Filter as you type (Enter keeps the filter, Esc clears it) |
| Esc                 | Clear the filter, else the marks          |
| Shift-F4            | New file: create and edit it |
| Ctrl-C / Ctrl-X / Ctrl-V | Yank / cut / paste entries (paths also go to the system clipboard); paste runs as a background copy/move |
| F5                  | Copy marked (or current) to other panel   |
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
| Ctrl-G / Alt-F1     | Go to: bookmarks (★), desktop places (⌂: home, user folders, GTK/KDE bookmarks, drives), frequently visited directories (↻) |
| Ctrl-D              | Bookmark the current directory (saved in `config.toml`) |
| F9                  | Open a terminal in the panel directory (`[terminal]` in `config.toml`) |
| F3                  | Toggle quick preview (replaces the other panel, follows the cursor) |
| F4                  | Edit the cursor file (UTF-8 text, up to 8 MiB) in place of the other panel; Ctrl-S saves, Esc/Ctrl-W closes (asks before discarding changes); saving asks before overwriting a file changed on disk. `[editor] command` in `config.toml` uses an external editor instead |
| Ctrl-P              | Command palette: all commands with their keys, filtered by typing |
| Ctrl-K Ctrl-T       | Theme picker: arrows preview live, Enter keeps and saves to `config.toml`, Esc restores |
| Right / Left        | Tree: expand / collapse (or go to parent) |
| Ctrl-R              | Refresh                                   |
| Ctrl-Shift-C        | Cancel running copy/move/delete jobs (also a Cancel button per job) |
| F10 / Ctrl-Q        | Quit                                      |

In tree mode, clicking a folder's chevron also expands/collapses it; Enter
still makes the folder the panel's root.

Click a column's sort icon to sort (directories always stay on top); drag
header borders to resize. Columns… (in the command palette) picks the
columns from a grouped catalog: file, owner and permissions, Git status,
extended attributes, and EXIF fields of photos (date taken, camera, lens,
exposure, aperture, ISO, focal length, dimensions, GPS).

Windows: ".." from a drive root lists all drives; files with the hidden
attribute count as hidden.

Copy and move never overwrite existing targets. Copy, move, trash and delete
run in the background with progress in a jobs bar.

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

Ctrl-K Ctrl-T (or the "Theme" button) opens a picker; the highlighted
theme is applied immediately as a preview, Enter keeps it and writes it to
`config.toml` (other keys and comments are preserved), Esc restores the
previous theme.

`theme = "Tokyo Night"` selects a theme by name (case-insensitive);
`"system"`, `"light"` and `"dark"` use the built-in themes. 36 themes from
GPUI Kit are bundled (Ayu, Catppuccin, Everforest, Flexoki, Gruvbox, Solarized,
Tokyo Night, …; full list and attribution in [`themes/README.md`](themes/README.md)).
More can be added as gpui-component theme files (`*.json`) in a `themes`
folder next to `config.toml`; a user theme with the same name as a bundled
one replaces it.

## Build dependencies

- Linux (Debian/Ubuntu): `libxkbcommon-dev libxkbcommon-x11-dev libfontconfig1-dev libxcb1-dev libwayland-dev`,
  plus a Vulkan driver at runtime.
- Windows: MSVC toolchain.

CI builds and tests on Linux (Windows job disabled for now) (`.github/workflows/ci.yml`).

## Development sandbox

A Fedora 44 container (rootless Podman, or Docker) with the Rust toolchain,
the build dependencies and Claude Code, so an agent can work on the repo
autonomously with the container as the safety boundary. The repo is
bind-mounted read-write at its host path (sharing `target/`), and the
container runs as your UID, so files stay yours.

```sh
just sandbox-build              # build the image (also done on first run)
just sandbox                    # bash shell
just sandbox --claude           # Claude Code with permissions skipped
just sandbox --gui --mount ~/   # Wayland + GPU, extra dirs read-only
just sandbox -- cargo test      # one-shot command
```

`.devcontainer/` uses the same image for VS Code, JetBrains and Zed.

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
`src/search.rs` finds files and searches contents with ripgrep's libraries
(`ignore`, `grep-searcher`); results are a mode of the `Folder` model.
`src/icons.rs` maps names/extensions to semantic icon kinds; the table view
picks the glyph for each kind.

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
