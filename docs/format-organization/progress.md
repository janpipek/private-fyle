# Provider migration progress and handoff

Read this file first when joining the work, then the
[implementation plan](implementation-plan.md) and the repository's
[AGENTS.md](../../AGENTS.md). The [synthesis](synthesis.md) explains the design
motivation. Update this log in every implementation commit so the working state
can be reconstructed without the chat.

## Current state

- The owner authorized starting the plan after the planning commit `5da8dee`.
- Completed: slices 1–7, including the sensor Properties/column/viewer proof,
  contextual Git column/Properties proof, and verified slice 7 cleanup; slice 8
  Media, Parquet and EXIF/RAW are also complete under the owner's authorization
  to finish the full approved migration. The cross-presentation closeout audit is
  recorded at the end of this log and in the implementation plan.
- Slice 4a static package validation/loading is committed as `c2913dc`, and the
  protocol/broker/queue foundation as `cf11f3b`. Standalone worker execution and
  Linux supervision are committed as `a2f7021`. Settings/catalog integration
  (4c) is complete and verified as `0db1dbd`; slice 5 is complete.
  Its pure scene model is committed as `0e179cd` (652 tests), and typed
  contribution execution as `ca1bc84` (665 tests). Final app integration passes
  681 tests, fmt/clippy/build and the live sensor proof (commit `b44de7d`).
  Contextual Git (slice 6) is also complete: 706 tests, fmt/clippy/build and
  final-build live checks pass; see [git-plan.md](git-plan.md) and the latest log.
  Read [worker-plan.md](worker-plan.md) for execution and
  [renderer-plan.md](renderer-plan.md) for current slice 5 contracts.
- Current roles, requested by the owner: Sol plans; Luna does the actual
  implementation; consult Astra only for a new abstraction, then offer it to
  the owner for approval. The coordinator keeps review and handoff records
  accurate. Historical entries below retain their original roles and findings.
- Existing GPX/FIT presentation and scheduling are preserved by the shared
  adapter. The owner approved the proposed Lua worker and explicit package
  enablement. Static layout, selectable text, typed tables and a bounded path
  canvas were approved on 2026-10-17; native code owns interaction.
  The owner approved display-only extension columns first; new
  metadata sorting remains a separate design task.

## How to join

1. Check `git status --short` and the completion record below before editing.
   Others may be working in the same tree; stage only your changes.
2. Pick a task whose dependencies are complete in the implementation plan.
   Record ownership and file boundaries here before working in parallel.
3. Read the contracts and their tests before adding adapters. Format parsing
   stays under `formats/`; feature scheduling and presentation stay with their
   existing owners until the corresponding slice changes them.
4. Record decisions, changed files, verification results, limitations and the
   exact next task in this log. Update README/TODO and commit each completed
   feature after the required checks. Do not push or open a PR.

## Step log

### 2026-10-16 — Start slice 1

- Read the plan, TODO, README architecture, architecture record and existing
  identity, Properties, preview-session and column models.
- Confirmed the clean tracked baseline is planning commit `5da8dee`; only the
  unrelated `title.png` is untracked.
- Assigned architecture review to Astra and implementation to Luna. Luna owns
  `src/data_providers.rs`, `src/data_providers/` and its module declaration in
  `src/main.rs`; the coordinator owns documentation and final validation.
- Bound the initial work to pure models and in-memory sensor/Git examples.
  No extension runtime, dynamic-column UI, filesystem probes or shared cache
  will be added in this slice.
- Architecture decisions and verification are pending; append their outcomes
  below before marking the slice complete.

### 2026-10-16 — Set the slice 1 contract boundaries

This records the initial proposal. Slice 7 later removed the generic
observation/request-reuse portion; see the review and latest decision below.

Astra supplied these decisions; Luna is implementing them:

- A provider definition bundles fields and separate Properties, column and
  viewer descriptors. A pure validation function checks a collection before it
  is installed; this is not a shared execution registry or reader trait.
- Provider IDs use `builtin.<name>` or `extension.<package>.<provider>`.
  Registration origin is supplied by the host, not trusted from a package.
  Contributions/fields are owner-qualified. Persisted built-in column IDs keep
  their existing spelling separately from the contribution's qualified ID.
- Initially support text and finite nonnegative duration/distance quantities
  with seconds/metres units. Validate both type and unit against field
  definitions. Do not replace native format models with dynamic values.
- Model selected items, symlink objects and symlink targets as distinct
  subjects, retaining original `Location`s and native path bytes.
- Result availability is independent from completeness: a ready value can be
  complete, truncated or a lower bound. One failed contribution does not remove
  a sibling's ready result. Observations retain producer and provenance.
- Request acceptance checks identity, source/context revisions, demand and UI
  generation. Reuse ignores generation only when revisions are known and all
  other demand/identity fields agree. An unknown revision permits delivery to
  its original request but forbids reuse across requests.
- Applicability consumes captured listing/format/context evidence only.
  Initial demand limits describe input bytes and item counts; the model does
  not claim to enforce execution budgets.
- Use a public pure module, following `properties`' existing module precedent,
  so its contracts can be introduced before production adapters without
  suppressing dead-code warnings. No additional dependencies or GUI changes.

Planned evidence: in-memory sensor (all three presentations) and Git (Properties
and column, context-based applicability) examples, plus boundary and stale-work
tests. Architecture review will check the implemented contract before commit.

### 2026-10-16 — Review the initial contract implementation

- Luna added the initial pure module, ID types and main-module declaration.
  These files are under review; their presence is not slice completion.
- The coordinator requested private validated quantity storage, actual typed
  sensor/Git projection examples rather than constructor-only assertions,
  portable temporary roots, native-byte coverage and stronger same-generation
  stale-result tests.
- Astra is reviewing ownership/collision validation, descriptor applicability
  and the request identity rules before the first verification run.
- Prepared the slice 2 handoff: Properties keeps `route_activity` and current
  decimal strings; View keeps its summary formatting and raster/cache behavior.
  GPX-derived values and FIT recorded session totals need provenance captured
  at the parser assignment. Truncated coordinates must not mark FIT's complete
  recorded totals as incomplete. No production adapter is implemented yet.

### 2026-10-16 — Complete and verify slice 1

- Implemented `src/data_providers.rs` and `src/data_providers/identity.rs`,
  exposed through a documented public module in `src/main.rs`. Namespaces have
  exact shapes, preventing overlapping provider ownership. Validation rejects
  duplicate definitions, foreign fields, undeclared bindings, malformed column
  IDs and conflicting Git alias/canonical registrations.
- Sensor contract example projects native typed input into validated
  observations, selects a column value through its field binding (including a
  rebound distance column), and bounds a typed viewer sample through its
  descriptor. Git applies through captured repository context, contributes
  Properties/column results, and does not require a viewer or format match.
- Added coverage for zero/invalid quantities and units, independent failure and
  truncation, distinct link subjects, native non-UTF-8 identity, and generation,
  source, context identity/revision and demand changes. Unknown revisions accept
  their current completion but cannot authorize reuse.
- Astra reviewed the corrected source and approved slice 1 with no remaining
  blockers. Luna implemented all Rust changes; the coordinator reviewed and
  ran final repository checks.
- Verification: `cargo fmt --check` passed;
  `cargo clippy --all-targets -- -D warnings` passed; `cargo test` passed
  **565 tests, zero failures/ignored**. Luna's focused
  `cargo test data_providers::tests -- --nocapture` passed 8 tests; the full run
  also covers the ID module test. `git diff --check` passed.
- Updated README's module map, TODO and the architecture record. No dependency,
  setting, command, production reader or GUI behavior changed. No live UI check
  was needed for this pure-model slice. No Windows build was run; the model
  introduces no platform-specific production code.

## Slice 2 handoff

Start from the commit titled `Define provider contribution contracts` and read
`src/data_providers.rs` plus its inline examples. Run `cargo test data_providers`
to check both contract and ID tests. Existing presentation registries still use
their original paths; the model examples are not installed user extensions.

1. Assign the GPX/FIT adapter files before editing: new
   `src/data_providers/geo.rs`; existing `src/properties/geo.rs`,
   `src/preview/geo.rs`, their registry wiring, and `src/formats/geo.rs` plus
   `geo/gpx.rs` and `geo/fit.rs` for provenance. Keep documentation with the
   coordinator when delegating implementation.
2. Construct one provider-owned Properties/viewer contribution bundle. Make
   both existing adapters consume its definitions/applicability and native
   prepared data; an unused descriptor beside the old paths does not complete
   this slice. Keep native readers independent from presentation modules.
3. Capture numeric provenance at the parser branch: GPX duration/distance are
   inferred; FIT duration uses recorded session totals, while FIT distance can
   be recorded totals or inferred coordinates. Do not guess from the format.
4. Preserve existing fixed read limits and UI formatting. FIT session totals
   can be complete even with truncated geometry; GPX-derived summaries become
   truncated if traversal stopped. Invalid numeric projection must not discard
   unrelated readable facts or a route outline. Preserve absence versus zero.
5. Use honest unknown revisions until a caller supplies captured freshness;
   never manufacture revision tokens from UI generations. Keep Properties'
   cancellation and listing-only nonlocal boundary, and View's current
   materialization/outline behavior. Do not add a shared acquisition cache.
6. Validate output equivalence, session-total precedence, no-GPS data, segment
   gaps, truncation, cancellation and parse failures. Run normal repository
   checks and a scratch-config live UI check for both inspector perspectives.
   Update this log, README/TODO and the completion record before committing.

