# Sensor renderer: slice 5 handoff

The owner approved static layout, selectable text, typed tables and bounded
path canvas on 2026-10-17. Native code owns interaction; executable widgets and
arbitrary callbacks are outside this scope. Read this document when implementing
the sensor Properties/View proof. Existing column execution is the baseline;
[worker-plan.md](worker-plan.md) remains the execution-limit reference.

## Implement in three reviewable steps

1. **Validate scenes.** Add the pure `preview_scene.rs` model and inline tests.
   Complete when every grammar variant and cumulative bound has boundary tests,
   invalid input cannot construct a validated scene, and the existing suite
   passes. Commit this model independently before runtime integration.
2. **Prepare typed contributions.** Generalize the supervised request/result
   protocol, preserving existing Lua column syntax. Add Properties field results
   and scene results, worker conversions and real-helper fixture tests. Complete
   when all three presentation results are validated and cancellation/reaping
   still works through the existing one-active-worker supervisor.
3. **Wire the proof.** Build immutable feature registrations from the same
   enabled package snapshot, adapt Properties and View, render scenes, and ship
   the sensor package. Complete when changing only the package changes its graph
   and layout in the app, all three presentations work, and source/lifecycle and
   fallback regressions pass. Update the progress log and feature documentation.

The coordinating agent assigns files before concurrent edits. One Luna owns the
pure scene model; the other initially owns protocol/supervisor/worker changes.
Thin view and feature adapters follow once those APIs are reviewed. Sol owns
this plan and read-only architecture review. Stage only each step's files.

## Pure scene grammar

`preview_scene` imports neither GPUI nor extension execution. A strict serde
`SceneDto { version: 1, root: NodeDto }` validates into `Scene` with private
storage and read-only getters. Rendering accepts only `Scene`. DTO numeric
values convert through existing validated `data_providers::Value` quantities;
deserialization must not bypass quantity validation.

Every node has a stable ASCII identifier, unique across the scene. The grammar
has four variants, with `kind` as its discriminator:

| Node | Data |
| --- | --- |
| Stack | Axis `row`/`column`, gap `small`/`normal`, children |
| Text | UTF-8 text; semantic role `body`/`heading`/`muted`; always selectable |
| Table | Columns with ID, label and declared text/quantity type; rectangular rows of optional typed values |
| Canvas | Logical width/height and polyline paths; each path has semantic color role, stroke weight and points |

Canvas roles are `foreground`, `muted`, `accent`, `success`, `warning`; weights
are `thin`, `normal`, `strong`. Native theme colors and widths implement them.
Coordinates are finite and inside the logical canvas; native painting scales
uniformly to the available width and clips to bounds. A path has at least two
points. An empty path list is valid for an activity with no drawable sample.
Disconnected segments use separate paths. Images, URLs, curves, fills, absolute
screen positioning and worker-defined style/widget objects are unnecessary for
this proof and are excluded.

The model implementation's DTO/getters are the source of truth for exact Rust
names. Table column IDs are unique within their table; each nonempty cell must
match its column type/unit. Empty cells use `None`, not a fabricated zero.
Quantities remain typed until native presentation formats them.

| Cumulative scene limit | Value |
| --- | --- |
| Nodes / depth | 128 / 8 |
| Identifier bytes | 128 |
| One string / total text including labels and cells | 4 KiB / 64 KiB |
| Table rows / columns per table / total cells | 256 / 16 / 2,048 |
| Canvas nodes / paths / points | 8 / 64 / 8,192 |
| Logical width and height | Finite, 1 through 4,096 |

Reject invalid depth, dimensions, coordinates, row shapes, duplicate IDs and
unknown keys. Counters include every sibling/table/canvas; validating each
child independently must not reset aggregate budgets. Lua conversion enforces
these bounds before copying unbounded collections, then the host validates the
decoded DTO again. A bounded frame alone does not validate scene semantics.

## Typed worker requests and results

Bump the internal same-executable protocol to version 2; manifest API 1 and
existing Lua column source remain compatible. Keep the broker, frame sizes,
instruction/memory/deadline limits and global active/pending capacities.
Properties/View do not create private supervisors or shared acquisition caches.

`ContributionWork` retains the current column work fields: validated package,
contribution ID, original `Location`, captured name/directory fact and generation
cancellation token. Resolve presentation and expected schema from that package's
declaration; callers cannot supply replacement bindings. `Init.request` is:

| Request | Expected declaration |
| --- | --- |
| Column | One field ID and field type |
| Properties | The ordered set of field IDs and field types |
| Viewer | Representation `fyle.scene`, version 1 |

Use a presentation-tagged wire result with separate Column, Properties and
Viewer payloads. A scalar column still returns its existing Lua result table.
Properties returns `{state="ready", fields={...}}`; each array element names
its field and has terminal state `ready`, `absent`, `unavailable` or `failed`.
Ready fields carry the existing typed `value` and explicit `completeness`;
failed/unavailable fields carry a reason. Outer Properties can also be absent,
unavailable or failed, without a field array.

Properties must return exactly its declared field set, once each. Reject missing,
duplicate and undeclared IDs for the contribution. Normalize valid output to
manifest field order. Validate each known field independently: a wrong numeric
type/unit becomes that field's failure while valid siblings survive. Enforce
128 fields and 64 KiB aggregate text/reasons, in addition to individual string
limits. Preserve independent completeness; a ready field may be truncated or a
lower bound. There is no invented group-level completeness claim.

