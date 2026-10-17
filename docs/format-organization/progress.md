# Provider migration progress and handoff

Read this file first when joining the work, then the
[implementation plan](implementation-plan.md) and the repository's
[AGENTS.md](../../AGENTS.md). The [synthesis](synthesis.md) explains the design
motivation. Update this log in every implementation commit so the working state
can be reconstructed without the chat.

## Current state

- The owner authorized starting the plan after the planning commit `5da8dee`.
- Completed: slices 1–4, contracts, the GPX/FIT adapter, display-only columns and
  explicitly enabled reader packages with supervised execution.
- Slice 4a static package validation/loading is committed as `c2913dc`, and the
  protocol/broker/queue foundation as `cf11f3b`. Standalone worker execution and
  Linux supervision are committed as `a2f7021`. Settings/catalog integration
  (4c) is complete and verified as `0db1dbd`; slice 5 is now in progress.
  Luna owns the Rust extension files and helper dispatch; Sol reviews; the
  coordinator owns documentation, final checks and commits. Read
  [worker-plan.md](worker-plan.md) before joining 4b.
- Working roles, requested by the owner: Sol makes architecture decisions;
  Luna implements; the coordinating agent reviews, verifies and documents.
  Earlier slices were reviewed by Astra; the owner replaced that role with Sol
  before slice 3 implementation resumed.
- Existing GPX/FIT presentation and scheduling are preserved by the shared
  adapter. The owner approved the proposed Lua worker and explicit package
  enablement. Static layout, selectable text, typed tables and a bounded path
  canvas were approved on 2026-10-17; native code owns interaction.
  The owner approved display-only extension columns first; new
  metadata sorting remains a separate design task.
- Keep the unrelated, pre-existing untracked `title.png` out of commits.

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

## Slice 5 handoff

The owner approved static layout, selectable text, typed tables and a bounded
path canvas on 2026-10-17. Native code owns interaction, with no arbitrary
callbacks/widgets. Slice 5 implementation may proceed.
Then follow the plan's sensor package proof across Properties, a column and a
custom graph. Keep source/permissions and feature lifecycles independent; do not
infer format/context evidence or label partial results complete. The user guide's
distance-column example is already verified and can inform the authoring API.
Contextual Git and the proof review follow before remaining built-in migrations.

## Completion and next work

| Slice | State | Evidence / handoff |
| --- | --- | --- |
| 1. Contribution contracts | Complete | Astra approved; fmt/clippy and all 565 tests passed |
| 2. Existing GPX/FIT adapter | Complete | Astra approved; fmt/clippy/build and all 573 tests passed; 11 live inspector comparisons identical |
| 3. Dynamic columns | Complete | Sol approved; fmt/clippy/build and all 579 tests passed; unavailable-ID persistence/removal and native UI checked |
| 4. Packages and worker | Complete | Explicit config activation and display-only columns; Sol approved, 644 tests/fmt/clippy/build and live UI/active-helper normal-exit checks pass |
| 5. Sensor renderer proof | In progress | Renderer scope approved on 2026-10-17; Sol architecture, Luna implementation |
| 6. Contextual Git | Pending | New Git Properties behavior is within the authorized proof scope |
| 7. Review proof | Pending | Both sensor and Git cases must work before broader migration |
| 8. Remaining built-ins | Pending | Depends on slice 7 |

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
