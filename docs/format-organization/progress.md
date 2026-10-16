# Provider migration progress and handoff

Read this file first when joining the work, then the
[implementation plan](implementation-plan.md) and the repository's
[AGENTS.md](../../AGENTS.md). The [synthesis](synthesis.md) explains the design
motivation. Update this log in every implementation commit so the working state
can be reconstructed without the chat.

## Current state

- The owner authorized starting the plan after the planning commit `5da8dee`.
- Completed: slices 1–3, contracts, the GPX/FIT adapter and display-only columns.
- Next: slice 4, static packages and a supervised Lua reader. Sol has designed
  the boundary and Luna has surveyed the code read-only; implementation starts
  after the slice 3 commit.
- Working roles, requested by the owner: Sol makes architecture decisions;
  Luna implements; the coordinating agent reviews, verifies and documents.
  Earlier slices were reviewed by Astra; the owner replaced that role with Sol
  before slice 3 implementation resumed.
- Existing GPX/FIT presentation and scheduling are preserved by the shared
  adapter. The owner approved the proposed Lua worker and explicit package
  enablement; the renderer scope checkpoint remains pending.
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

## Completion and next work

| Slice | State | Evidence / handoff |
| --- | --- | --- |
| 1. Contribution contracts | Complete | Astra approved; fmt/clippy and all 565 tests passed |
| 2. Existing GPX/FIT adapter | Complete | Astra approved; fmt/clippy/build and all 573 tests passed; 11 live inspector comparisons identical |
| 3. Dynamic columns | Complete | Sol approved; fmt/clippy/build and all 579 tests passed; unavailable-ID persistence/removal and native UI checked |
| 4. Packages and worker | Pending | Supervised Lua/config-only packages/explicit enablement approved |
| 5. Sensor renderer proof | Pending | Requires renderer scope decision |
| 6. Contextual Git | Pending | New Git Properties behavior is within the authorized proof scope |
| 7. Review proof | Pending | Both sensor and Git cases must work before broader migration |
| 8. Remaining built-ins | Pending | Depends on slice 7 |
