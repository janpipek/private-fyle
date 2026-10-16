# Data providers and contributions: implementation plan

Status: proposed sequence, not implementation approval. This makes the
[synthesis](synthesis.md) actionable. No application behavior has changed.
The [TODO](../../TODO.md) remains the backlog and approval record. Record owner
decisions in this document before implementing the affected slice.

## Outcome and scope

A provider acquires bounded data from a file, native attributes or surrounding
context, and registers contributions to Properties, columns and View. Each
presentation follows definition → applicability → request → prepared result →
rendering. It retains its own selection policy and lifecycle.

The first delivery must demonstrate two cases:

1. An enabled user package reads a small local sensor file, contributes typed
   duration/distance fields, a Properties section, a column and its own canvas
   graph in View, without rebuilding fyle.
2. A built-in Git provider supplies the existing status column and a new
   Properties section from repository context, independent of file format.

Prove these before migrating the remaining built-ins. Existing previews,
column IDs/defaults, inspector behavior and Tools plugins remain compatible.
No crate split, Git library replacement, general dependency graph, universal
fact store, new directory detection, related-file actions or remote inspection
opt-in belongs to this delivery.

## Architecture to follow

- Use `data_providers.rs` plus `data_providers/` for pure contribution contracts
  and provider-owned acquisition/adapters. This name distinguishes them from
  remote storage providers. Keep low-level parsing in `formats/`.
- Register a provider's contributions together. Share descriptor vocabulary,
  identity, availability, provenance and result-state rules; use typed
  Properties, column and viewer outputs. Do not erase them into one universal
  reader/result interface or require a registry for each concept.
- Properties combines applicable sections; panels request selected visible
  columns; View selects one viewer using deterministic ordering. Keep Tools'
  discovery/planning/execution contract separate.
- Preserve native Rust models internally. Validate dynamic values at extension
  registration and worker boundaries. Introduce only the value kinds and units
  required by the first examples, rather than converting every reader to JSON.
- Reuse `Location`, `Entry`, `PropertiesSession`, `preview::session` and the
  metadata queue's policies. Controllers own GPUI tasks/resources; independent
  models own requests, applicability, invalidation and stale-result acceptance.
- Distinguish pending/ready/absent/unavailable/failed from completeness. A ready
  result may be truncated or a lower bound. A field's identity includes its
  subject, producer and source/context revision, not just a string key.
- Properties for remote/archive items stays listing-only, including after View
  materializes the same item. Source identity, readable access and permission
  to use that access remain separate. V1 user readers accept local sources only.

The current [architecture record](../architecture-plan.md) describes separate
registries and a fixed column catalog. The proposed common contribution
contracts and dynamic IDs supersede those parts only when their slices land;
they do not require merging feature execution or changing Tools.

## Decision checkpoints

These are recommendations to confirm at the indicated checkpoint, not implicit
approval from adding this plan. Other implementation details can be settled in
the corresponding model and tests.

| Checkpoint | Recommended choice | Needed before |
| --- | --- | --- |
| First feature scope | Sensor package plus contextual Git, with a new Git Properties section | Slice 1 |
| Extension execution | Supervised Lua helper in the same executable, dispatched before GUI/config startup; reuse vendored `mlua` | Slice 4 |
| Loading and trust | Explicit enabled package IDs in user config; packages only under `<config dir>/fyle/extensions/`; no project auto-loading; enabling permits automatic declared read-time work | Slice 4 |
| Initial renderer | Static layout, selectable text, typed table and bounded path canvas; no worker-defined native widgets or arbitrary callbacks | Slice 5 |
| Metadata sorting | First delivery displays extension columns but does not sort them; confirm whether this narrower milestone is acceptable | Slice 3; otherwise design sorting before implementing it |

The synthesis's sortable-column example remains a follow-up acceptance goal if
the display-only milestone is accepted. Sorting requires a separate explicit
choice about whole-list acquisition cost, row limits and cancellation; never
sort only whichever visible rows happen to have loaded. Existing sortable
columns preserve their behavior.

General recognition/ambiguous text, related-member operations, sidecar
precedence, arbitrary renderer interaction, external decoder grants and
nonlocal inspection remain deferred owner decisions. They do not block the
local sensor/Git proof.

## Delivery sequence

Work in order. Each numbered slice has a reviewable result; split model and UI
wiring into separate commits if needed. Keep each commit about one feature.
Update the completion record and README as behavior lands.

### 1. Define the smallest contribution contract

Touch: new `src/data_providers.rs` and focused files under
`src/data_providers/`; register the module in `src/main.rs`.

- Define namespaced provider/contribution IDs, field descriptors, typed
  quantities, captured applicability context, demand limits and result states.
  Reserve built-in namespaces; preserve existing public column spellings.
- Keep source and context revisions distinct from UI request generations.
  Unknown revision disables cross-request reuse; it is not a stable cache key.
