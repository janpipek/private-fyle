# Sensor reader example

This package demonstrates a local reader shared by a Properties section,
a display-only distance column and a custom View graph. The package is under
construction as part of slice 5; installation instructions follow verification.

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