## Slice 2 work log

### 2026-10-16 — Start the production GPX/FIT adapter

- The owner asked to continue after slice 1 commit `2ba942f`. Read this handoff,
  the plan, TODO/README and current readers/adapters before editing.
- Assigned Luna the new `data_providers/geo.rs`, its module declaration,
  `formats/geo.rs`, `formats/geo/{gpx,fit}.rs`, `properties/geo.rs`,
  `preview/geo.rs`, and minimal registry wiring in `properties.rs`/`preview.rs`.
  Astra reviews design and implementation; the coordinator owns documentation.
- Scope is behavior-preserving production use of the contribution contract.
  No new columns, extension runtime, shared cache or detection policy.
- Prepared isolated GPX/FIT, no-GPS and malformed fixtures under a temporary
  directory for before/after UI checks. All app launches use scratch config,
  data and runtime directories; the real user's settings/history stay untouched.
- Architecture decisions, implementation and verification are in progress.

### 2026-10-16 — Set the GPX/FIT adapter design

- Astra chose one immutable `GeoProvider` bundle with typed Properties/viewer
  handles, installed at the existing registry positions. Both adapters must
  consume the bundle's applicability and fixed reader limits, then render
  prepared native data using their current formatting and outline functions.
- Keep parsing in `formats/geo`; represent duration/distance with their native
  value and recorded/inferred origin at the assignment branch. The provider
  validates each numeric result independently into a quantity. A bad metric
  cannot fail the entire readable activity or suppress its outline.
- Prepared numeric completeness is independent of route truncation: FIT
  recorded session totals remain complete, while inferred values from truncated
  coordinates are truncated. Missing values remain absent, including no-GPS
  files with no usable duration/distance.
- Do not fabricate an original `Location`, source revision or observation from
  a materialized preview path. These adapters retain their current lifecycle
  owners; the native prepared result need not pretend to be a cacheable
  source-bound observation.
- Captured before-state screenshots for GPX, FIT, no-GPS and malformed files in
  both inspector perspectives under `/tmp/fyle-slice2-ui-6xgcdhih`. Bare Xvfb
  reproduced the documented F3 input issue, so UI verification uses the View
  menu and inspector tabs. No keybinding change is part of this slice.

### 2026-10-16 — Review the first adapter draft

- Luna produced the provider, native provenance changes, both adapter updates
  and unchanged-order registry wiring. The coordinator and Astra reviewed this
  source before repository verification.
- Astra confirmed fixed demand descriptors may reference the exact existing
  reader constants; this slice must not pretend to enforce arbitrary runtime
  limits or introduce a general acquisition layer.
- Required stable field-ID binding instead of field-vector positions, with a
  regression that reorders definitions without swapping duration/distance units.
- Requested production-handle read tests for cancellation, malformed/oversized
  inputs, no-GPS absence and truncation; valid formatting and independently bad
  metrics must also be covered. Corrected an import left behind by adapter
  extraction. Tests and final verification remain in progress.
- Added and captured the existing 100,001-point GPX truncation case in both
  perspectives. The sorting milestone question for slice 3 is pending with the
  owner; it does not block this slice.

### 2026-10-16 — Set the slice 3 sorting scope

- The owner answered the queued checkpoint: **“Display-only first; design
  sorting separately.”** Dynamic provider columns will be unsortable in the
  first milestone; the four currently sortable built-ins retain their behavior.
- Astra is preparing slice 3 architecture while the coordinator finishes
  slice 2 UI validation/commit. Luna is surveying the column call sites read-only;
  no slice 3 code will be mixed into the GPX/FIT commit.

### 2026-10-16 — Complete and verify slice 2

- Added `data_providers/geo.rs`, provider-owned typed handles and one immutable
  validated bundle. The existing registries install those adapters at their
  old positions. Both use shared applicability and native acquisition; the
  definitions' limits come directly from the unchanged reader constants.
- Native GPX/FIT duration and distance now carry their parser-assigned origin
  alongside the magnitude. Provider preparation resolves stable field IDs,
  preserves independent completeness/failure, and leaves native geometry and
  unrelated facts available. Properties and View retain their old formatting.
- Astra approved the final architecture/source after the field-ID fix and
  meaningful boundary/projection tests. No additional dependencies, commands,
  settings, cache, scheduler or nonlocal inspection permissions were added.
- Luna ran `cargo fmt` and `cargo test geo -- --nocapture`: **16 passed**.
  The coordinator ran `cargo fmt --check`,
  `cargo clippy --all-targets -- -D warnings`, `cargo test --quiet` and
  `cargo build`: **all passed; 573 tests, zero failures/ignored**.
- Launched the rebuilt app with the same scratch config/data/runtime and inputs
  as the baseline. Checked GPX, FIT, no-GPS, malformed and 100,001-point GPX
  files in View and Properties, then rapid cursor/tab changes ending at GPX.
  Visually inspected the outputs. ImageMagick `compare -metric AE` over the
  inspector content (`360x650+870+100`) reported **zero changed pixels** in all
  ten before/after comparisons and the rapid-navigation final-state comparison.
  Logs were empty. Screenshots remain in `/tmp/fyle-slice2-ui-6xgcdhih`.
- UI checks used menus/tabs because the documented bare-Xvfb F-key issue still
  occurs. They do not claim to validate F3 keyboard delivery. No Windows build
  was run; this slice adds no platform-specific production code.
- README, TODO and architecture/implementation records now describe the shared
  adapter. Next feature is the approved display-only dynamic-column milestone.

## Slice 3 work log

### 2026-10-16 — Resume dynamic columns with Sol

- Resumed from clean tracked commit `6166617`; no partial slice 3 Rust changes
  survived the earlier agent usage-limit interruption. `title.png` remains
  unrelated and untracked.
- The owner requested Sol instead of Astra. Sol now owns architecture and
  review; Luna owns all slice 3 Rust changes. The coordinator owns documentation,
  final checks, UI validation and commits. Historical Astra reviews above still
  describe the first two slices.
- Sol reviewed and confirmed the display-only design: persisted `ColumnId`s
  retain unknown IDs and canonicalize `git`; the builtin enum remains native
  rendering/sorting dispatch. One immutable shared catalog supplies the Columns
  dialog, tables and background metadata reader.
- Registered native provider columns use validated definitions and typed
  background callbacks. Reuse the existing metadata queue and its 256 cache /
  512 pending / 4 active bounds, preserving stale-worker occupancy. Missing,
  failed and unavailable values count as completed; readers cannot return a
  permanently pending cell.
- Available provider columns and saved-but-unavailable IDs are unsortable.
  The four existing sortable built-ins keep their exact behavior. Only Name is
  mandatory; the owner can deliberately uncheck an unavailable saved column.
- No package discovery, hot reload, permanent sensor provider, new dependencies
  or synthesized format/repository evidence belongs to this slice. Tests will
  supply a fake provider to prove the native seam and queue path.
- Implementation files: `columns.rs` / `columns/`, settings and persistence,
  Commander construction/appearance, panel/table and metadata model/reader,
  plus callers affected by changed signatures. Source review and checks pending.

### 2026-10-16 — Review the initial column/catalog wiring

- Luna connected saved IDs and the shared catalog through Commander, panels,
  table headers/cells and metadata workers. The coordinator reviewed the first
  draft while Sol checked registration and queue semantics.
- Requested atomic registration that rejects real built-in ID collisions and
  duplicate provider bundles while allowing multiple columns to bind one
  declared field. Collision tests must reach the catalog check, not fail earlier
  for an invalid extension namespace.
- Found that legacy link-metadata requests could replace a pending provider
  request's captured row name/directory flag with default values. Required
  preserving known row facts during coalescing and a regression test.
- Requested raw saved IDs plus an unavailable explanation in the dialog, owned
  GPUI header strings, portable test paths, and a small row-data value instead
  of adding long argument lists or lint suppressions.
- Preserve one ID normalization/toggle path rather than retaining obsolete
  parallel helpers. Sol and Luna are resolving these points before verification.

### 2026-10-16 — Finish wiring and review acceptance coverage

- Luna reports `cargo check --tests` passes after ID-based settings, catalog,
  table and metadata signatures were connected. This is an intermediate compile
  check; final Clippy/tests/UI checks are not complete yet.
- Optional captured row facts now preserve known facts when legacy metadata
  requests coalesce. Workers merge only requested provider fields into the
  current generation's cache; render paths only inspect descriptors/cache and
  queue work, never execute provider readers.
- Sol requested explicit guards against built-in overrides even in an empty
  catalog and duplicate provider bundles, plus tests using valid declarations
  that reach those guards. Also required shared-field columns, unavailable
  selection recovery, stale provider results and absent/error no-retry coverage.
- The initial native callback returns a complete value or absence through
  `io::Result<Option<Value>>`; it does not yet express partial/lower-bound cell
  completeness. Native readers must honor their declared demand and return an
  error for incomplete acquisition. Before bounded user-package cells ship,
  extend this seam to preserve completeness rather than marking partial values
  complete. Catalog demand equality is validated; enforcement is the native
  reader's responsibility until supervised workers are implemented.

### 2026-10-16 — Set the slice 4 execution and enablement scope

- The owner answered the queued checkpoint: **“Use the proposed Lua worker and
  explicit enablement.”** Use a supervised helper, load packages only from the
  user's fyle config directory, and require explicit enabled package IDs before
  automatic read-time execution. Existing Tools plugins retain their behavior.