- Define separate typed contribution descriptors for Properties, columns and
  viewers, using the same stage names. Applicability consumes captured data;
  filesystem/context discovery runs in background execution.
- Use in-memory sensor and Git fixtures to exercise the interface before
  committing to a generic trait. One provider can omit any presentation.

Done when model tests cover duplicate IDs, invalid/nonfinite quantities,
independent failure/completeness, link-object versus target identity and
rejection of stale source/context results. Contracts import neither GPUI nor
concrete remote providers, and do not depend on presentation views.

### 2. Adapt one existing reader through Properties and View

Touch: new `src/data_providers/geo.rs`, `src/properties/geo.rs`,
`src/preview/geo.rs`, their registry wiring and `src/formats/geo.rs` as needed.

- Register the GPX/FIT provider's Properties and viewer contributions together;
  adapt the existing typed `GeoData` reader and existing rendering.
- Project duration/distance as numeric quantities with explicit units;
  presentation adapters retain current text formatting. Capture whether a
  value was recorded or inferred where needed, rather than inventing provenance.
- Preserve 16 MiB/100,000-point bounds, segment gaps, FIT total precedence,
  cancellation, no-GPS behavior and the existing preview fallback order.
- Keep feature scheduling independent. Sharing a reader does not yet imply a
  shared cache or a single task across Properties and View.

Done when the existing fixture outputs and live Properties/View remain
equivalent, including truncated data and failures. This is the first production
test of the contract; revise slice 1 if adapting it needs special-case dispatch.

### 3. Make column identity and projection extensible

Touch: `src/columns.rs`, `src/settings.rs`,
`src/settings/persistence.rs`, `src/commander/appearance.rs`,
`src/panel.rs`, `src/panel/table_view.rs`, `src/panel/metadata/` and
relevant pure sorting models if sorting is approved.

- Separate persisted `ColumnId` from built-in `ColumnKind` implementation
  details. Register built-in descriptors in their current order with unchanged
  widths, labels, default IDs and the `git` alias for `git.status`.
- Preserve unknown saved IDs across load/save. Display them as unavailable in
  the Columns dialog; leave their cells blank until the package is available.
  Name stays first, mandatory and unique. Save with `toml_edit`.
- Add typed optional cell values and a provider request path without rewriting
  every built-in renderer. Keep specialized native cells behind adapters.
- Preserve visible-row demand and queue/cache/concurrency bounds. Rendering
  reads prepared state only. Exercise a fake sensor contribution; do not add a
  permanent built-in sensor feature just to test the catalog.

Done when old configs and defaults pass, unknown IDs survive changing another
column, disabling/re-enabling a fake provider restores its selection, and rapid
navigation rejects old cells. Check the Columns dialog and both panel views
with scratch config. New columns follow the approved sorting checkpoint.

### 4. Validate packages and supervise a local reader worker

Touch: new `src/extensions.rs` and `src/extensions/` for manifest validation,
protocol, Lua worker and supervisor; early dispatch in `src/main.rs`;
settings schema/persistence and `config.example.toml` for enabled packages.
Read [Tools contracts](../tools.md) before touching plugin-related wiring.

- Introduce one versioned manifest with package/provider IDs, field definitions,
  contributions and API/representation versions. Reject duplicate ownership,
  invalid references and incompatible major versions before registration.
  V1 rejects built-in overrides and external executables; no hot reload yet.
- Use the existing `serde`, `serde_json` and `mlua` dependencies. Keep source
  paths native in the host; pass opaque source handles and bounded bytes/ranges
  through the protocol. A display name is never a path to reopen.
- Start one supervised request worker with bounded protocol frames, output,
  Lua allocation/instruction budget and wall deadline. Broker reads from the
  declared local source only. Disable ambient file/process/network/module APIs.
  Static manifests may load in the host; all executable package evaluation,
  registration, applicability, reader and renderer code runs under these worker
  limits. Loading a Lua module must not bypass supervision or block startup.
- Record concrete byte, node, queue, worker and time limits as named constants
  before enabling packages. Account for cumulative source reads, not just the
  size of each read. Test each limit at and beyond its boundary.
- Cancellation kills and reaps the helper; its slot stays occupied until exit.
  Crashes, invalid output and deadlines become independent contribution errors.
  Make startup dispatch headless, without creating a GPUI application.

Done when TempDir fixtures exercise good/bad manifests, disabled packages,
malformed/oversized frames, infinite Lua loops, worker crashes, native non-UTF-8
source access and cancellation cleanup. Integration tests use the fyle helper
itself or controlled fixtures, without network or optional external programs.
Do not claim hostile-code OS isolation; document the trusted-package contract.

### 5. Deliver the user-defined renderer and complete the sensor proof

Touch: new pure `src/preview_scene.rs` and thin `src/preview_scene_view.rs`,
`src/preview.rs`, `src/preview_view.rs`, extension adapters and an example
package under `examples/extensions/sensor/`.

