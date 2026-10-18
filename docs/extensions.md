# Reader packages

On Linux, an explicitly enabled reader package can add Properties sections,
display-only columns and custom viewers for local regular files. The
[provider plan](format-organization/implementation-plan.md) records the migration. Existing
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
selected columns, Properties or View need its declared contribution. Enable only packages you trust. The
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

## Properties and a custom viewer

The complete [sensor package](../examples/extensions/sensor/README.md) reads
elapsed-time/distance samples and contributes all three presentations. Copy its
`manifest.toml` and `main.lua` into `extensions/sensor/`, enable `sensor` and
restart. This replaces the small single-number example above; the two examples
use the same package ID and different `.sens` formats.

Properties combines matching package sections with native filesystem and format
facts. Each section binds a list of manifest field IDs. Its callback returns
one terminal result per declared field, with independent completeness:

```lua
return { state = "ready", fields = {
  { field = "extension.sensor.reader.duration", state = "ready",
    value = { kind = "quantity", value = 30, unit = "seconds" },
    completeness = { kind = "complete" } },
  { field = "extension.sensor.reader.distance", state = "failed",
    reason = "distance sample is invalid" },
} }
```

Return every declared field once. An absent field uses `state="absent"` and
its field ID. A section can itself return absent, unavailable or failed without
fields. A failed field leaves readable siblings and native facts visible.
Partial and lower-bound quantities retain their markers in Properties.

A viewer declares `representation = "fyle.scene"` and `version = 1`. Its
callback returns a scene and explicit complete/truncated coverage:

```lua
return { state = "ready", completeness = { kind = "complete" }, scene = {
  version = 1,
  root = { kind = "text", id = "title", role = "heading", text = "Sensor data" },
} }
```

A scene supports these native primitives:

| Node | Required data |
| --- | --- |
| `stack` | `id`, `axis` (`row`/`column`), `gap` (`small`/`normal`), `children` |
| `text` | `id`, `text`, `role` (`body`/`heading`/`muted`) |
| `table` | `id`, typed `columns`, rectangular `rows` |
| `canvas` | `id`, finite `width`/`height`, `paths` |

Table columns carry `id`, `label` and a `kind` table such as
`{kind="quantity", unit="metres"}`. Cells use the same typed value shape as
columns, with no field ID or completeness. Use `false` for a blank cell; rows
remain dense arrays of exactly the declared width. `nil` holes and `true` are
invalid. Canvas paths carry `role`
(`foreground`/`muted`/`accent`/`success`/`warning`), `weight`
(`thin`/`normal`/`strong`) and at least two `{x=..., y=...}` points inside the
canvas. An empty path list is allowed. See the full package for a graph/table
example. Native code handles selection, copying, scrolling, theme colors and
canvas scaling. There are no script-defined widgets or interaction callbacks.

Scenes are bounded: 128 nodes, depth 8, 4 KiB per string / 64 KiB total text,
256 table rows, 16 columns per table, 2,048 cells, eight canvases, 64 paths and
8,192 points. Canvas dimensions range from 1 to 4,096. Node IDs must be unique;
table column IDs must be unique within each table. Invalid output produces a
contribution diagnostic.

Specialized built-in viewers keep their existing priority. Matching package
viewers run before generic text/binary, ordered by package ID then contribution
ID. Returning absent or unavailable lets the next viewer try; failure is shown
as a View error. Use `applies(ctx)` to decline unrelated files. Remote/archive
items cannot invoke packages through their downloaded/extracted copies.

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