- This settles the slice 4 runtime/trust checkpoints. Slice 3 source and UI
  verification still come first; no worker code is mixed into its commit.

### 2026-10-16 — Complete and verify slice 3

- `ColumnId` now owns saved identity and canonicalizes `git` to `git.status`.
  Settings/persistence, Commander and panels carry IDs while `ColumnKind`
  retains all existing native rendering, defaults and sorting. One immutable
  `Arc<Catalog>` supplies descriptors to dialog, table and background reader.
- The catalog validates provider bundles atomically, protects built-in IDs
  even in an empty catalog, rejects duplicate providers and supports several
  columns bound to one field. Native callbacks run only through the metadata
  worker, with declared demand and type/unit validation. Production currently
  registers built-ins only; no fake provider or package runtime was installed.
- The existing queue now caches typed provider cells by listing generation and
  source path. Tests cover actual queue→reader→completion, absent/I/O-error/
  wrong-unit no-retry behavior, captured facts, stale completion/slot release,
  native bounds, shared fields and provider removal/re-registration. Legacy
  metadata merges preserve captured row facts and only requested fields merge.
- Sol approved the final source/tests. Final coordinator checks:
  `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, and
  `cargo test --quiet` all passed: **579 tests, zero failures/ignored**.
  `cargo build` also passed; the last changes afterward were test-only.
- Live scratch-config checks confirmed: default table content is pixel-identical
  to the prior build; existing Columns groups/labels render correctly; a saved
  unavailable ID shows blank cells and its raw ID/reason in the dialog;
  toggling Created preserves that ID and config comments; list/icon switching
  and restart preserve selections; clicking the unavailable header does not
  reorder rows, while Size still sorts numerically; explicitly unchecking the
  unavailable label removes only that ID from both panels and settings.
- Screenshots/logs remain under `/tmp/fyle-slice2-ui-6xgcdhih` with
  `columns-*` filenames. The default table comparison used
  `580x215+51+96` and reported zero changed pixels. A dialog baseline capture
  missed the open dialog, so no pixel-equivalence claim is made for the dialog;
  labels/layout were checked visually and all 20 descriptors have compatibility
  tests. The unavailable-header click changed selection shading but preserved
  row order. App logs were empty. The scratch app/Xvfb remain available for the
  next slice; their current PIDs are in the scratch `state.json`.
- Updated README, example-config commentary and architecture/TODO/plan records.
  No dependency, new sorting mode, package execution, nonlocal column reading
  or Windows-specific behavior was added. No Windows build was run.

## Slice 4 handoff

Read the owner-approved execution/enablement decision above and the plan before
editing. Sol has supplied the protocol/bounds architecture to Luna. Implement
static validation before execution, then a headless helper and supervisor, then
connect explicitly enabled packages to the column worker path. Keep the worker
changes separate from the completed column commit.

- Dispatch the reserved helper argument before settings/config/GUI startup.
  Leave `tools/lua.rs` and its existing plugin contract untouched.
- Load only explicitly enabled IDs beneath the canonical config extensions
  root. Enforce package-directory containment as well as manifest/script
  containment, so a symlinked package cannot escape the root. Host loading is
  static; all Lua evaluation/applicability/acquisition runs supervised.
- Retain the opened regular source file and native identity in the host; broker
  bounded reads by opaque handles, never worker-supplied paths. Reject special
  files/directories. Use capped frames/cumulative reads/output, instruction and
  allocation limits, deadlines and kill/reap/drain ownership. One active worker
  globally and a bounded pending model are the initial architecture.
- Extend the native column result seam to preserve completeness before exposing
  bounded package output. Do not silently label a partial result complete.
- Use existing dependencies. New renderer grammar and its host view remain
  slice 5, with the renderer scope decision still pending. Each implementation
  commit needs model tests, repository checks, updated handoff and relevant UI
  verification.

## Slice 4 work log

### 2026-10-16 — Start with static package validation

- Column milestone committed as `85fa3d0`. The tracked tree was clean before
  assigning the next work; `title.png` remains unrelated.
- Split slice 4 into reviewable features: **4a** bounded static manifest/package
  loading and validation; **4b** headless helper/protocol/supervision;
  **4c** explicit enablement and catalog integration with completeness-aware
  results. Full slice 4 is not complete until those pieces are connected.
- Assigned Luna the new extensions module and focused manifest/loading files
  for 4a. Sol settles schema details and reviews. The coordinator owns this log,
  README/TODO/plan changes, final verification and commits.
- 4a must not evaluate Lua, spawn helpers, register columns or change Tools.
  Disabled packages are not scanned. Tests use TempDir for valid/invalid
  versions and declarations, path containment, file bounds, independent package
  errors and nonregular files. Settings enablement is connected when execution
  is available, rather than presenting a nonfunctional runtime option.
- Sol's execution design is recorded in the handoff above. Inline helper-test
  strategy must be resolved before 4b so tests exercise the real worker loop
  without depending on a prebuilt application binary or optional programs.
- Stopped the owned scratch app and Xvfb after column UI verification. Artifacts
  remain in the recorded scratch directory; `state.json` marks those processes
  stopped. Future UI checks must launch fresh owned processes.
- Sol's first review requested a single fully validated manifest before script
  access, structured diagnostics retaining native paths and original I/O error
  causes, and 256-byte limits on all declared IDs. Luna is applying these before
  final verification. Static loading keeps script text unevaluated, including
  invalid Lua; enabled neighbors fail independently.
- Sol is recording the concrete helper protocol, Lua callback API and process
  test/cleanup strategy in a separate worker handoff for 4b. The initial Init
  write must also be supervised: a child that never reads its pipe must not
  block the parent's deadline/cancellation loop.
- Refined 4b transport: on Linux, the supervisor owns nonblocking child pipes
  in one event loop covering Init writes, framed output, diagnostics, deadline,
  cancellation and child exit. This avoids unbounded pipe-writer or drainer
  joins. Other platforms keep compiling and return `Unsupported` from this
  new supervisor; existing Tools behavior remains unchanged. The worker loop
  itself uses generic blocking `Read`/`Write` for direct process tests.

### 2026-10-16 — Verify static package loading

- Added `src/extensions.rs`, `extensions/manifest.rs` and
  `extensions/loading.rs`, exposed from `main.rs`. Strict version-1 TOML
  declarations convert to the existing provider contracts; no Lua is evaluated.
  Full declaration validation precedes entry-source access.
- Explicit IDs select packages without directory discovery. Canonical
  containment covers package directories, manifests and entry scripts; bounded
  regular-file reads reject excess bytes and invalid UTF-8. Linux nonblocking
  open avoids FIFO hangs. Independent errors retain package ID, native path,
  I/O category and source error. Disabled packages are untouched.
- Sol approved the final source. All **20 extension tests** passed, including
  count/ID/file bounds, invalid declarations, unevaluated scripts, independent
  errors, symlink escapes, native non-UTF-8 roots and FIFO rejection. Clippy
  found one needless borrow in a test; the coordinator removed it and reran
  fmt/clippy successfully. `cargo test --quiet` passed **599 tests, zero
  failures/ignored**. `git diff --check` passed.
- Added [worker-plan.md](worker-plan.md) with the concrete 4b API, protocol,
  limits, Lua results, static/dynamic applicability, nonblocking process
  transport, cleanup and inline real-worker subprocess tests. This documents
  the next work; it does not represent implemented execution.
- README, TODO and architecture/implementation records now describe the static
  boundary. No dependency, setting, startup loading, GUI behavior or Tools
  behavior changed. No live UI check is needed for this unused static loader;
  no Windows build was run.

### 2026-10-16 — Start the standalone worker boundary

- Committed 4a as `c2913dc`; tracked tree was clean before 4b work. The unrelated
  `title.png` remains untracked.
- Assigned Luna protocol/broker/queue models first, then Lua execution and Linux
  supervision/helper dispatch. Sol reviews each boundary. These may become
  separate commits within 4b so the bounded model can be reviewed before process
  wiring. No settings or catalog edits belong here.
- Asked the owner for the later slice 5 renderer-scope checkpoint while worker
  implementation proceeds. Its answer does not block 4b; do not implement the
  renderer until the scope decision arrives.
- The first 4b draft contains `extensions/protocol.rs`, `broker.rs` and
  `model.rs`: typed versioned frames and completeness-preserving result
  validation, retained-file byte/call brokerage, and FIFO admission with
  ticket-local versus generation cancellation. Sol is reviewing the APIs and
  boundary tests before any executable worker wiring.
- Initial 4b1 focused checks passed 28 tests and fmt/clippy. Sol requested
  bounded serialization before allocating a full payload, enforced envelope
  versions, explicit broker read-ID sequencing, exact-end EOF semantics and
  additional retained-file/nonregular/budget/cancellation tests. These are
  being fixed before this foundation is committed; passing the initial tests
  alone did not settle the boundary.
- Split the upcoming executable work between two Luna agents: the existing
  implementation agent owns supervisor/transport, module wiring and early
  helper dispatch; `/root/lua_worker` owns only new Lua worker/value-conversion
  files and their inline tests. The latter does not run Cargo or edit shared
  modules. This allows worker implementation while 4b1 finishes review/checks;
  its files stay out of the foundation commit. Sol reviews both halves.

### 2026-10-16 — Complete the worker foundation

- Added `extensions/protocol.rs`, `broker.rs` and `model.rs`, exposed through
  documented extension submodules. No Lua/process execution is connected yet.
- Frames use checked versions, strict typed JSON and bounded serialization
  before emitting bytes; lengths are checked before input allocation. Result
  validation preserves zero values and completeness and rejects foreign fields,
  wrong types/units, invalid quantities and oversized output text.
- Broker reads retain the selected opened regular file, exclude remote/archive
  sources, sequence read IDs and charge overlapping requests again. Limits are
  independent named constants: 64 KiB/read, 16 MiB total (or lower declaration)
  and 512 calls. Exact-end EOF, path replacement, non-UTF-8 paths, directories
  and FIFOs have regressions.
- Queue tests cover FIFO/capacity, stale pending removal, independent ticket
  cancellation, generation cancellation and shutdown. Active cancellation
  retains occupancy until matching release after future process cleanup.
- Sol approved the final source and focused regressions. All **36 extension
  tests** passed. Final coordinator checks passed: `cargo fmt --check`,
  `cargo clippy --all-targets -- -D warnings`, and `cargo test --quiet`:
  **615 tests, zero failures/ignored**. No dependency or UI behavior changed.
  No live UI check or Windows build was needed/run for this standalone layer.
- Next: connect the real Lua worker, Linux nonblocking supervisor and early
  helper dispatch per [worker-plan.md](worker-plan.md); run real subprocess
  cancellation/deadline tests and a headless production-dispatch smoke test.

### 2026-10-16 — Build executable worker and supervision

- From `cf11f3b`, two Luna agents are implementing disjoint runtime files.
  Initial drafts cover Lua library allowlisting, instruction/allocation limits,
  broker RPC and result conversion; and shared supervisor/ticket ownership,
  nonblocking Linux pipes, framing, deadlines and process cleanup.
- Early coordinator review flagged mlua's `send` callback constraints and
  borrowed-I/O lifetimes, strict Lua type conversion, separate structural and
  traversal validation, and explicit flushes before waiting for RPC replies.
  These are draft fixes, not claims that executable checks have passed.
- The process loop must continue after a terminal result until clean EOF and
  successful child exit, retaining deadline/cancellation enforcement. Never
  enter an unsupervised `wait` on a still-running child after accepting output.
  Reviewed nonblocking Init writes, retained-file ownership, last-handle
  shutdown signaling, panic-safe completion and decoder chunk boundaries.
- Work resumed after the subagents hit a service usage limit. The coordinator
  resumed final checks and reactivated Sol/Luna; the saved source and handoff
  preserved the exact stopping point. The initial integrated run passed 49
  extension tests, but final clippy exposed an unnecessary unwrap and test-helper
  unwraps; Luna corrected them without suppressions.
- Sol's integrated review found an error path where a failed `try_wait` could
  skip kill/reap; cleanup now always attempts kill before waiting on errors.
  Requested PID-based reap assertions, last-supervisor-drop cleanup coverage and
  stronger worker validation regressions before final approval.
- Lifecycle source is approved after the fixes. Tests now check direct-child
  PIDs are reaped after cancellation and shutdown, and asynchronous last-handle
  drop eventually removes/reaps the child. Tests wait for complete PID contents
  rather than just file existence, avoiding a publication race. Infinite Lua
  loops run only in supervised subprocess tests
  and must report the instruction limit; memory tests use otherwise-valid output
  so unrelated callback errors cannot produce a false pass.
- Worker schema tests start from valid output and then change ownership,
  directory facts or demand. Strict output regressions reject numeric strings,
  non-string text, metatables and extra keys. Broker-reply regression cases are
  the last focused checks before full verification.
- Settled 4c startup ownership: perform bounded static package loading/catalog
  preparation after settings and before creating the GPUI application, when no
  UI exists. Pass the immutable shared catalog into Commander. Construct one
  shared supervisor, evaluate no Lua at startup, and format structured package
  diagnostics only at the UI boundary. No catalog hot replacement is needed.
- For normal app exit in 4c, retain a supervisor handle outside the GPUI run
  closure and explicitly shut it down after the event loop returns. No UI is
  running then. Last-handle drop remains the nonblocking fallback; abrupt host
  termination/crash cleanup is outside the current acceptance guarantee.

### 2026-10-16 — Complete and verify the standalone runtime

- Added restricted `extensions/worker.rs`, shared `supervisor.rs` and Linux
  `supervisor/process.rs`, with the reserved helper dispatch as the first main
  operation. Only the selected declared callback runs; all module evaluation,
  applicability and result conversion remain in the child under allocation and
  instruction limits. Static applicability is checked in the host first.
- The host retains native source identity/file access, pumps bounded protocol
  and diagnostics through nonblocking pipes, supervises Init transfer and
  post-result exit, and kills/reaps before freeing active occupancy. Tickets
  cancel independently; normal shutdown and last-handle drop have PID checks.
- Lua schemas use strict raw access with no metatables/coercions; bad reply IDs,
  oversized bytes, malformed read errors and foreign result fields fail. Zero,
  truncated and lower-bound values retain their states. The fixed column schema
  bounds nesting structurally; no arbitrary scene/tree grammar is enabled.
- Sol approved the final source/tests. All **52 extension tests** passed.
  Coordinator verification passed: `cargo fmt --check`,
  `cargo clippy --all-targets -- -D warnings`, `cargo test --quiet`:
  **631 tests, zero failures/ignored**, and `cargo build`.
- Manual production-helper smoke checks passed without DISPLAY/WAYLAND_DISPLAY,
  with scratch XDG config/data/cache/runtime: valid zero/lower-bound output and
  denied APIs; unsupported version, oversized prefix, malformed JSON and empty
  input rejected; interactive flushed read RPC returned the expected byte-derived
  result. No config/data/cache directories were created. Results remain at
  `/tmp/fyle-worker-smoke-7ot6wl6y/results.json`; all owned helpers exited/reaped.
- No dependency or normal GUI behavior changed. New supervision is Linux-only;
  non-Linux backend reports unsupported, and no Windows build was run. This is
  trusted-package resource supervision, not hostile-code OS isolation. Automatic
  package startup/loading, settings, columns, Properties and View remain unwired.

### 2026-10-16 — Start explicit activation and column integration

- Committed standalone runtime as `a2f7021`; tracked tree was clean apart from
  unrelated `title.png` before the next task.
- Assigned `/root/lua_worker` only settings schema/persistence/example config
  and validated package-ID deserialization. The existing implementation Luna
  owns startup/catalog adapters, completeness-aware column callbacks, main and
  Commander wiring, and thin cell presentation. They coordinate Cargo; Sol
  settles/reviews interfaces; the coordinator owns docs, UI checks and commits.
- Scope is the approved config-only explicit enablement plus display-only
  columns. No new sorting, Properties sections or custom viewers enter 4c.
  Reader startup remains static; callbacks execute only on background demand.
- Startup builds one immutable catalog shared by both panels and one supervisor
  retained through normal post-GPUI shutdown. Existing Tools plugin behavior and
  unknown saved column IDs must remain intact. Ready partial values must retain
  completeness instead of being relabeled complete by the native callback seam.
- Prepared user guidance in `docs/extensions.md` with an install/enable path,
  minimal distance-column package, typed output/completeness and bounded worker
  contract. It describes the intended verified 4c behavior; do not commit it
  until integration is checked.
- Captured the pre-4c app baseline using scratch config/data/cache/runtime at
  `/tmp/fyle-extensions-ui-qogphagg`, display `:90`. The persistent harness owns
  and reaps its app/Xvfb; current PIDs/window are in `state.json`. Fixtures cover
  complete, partial, lower-bound, malformed and inapplicable files plus a nested
  folder. Screenshots and per-run logs stay in that scratch directory.
- During 4c, the owner requested a TODO entry for RPM, AppImage and DEB
  Properties. Recorded it separately as `dd8a4d6`; no parser implementation was
  added and the active integration scope stays unchanged.
- The owner then clarified Linux `attr`/xattrs and requested them in Properties.
  Verified the existing column reads names, then recorded names/values and
  bounded/binary presentation as a separate TODO commit `bd1ebf5`. No `chattr`
  filesystem-flag feature was inferred or implemented.
- Sol approved the integrated seams and real-helper adapter test. Split bounded
  pipe/frame helpers into `supervisor/process/transport.rs` so the process owner
  stays below the source-size guideline; added fragmented/coalesced/max-frame
  decoder regressions. Lower-bound formatting uses exact numeric units so
  rounding cannot strengthen a bound; extreme finite durations have a fallback.
- Initial final checks passed 642 tests, fmt/clippy/build. Live checks found
  two diagnostics defects: Lua identified its Rust callsite instead of the
  package, and initial local listing messages erased startup package errors.
  Luna named chunks from validated package IDs and added a regression; then
  added a pure Startup-only status policy, preserving real listing errors and
  later user statuses. Remote initial navigation retains its existing intent.
  Full checks are repeated after these changes.
- Live evidence so far: default table crop `580x240+51+96` is pixel-identical
  (zero changed pixels); the exact package from `docs/extensions.md` works;
  complete/partial/fractional lower-bound cells and hover diagnostics work;
  provider header clicks preserve order while native Size sorting keeps each
  value with its source; Columns and list/icon saves/restart preserve IDs,
  enablement and comments; disabling retains a checked unavailable ID and blank
  cells; re-enabling restores values. Final startup-error and active-helper
  normal-exit checks remain before commit.

### 2026-10-16 — Complete explicit activation and verify the app

- Added strict default-empty `[extensions].enabled`, preserving valid unknown
  IDs and duplicates for independent loader diagnostics. Unrelated saves retain
  IDs/comments. Startup prepares enabled config packages and one immutable
  catalog before GPUI; one shared supervisor is retained through explicit
  post-event-loop shutdown. Structured paths/I/O errors survive to the UI.
- Provider callbacks now carry full result states. Catalog validation retains
  completeness and rejects completed Pending results. Pure `columns/cell.rs`
  prepares partial/lower-bound text and hover explanations; native sorting and
  default descriptors stay intact. No Tools, Properties or viewer behavior was
  added to packages in this slice.
- Sol approved the final source/tests. Final checks passed after the UI fixes:
  `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`,
  `cargo test --quiet`: **644 tests, zero failures/ignored**, and `cargo build`.
  No dependencies were added. No Windows build or remote-account UI check ran.
- Final live scratch checks also passed: the exact documented example works;
  partial/lower-bound markers and blank error hover are visible; errors name the
  package instead of Rust code; disabled selections survive an unrelated UI save
  and regain values after re-enable; malformed enabled neighbors remain
  independent and their startup error stays visible through local initial load.
  Navigating into/out of the nested fixture replaces that error normally and
  shows the correct new-path values without stale cells.
- Normal File → Quit with an observed active helper exited successfully in
  **0.257 seconds** and the helper PID was gone before app exit. Evidence is
  `/tmp/fyle-extensions-ui-qogphagg/active-helper-quit.json`. All per-run app logs
  were empty. The harness stopped and reaped the owned app/Xvfb; `state.json`
  records `ui_stopped: true`. Screenshots/logs are retained in the same directory.
- Added the package guide and updated README, TODO and architecture/navigation
  records. The owner-requested package-format/xattr Properties TODOs remain open.
  Limitations: new worker execution is Linux-only; remote initial-navigation
  status retains its existing behavior; abrupt host termination is outside the
  normal-shutdown guarantee. Package Properties/View and renderer grammar remain
  slice 5, whose named owner checkpoint is still pending.

## Current handoff

The sensor and contextual Git proofs are complete and slice 7 accepted the
shared vocabulary with the generic reuse model removed. All owner-approved
slice 8 migrations and the cross-presentation reader audit are also complete.
Read
[renderer-plan.md](renderer-plan.md) and [git-plan.md](git-plan.md) for the
implemented boundaries, [implementation-plan.md](implementation-plan.md) and
the final audit below for current reader ownership, and
[docs/extensions.md](../extensions.md) for the package authoring API. Preserve
separate feature lifecycles and original-source access policies. Metadata
sorting, package-format Properties and xattrs remain separate open TODOs.

## Completion and next work

| Slice | State | Evidence / handoff |
| --- | --- | --- |
| 1. Contribution contracts | Complete | Astra approved; fmt/clippy and all 565 tests passed |
| 2. Existing GPX/FIT adapter | Complete | Astra approved; fmt/clippy/build and all 573 tests passed; 11 live inspector comparisons identical |
| 3. Dynamic columns | Complete | Sol approved; fmt/clippy/build and all 579 tests passed; unavailable-ID persistence/removal and native UI checked |
| 4. Packages and worker | Complete | Explicit config activation and display-only columns; Sol approved, 644 tests/fmt/clippy/build and live UI/active-helper normal-exit checks pass |
| 5. Sensor renderer proof | Complete | Sol approved; 681 tests/fmt/clippy/build, package-only graph edits, source/failure/fallback/copy and active-helper quit verified |
| 6. Contextual Git | Complete | Sol reviewed; 706 tests/fmt/clippy/build, shared column/Properties scopes, unchanged-file index refresh, archive exclusion and active-child quit verified |
| 7. Review proof | Complete | Accepted after removing unused generic reuse API; fmt, all-target Clippy, focused and full tests pass; see [review](proof-review.md) |
| 8. Remaining built-ins | Complete | Media, Parquet and EXIF/RAW migrated; cross-presentation audit found no further same-demand seam; 711 tests/fmt/clippy and scratch UI checks pass |

### 2026-10-17 — Approve and start the sensor renderer proof

- The owner answered “Yes please” to the clarified renderer scope: static
  layout, selectable text, typed tables and a bounded path canvas. Native code
  owns interaction; arbitrary Lua callbacks/widgets remain deferred. Recorded
  the decision in TODO and the implementation plan before coding.
- Resumed Sol architecture/review and Luna implementation. Sol first owns
  `renderer-plan.md`, specifying the validated model, result boundaries and
  integration sequence. Luna inspects the existing feature lifecycles and
  reports seams before edits. The coordinator owns decision/handoff records,
  final verification, UI checks and commits.
- Starting point is `0db1dbd`, 644 passing tests and no tracked changes. The
  unrelated untracked `title.png` remains untouched. Package-format and xattr
  Properties TODOs remain separate open work.

- Sol settled the first pure grammar: row/column stacks, selectable text, typed
  table cells and canvas polylines with semantic theme roles. No fills, curves,
  resources, URLs or callbacks. Validation caps depth at 8, nodes at 128, text
  at 4 KiB per string / 64 KiB total, rows at 256, cells at 2048, canvas
  dimensions at 4096 and points at 8192. Coordinates must be finite and inside
  their canvas; IDs are bounded and unique.
- Assigned `/root/lua_worker` only `src/preview_scene.rs` and focused files
  under `preview_scene/`, including inline model tests. It exposes validated
  read-only data and depends on existing semantic values, not GPUI/protocol.
  Sol owns the renderer plan; `/root/contract_implementation` waits for the
  worker-result API before touching shared runtime files.

- The coordinator drafted the sensor example format and complete/malformed/
  sample-limited fixtures under `examples/extensions/sensor/`. Samples carry
  monotonic elapsed seconds and cumulative metres; bounded partial readings
  report lower-bound totals and an explicitly partial graph. The README marks
  this as work in progress until callbacks and live integration are verified.

- Settled internal protocol v2 with separate typed Column, Properties and Viewer
  requests/results. Existing column Lua output remains source-compatible.
  Properties returns the exact declared field set with independent terminal
  states/completeness; viewers return a versioned scene with complete/truncated
  coverage. Neither typed native Properties nor preview models are erased.
- Assigned `/root/contract_implementation` protocol, worker and supervisor
  generalization only for this checkpoint. Root owns the example manifest/Lua
  and fixtures; the runtime tests should exercise that exact package, including
  its malformed and sample-truncated data. Feature/startup/UI wiring follows
  review of these independently testable models.

- Exercised the exact draft Lua column callback through the existing production
  helper: complete input returns 95 m, 140-sample input stops at sample 128
  and returns a 381 m lower bound, and non-monotonic input fails with a package
  line diagnostic. All three helpers exited successfully and were reaped.
  Results: `/tmp/fyle-sensor-parser-1dhrmj53/results.json`. This verifies the
  parser only; new Properties/scene output still awaits protocol integration.

### 2026-10-17 — Verify the pure scene model

- Added `preview_scene.rs` with strict serde DTOs and focused validation modules.
  Validated scenes have private storage and read-only accessors; typed cells
  convert through the existing quantity constructor. No GPUI, Lua execution or
  filesystem work enters this model.
- Sol approved schema, identity, geometry and aggregate resource checks. His
  repository-rule review requested inline tests; the coordinator moved the
  boundary tests into the 600-line root module and removed the external test
  file. Fixed two split-import compile errors before final verification.
- Final checks after that move passed: `cargo fmt --check`,
  `cargo clippy --all-targets -- -D warnings` and `cargo test --quiet`: **652
  tests, zero failures/ignored**. No dependencies or application behavior
  changed, so no live UI check was needed for this standalone model.
- Added [renderer-plan.md](renderer-plan.md) with the concrete model/runtime/UI
  handoff. The example and typed runtime draft remain uncommitted work owned by
  their assigned agents. Next: review and verify generalized worker results,
  then wire Properties/View and run the sensor proof in the application.

- Captured a fresh live baseline at `/tmp/fyle-scene-ui-le1si8bh`: existing
  columns show 95 m / ≥381 m from the new example, Properties retains native
  filesystem facts, and ordinary text uses the native editor. The scratch-only
  harness owns Xvfb (`:90`) and reaps app runs; the baseline app quit normally
  (exit 0). Restart it after the integrated build for graph/Properties checks.

- The standalone native renderer draft is in `preview_scene_view.rs`. Sol
  reviewed it after sizing fixes: responsive uniform canvas scaling/clipping,
  fixed nonshrinking table columns and selectable document order. It remains
  unattached and uncompiled until the runtime check window; live narrow-width
  and copying checks remain required.
- Settled explicit Lua `false` as a blank table cell, mapped to DTO `None`.
  Rows remain dense and rectangular; nil holes/omissions and `true` are invalid.
- Assigned `/root/lua_worker` the next disjoint drafts, only new
  `extensions/properties.rs` and `properties_view/extension.rs`, pending Sol's
  exact handle/section API. No shared Properties/module/startup wiring may
  change during the typed runtime checkpoint. `/root/contract_implementation`
  retains protocol/worker/supervisor/process ownership and Cargo coordination.

- The Properties adapter/view drafts preserve manifest section labels and typed
  per-field results. Sol reviewed local-source gating, independent failures,
  visibility and selectable partial reasons. Compile-review fixes cloned a
  borrowed label before moving its package, corrected a type import and removed
  an unused import. Inline visibility tests were added.
- Review also fixed cancellation ordering in that adapter: check its generation
  token after waiting, before propagating the wait error, so a cancelled queued
  request reports Interrupted rather than BrokenPipe. Shared feature wiring
  and real-helper integration remain pending.

- The generalized runtime draft passed `cargo check --tests`. Sol's review
  requested actual new-path regressions before approval: Properties field-set/
  type/unit/completeness independence, invalid scene output and the exact sensor
  package through real supervised helpers. `/root/lua_worker` now owns only a
  new `supervisor/sensor.rs` test module for that last case; the runtime owner
  attaches it and retains Cargo ownership.
- Runtime review is correcting metadata strings accidentally consuming display
  text budgets, preserving exact text boundaries and treating known-field
  semantic errors independently while keeping aggregate-budget errors fatal.
  Properties Init schemas must be nonempty/unique before Lua; scenes must pass
  semantic validation in both child and host. These are review requirements,
  not completion claims.
- Because the runtime regressions use the exact example files, the sensor
  package/fixtures will be included in the runtime checkpoint commit. Its
  README explicitly marks app Properties/View wiring as unfinished; the full
  user-guide update stays with the later app feature commit.

- Focused runtime verification passed **72 extension tests** and full Clippy,
  including temporary compilation of the native scene renderer. Corrected a
  test helper that incorrectly decoded a Properties failure as a scalar result
  and one test-only needless lifetime. The coordinator removed the temporary
  renderer module declaration before standalone-runtime final checks, keeping
  the next commit independent of app UI wiring.
- New tests cover exact scalar text limits, Properties field normalization and
  missing/duplicate/foreign bindings, per-field semantic errors, child/host
  aggregate bounds, host scene revalidation, blank cells/sparse rows/invalid
  geometry, and all three exact-package helper results. Existing process
  cancellation/deadline/cleanup regressions remain part of that focused suite.

### 2026-10-17 — Verify typed contribution execution

- Generalized the internal protocol to v2 and retained the scalar column API as
  a wrapper. Added typed Properties field results and viewer scenes, strict Lua
  conversion and host revalidation. All requests share the existing queue,
  broker, deadline, cancellation and child-reaping owner.
- Sol approved the final source/tests after replacing diagnostic-text matching
  with the actual budget counter and correcting the sparse-row regression.
  An overlapping temporary renderer module edit was removed before verification;
  the runtime owner held edits during the final coordinator check window.
- Final checks passed after those fixes: `cargo fmt --check`,
  `cargo clippy --all-targets -- -D warnings`, `cargo test --quiet`: **665
  tests, zero failures/ignored**, and `cargo build`. No dependency was added.
- Production helper smoke passed **10 checks** without DISPLAY/WAYLAND_DISPLAY:
  complete/truncated/malformed sensor data across column, Properties and viewer;
  explicit numeric/completeness/graph assertions; old protocol rejection. All
  children exited/reaped and no scratch config/data/cache directories were
  created. Evidence: `/tmp/fyle-worker-v2-1_ahp3zn/results.json`.
- Included the exact example package/fixtures used by runtime tests. Its README
  still marks application integration unfinished. The native scene renderer and
  Properties adapter/view remain uncommitted drafts; no new app Properties/View
  behavior is claimed. Next: immutable startup feature handles, independent
  Properties/preview wiring, original-source/cancellation tests and live proof.

### 2026-10-17 — Start the application proof

- Committed typed execution as `ca1bc84`. Assigned `/root/lua_worker` Properties
  model/registry/controller rendering and the existing new adapter drafts.
  It may add only the Properties export in `extensions.rs`. Native types and
  independent inspection scheduling remain intact. Split the registry by
  responsibility if the root file would exceed about 1000 lines.
- Assigned `/root/contract_implementation` startup catalog/main/Commander and
  preview adapter/registry/session/controller wiring. It owns Cargo coordination
  and installs the already reviewed native scene renderer. Original source
  gates, native-only path wrappers, ordered viewer decline and explicit request
  cancellation are required before live checks.
- Sol reviews those seams. The coordinator owns docs, the example, final checks,
  scratch UI evidence and the feature commit. No renderer or Properties draft
  is yet wired into the running application.

- Checked the standalone runtime build in the scratch app before feature wiring:
  existing sensor columns still load and the ordinary text preview crop
  `362x653+870+99` is pixel-identical to the pre-slice baseline (zero differing
  pixels). Captured `runtime-native-text.png` in the UI scratch directory;
  that app run quit normally. Xvfb/harness remain owned for the final proof.

### 2026-10-17 — Resume saved integration

- Both Luna agents hit a service usage limit during application wiring. The
  owner requested continuation; resumed Sol/Luna with the same file ownership
  and saved tree, without redoing the committed model/runtime work.
- Confirmed the scratch Xvfb is still the owned `:90` process; the last app
  already exited normally. The persistent harness remains available for restart.
  Uncommitted Properties/preview drafts and user-guide updates are preserved;
  they still require compilation, review and live verification.

- Properties registry extraction and typed section rendering are now drafted.
  Preserved native symlink policy: metadata/opened-file checks follow an
  original local symlink to a regular file; retain the selected identity/name
  rather than fabricating a canonical target identity.
- Coordinator review found a test setup defect: Properties tests created a
  production Supervisor inside the test executable, so its child rejected the
  reserved CLI flag and failure assertions passed for the wrong reason. Luna
  is replacing this with a controlled real-helper test constructor and launch
  counters, with actual summary-value/malformed-reason assertions and zero
  launches for excluded or already-cancelled sources. Sol will review the fix.

- Sol approved the corrected Properties source/tests: controlled helper returns
  30 s / 95 m, malformed sensor input reports its specific parser error while
  native groups survive, and excluded/already-cancelled sources assert zero
  launches. Symlink-to-regular-file dispatch now proves a real successful read.
- Startup drafts now carry columns, Properties and viewer handles from the same
  package snapshot/supervisor. Preview has captured original identity and
  explicit cancellation, with replacement/reload/suspend/drop model tests;
  deterministic viewer selection/source-gate tests and final compilation are
  still pending. Removed the unused default PropertiesPane constructor after
  startup switched to injecting its shared registry.

- The first integrated full run passed **675 tests**. Clippy found a larger
  enum after adding scenes and a chained string-replacement style warning.
  Boxed only the new scene payload, preserving existing native preview/session
  state layout, and fixed the Properties formatter warning without suppressions.
  Reverification/build is underway before live UI checks.

- Integrated verification reached **678 tests**, fmt/clippy/build passing before
  live checks. In the scratch app, the exact example produces summary, table and
  canvas; Properties shows 30 s / 95 m. Partial input shows ≥127 s / ≥381 m
  and the graph/table disclose partial input and the first-16 sample table.
  Malformed input leaves native Properties facts visible with its package line.
- Live checks found three presentation defects and assigned fixes: the accent
  path used a pale background color (map it to theme primary), scene text lacked
  padding, and long View errors were centered/clipped (use constrained padded
  selectable scrolling text). Properties rows are being aligned with native
  label/value columns and constrained for long error wrapping.
- Native text fallback with the enabled package remains pixel-identical to the
  original preview crop (zero differences). Selecting “Sensor” in scene text,
  copying and pasting into the scratch location input proves text copy priority;
  after switching to Properties, Ctrl-C yanks the file instead, proving hidden
  scene selection is cleared. No navigation was committed from the test input.
  Screenshots: `complete-scene.png`, `complete-properties.png`, `partial-*`,
  `malformed-*`, `integrated-native-text.png`, `scene-copy.png` and
  `hidden-scene-copy-cleared.png` in the owned UI scratch directory.

### 2026-10-17 — Complete and verify the sensor application proof

- Registered package columns, Properties and viewer handles from one immutable
  startup snapshot and shared supervisor. Properties retains independent native
  groups and typed extension fields. Preview keeps specialized built-in priority,
  then tries deterministic package/contribution order before text/binary.
  Path-only, remote, archive and mismatched-local calls cannot invoke packages.
- Preview requests capture original identity and cancellation; replacement,
  reload, suspension and drop cancel supervised work, and stale completions are
  rejected. Native scene rendering handles text/table selection, theme mapping,
  uniform clipped paths and scrolling. Hidden selections clear; footer text has
  deterministic final document order.
- Sol approved the production/source policies and real-helper regression tests.
  A test-only private-field inspection was replaced with actual package-order
  dispatch, namespaced scripts and an exact one-launch assertion. Same-package
  tests verify reversed order, absent-to-ready continuation and failed-first
  stopping without executing the next candidate.
- Live checks drove small presentation fixes: visible accent paths, padding,
  aligned/wrapping Properties rows and selectable/wrapping View diagnostics.
  Narrowing the inspector exposed an intrinsic-width constraint; shrinkable
  wrappers now fit the canvas while fixed table content scrolls horizontally.
  `fixed-narrow-inspector.png` shows the full path inside the smaller viewport.
- Final source verification after these fixes passed `cargo fmt --check`,
  `cargo clippy --all-targets -- -D warnings` and `cargo test --quiet`: **681
  tests, zero failures/ignored**. `cargo build` passed for the same final
  production source. No dependencies were added; no Windows build or real
  remote-account UI check ran.
- Live proof uses `/tmp/fyle-scene-ui-le1si8bh`: package-only Lua title/spacing/
  path-role edits followed by restart changed the layout/graph with an unchanged
  binary SHA-256; complete/truncated/malformed results and native fallback were
  checked; competing packages/contributions selected by ID and declined properly;
  a failed field preserved its readable sibling/native facts; invalid geometry
  gave a useful View error. Disable/restart restored native text/Properties and
  blank saved columns; re-enable restored the package. Rapid selection ended on
  the correct scene. Config comments and saved IDs remained intact.
- Graph helper deadline failure was visible and readable. Normal File → Quit
  with an observed active graph helper took **0.308 seconds**, with the helper
  reaped before app exit (exit 0). The PID check enumerates children across all
  app threads; checking only the main thread initially missed the supervisor's
  child. Evidence: `active-graph-quit.json`. All per-run app logs were empty.
- Restored the scratch package to the shipped example, stopped/reaped the owned
  app and Xvfb, and marked the harness state `ui_stopped: true`. Updated README,
  TODO, architecture/worker plans and package guidance. Only unrelated untracked
  `title.png` should remain after commit. Next: contextual Git; keep broader
  migrations behind the planned proof review.


### 2026-10-17 — Start contextual Git (slice 6)

- The owner asked to continue after completed slice 5. Re-read TODO, README,
  architecture/implementation plans and navigation lifecycle before editing.
  The authorized proof scope includes contextual Git; the separate idea of
  replacing the Git executable with a library remains deferred.
- Ownership: Sol reviews the architecture and writes `git-plan.md`; one Luna
  owns `git_status.rs` and its bounded native-path runner/parser; another Luna
  owns `data_providers/git.rs` and the shared request/snapshot service. The
  coordinator integrates the existing controllers, records progress and runs
  final automated/live verification. No dependencies are planned.
- Existing Git work uses unbounded `Command::output`, lossy path conversion and
  a panel task without completion identity checks. Preserve status priorities,
  ignored/renamed/directory marks while replacing these boundaries.
- Design: capture the local panel query scope and service refresh epoch without
  disk access; resolve repository root in background work. Identical keys share
  pending/prepared snapshots, with independent subscriber cancellation. Listing
  reloads invalidate repository context even when selected file metadata stays
  unchanged; continuous watching is outside this slice. Properties must retain
  the panel scope, original selected identity and independent native sections.
- Implementation and verification are pending. Keep unrelated `title.png`
  untouched; no push or PR.


### 2026-10-17 — Integrate and review contextual Git

- Implemented native-byte porcelain parsing, lexical-to-canonical scope mapping,
  literal pathspecs and bounded Linux nonblocking process acquisition. Timeout,
  cancellation and output limits kill/reap the direct child. Captured/fake
  executable tests replace the former test requiring an installed Git.
- Implemented one shared service with eight active jobs (including cancelled jobs
  until runner cleanup), 64 subscribers per job, 16 cached snapshots/16 MiB and
  256 explicit scope revisions. Revision overflow safely invalidates all cached
  contexts. Main explicitly shuts down the service after the event loop.
- Both production adapters consume the same provider definition. Properties
  captures its context inside its independent session request; changing only the
  epoch cancels/replaces an unchanged selected file. Native path values and
  independent baseline facts survive contextual failures.
- Sol review caught and resolved repeated unchanged-demand restarts, shutdown
  ownership, ancestor-fallback invalidation and unused production descriptors.
  Root added proactive invalidation/resubscription for both panels and the
  active Properties perspective at the common listing boundary. Existing
  editor/tool/job reload paths already pass through it.
- Git may collapse ignored/untracked parent directories. Preserve existing
  column marks and label a successful missing path record “No changes reported”
  rather than asserting “Clean”. Tests cover grouped descendants explicitly.
- Initial live check: `/tmp/fyle-git-ui-yw0cbt_j` shows matching file/directory,
  ignored and renamed marks/Properties, return from View to Properties, and
  both panels on one scope. Staging `30-untracked.txt` changed no file size or
  modification time; Ctrl-R changed both columns and Properties from Untracked
  to Added. The owned app/Xvfb were stopped and reaped.
- Initial full suite found one outdated test expectation (“Clean”) after the
  wording correction; it is updated. Final checks and final-build UI evidence
  remain pending; decoded-index memory/item limits are being tightened before
  that final run.


### 2026-10-17 — Finish contextual Git verification

- Tightened checked porcelain decoding to 100,000 records and a conservative
  16 MiB index estimate before allocating the path map. Both acquisition output
  and retained decoded data are bounded; provider demand uses named limits.
- Discovery now returns absence only for the stable non-repository diagnostic;
  per-child `LC_ALL=C` makes that classification deterministic. Other errors,
  including ownership failures, preserve bounded diagnostics. Linux spawn retries
  only executable-busy errors within the same deadline/cancellation policy.
- The first full suite exposed an executable-busy race in a newly written fake
  executable. The bounded retry fixes that case. Final exact-source checks pass:
  `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`,
  `cargo test --quiet` (**706 passed, zero failed/ignored**) and `cargo build`.
  No dependency changes. No Windows build or real remote-account UI run.
- Final-build evidence is in `/tmp/fyle-git-ui-1ck__zjx`: `final-columns.png`,
  `final-no-changes.png`, `final-added.png`, `final-nested.png` and
  `final-renamed.png` show native mark/Properties agreement, distinct root versus
  nested query scope, corrected no-record wording and return from View.
  Staging again preserved file size/mtime and Ctrl-R updated both panels and
  Properties. `archive-properties.png` is listing-only with blank Git marks;
  `outside-properties.png` retains native facts without a Git section.
- Navigation through nested, archive and outside directories then back to the
  repository ends on the selected Added entry (`rapid-final.png`), with no stale
  Properties or column marks. `deadline-properties.png` shows a deliberately
  blocked scratch Git wrapper timing out without hiding filesystem facts.
- Normal Ctrl-Q while the scratch Git wrapper was actively running exited 0 in
  **0.069 seconds**, and its direct child was reaped before the app returned;
  `quit.json` records PID 58429 and the check. App 58084 and Xvfb 58082 were
  stopped/reaped; both UI harness state files now say `ui_stopped: true`.
  The wrapper, repositories, config and data are scratch-only. Application logs
  were empty. Real config/history and unrelated `title.png` remain untouched.
- README, TODO and architecture/implementation/Git plans describe the resulting
  behavior and handoff. The next task was slice 7 proof review.

## Slice 7 review

### 2026-10-18 — Accept the sensor and Git proofs; remove unused generic reuse model

- Compared the exact sensor manifest and fixtures, enabled-package loading and
  catalog, typed worker results, Properties/View/column adapters, and the Git
  definition/service plus panel/Properties consumers. See
  [proof-review.md](proof-review.md) for the full trace and resource bounds.
- Sensor registers all three presentation contributions through existing
  registries. Exact local-source gating, manifest demand, helper validation,
  typed partial results, failures and generic fallback are covered by focused
  tests and the prior live package-only layout, disable/fallback and
  malformed/truncated checks.
- Git shares a bounded snapshot only for the same typed local listing scope and
  service epoch. Panel and Properties keep independent cancellation leases and
  generations. Native paths, refresh invalidation, failures, limits and child
  cleanup have focused tests and recorded scratch-repository checks.
- Production search showed `Observation`, `RequestIdentity`, `Revision`,
  `ContextRevision`, `accepts_result` and `can_reuse` had no callers outside
  their synthetic module tests. Replaced the fake sensor/Git example with the
  exact sensor manifest and production Git definition, then removed those
  generic APIs. Git's production scope/epoch tests still exercise freshness and
  reuse. Native GPX/FIT, Git snapshot and extension scene models remain typed.
- Checked candidate slice 8 consumers. Media already shares its native
  `MediaMetadata` reader between Preview and Properties, while stream facts and
  frame extraction remain distinct. Parquet footer/schema and row sampling use
  different demands. EXIF/RAW has bounded, cancellable metadata paths and
  different native outputs. None needs a universal scalar observation; future
  shared acquisition must identify source freshness, producer/version, demand,
  limits and access policy.
- Package adapters validate presentation-specific support independently. A
  regression confirms an unsupported viewer reports its own diagnostic while
  valid Properties and column siblings install; the sensor provider registers
  all three successfully. This is documented per-contribution failure handling,
  not evidence for a cross-presentation transaction.
- Decision: **accept the shared contribution vocabulary; revise by removing the
  unused generic observation/reuse layer; stop before slice 8 until the owner
  chooses a migration.** No new abstraction was introduced. The slice 1
  generic revision/reuse contract and synthetic projection example are
  superseded; earlier verification entries remain historical.
- The cleanup changed no presentation dispatch, rendering or scheduling, so the
  previously recorded sensor and Git live UI evidence remains applicable.
- Repository verification passed: `cargo fmt --check`, isolated offline
  `cargo clippy --all-targets -- -D warnings`, focused `cargo test
  data_providers` (21 passed), and full `cargo test` (704 passed). Cargo used a
  copied target directory because the shared target lock was held outside this
  process namespace; the shared target was left untouched. All dependencies
  were available offline.
- Decision: **accept after unused API cleanup and repository verification**.
  Slice 7 is complete. The owner later authorized Media as slice 8's first
  migration; Parquet and EXIF/RAW remain undecided.

## Slice 8 media migration

### 2026-10-18 — Migrate existing Duration, Properties and Preview adapters

- The owner authorized Media as the first slice 8 migration, preserving behavior
  and limits. Read the implementation plan, TODO, README, format organization
  docs and current adapters before editing. Sol's plan kept the native ffprobe
  reader and existing consumer lifecycles; no new shared scheduler/cache was
  proposed or introduced.
- Added one validated `builtin.media` definition with a seconds duration field,
  the built-in `duration` column binding, a Properties contribution and a
  `media.streams` viewer descriptor. Three small handles prepare the typed
  duration projection from the existing native `MediaMetadata` reader. Field
  lookup follows IDs, with a reordered-definition regression covering column
  and Properties bindings.
- Rewired the existing Duration metadata queue, media Properties mapper and
  Preview adapter through their provider handles. Preserved the native typed
  metadata, icon-based applicability, local Properties/Duration source gates,
  Preview's existing materialization policy, audio information, video frame
  extraction at duration/10, and missing-ffprobe hint. Each consumer retains
  independent scheduling and cancellation. The built-in `ColumnKind::Duration`
  remains because provider catalog registration deliberately rejects collisions
  with built-in IDs; no user-facing ID, sort, display or queue behavior changed.
- Preserved the ten-second ffprobe deadline, 1 MiB stdout and 8 KiB stderr caps,
  32 stream limit, 1,024-character displayed value/tag bound, and existing probe
  size/analysis window. Stream truncation leaves a valid container duration
  complete. Missing or invalid duration stays blank in the column; Properties
  and audio/video Preview keep their previous behavior.
- Focused tests cover one definition and all three descriptor bindings,
  reordered fields, typed complete duration under truncated stream details,
  missing/invalid duration, filename icon policy, Properties projection and
  Preview duration/audio/video behavior. Existing parser/runner tests cover
  bounded output and stream handling. No optional ffmpeg/ffprobe program is
  required by the tests.
- Live scratch-XDG app checks used `/tmp/fyle-slice8-media.97R0NH`: Duration
  displayed `00:02`; audio Properties showed Container/Audio and `00:02`; audio
  Preview showed the existing info; video Properties showed Container/Video;
  video Preview extracted a frame; hiding `ffprobe` from the scratch app's PATH
  left Duration blank and displayed the existing installation hint in Preview.
  Config, data and runtime directories were scratch-only. The UI harness was
  stopped after checking. This did not exercise remote or archive media paths.
- Final verification on the exact source: `cargo fmt --check` passed;
  `cargo clippy --all-targets --offline -- -D warnings` passed;
  `cargo test media:: --offline` passed 10 tests; full `cargo test --offline`
  passed **710 tests, zero failures/ignored**. Cargo used the warmed isolated
  `/tmp/fyle-slice7-build/target` to avoid the shared target lock held outside
  this process namespace. Dependencies were available offline. `git diff
  --check` and the final commit remain to be completed.
- Decision: **accept after final-source verification**. At that point Parquet
  and EXIF/RAW remained undecided; the owner later authorized all remaining
  migrations. If future consumers demonstrate identical reusable acquisition,
  its key must capture freshness, demand/limits and access policy; Media did not
  justify cross-consumer reuse. No new abstraction was introduced.

### 2026-10-18 — Migrate Parquet footer metadata through a provider

- The owner authorized finishing all remaining built-in migrations. Read the
  current plan, TODO, README, architecture map, native reader, adapters and
  limits before editing. The native `ParquetMetaData` stays the prepared value;
  the provider adds no scalar field model or row-sample conversion.
- Added one `builtin.parquet` definition with independent Properties metadata
  and Preview row-sample descriptors. Their demands record the distinct
  8 MiB footer/384 metadata-item bound and the footer plus 64 MiB selected-chunk,
  20,000-cell Preview bound. Properties and Preview keep their own calls and
  cancellation; there is no cross-consumer cache.
- Both adapters now read the checked native footer through provider-owned
  handles. Preview passes the same open file descriptor to footer validation
  and `SerializedFileReader`, so metadata and row groups cannot come from two
  different files if the path is replaced between opens. Properties continues
  to use a nonblocking regular-file open, with symlink-following behavior.
  Existing extension-or-magic gates remain separate: Properties checks local
  path extension or file magic; Preview checks extension or its captured head.
- The actual Properties registry and Preview registry regression uses the same
  generated repeated-schema fixture: Properties retains both schema leaves,
  while the row-sample adapter declines repeated columns. Other tests validate
  distinct descriptors/demands, extension and magic matching, metadata reads,
  8 MiB footer validation, Properties caps/truncation and the 500-row/40-column/
  64 MiB Preview sample limits.
- Scratch-XDG/Xvfb app check in `/tmp/fyle-parquet-ui` showed a generated
  two-row Parquet file as a Preview table (`7`, `42`) and its Properties group
  with row count, schema and row-group details. Menu selection was used because
  F3 key synthesis is unreliable under bare Xvfb; the app and Xvfb were stopped
  afterward. No host settings or data were used.
- Focused tests pass: provider 3, Properties 5 and Preview 10. The full suite
  passes **713 tests, zero failed/ignored**; `cargo fmt --check`, all-target
  Clippy with `-D warnings`, and `cargo build --offline` also pass. Cargo used
  `/tmp/fyle-slice7-build/target` and offline dependencies to avoid the shared
  target lock. `git diff --check` passes.
- Decision: **accept Parquet after final-source verification**. Metadata and
  row samples are concrete different demands. Any later reuse key would need to
  name freshness, producer, demand/limits and access policy; this migration
  does not share in-flight work.

### 2026-10-18 — Bind EXIF Properties and columns to shared semantic tags

- Owner authorization covers completing the remaining migrations. Added one
  validated `builtin.exif` definition with all 13 existing semantic tags,
  existing Properties fields and the nine persisted `exif.*` column IDs.
  `properties/exif_reader.rs` and the duplicate panel-side projection were
  removed; `properties_view.rs` remains a label-only adapter, and
  `panel/metadata/exif.rs` forwards the existing queue to the provider.
- `formats/exif.rs` now owns one ordered tag mapping, semantic parsing, the
  legacy regular-image column reader and Properties' stricter metadata-only
  reader. Properties keeps its original 8 MiB ordinary-image and 64 MiB RAW
  prefix limits, 16 EXIF candidates, 1 MiB per embedded TIFF, 4,096 characters
  per field, regular-file checks, cancellation and partial/truncation results.
  The existing column path retains regular-image seekable reads, its original
  image extension gate, bounded 64 MiB RAW prefix and `select_jpeg` fallback.
  No decoder, image cache or materialization was added to Properties.
- The old EXIF reader's unique boundaries remain covered: capped reads and
  truncation, pre-cancellation, ordinary JPEG APP1, direct TIFF, RAW-prefix
  embedded TIFF without decoding, and Properties' embedded JPEG metadata path.
  The column adapter tests preserve composition/formatting, extension gating,
  JPEG and direct-TIFF results, and RAW's selected-JPEG fallback. Two redundant
  adapter tests were consolidated: the old cancellation and cap tests now share
  one test, and the provider's production-column test covers the former
  extension-gate and reader-projection fixtures together. No distinct reader or
  adapter case was dropped. The full suite now has 711 tests versus 713 before
  this consolidation.
- Repository verification on the final EXIF source passed: `cargo fmt --check`,
  isolated offline `cargo clippy --all-targets -- -D warnings`, and full
  `cargo test --offline` (**711 passed, zero failed/ignored**). Cargo used the
  warmed `/tmp/fyle-slice7-build/target` to avoid the shared target lock;
  dependencies were available offline.
- Scratch-XDG/Xvfb UI check used `/tmp/fyle-exif-ui` and a generated JPEG with
  EXIF metadata. The running app's Properties view showed Camera maker `Acme`
  and Camera model `Snap 1`. The app and Xvfb were stopped afterward. Existing
  column projection was exercised through the production provider test; the
  live check covered Properties rendering.
- Decision: **accept after final-source verification**. Image Preview has no
  EXIF metadata consumer: image Preview presents the source image, while RAW
  frame selection/materialization remains shared by the existing RAW preview
  and thumbnail paths. EXIF metadata has intentionally distinct read limits
  for Properties and columns; no shared cache or scheduler is justified.

### 2026-10-18 — Close out the approved cross-presentation audit

- Traced every built-in `Previewer`, default Properties reader, visible-row
  metadata field, thumbnail consumer and archive navigation entry point.
  Concrete shared seams are Geo (Properties/View), Media
  (Duration/Properties/Preview), Parquet (footer Properties/Preview), EXIF
  (Properties/columns), Git (status column/Properties) and the sensor package
  (Properties/column/View). Native typed models remain; only Git has a proven
  cross-request snapshot service with scope and epoch ownership.
- The remaining readers are single-presentation or have distinct demands:
  PDF; spreadsheet; DOCX/ODT; PPTX/PPTM; CSV/TSV; Markdown; notebook; generic
  text/binary; SQLite Properties; and image Preview/thumbnails. RAW Preview and
  thumbnails already share the bounded JPEG selector and `raw_preview` cache.
  Archive navigation's cached complete index, the first-2,000-item F3 archive
  summary, archive-directory child summaries and capped archive-member Preview
  serve different requests. No new abstraction was warranted.
- Also traced native filesystem snapshots, fresh Properties `lstat` and
  symlink-target facts, owner lookup, xattr/SELinux columns, recursive folder
  size, immediate-child directory Preview, name-based icon/Open With selection
  and Preview's bounded content probe. These use different sources, freshness
  or work bounds; no same-fact shared acquisition was identified.
- Closeout adds no file recognition, Properties, sorting or caching behavior.
  No authorized reader migration remains open. Other TODO items are unrelated
  product decisions and stay in the backlog.
