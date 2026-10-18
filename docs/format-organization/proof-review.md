# Sensor and Git proof review

Status: accepted on 2026-10-18 after unused API cleanup and repository
verification. This review compares the production sensor package and
contextual Git paths before any further built-in migration. The
[implementation plan](implementation-plan.md) remains the migration scope
authority; [progress.md](progress.md) records the previous implementation
checks and this review's evidence.

## Decision

Verdict: **accept the shared contract after the unused API cleanup and
repository verification pass**. Slice 8 remains stopped pending the owner's
provider choice.

Keep the shared contribution vocabulary and typed `ProviderDefinition`:
provider/contribution/field IDs, registration origin, applicability, demand,
field types, result state, completeness and provenance are used by the
production adapters. Keep native provider models and their independent
lifecycles.

Remove the unused generic `Observation` and `RequestIdentity`/revision/reuse
model. Neither proof consumed them in production. The sensor uses an exact
local `Location`, typed worker results, declared demand and result completeness;
it does not cache across requests. Git captures its own query scope and epoch,
then uses separate subscriber leases for the column and Properties consumers.
Its tested scope/epoch model is the freshness and reuse boundary actually
needed by Git. This review does not add a general observation store, cache key,
reader trait or scheduler.

The shared stages remain useful vocabulary, but they are not a universal runtime
pipeline. Each presentation retains its own adapter and decides how to schedule
and display its result.

## Sensor trace

The exact example manifest is parsed and validated by
`extensions::manifest::parse`; it declares Properties, column and viewer
contributions in one provider. `extensions::catalog::prepare` loads only
explicitly enabled packages, and `install_packages` creates Properties and
viewer handles plus column catalog entries. Existing `Properties::Registry`,
`columns::Catalog`, and `preview::Registry` dispatch them. No format-specific
dispatch edits are needed to add the three example contributions.

All three adapters require a local regular file. Properties retains the
original selection and submits one contribution to the supervised worker. The
column adapter checks declared demand and captured applicability before its
metadata worker runs. The viewer additionally requires the readable path to
match the original local `Location`; it runs only after specialized native
previewers and before generic text/binary fallback. An absent or inapplicable
viewer continues to fallback, while a failed or pending viewer reports an
error. A ready viewer returns a validated native scene.

The manifest declares 65,536 input bytes and 128 items per contribution. The
host brokers only the declared source range; the Lua parser also bounds its
sample count. Worker and host validation retain typed values, field identity,
scene limits and completeness. Properties fields fail independently; lower
bounds and scene truncation remain explicit. The three contributions may read
the same small input separately: there is no shared sensor acquisition cache.
That duplication is bounded and does not justify a scheduler or common cache.

The existing live proof recorded in `progress.md` changed only package layout,
verified the graph and column/Properties output, disabled the package and
checked native fallback, and covered malformed/truncated input, rapid selection
changes, source gating and normal helper shutdown. Unit tests exercise the exact
manifest, fixtures and worker through all three APIs.

## Git trace

`data_providers::git::definition` declares the existing `git.status` column and
Properties fields. The panel and Properties adapters use that definition and
one application-owned `git::Service`. For each typed local listing scope, the
service captures a scope/epoch key. Background discovery yields the repository
root, queried scope and native relative path; the same bounded snapshot feeds
both consumers only when scope and epoch match. Panel and Properties have
independent leases and generations, so one consumer can cancel or become stale
without cancelling the other's work. Native `PathBuf` identity is preserved
through acquisition and projection.

The Git runner bounds stdout to 8 MiB, stderr to 64 KiB and discovery plus
status to three seconds; parsing caps records and the decoded index estimate.
Service admission, subscribers and retained snapshots are bounded. Explicit
refresh/listing reload advances context freshness even when the selected file
metadata is unchanged. Missing Git, outside-repository, cancellation, timeout
and malformed/limited acquisition remain failures or unavailable results;
they do not become a successful clean status. Directory aggregation and native
status priorities are unchanged.

The recorded scratch-repository UI checks verified column/Properties agreement,
staging followed by refresh without changing the selected file, separate scope
behavior, archive exclusion, timeout presentation and normal quit with child
cleanup. Controlled-runner tests verify same-scope reuse, distinct scopes,
independent leases, stale rejection, refresh epochs and bounds.

## Registration and remaining limits

The package catalog adapts each declared contribution to its presentation
registry. An unsupported viewer produces a startup diagnostic while valid
Properties and column siblings still install; this independent-failure behavior
has a regression test. The sensor package itself registers all three
contributions successfully. Manifest validation checks a provider definition
before package installation, while each host adapter additionally validates
its own supported representation and runtime result. These are documented
per-contribution outcomes, not evidence for a cross-presentation transaction.

Native data remains richer than the shared scalar field vocabulary: Geo keeps
`GeoData`; Git keeps its snapshot and native status model; package scenes use
the validated scene grammar. Existing format readers may share a semantic
reader while their consumers keep distinct demands. Media stream details,
Parquet footer versus row samples, and EXIF/RAW metadata do not require a
universal `Observation` shape. Cross-consumer reuse should be designed only
after a concrete pair of requests needs identical bounded work and can supply
the relevant typed source freshness.

At the time of this review, slice 8 still needed the owner's provider choice.
The owner later authorized Media as its first migration; see the slice 8 entry
in the [implementation plan](implementation-plan.md) and the
[verification log](progress.md). Parquet and EXIF/RAW remain undecided.
