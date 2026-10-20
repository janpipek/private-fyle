---
title: "Settings"
weight: 90
---

Fyle reads optional settings from `config.toml` at startup. On Linux, the file
is `~/.config/fyle/config.toml`, or `$XDG_CONFIG_HOME/fyle/config.toml` when
that environment variable is set. Open the file from Help > Configuration.
The complete commented template is [`config.example.toml`](https://github.com/janpipek/private-fyle/blob/main/config.example.toml).
Missing values use defaults; an unknown key is reported and defaults are used.

Common settings include:

| Setting | What it controls |
| --- | --- |
| `show_hidden` | Whether hidden files start visible |
| `theme`, `font_size` | Theme and interface size |
| `bookmarks` | Folders offered first in Go To |
| `[panels] left`, `right` | Startup directories; command-line paths override them |
| `[panels] left_view`, `right_view`, `tree` | Startup panel views |
| `[panels] columns`, `sort` | Shared column order and initial sort |
| `[workspace] layout` | Startup workspace layout |
| `[editor] command` | External editor; empty uses Fyle's editor |
| `[terminal] command` | External terminal used by F9 |
| `[[opener]]` | Programs for opening matching files |
| `[[keymap]]` | Additional or overriding keyboard bindings |

Settings saved through Fyle preserve comments and other keys. Panel view, icon
size, theme and zoom commands save their choices. The title bar preference
applies after restarting Fyle.

Reader packages are stored under the config directory's `fyle/extensions/`
folder. Add their IDs to `[extensions].enabled` to authorize their readers;
package and enablement changes require a restart. These packages are distinct
from Lua Tools, which live under `fyle/plugins/`.