Viewer output is `{state="ready", scene={version=1,root=...},
completeness={kind="complete"}}`, or truncated with a reason. Its other terminal
states match the existing contribution convention. A scene itself is complete
or truncated; lower-bound facts belong to individual metrics, not an entire
display tree. Pending and unknown output keys/states are rejected.

Prepared host types live in a pure `extensions/results.rs`:

- `PreparedResult::{Column(ResultState<Value>), Properties(PropertiesResult),
  Viewer(ResultState<Scene>)}`.
- `PropertiesResult::{Ready(Vec<FieldResult>), Absent, Unavailable(String),
  Failed(String)}`.
- `FieldResult { field: FieldId, state: ResultState<Value> }`.

Keep `Supervisor::submit(ColumnWork) -> Ticket` and its scalar wait method as a
small compatibility wrapper. Add `submit_contribution(ContributionWork) ->
ContributionTicket`, whose wait returns a `PreparedResult`. One inner ticket
owns cancellation; wrapping it must not duplicate tokens or cancel siblings.
Check response presentation against the request before accepting it.

Module exports stay keyed by declared contribution ID, each with optional
`applies(ctx)` and a `read(ctx)` callback. Static applicability is always a
prerequisite. Lua applicability narrows it inside the supervised worker. Empty
format/context evidence remains empty. Evaluate all module code, callbacks and
scene conversion under the same budget; render never calls Lua or reads disk.

## Immutable startup registration

Extend the existing startup preparation to retain one immutable enabled-package
catalog with column, Properties and viewer handles plus the one supervisor.
Build feature registrations from that same snapshot before GPUI starts, with
structured diagnostics retained until the UI boundary. Instantiate a supervisor
when any supported executable contribution needs one, not only columns.

Properties declarations require a nonempty unique field binding set, at most
128. Viewer activation accepts only `fyle.scene` version 1. Static manifest
parsing may retain other representation names; unsupported viewer activation
must explain why and leave valid sibling columns/Properties usable. Do not leak
strings to satisfy static trait lifetimes; dynamic adapters use owned IDs/labels
or trait return values borrowed from `self`.

Retain one outer supervisor handle through normal post-GPUI shutdown, as in
slice 4. Keep feature scheduling, selection policies and stale-result acceptance
independent. Sharing definitions/supervision does not imply sharing results.

## Properties and preview integration

Properties uses the original `Selection` and its cancellation token under the
existing local-regular-file branch. Remote/archive inspection remains listing
only, even after View downloads/extracts the item. Existing filesystem, EXIF and
native additional groups survive any package failure.

Keep native `PropertyGroup`/`PropertyValue` models intact. Attach separate typed
extension sections to `Inspection`: contribution ID, manifest label and
`PropertiesResult`. Field labels can humanize the last field-ID segment; the
current field declaration has no label key. Format typed values only in the
view, preserving partial/lower-bound markers and per-field diagnostics. Hide
absent/unavailable sections; show failed sections and failed fields beside
readable siblings. Reuse native selectable-text behavior.

Preview requires an explicit source request carrying readable path, original
identity and cancellation. A path-only wrapper is native-only. Package readers
are eligible only when the original identity is local and its exact native path
equals the readable path. Missing identity, remote materialization and archive
extraction never authorize a package. Native preview behavior stays intact.

Add full-load cancellation to `PreviewSession`: reset, reload, suspension and
target replacement cancel the old token; a background request retains its own
original identity/token. Dropping the GPUI task alone is insufficient to stop a
supervised helper. Current generation/source checks still reject stale scenes.

Preserve every existing specialized built-in's order. Try package viewers next,
sorted by package ID and contribution ID, before generic Text/Binary. Selection
runs in background: unavailable/absent means try the next candidate, ready wins,
and failure/invalid output becomes a useful View diagnostic. A declaration of
Any plus `applies=false` must not hijack unrelated files. Do not execute callbacks
inside `Previewer::matches` or infer applicability from a temporary filename.

Add `Content::Scene` with the validated scene and retained completeness. A thin
`preview_scene_view.rs` renders static stacks, selectable text/table cells and a
bounded native canvas. Native code owns scrolling, text selection/copy and theme
mapping. Clear selection on target/perspective changes as Properties already
does. Existing commander copy priority handles selected text. No new script
interaction intents or raster resources are needed.

## Sensor proof and acceptance

The example under `examples/extensions/sensor/` uses `FYLE-SENSOR 1`, followed by
time-seconds/cumulative-metres samples. Start at `0,0`; require monotonic time and
distance, finite nonnegative numbers, 64 KiB input and 128 samples. Package-only
Lua parsing produces duration/distance metrics, Properties, a column and its
scene. Bounded sample totals are lower bounds; the graph is explicitly partial.
Use the same filename applicability for all three contributions. The coordinator
owns the example/docs after the runtime schema is stable.

Tests must exercise real supervised package execution through all adapters,
including column compatibility, independent Properties failures/completeness,
scene DTO/worker rejection, deterministic viewer decline/selection and unchanged
native fallbacks. Explicitly test that downloaded/extracted nonlocal targets
cannot invoke packages, cancellation retains occupancy until reap, and rapid
target/perspective changes reject old scenes/fields. Use TempDir and the existing
inline helper fixture; no optional programs/network or prebuilt binary reliance.

Live checks use scratch config/data: complete/truncated/malformed inputs in all
three presentations, selectable text copy, a native typed table, graph/layout
changes made only in Lua, competing/declining viewers, disable/restart, rapid
selection changes and normal active-helper quit. Run fmt/clippy before commits
and the full test suite before completing each reviewed step. The contextual Git
proof and broader migrations remain their later slices.
