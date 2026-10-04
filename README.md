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
| Ctrl-R              | Refresh                                   |
| F10 / Ctrl-Q        | Quit                                      |

Click a column's sort icon to sort (directories always stay on top); drag
header borders to resize.

Windows: ".." from a drive root lists all drives; files with the hidden
attribute count as hidden.

Copy and move never overwrite existing targets.

## Build dependencies

- Linux (Debian/Ubuntu): `libxkbcommon-dev libxkbcommon-x11-dev libfontconfig1-dev libxcb1-dev libwayland-dev`,
  plus a Vulkan driver at runtime.
- Windows: MSVC toolchain.

CI builds and tests on Linux and Windows (`.github/workflows/ci.yml`).

## Architecture

Model first: `src/model.rs` holds each panel's state (`Folder`: path, entries,
cursor, marks, sort, hidden files) with no UI code and its own unit tests.
`src/panel.rs` is a `DataTable` view on top of a `Folder`; it only renders the
model and forwards cursor moves, sorting and double-clicks to it.
`src/commander.rs` owns the two models and their views and implements the
commands. `src/fs_ops.rs` contains the file system operations.
