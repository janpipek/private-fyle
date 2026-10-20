---
title: "Settings"
weight: 90
---

Fyle reads optional settings from `config.toml` at startup. On Linux, the file
is `~/.config/fyle/config.toml`, or `$XDG_CONFIG_HOME/fyle/config.toml` when
that environment variable is set. Open it from Help > Configuration. The
complete commented template is [`config.example.toml`](https://github.com/janpipek/private-fyle/blob/main/config.example.toml).
Every setting is optional: omitted values use the defaults. The template itself
contains every setting at its default value.

## Start with a small config

You can add only the choices you want to change. This is a complete, valid
example that starts both panels in useful folders, shows hidden files, and uses
a dark theme:

```toml
show_hidden = true
theme = "dark"
bookmarks = ["~/projects", "/srv/shared"]

[panels]
left = "~/projects"
right = "/srv/shared"
```

To build a complete custom config, start from the full example below and add
only the sections you need. The example is a single standalone file; the smaller
examples on this page are snippets to merge into your existing file. Keep one
copy of each table header such as `[panels]` or `[terminal]`. TOML table
headers set the scope for following keys: after `[panels]`, a bare `sort =`
belongs to `panels`; it does not return to the top level. Put top-level keys
such as `show_hidden`, `theme`, `font_size`, and `bookmarks` before the first
table header. `[[opener]]` and `[[keymap]]` start top-level arrays of tables;
they can appear after ordinary tables.

Fyle rejects unknown keys, invalid enum values such as an unsupported layout,
incorrect value types, and invalid TOML. If parsing
fails, it reports the config path and error in the status line and uses the
full defaults for that run. It does not partially apply valid settings from a
file with an error. Correct the reported problem and restart.

## Full example

This standalone example demonstrates the available settings. Replace the
sample paths, theme, commands, and package ID with values installed on your
system. You do not need to copy defaults into your own config.

```toml
show_hidden = true
theme = "Tokyo Night"
font_size = 16.0
bookmarks = ["~/projects", "/srv/shared"]

[window]
width = 1440
height = 900
custom_titlebar = true

[panels]
left = "~/projects"
right = "~/Downloads"
tree = false
left_view = "list"
right_view = "icons"
icon_size = 96
columns = ["name", "ext", "size", "modified", "permissions"]
sort = "modified"
sort_descending = true

[workspace]
layout = "two_panels_inspector"

[editor]
command = ["code", "--wait"]

[terminal]
command = ["kitty", "--directory", "{dir}"]

[extensions]
enabled = ["sensor"]

[[opener]]
label = "Image viewer"
command = ["imv"]
kinds = ["image"]

[[opener]]
label = "Text editor"
command = ["code", "--goto", "{path}:1"]
exts = ["md", "rs", "toml"]
names = ["Makefile"]

[[keymap]]
keys = "ctrl-shift-h"
action = "ToggleHidden"
context = "global"
```

## Appearance and window

`theme` accepts `"system"` (follow the OS light or dark setting), `"light"`,
`"dark"`, a bundled theme name, or a custom theme name. Theme names are
case-insensitive. For example:

```toml
theme = "Catppuccin Mocha"
font_size = 18.0

[window]
width = 1360
height = 860
custom_titlebar = false
```

Bundled themes come from GPUI Kit; see [`themes/README.md`](https://github.com/janpipek/private-fyle/blob/main/themes/README.md).
To add your own, put a GPUI Component theme JSON file in the `themes/` folder
next to `config.toml`, then set `theme` to its theme name. The theme picker
({{< key "Ctrl" "K" >}} then {{< key "Ctrl" "T" >}}) previews available themes and saves the selection. Zoom ({{< key "Ctrl" "+" >}}
/ {{< key "Ctrl" "-" >}} / {{< key "Ctrl" "0" >}}, or {{< key "Ctrl" "wheel" >}}) changes and saves `font_size`; Fyle clamps
the effective UI font size to 8–36 pixels. In the theme picker, press
{{< key "Enter" >}} to keep and save a theme, or {{< key "Esc" >}} to restore
the previous theme. The default config leaves the
`[window]` dimensions at 1200 by 760. `custom_titlebar` defaults to `true`;
the title-bar preference is applied after restarting. Window dimensions are
read as numbers and do not have a documented settings range.

{{< shot src="images/fyle-theme-picker.png" alt="Theme picker with Dark selected and the panels previewing its colors" caption="The theme picker previews the highlighted theme before you keep or cancel the change." >}}

## Startup folders and panels

`[panels] left` and `right` set the starting directories. A leading `~`
expands to your home directory. Command-line directories take precedence over
these settings. If neither command-line arguments nor configured paths are
present, the left panel starts in the current directory and the right panel
in your home directory.

```toml
[panels]
left = "~/work/fyle"
right = "~/Downloads"
tree = true
left_view = "list"
right_view = "icons"
icon_size = 128
sort = "size"
sort_descending = true

[workspace]
layout = "one_panel_inspector"
```

Tree mode is a list view, so an `icons` panel does not start in tree mode.
`left_view` and `right_view` accept `"list"` or `"icons"`. `icon_size` snaps
to the nearest available size: 48, 64, 96, 128, or 160 pixels. {{< key "Alt" "=" >}} and
{{< key "Alt" "-" >}} step through these sizes and save the choice. `sort` accepts `"name"`,
`"ext"`, `"size"`, or `"modified"`; `sort_descending` chooses the direction.
The startup sort applies to the panels, but sorting from the View menu changes
the active panel for the current session.

`columns` is one ordered list shared by the panels. The Name column is always
first. For example:

```toml
[panels]
columns = ["name", "ext", "size", "modified", "permissions", "owner"]
```

Column IDs include file columns `name`, `ext`, `size`, `modified`, and
`created`; Unix columns `permissions` and `owner`; `duration` for local audio
and video; `git.status`; `xattrs` and `selinux`; and EXIF IDs such as
`exif.taken`, `exif.camera`, `exif.lens`, `exif.exposure`, `exif.aperture`,
`exif.iso`, `exif.focal_length`, `exif.dimensions`, and `exif.gps`. The
Columns command in the palette edits and saves this list. Unknown IDs are
retained and shown as unavailable until a provider registers them.

{{< shot src="images/fyle-columns.png" alt="Columns chooser showing optional file details and metadata columns" caption="The Columns command edits the shared column selection and saves it to config.toml." >}}

The workspace layout can be `"two_panels"`, `"two_panels_inspector"`, or
`"one_panel_inspector"`. In the one-panel layout, the other panel remains the
destination for copy and move commands. Layout changes from the View menu are
saved. The active panel's list/icon view and the shared icon size are also
saved as you change them. Tree mode and sort settings are startup values, not
saved UI preferences.

## Bookmarks

`bookmarks` is a list of folder locations shown first in Go To ({{< key "Ctrl" "G" >}}). For
hand-written config, ordinary strings are local paths; `~` expands to your
home directory:

```toml
bookmarks = ["~/projects", "/mnt/archive"]
```

{{< key "Ctrl" "D" >}} adds or removes the current local or remote folder bookmark and saves
it. Remote bookmarks are encoded as versioned location strings by Fyle. Keep
those generated values when editing the file; a normal remote address string
is read as a local path. Use {{< key "Ctrl" "D" >}} or the Locations UI to create remote
bookmarks instead of writing the encoded value by hand. Archive members cannot
be bookmarked.

## External editor and terminal

{{< key "F4" >}} and {{< key "Shift" "F4" >}} use `[editor] command` when it is nonempty. Fyle appends the
file path as the final argument, so do not include a `{path}` placeholder:

```toml
[editor]
command = ["code", "--wait"]
```

The empty default uses Fyle's built-in editor. Commands are argument arrays,
not shell command strings; quote each argument as its own array item.

{{< key "F9" >}} starts a terminal in the active panel's directory. Set `[terminal]
command` to an executable and its arguments. Fyle replaces `{dir}` wherever it
appears and also sets the process working directory:

```toml
[terminal]
command = ["kitty", "--directory", "{dir}"]
```

When the command is empty (the default), Fyle tries `$TERMINAL`; otherwise it
checks known terminals in preference order, including `x-terminal-emulator`,
Ptyxis, GNOME Terminal, Konsole, Xfce Terminal, Kitty, Alacritty, WezTerm,
Foot, Ghostty, and xterm. If none is available, set `[terminal] command`.

## File openers

Add one `[[opener]]` block per rule. {{< key "Enter" >}} opens with the first matching rule
in file order, or the system default if none matches. {{< key "Shift" "Enter" >}} offers all
matching rules. A rule matches if any configured `exts`, `kinds`, or `names`
entry matches; leaving all three empty makes it match every file. Extensions
are case-insensitive and are written without a leading dot. Names match the
full filename. `kinds` use file-kind names such as `image`, `code`, `text`,
`audio`, `video`, and `archive`.

```toml
[[opener]]
label = "GIMP"
command = ["gimp"]
kinds = ["image"]

[[opener]]
label = "Source editor"
command = ["code", "--goto", "{path}:1"]
exts = ["rs", "toml"]
names = ["Makefile"]
```

The target path is appended to the command array unless `{path}` occurs in an
argument, in which case each occurrence is substituted. The executable and
arguments are launched directly; they are not interpreted by a shell. A
command with no executable cannot be launched. Open With also includes the
system's registered applications where available.

## Keyboard shortcuts

Open Help > Edit Keyboard Shortcuts to inspect commands and their current
shortcuts. Choose Edit next to a command, enter its shortcut and context
(`panel`, `global`, or `editor`), and apply the change. Choose Save in the
shortcut window to write the validated changes to `config.toml` and apply
them immediately. The editor checks shortcut syntax and reports conflicts in
overlapping contexts. Disable removes a command's shortcuts; Reset restores
that command's defaults, and Reset all restores the default keymap. This is
the recommended way to customize shortcuts.

{{< shot src="images/fyle-shortcuts-editor.png" alt="Keyboard shortcuts editor with Edit, Disable, Reset, Reset all and Save buttons" caption="Edit shortcuts in the interface, then Save to apply them and keep them for future sessions." >}}

{{< shot src="images/fyle-shortcut-edit.png" alt="Edit Open dialog with shortcut keys and context fields and an Apply button" caption="Apply updates the draft. Save in the main shortcuts window commits the changes." >}}

For manual overrides, add `[[keymap]]` entries. Actions use the command names
listed in the commented config template, such as `Copy`, `ToggleHidden`, or
`CommandPalette`; `"none"` unbinds a key. Context is `"panel"` (the default,
applies in both list and icon views), `"global"` (the main window), or
`"editor"` (the {{< key "F4" >}} editor). Key names follow GPUI's format; a chord is written
as space-separated strokes, such as `"ctrl-k ctrl-c"`.

```toml
[[keymap]]
keys = "ctrl-shift-h"
action = "ToggleHidden"
context = "global"

[[keymap]]
keys = "ctrl-k ctrl-h"
action = "ToggleHidden"
context = "global"

[[keymap]]
keys = "f8"
action = "none"
context = "panel"
```

These examples add {{< key "Ctrl" "Shift" "H" >}} and the chord
{{< key "Ctrl" "K" >}} then {{< key "Ctrl" "H" >}} for Toggle Hidden while
keeping its default {{< key "Ctrl" "H" >}} shortcut. The `none` example disables {{< key "F8" >}} in panels; {{< key "Delete" >}} remains a
separate shortcut for the same command. Bindings are installed after defaults,
but manual config does not provide the editor's conflict checking. A shortcut
can overlap another binding or depend on focus context; if one behaves
unexpectedly, use the keyboard shortcuts editor to resolve it.

To replace a default manually, add an entry for the new key and a separate
`action = "none"` entry for the old key in its original context. For example,
to keep only the new Toggle Hidden bindings above, disable its default:

```toml
[[keymap]]
keys = "ctrl-h"
action = "none"
context = "panel"
```

An explicit manual override affects a keystroke, whereas the editor's
**Disable** button handles all default bindings of the chosen command.
**Reset** removes that command's overrides. Changes made directly to the file
require a restart; changes saved in the shortcut editor take effect at once.

## Reader packages and Tools

Reader packages and Tools are separate features. Reader packages add
automatically used file readers. Put packages under
`$XDG_CONFIG_HOME/fyle/extensions/` (or the platform config directory's
`fyle/extensions/`) and explicitly enable their package IDs:

```toml
[extensions]
enabled = ["sensor"]
```

Only enabled packages may load their declared readers; changing package
installation or enablement takes effect after restarting Fyle. `enabled` is
empty by default. This setting does not install a package.

Declarative Tools use the separate `tools.toml` file in Fyle's config
directory. Lua Tools live in the `plugins/` folder there. Neither belongs in
`config.toml`; see the [Tools guide]({{< relref "tools.md" >}}) for their
configuration and execution rules.

## Saving settings

Fyle saves choices made in the interface back to `config.toml` while keeping
comments and unrelated keys. Theme, font size/zoom, custom titlebar, columns,
workspace layout, panel list/icon view, icon size, keyboard shortcuts, and
bookmarks are saved. The titlebar choice applies on the next restart. Startup
directories, hidden-file state, tree mode, sort, window dimensions, editor,
terminal, openers, and enabled packages are read from your config and are not
written by changing the corresponding UI state.
