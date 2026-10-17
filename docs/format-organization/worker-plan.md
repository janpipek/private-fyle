# Extension worker contract

Read this when implementing or reviewing the supervised reader worker. Slice
4a supplies validated manifests and bounded, unevaluated entry source. The
owner approved the proposed Lua worker and explicit enablement on 2026-10-16.
The owner approved the bounded renderer scope on 2026-10-17. This document
records the execution boundary; [renderer-plan.md](renderer-plan.md) specifies
the scene and multi-presentation result contracts added in slice 5.

## Delivery boundary

Implement standalone protocol, Lua execution, source brokerage, request policy
and process supervision under `src/extensions/`. Add the reserved helper branch
as the first operation in `main`, before settings, command-line directory
handling or GPUI initialization. Keep all tests inline, following AGENTS.md.

Slices 4b/4c implemented execution, explicit settings and column registration.
Slice 5 extends the same supervisor to typed Properties and viewer requests,
with immutable startup feature registration and native rendering now integrated. Existing Tools Lua plugins stay separate.

Use focused files: `protocol.rs` for bounded wire types, `worker.rs` for Lua,
`broker.rs` for authorized file reads, `model.rs` for queue policy and
`supervisor.rs` for process ownership. Use existing dependencies only.

## Host API and ownership

Use one shared supervisor instance, with one active helper and 64 pending
requests. Create no process until a request is admitted. The scalar compatibility API
is `Supervisor::new() -> io::Result<Self>`, `submit(ColumnWork) ->
io::Result<Ticket>`, and `Ticket::wait() -> io::Result<ResultState<Value>>`.
`wait` and final shutdown are blocking background operations. `submit` performs
bounded in-memory work; a full pending queue returns `WouldBlock`.

`ColumnWork` carries an `Arc<Package>`, selected column contribution ID,
original `Location`, captured display name/directory flag and the caller's
generation cancellation token. Resolve its provider, field and demand from the
validated package, rather than accepting independently supplied definitions.
Reject missing/non-column contributions and nonlocal/directory sources before
execution. `submit_contribution(ContributionWork)` adds a typed contribution
ticket for columns, Properties and viewers; its `wait` returns `PreparedResult`.
The scalar API wraps that ticket and retains its existing return type. All
requests share the same queue, cancellation and cleanup owner. Open and inspect the source in background execution.

A ticket owns a separate cancellation token: dropping/cancelling it must not
set a generation token shared with other requests. Either token cancels its
work. Queued cancellation removes work without starting a child. Active
cancellation retains the active slot until the child is killed and reaped and
all host pipe handles are closed. Completion delivery failure follows the same
cleanup path. Make explicit background shutdown cancel queued/active work and
wait for cleanup; dropping the shared handle signals shutdown without blocking
the UI thread. Test shutdown as well as individual cancellation.

The pure model owns FIFO admission, capacity, cancellation and slot release.
The supervisor owns `Child`, pipe buffers, source `File`, deadlines and cleanup.
UI generations and stale cell acceptance remain with the metadata queue in 4c.

## Protocol and Lua callback

Use a four-byte unsigned big-endian length followed by UTF-8 JSON. Check the
length before allocation. Every message has `version = 2` and an explicit
`kind`; deny unknown keys and reject unexpected messages, duplicate terminal
responses, partial frames and EOF before completion. Keep serde's recursion
limit enabled. Wire values are typed DTOs, not unrestricted JSON trees.

| Direction | Message and payload |
| --- | --- |
| Host → worker | `Init`: script source, package/provider/contribution IDs, presentation request/schema, declaration demand, captured name/is_dir and opaque source handle |
| Worker → host | `Read`: monotonically increasing read ID, source handle, unsigned offset and length |
| Host → worker | `ReadResult`: matching read ID, byte array, EOF flag; or a bounded read error |
| Worker → host | `Result`: presentation-tagged scalar, Properties field results or scene result; each has terminal availability and appropriate completeness |

Use JSON byte arrays with the existing serde dependencies. Only the host knows
the native source path. The child receives neither a reopenable path nor a
directory handle. Request IDs need only identify reads inside this fresh child.
Accept one final contribution result, then require successful child exit.
An abnormal exit or malformed output fails the request even if it emitted a
plausible result first. The worker emits its terminal result and exits directly.

The module returns a table keyed by declared contribution ID. The requested
contribution entry is `{ applies = optional_function, read = function }`. Validate
that this entry exists and contains the required callback. Executable exports
cannot register new IDs or override declarations. Only the selected callback executes. The version bump is internal to the
same executable; manifest API 1 and existing column Lua remain compatible.

Pass `ctx = {source, name, is_dir, demand={max_bytes,max_items}}`. Expose only
the host function `fyle.read(source, offset, length)` for bounded source access;
it returns a Lua byte string and EOF boolean. All names are captured display
data, never filesystem lookup instructions. `applies(ctx)` must return a
boolean; false yields unavailable. Callback errors yield failed. Static
applicability is always a prerequisite, evaluated before module execution;
`applies` can narrow it, never override it. Evaluate static predicates against
honest captured facts: format/context evidence is empty until supplied by an
actual feature owner. A missing `applies` adds no further restriction.

A column `read(ctx)` returns one explicit result table:

```lua
return {
  state = "ready",
  field = "extension.sensor.reader.distance",
  value = { kind = "quantity", value = 42.5, unit = "metres" },
  completeness = { kind = "complete" },
}
```

