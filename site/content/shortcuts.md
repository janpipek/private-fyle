---
title: "Keyboard reference"
weight: 100
---

{{< key "F1" >}} shows all available commands inside Fyle; {{< key "Ctrl" "P" >}} opens the searchable command
palette. The shortcuts throughout this guide are the defaults. **Keybindings
are customizable:** open **Help > Edit Keyboard Shortcuts**, find a command,
and click **Edit**. Type a shortcut such as `ctrl-shift-h`, choose its context,
click **Apply**, then **Save** in the main shortcut editor. Saved changes apply
immediately and are written to `config.toml`.

**Reset** restores a command's default bindings; **Disable** removes its
shortcuts; **Reset all** restores the default keymap. Save these changes too.
The editor checks for conflicts between shortcuts in overlapping contexts.
You can also edit `[[keymap]]` entries by hand; see
[customizing keyboard shortcuts]({{< relref "settings.md" >}}#keyboard-shortcuts)
for examples, context rules and how to disable an old binding. Manual file
changes take effect after restarting Fyle. `Ctrl` means Control and `Alt`
means Alt.

{{< shot src="images/fyle-shortcuts-editor.png" alt="Keyboard shortcuts editor listing each command and its current binding" caption="Help > Edit Keyboard Shortcuts shows the current bindings and controls for changing them." >}}

| Shortcut | Action |
| --- | --- |
| {{< key "Up" >}} / {{< key "Down" >}}, {{< key "Page Up" >}} / {{< key "Page Down" >}} | Move through entries |
| {{< key "Home" >}} / {{< key "End" >}} | First / last entry |
| {{< key "Enter" >}} / double-click | Enter a folder or archive, or open a file |
| {{< key "Shift" "Enter" >}} | Open With choices |
| {{< key "Backspace" >}} | Parent folder |
| {{< key "Alt" "Left" >}} / {{< key "Alt" "Right" >}} | Back / forward in panel history |
| {{< key "Tab" >}} | Switch active panel |
| {{< key "Space" >}} / {{< key "Insert" >}} | Mark or unmark entry |
| {{< key "Ctrl" "A" >}} / {{< key "*" >}} / {{< key "+" >}} | Select all / invert / select by pattern |
| {{< key "/" >}} or {{< key "Ctrl" "F" >}} | Filter the current listing |
| {{< key "Esc" >}} | Clear filter, then marks; stop a running search |
| {{< key "Ctrl" "L" >}} | Edit active location |
| {{< key "Ctrl" "G" >}} / {{< key "Alt" "F1" >}} | Open Locations |
| {{< key "Ctrl" "D" >}} | Bookmark current folder |
| {{< key "Ctrl" "U" >}} | Swap panel contents |
| {{< key "Ctrl" "Left" >}} / {{< key "Ctrl" "Right" >}} | Show the other panel's folder here |
| {{< key "Ctrl" "H" >}} / {{< key "Ctrl" "T" >}} | Toggle hidden files / tree mode |
| {{< key "Ctrl" "1" >}} / {{< key "Ctrl" "2" >}} | List / icon view |
| {{< key "Alt" "=" >}} / {{< key "Alt" "-" >}} | Larger / smaller icons |
| {{< key "F2" >}} | Tools for selection or folder |
| {{< key "F3" >}} | Toggle View inspector |
| {{< key "Alt" "Enter" >}} | Open Properties |
| {{< key "F4" >}} / {{< key "Shift" "F4" >}} | Edit file / create and edit file |
| {{< key "F5" >}} / {{< key "F6" >}} | Copy to other panel / move to other panel |
| {{< key "Shift" "F5" >}} | Extract archive to other panel |
| {{< key "F7" >}} | Create folder |
| {{< key "Shift" "F6" >}} | Rename, or bulk rename marked entries |
| {{< key "F8" >}} / {{< key "Delete" >}} | Move to trash |
| {{< key "Shift" "F8" >}} / {{< key "Shift" "Delete" >}} | Delete permanently |
| {{< key "Alt" "F7" >}} / {{< key "Ctrl" "Shift" "F" >}} | Find files by name or contents |
| {{< key "Ctrl" "R" >}} | Refresh active panel |
| {{< key "Ctrl" "Shift" "S" >}} | Calculate folder sizes |
| {{< key "F9" >}} | Open terminal in active folder |
| {{< key "Ctrl" "C" >}} / {{< key "Ctrl" "X" >}} / {{< key "Ctrl" "V" >}} | Copy, cut or paste files |
| {{< key "Ctrl" "Shift" "C" >}} | Cancel running file jobs |
| {{< key "Ctrl" "P" >}} / {{< key "Ctrl" "Shift" "P" >}} | Command palette |
| {{< key "Ctrl" "K" >}} then {{< key "Ctrl" "T" >}} | Choose theme |
| {{< key "Ctrl" "+" >}} / {{< key "Ctrl" "-" >}} / {{< key "Ctrl" "0" >}} | Zoom in / out / reset |
| {{< key "F10" >}} / {{< key "Ctrl" "Q" >}} | Quit |

In the built-in editor, {{< key "Ctrl" "S" >}} saves and {{< key "Esc" >}} or {{< key "Ctrl" "W" >}} closes the editor window.
The active panel context is suspended while typing in the editor.
