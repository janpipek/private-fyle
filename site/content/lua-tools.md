---
title: "Lua tools"
weight: 71
---

Lua tools let you add context-aware actions without rebuilding Fyle. Put each
plugin in `<config>/fyle/plugins/` with a `.lua` extension. Fyle reads the
files when it starts, in filename order. A plugin returns an array of tool
tables. The example below prints checksums for selected Markdown and text
files, just like the [declarative TOML example]({{< relref "tools.md" >}}),
but uses Lua to decide which selection applies.

Save it as `~/.config/fyle/plugins/checksums.lua`, or
`$XDG_CONFIG_HOME/fyle/plugins/checksums.lua`. Create the `plugins` directory
if needed and restart Fyle. Lua 5.4 is embedded through `mlua` and built with
the application; you do not need a separate Lua interpreter or a package
manifest for a tool plugin.

```lua
return {
  {
    id = "checksums.text.lua",
    group = "Checksums",
    label = "Print SHA-256 checksums",
    requires = { "sha256sum" },
    params = {
      { key = "style", label = "Output style", choices = { "--text", "--tag" } },
    },

    applies = function(ctx)
      if #ctx.targets == 0 then return false end
      for _, target in ipairs(ctx.targets) do
        if target.is_dir or (target.ext ~= "md" and target.ext ~= "txt") then
          return false
        end
      end
      return true
    end,

    plan = function(ctx, values)
      local args = { ctx.programs.sha256sum, values.style, "--" }
      for _, target in ipairs(ctx.targets) do
        table.insert(args, target.path)
      end
      return { run = args, output = "log" }
    end,
  },
}
```

Select one or more `.md` or `.txt` files and choose the tool from Tools or
press {{< key "F2" >}}. Every selected path is passed as a separate argument. The
command is confirmed before it runs, and its combined output appears in a
dialog. The parameter form lets you choose the usual checksum output or GNU
tagged output. This example requires GNU `sha256sum`. No shell parses the paths.

## Definition

Each plugin returns an array, even when it contains one tool. A tool requires
these fields:

- Nonempty `id`, `group`, and `label` strings.
- A `plan` function.

The `id` is the stable key used for overrides. `requires`, `params`, and
`applies(ctx)` are optional; without `applies`, the tool applies everywhere.

| Field | Meaning |
| --- | --- |
| `id` | Stable identifier. A later tool with the same id replaces the earlier definition. |
| `group`, `label` | Group and menu text shown to the user. |
| `requires` | Array of external program requirements. A requirement such as `"magick|convert (ImageMagick)"` tries the names in order; the first name is the alias exposed in `ctx.programs`, and the parenthesized text explains a missing requirement. |
| `params` | Optional parameter array. Each item has a required `key`, optional `label`, and either nonempty `choices` or a `default` text value. |
| `applies(ctx)` | Optional predicate. Return true to offer the tool for this context. An error or non-boolean return hides the tool. |
| `plan(ctx, values)` | Required function that returns one action table. `values` maps parameter keys to the chosen strings. |

Fyle checks `applies` while listing offers and checks it again before planning.
`requires` programs are resolved from `PATH`; if one is missing, the offer is
shown disabled with the requirement hint. `plan` receives resolved executable
paths in `ctx.programs`, keyed by each requirement's first name. If planning
raises an error, Fyle shows it in the status line.

For a choice parameter, the first choice is the default. A text parameter
uses `default`, or an empty string when it is absent; its label falls back to
its key. For example, `{ key = "message", label = "Message", default = "Hello" }`
asks for text available as `values.message`.

Parameters are strings. Validate numbers or other structured input in `plan`
and call `error("reason")` when a value cannot be used. Parameter definitions
are read when the plugin loads; they are not regenerated for each selection.

## Context

Both functions receive a fresh context table. Paths are strings. `ctx.targets`
is the marked entries, or the cursor entry when there are no marks.

| Field | Contents |
| --- | --- |
| `ctx.dir` | Active panel's directory. |
| `ctx.other` | Other panel's directory, or `nil` when unavailable. |
| `ctx.targets` | Array of targets with `path`, `name`, `stem`, `ext`, `parent`, `is_dir` and `kind`. `stem` removes the last extension; `ext` is lowercase without a dot and empty for directories. `kind` is Fyle's file-icon category, such as `image` or `code`. |
| `ctx.facets` | Detected project roots by kind, such as `ctx.facets.git_repo`, `ctx.facets.cargo_project` or `ctx.facets.uv_project`. A missing facet is `nil`. The active directory and its ancestors are inspected; the nearest root of each kind is used. |
| `ctx.programs` | Available only in `plan`: resolved executable paths keyed by the first name in `requires`. |

