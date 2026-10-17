# Sensor reader example

This package demonstrates a local reader shared by a Properties section,
a display-only distance column and a custom View graph. Parsing and layout live
entirely in `main.lua`; changing them requires a restart, without rebuilding
fyle.

## Install and try

1. Copy `manifest.toml` and `main.lua` into your fyle config directory under
   `extensions/sensor/` (normally `~/.config/fyle/extensions/sensor/` on Linux;
   `XDG_CONFIG_HOME` is respected).
2. Add `sensor` to `[extensions] enabled` in `config.toml`, then restart fyle.
3. Select a file from `fixtures/`. F3 / the inspector's View tab shows the graph
   and typed sample table; Alt-Enter / File → Properties shows the summary.
4. Select **Sensor distance** in View → Columns… for the display-only column.

`complete.sens` produces 30 seconds and 95 metres. `truncated.sens` exceeds the
sample limit and produces lower bounds of 127 seconds and 381 metres, plus a
partial graph. `malformed.sens` reports non-monotonic times; Properties keeps
native filesystem facts visible.

Restart after editing either package file or enablement. Disabling the package
restores native text View and removes its Properties section; saved column IDs
remain available for re-enablement. See the [reader package guide](../../../docs/extensions.md)
for the authoring API and trusted-package execution limits.

## File format

A UTF-8 `.sens` file starts with `FYLE-SENSOR 1` on its own line. Every following
line has two whitespace-separated nonnegative finite numbers: elapsed seconds
and cumulative metres. The first sample is `0 0`; subsequent times strictly
increase and distances never decrease. Blank lines are ignored. CRLF is accepted.

```text
FYLE-SENSOR 1
0 0
10 24
20 61
30 95
```

The reader examines at most 64 KiB and 128 samples per request. Duration and
distance are the last accepted sample's values. When the byte or sample limit
cuts off input, both quantities are lower bounds and the graph is explicitly
partial. A line cut off by the byte limit is discarded. Invalid complete lines,
unsupported headers and non-monotonic samples fail the contribution with a
useful message. A header without samples has no sensor values.

The graph uses elapsed time on the horizontal axis and cumulative distance on
the vertical axis. A short typed sample table and selectable summary accompany
it. Every presentation reads independently under the same declared limits;
there is no shared cross-request sensor cache.
