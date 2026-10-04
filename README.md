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
| Enter / dbl-click   | Enter directory, or open file externally  |
| Backspace           | Parent directory                          |
| Tab                 | Switch active panel                       |
| Space / Insert      | Mark entry                                |
| Esc                 | Clear marks                               |
| F5                  | Copy marked (or current) to other panel   |
| F6                  | Move marked (or current) to other panel   |
| F7                  | New directory                             |
| F8 / Delete         | Delete (with confirmation)                |
| Ctrl-H              | Toggle hidden files                       |
| Ctrl-T              | Toggle tree mode (per panel)              |
| F3                  | Toggle quick preview (replaces the other panel, follows the cursor) |
| Right / Left        | Tree: expand / collapse (or go to parent) |
| Ctrl-R              | Refresh                                   |
| F10 / Ctrl-Q        | Quit                                      |

In tree mode, clicking a folder's chevron also expands/collapses it; Enter
still makes the folder the panel's root.

Click a column's sort icon to sort (directories always stay on top); drag
header borders to resize.

Windows: ".." from a drive root lists all drives; files with the hidden
attribute count as hidden.

Copy and move never overwrite existing targets.

## Build dependencies

- Linux (Debian/Ubuntu): `libxkbcommon-dev libxkbcommon-x11-dev libfontconfig1-dev libxcb1-dev libwayland-dev`,
  plus a Vulkan driver at runtime.
- Windows: MSVC toolchain.

CI builds and tests on Linux (Windows job disabled for now) (`.github/workflows/ci.yml`).

## Architecture

Model first: `src/model.rs` holds each panel's state (`Folder`: path, rows,
cursor, marks, sort, hidden files, tree expansion) with no UI code and its own unit tests.
`src/panel.rs` is a `DataTable` view on top of a `Folder`; it only renders the
model and forwards cursor moves, sorting and double-clicks to it.
`src/commander.rs` owns the two models and their views and implements the
commands. `src/fs_ops.rs` contains the file system operations.

Previews follow the same split. `src/preview/` is the model: a `Registry`
of `Previewer`s (directory, image, text, binary fallback) turns a path into
plain `Content` data; the first previewer whose `matches` accepts the file's
`Probe` (name, extension, size, first 8 KiB) wins. `src/preview_view.rs` loads
in the background and has one render arm per `Content` variant. A new file
type is a new `Previewer` registered before the fallbacks; only a new kind of
display needs a new `Content` variant and render arm.
