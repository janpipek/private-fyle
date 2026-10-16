# Reader packages

On Linux, an explicitly enabled reader package can add display-only columns for
local regular files. Properties sections and custom viewers are later slices of
the [provider plan](format-organization/implementation-plan.md). Existing
[Tools Lua plugins](tools.md) have a separate contract.

## Install and enable

Place each package under your fyle config directory:

```text
~/.config/fyle/extensions/sensor/
  manifest.toml
  main.lua
```

The config directory respects `XDG_CONFIG_HOME`. Packages are never loaded from
the current project or discovered automatically. In `config.toml`, add:

```toml
[extensions]
enabled = ["sensor"]
```

Enabling an ID authorizes automatic execution of its declared readers when
selected columns need visible-row values. Enable only packages you trust. The
default is an empty list. Restart after changing packages or enablement, then
select the package's column in **View → Columns…**. No Lua is evaluated during
static package loading.

Disabling a package leaves saved column selections intact. Its cells become
blank and Columns… lists those IDs as unavailable. Re-enabling it restores the
same selections after restart. Package errors do not prevent valid neighbors
from loading. Unrelated settings saves preserve enabled IDs and config comments.

## A small distance column

This example reads a file such as `track.sens` containing a single nonnegative
distance in metres, for example `42.5`.

Save this as `manifest.toml`:

```toml
api_version = 1
id = "sensor"
entry = "main.lua"

[[providers]]
id = "extension.sensor.reader"

[[providers.fields]]
id = "extension.sensor.reader.distance"
kind = "quantity"
unit = "metres"

[[providers.columns]]
id = "extension.sensor.reader.distance-cell"
label = "Sensor distance"
column_id = "extension.sensor.reader.distance"
field = "extension.sensor.reader.distance"
max_bytes = 32
max_items = 1
```

Save this as `main.lua`:

```lua
return {
  ["extension.sensor.reader.distance-cell"] = {
    applies = function(ctx)
      return ctx.name:match("%.sens$") ~= nil
    end,
    read = function(ctx)
      local bytes, eof = fyle.read(ctx.source, 0, ctx.demand.max_bytes)
      local distance = tonumber(bytes:match("^%s*(%S+)%s*$"))
      if distance == nil or distance < 0 then
        error("expected one nonnegative distance in metres")
      end
      local completeness = { kind = "complete" }
      if not eof then
        completeness = { kind = "truncated", reason = "input byte limit" }
      end
      return {
        state = "ready",
        field = "extension.sensor.reader.distance",
        value = { kind = "quantity", value = distance, unit = "metres" },
        completeness = completeness,
      }
    end,
  },
}
```

The manifest declares identities, fields, labels and acquisition bounds. Lua
cannot register new IDs. Each module returns callbacks keyed by declared
contribution ID; ready output must name that contribution's bound field. Static
applicability is a prerequisite; an optional Lua `applies(ctx)` can narrow it.
Format/context predicates require evidence captured by the host, which the
initial column path does not supply.

Values are UTF-8 text or finite nonnegative quantities in seconds/metres. A
callback can also return `{state="absent"}`, or `unavailable`/`failed` with a
`reason`. Ready values require explicit completeness: `complete`, `truncated`
with a reason, or `lower_bound`. Partial values retain that distinction in the
column display; sorting these columns remains disabled.

## Execution boundary

The host opens the selected native file and brokers bounded offset/length reads
through an opaque handle. Lua receives a display name, directory fact and
declared demand, never a source path to reopen. Remote/archive entries are not
read. Lua has no ambient filesystem, process, network or module-loading API.

One helper executes at a time, with 64 pending requests. Limits include a
two-second deadline, 32 MiB Lua allocation, five million Lua instructions,
64 KiB per source read, at most the declared input byte budget (capped at
16 MiB), 512 read calls, bounded protocol frames and 64 KiB result text.
Repeated reads are charged again. Readers must honor declared item limits and
report truncation when acquisition is partial. Native code validates schemas,
units, field ownership and completeness.

Cancellation and normal shutdown kill/reap active helpers before releasing
their slot. These are resource limits for trusted packages, not an OS sandbox.
See the [worker contract](format-organization/worker-plan.md) for the full
protocol, limits and cleanup ownership.
