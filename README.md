# fyle

A minimal two-panel file manager in the spirit of Krusader / Midnight Commander,
built with [GPUI Kit](https://gpui-kit.com) (GPUI + gpui-component's `DataTable`).
Runs on Linux, macOS and Windows.

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
| Ctrl/Cmd-R          | Refresh                                   |
| F10 / Ctrl/Cmd-Q    | Quit                                      |

Click a column header to sort (directories always stay on top); drag header
borders to resize.

macOS extras: Cmd-Up (parent), Cmd-Down (open), Cmd-Backspace (delete),
Cmd-Shift-. (hidden files), Cmd-Shift-N (new directory).

Windows: ".." from a drive root lists all drives; files with the hidden
attribute count as hidden.

Copy and move never overwrite existing targets.

## Build dependencies

- Linux (Debian/Ubuntu): `libxkbcommon-dev libxkbcommon-x11-dev libfontconfig1-dev libxcb1-dev libwayland-dev`,
  plus a Vulkan driver at runtime.
- macOS: Xcode (GPUI compiles Metal shaders with it).
- Windows: MSVC toolchain.

CI builds and tests on all three (`.github/workflows/ci.yml`).