Text values use `{kind="text", value="..."}`. Also accept `state="absent"`,
`state="unavailable"`/`"failed"` with `reason`, and completeness
`{kind="truncated",reason="..."}` or `{kind="lower_bound"}`. Reject pending,
unknown/extra fields, wrong Lua types, invalid quantities, unexpected units and
oversized strings. Ready results must name exactly the selected column's bound
field; reject sibling/undeclared field IDs. Validate again in the host against
that field's schema. A partial ready value retains its completeness; reaching a limit never
silently converts partial work into complete output.

Properties and viewers use separate typed output schemas. Properties returns
the exact declared field set, with independent terminal states/completeness;
viewers return a validated scene with complete/truncated coverage. See the
[renderer contract](renderer-plan.md#typed-worker-requests-and-results) for those
shapes and the [package guide](../extensions.md) for authoring examples.

## Execution limits and brokerage

Install memory and instruction limits before evaluating module source. Module
evaluation, callback lookup, `applies`, `read` and result conversion share the
same request budget. Initialize an allowlist of Lua libraries/functions. Exclude
`io`, `os`, `package`, `debug`, `require`, `dofile`, `loadfile`, `load`,
`collectgarbage`, `string.dump` and ambient output functions. Source creation
uses host-owned mlua loading only. Keep protocol output exclusively host-owned.
This is a trusted-package execution contract, not hostile-code OS isolation.

| Named limit | Initial value |
| --- | --- |
| Manifest/script bytes | Existing 4a constants: 256 KiB each |
| Protocol frame | 1 MiB |
| Cumulative worker → host protocol bytes | 2 MiB, including framing |
| One source read | 64 KiB |
| Cumulative authorized source bytes | min(declared demand, 16 MiB) |
| Source read calls | 512, including zero-length/repeated reads |
| Lua memory | 32 MiB |
| Lua instructions | 5,000,000; hook every 1,000 instructions |
| Wall deadline | 2 seconds from process start, including Init transfer |
| Stderr | 64 KiB cumulative; exceeding it fails and kills the worker |
| Result text/reason | 64 KiB each |
| Captured display name | 4 KiB |
| Lua output traversal | 4,096 nodes, depth 32 |
| Supervisor polling interval | At most 10 ms, shortened to remaining deadline |
| Pending/active requests | 64 / 1 |

The host broker opens the original native path with Linux `O_NONBLOCK`, then
checks the opened file's metadata and accepts regular files only. Preserve
native bytes, including non-UTF-8 paths. Retain this `File` for the request.
Use checked offset/length arithmetic, reject unknown handles, enforce cumulative
bytes/calls before access and charge repeated ranges again. Bound each retained
read response and outgoing frame. Item counting belongs to the supervised Lua
reader and validated output; byte brokerage does not prove parser item counts.
Keep declaration limits in `ctx` and require truncation/error on bounded work.
Local regular-file I/O is bounded in volume; do not claim kernel filesystem I/O
is forcibly interruptible by a userspace deadline.

## Process transport and cleanup

On Linux, set all host pipe ends nonblocking using existing libc with documented
FFI safety. One background event loop pumps partial Init/ReadResult writes,
framed stdout reads and bounded stderr reads while checking cancellation,
deadline and `try_wait`. Supervise the initial write too: a 256 KiB script
exceeds typical pipe capacity and a child may never read it. Bound retained
buffers; a backlogged pipe cannot create another unbounded message queue.

Use `poll` or a maximum 10 ms idle wait. No host pipe operation or pipe-thread
join may block the supervisor past its deadline checks. This design needs no
pipe-pump threads or joins. On fault/cancellation/deadline, close input, kill
the child if still alive, wait/reap it and close remaining pipes before releasing
the slot. On normal completion also reap before delivery/release. Lua has no
API for creating descendants; test fixtures must respect that lifecycle.

Keep non-Linux builds compiling with an explicit `Unsupported` supervisor
backend; Windows expansion is paused. Pure protocol, model and worker remain
platform-independent. Production `--fyle-extension-worker-v1` dispatch runs the
generic `worker::run<R: Read, W: Write>` on stdin/stdout, then exits with a
meaningful status, without touching config or constructing a GUI application.

## Tests and completion

1. Test framing, schema/unit/completeness validation and broker boundaries at
   the limit and one beyond. Cover malformed/truncated frames, read-ID errors,
   unknown handles, integer overflow, cumulative overlapping reads, invalid
   UTF-8 text, excessive output and non-UTF-8 native source paths.
2. Test real Lua module evaluation and callbacks under memory/instruction
   limits. Cover infinite top-level code, infinite callback, error, false
   applicability, absent/partial values and denied ambient APIs.
3. Test the actual supervisor against an inline subprocess fixture. Spawn the
   current unit-test executable with `--exact` and the fixture's full test name,
   `--nocapture`, `--test-threads=1` and a dedicated fixture environment marker.
   Only that marked fixture runs the generic real worker on stdin/**stderr**;
   harness stdout is the diagnostic pipe. Production uses stdout for protocol.
   Normal fixture invocation returns, while substantive supervisor tests always
   spawn it and assert results. No prebuilt `target/debug/fyle`, test skipping,
   network or optional program is required.
4. Use controlled fixture modes for never reading Init, crashing, partial or
   oversized frames and excessive diagnostics. Test hard deadline independently
   of Lua hooks, cancellation while writing/reading, shutdown and stale active
   occupancy. Assert exit/reap and the next request starting only after cleanup.
5. Run repository fmt/clippy/tests. Build the app and manually smoke-test the
   production reserved dispatch with valid/bad protocol under a nonexistent
   scratch config and no display. This separately verifies the first-main-branch
   wiring that the unit-test harness cannot execute.

Done means these standalone paths are verified, reviewed and documented in the
progress log. Commit 4b separately before starting 4c integration. The sensor
package UI proof and renderer acceptance belong to their later slices.
