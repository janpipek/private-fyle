---
title: "Tools"
weight: 70
---

Press **F2** to find actions that apply to the current selection or folder.
The Tools menu groups the same offers by purpose. A disabled offer explains
which external program is missing. Some tools ask for parameters or let you
pick a target; before running commands, Fyle shows the exact command lines for
confirmation. Long work runs in the background, and results appear in the
status line or an output dialog.

Users can add or override declarative tools in `<config>/fyle/tools.toml` and
write Lua tool plugins in `<config>/fyle/plugins/`. Tools can run commands,
open URLs or perform supported in-process actions. Fyle does not load
project-local tool definitions. Lua Tools are distinct from reader packages:
reader packages provide selected columns, Properties sections or View
renderers and require explicit enablement in settings.

Tools, terminal access and search are unavailable in remote folders and
archives. **F9** opens an external terminal in the active local folder; Fyle
does not provide an embedded terminal.
