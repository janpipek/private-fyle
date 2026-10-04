# fyle

A minimal two-panel file manager in the spirit of Krusader / Midnight Commander,
built with [GPUI Kit](https://gpui-kit.com) (GPUI + gpui-component).

```sh
cargo run --release -- [LEFT_DIR] [RIGHT_DIR]
```

Defaults: left = current directory, right = `$HOME`.

## Keys

| Key                | Action                                   |
| ------------------ | ---------------------------------------- |
| Up / Down          | Move cursor                              |
| PgUp / PgDn        | Move by 20                               |
| Home / End         | First / last entry                       |
| Enter / dbl-click  | Enter directory, or open file externally |
| Backspace          | Parent directory                         |
| Tab                | Switch active panel                      |
| Space / Insert     | Mark entry                               |
| F5                 | Copy marked (or current) to other panel  |
| F6                 | Move marked (or current) to other panel  |
| F7                 | New directory                            |
| F8 / Delete        | Delete (with confirmation)               |
| Ctrl-H             | Toggle hidden files                      |
| Ctrl-R             | Refresh                                  |
| F10 / Ctrl-Q       | Quit                                     |

Copy and move never overwrite existing targets.

## Linux build dependencies

Debian/Ubuntu: `libxkbcommon-dev libxkbcommon-x11-dev libfontconfig1-dev libxcb1-dev`,
plus a Vulkan driver at runtime (`mesa-vulkan-drivers`).