- Add a versioned extension preview representation alongside existing `Content`
  variants. Workers prepare a validated display tree; native code owns GPUI
  entities, selection, theme mapping and cleanup.
- Implement only the approved primitives, with bounded nesting, text, rows,
  geometry and finite coordinates. No filesystem access or Lua callbacks in
  render. Defer raster leases and interaction intents until a consumer needs them.
- Supply a tiny documented sensor format and valid/truncated/malformed fixtures.
  The package parses bounded samples and produces duration/distance facts, a
  Properties section, a selectable column and a custom graph/layout.
- Make viewer placement explicit: extension matches precede generic text/binary
  fallback; built-in specialized ordering stays unchanged. Invalid extension
  output gives a useful diagnostic without discarding baseline Properties.
  Order competing user viewers by package ID and then contribution ID in V1;
  neither filesystem enumeration nor worker completion order decides the winner.

Done when editing only the package changes its graph/layout and all three
contributions work in the running app. Test invalid geometry/depth, rapid
selection changes, package disable/restart, stale scenes and request cleanup.
If the grammar cannot express this real example, revise it before expanding it.

### 6. Prove contextual applicability with Git

Touch: new `src/data_providers/git.rs`, `src/git_status.rs`,
`src/panel.rs`/`table_view.rs`, Properties registration and view formatting.

- Capture repository root, query scope and repository-relative selected path
  in background work. Keep the existing Git executable and status priorities,
  including ignored/renamed/nested-directory behavior.
- Prepare one bounded status snapshot per repository/query scope and refresh
  epoch for column rows and the new Git Properties section. Index/worktree
  changes can invalidate it even when the selected file's metadata is unchanged.
  Initially use explicit refresh/listing reload and operation-triggered refresh;
  do not promise continuous watching.
- Replace Git's panel-specific task lifecycle with a tested request model and
  generation checks. Bound command output/time and reap cancelled processes.
  Preserve native path bytes at the process/parser seam. Ensure local-only
  applicability excludes archive/remote compatibility paths.
- Share pending work only for identical context/scope/epoch. Cancelling one
  subscriber must not cancel another. Keep this repository-specific initially.

Done when deterministic captured-output/fake-runner tests cover directory
aggregation, stale results, scope differences, index changes and two consumers
with one cancellation. Check the Git column and Properties on a scratch local
repository. Git is not required by the automated model tests.

### 7. Review the proof before broad migration

Compare the sensor and Git implementations. Both must register contributions
without edits to every presentation's dispatch logic, and must explain source,
limits, applicability and failure using the same vocabulary. Native providers
may retain typed internal models; extensions must use the validated seam.

Record what actually needed sharing and remove experimental abstractions that
neither example uses. Keep scheduling separate except for demonstrated costly
work reuse. This checkpoint accepts, revises or stops the design before further
format migration; passing tests alone does not establish a useful authoring API.

### 8. Migrate built-ins one provider at a time

After slice 7, use separate behavior-preserving commits for media, Parquet,
EXIF/RAW and then other preview families as justified by real consumers.
Each commit names the old adapter removed and the behavior/limits preserved.

- Media first: duration already serves columns, Properties and Preview. Preserve
  timed bounded probing; video frame extraction remains a distinct demand.
- Parquet: keep footer/schema acquisition separate from row samples; unsupported
  sample types must not erase readable schema. Samples never feed conversion.
- EXIF/RAW: compare `properties/exif_reader.rs` against `formats/exif.rs` and
  panel adapters before consolidating. Keep Properties metadata-only behavior;
  embedded-image decoding/materialization is a separate request.
- PDF and other formats: move process/materialization responsibilities only when
  their contribution needs it. Preserve page/sheet navigation and sample caps.

Extract shared acquisition only when two real consumers need identical bounded
work. Keys include subject, source/context revision, producer/version, demand,
limits and access policy. A smaller truncated result cannot satisfy a larger
request. Give shared caches byte/disk bounds and explicit cleanup ownership.

## Verification and completion

Before each code commit run `cargo fmt --check` and
`cargo clippy --all-targets -- -D warnings`. Run focused model tests during each
slice and `cargo test` before finishing its feature. Tests use TempDir and
controlled readers/processes; no network, ffmpeg or Poppler requirement.
No new dependency is planned; justify any later addition explicitly.

Check affected UI in the running app with scratch `XDG_CONFIG_HOME` and
`XDG_DATA_HOME`: existing F3/Properties, column selection/persistence, graph,
Git context, fast navigation/tab switching and return to built-in fallback.
Keep existing Windows code compiling without extending platform support.

For every completed feature, update README's behavior/module map, move its TODO
item to Done, mark its slice below, and commit only that feature's changes.
Never push or open a PR without a request.

## Completion record

All implementation slices are pending. First next action: confirm the five
decision checkpoints, then implement slice 1. This documentation change does
not approve deferred TODO features or start an implementation.