`applies(ctx)` receives no `values` argument and does not receive
`ctx.programs`. `plan(ctx, values)` receives the selected/default parameter
values as its second argument.

The context is a snapshot, not a live reference to panel widgets. Changing a
Lua context table does not change the panels. Native paths are converted to
strings using lossy UTF-8 conversion, so names containing invalid UTF-8 cannot
be represented exactly by this API.

## Actions

Return one table describing the work. A run action has a program as its first
array element and the remaining elements as arguments. This fragment assumes
the tool declares `requires = { "tool" }` and has at least one target:

```lua
return {
  run = { ctx.programs.tool, "--flag", ctx.targets[1].path },
  cwd = ctx.dir,
  output = "log",
  refresh = false,
}
```

| Action table | Behavior |
| --- | --- |
| `{ run = { program, arg, ... }, cwd = path, output = mode, refresh = bool }` | Run a program with separate arguments. `cwd` defaults to `ctx.dir`; `output` defaults to `"status"` and accepts `"status"`, `"log"` or `"detached"`; `refresh` defaults to false. `"status"` and `"log"` both capture stdout and stderr, and nonempty output is shown in a dialog. `"detached"` starts without waiting or capturing output. Set `refresh = true` for a successful command that changes files so the panels reload. |
| `{ open_url = "https://..." }` | Open a URL. |
| `{ sequence = { action, ... } }` | Run actions in order. Execution stops at the first failure; earlier completed actions are not rolled back. |
| `{ pick = { title = "...", items = { { label = "...", detail = "...", action = action }, ... } } }` | Ask the user to choose an action. A pick should be the top-level action; the UI resolves it before running. |

Actions can be returned by `plan` or nested in a sequence or pick item. A
pick may contain a sequence, but keep picks at the top level: the UI resolves
the top-level pick, while a pick inside a sequence cannot be resolved during
execution. A sequence aggregates output and refresh requests from successful
actions. If an action fails, the sequence stops and reports that error. Fyle
confirms actions that run commands; URL opening and the pick prompt do not need
that command confirmation.

## How plugins run

At Commander startup, Fyle builds the registry in this order:

1. Built-in Rust tools.
2. Built-in TOML tools.
3. The user's `tools.toml`.
4. Lua plugins, loaded in filename order.

Registering an existing id replaces its earlier definition, so Lua can
override built-in or user TOML tools. The last duplicate id loaded wins. A
TOML parse failure or a Lua file that cannot be read, evaluated, or validated
is reported at startup; a failed Lua file contributes none of its tools.

Each `.lua` file is evaluated once with `mlua` when the registry loads. Its
returned tool tables and functions are retained in that file's Lua state.
Calls to `applies` and `plan` execute those functions later; top-level plugin
code does not run again for each menu opening. Tools from one file share its
Lua state, including any Lua globals the plugin changes. Different files have
separate Lua states. Calls into one file's state are protected by a mutex, so
calls for that plugin are serialized. The Tools menu discovery and planning
run off the UI thread, and command execution also runs in the background.

Lua plugins are trusted user configuration. Fyle does not configure a
restricted plugin environment or apply an instruction, memory or time limit
to tool scripts. Treat a plugin like code you chose to run: its Lua code can
perform work while loading or while `applies` and `plan` are called. The
returned command actions go through Fyle's confirmation step, but that is not
a sandbox for Lua itself. Tool commands have no cancellation mechanism; a
long-running foreground command must finish before the result is handled.
Fyle does not watch the plugin folder for changes, so restart after editing a
plugin.

The host keeps planning separate from execution: `src/tools/lua.rs` adapts
each Lua definition to the Rust `Tool` interface and converts returned tables
into `Action` values. `src/tools.rs` owns registration, applicability, and
program resolution. `src/commander/tools.rs` presents parameters, choices,
confirmation, and results; `src/tools/exec.rs` starts processes with Rust's
`Command`, without a shell.

Fyle reports these invalid results instead of executing them:

- An empty `run` array.
- An unknown output mode.
- A malformed action table.
- A Lua error.

Return one action form per table. Unknown extra fields are not a supported
extension mechanism.

Keep `applies` quick and free of side effects, and make `plan` return data
instead of running commands itself. Execution has these limits:

- Captured command output is collected until the process exits; there is no
  live stream or bounded-output guarantee.
- Detached commands have standard input, output, and error disconnected.
  Fyle reports that they started without waiting for their eventual result.
- Tool commands have no cancellation mechanism. A long-running foreground
  command must finish before Fyle handles its result.
- An external tool's output files follow that tool's overwrite rules, not
  Fyle's file-transfer conflict dialog.

Lua tools are separate from reader packages. Tools offer actions; reader
packages contribute columns, Properties sections, or View renderers. For the
overall tool model and declarative definitions, see [Tools]({{< relref "tools.md" >}}).
