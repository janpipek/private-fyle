---
title: "Tools"
weight: 70
---

Press {{< key "F2" >}} to find actions that apply to the current selection or folder.
The Tools menu groups the same offers by purpose. A disabled offer explains
which external program is missing. Some tools ask for parameters or let you
pick a target; before running commands, Fyle shows the exact command lines for
confirmation. Long work runs in the background, and results appear in the
status line or an output dialog.

{{< shot src="images/fyle-tools-picker.png" alt="Tools picker showing available ImageMagick actions and a disabled Zip action labeled needs zip" caption="The picker offers tools for the selection and explains which programs disabled tools need." >}}

Users can add or override declarative tools in `<config>/fyle/tools.toml` and
write Lua tool plugins in `<config>/fyle/plugins/`. Tools can run commands,
open URLs or perform supported in-process actions. Fyle does not load
project-local tool definitions. Lua Tools are distinct from reader packages:
reader packages provide selected columns, Properties sections or View
renderers and require explicit enablement in settings.

For example, this read-only TOML tool prints SHA-256 checksums for selected
Markdown and text files. It passes each path as its own argument and does not
use a shell:

Append this definition to `~/.config/fyle/tools.toml` (or
`$XDG_CONFIG_HOME/fyle/tools.toml`), creating the file if needed:

```toml
[[tool]]
id = "checksums.text"
group = "Checksums"
label = "Print SHA-256 checksums"
when = { entries = "files", exts = ["md", "txt"], min = 1 }
requires = ["sha256sum"]
run = ["{sha256sum}", "{paths}"]
output = "log"
```

Restart Fyle, mark `.md` or `.txt` files, then press {{< key "F2" >}} and choose
**Print SHA-256 checksums**. This requires the `sha256sum` program, commonly
provided by GNU coreutils on Linux; if it is missing the tool is disabled.
With no marks, the tool acts on the file under the cursor. Every target must
match the rule, so a mixed selection containing other file types hides it.

`{paths}` expands into separate arguments for all targets. Use
`per_target = true` with `{path}` to run one command per file instead.
`output = "log"` requests captured output; `refresh = true` requests a panel
reload after a successful command that changes files. The command is shown
for confirmation even though this example only reads files. A tool with an
existing `id` replaces that tool, so use your own IDs for additions.

The [Lua tools guide]({{< relref "lua-tools.md" >}}) explains the plugin
format, context, action forms and execution model, with a matching Lua example.

Tools, terminal access and search are unavailable in remote folders and
archives. {{< key "F9" >}} opens an external terminal in the active local folder; Fyle
does not provide an embedded terminal.
