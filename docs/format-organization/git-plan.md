# Contextual Git: slice 6 implementation boundary

Status: implemented and verified 2026-10-17. This document records
the resulting architecture and its limits. The [implementation plan](implementation-plan.md)
remains the scope authority; the [progress log](progress.md) records checks.
No Git library replacement, new dependency, general cache framework or broader
built-in migration is authorized by this slice.

## Current problem and required result

`git_status.rs` currently runs unbounded `Command::output`, converts native paths
through lossy UTF-8 and trims root whitespace. `Panel::refresh_git` owns a task
without an explicit completion-generation check, and excludes remote folders
rather than affirmatively requiring a local typed location. Properties has a
sound independent selection session but no repository context. A selected file's
metadata is insufficient to decide whether an index/worktree snapshot is fresh.

Keep the existing `git.status` column identity, `git` compatibility alias, symbols
and precedence. Register `builtin.git` column and Properties contributions
together, using contextual applicability rather than a format extension. Show
repository root, selected repository-relative path and projected status in a
Git Properties group. Directory status uses the same aggregation as its column.
A complete successful query with no mark says “No changes reported”: Git may
collapse ignored or untracked parent directories, so absence of a child record
does not prove it is clean. Missing Git, outside-repository and failed/limited
acquisition never become a successful no-mark status. Baseline filesystem/format Properties survives a Git failure.

## Ownership and seams

- `git_status.rs` and focused child modules own native process execution,
  repository discovery, porcelain parsing and `Status::mark`. They import no
  GPUI and expose a cancellable `io::Result` acquisition seam. A prepared
  snapshot retains native repository root, native query scope and typed status.
- `data_providers/git.rs` owns the immutable contribution definition, Git-specific
  request identities, refresh revisions, subscriber lifecycle and projections.
  The request model imports no GPUI. An injected reader/fake runner allows model
  tests without installing Git.
- One application-owned shared service is passed to both panels and Properties.
  It owns bounded pending/completed acquisition state, not GPUI entities.
  Controllers own tasks and subscriber leases. Render reads prepared data only.
- Properties' existing `Selection` remains source identity, not repository
  context. Capture Git request context separately and invalidate its session
  when that context changes even if the selected entry compares equal.
  Preserve `inspect_with_registry` as the context-free wrapper for existing
  callers/tests; contextual inspection receives an optional captured Git request.

## Identity, freshness and acquisition

Before background work, require `Location::Local` for both query and selected
source. Never infer local authorization from a compatibility `PathBuf`, archive
member, materialized preview or remote address. The query scope is the listing
directory that supplies the column rows. Properties uses that identical scope,
not a new scope chosen from whether its selected entry is a file or directory.
Repository discovery and repository-relative selected-path capture happen in
background work. Preserve lexical symlink/listing identity; do not canonicalize
away the original query address on the UI thread.

The shared request key is native query scope plus a service-owned refresh epoch.
Repository root is resolved by the shared acquisition and retained in its result;
sharing discovery as well as status is intentional. The complete prepared
identity includes root, query scope and epoch. Separate query scopes never
satisfy each other, including ancestor/descendant scopes in one repository.
Do not substitute per-folder listing generations for context revisions: two
folder entities can have equal generations while describing different history.

The service supplies the current epoch for a scope without disk access.
Invalidate local scope before starting a listing reload, through the common
`Commander::load_panel_listing` boundary. This covers explicit refresh,
operation-triggered refresh and navigation/listing reload. Invalidation evicts
completed state and makes old work/results ineligible. Different scopes remain
independent. Immediately after invalidation the commander refreshes Git demand
for both panel views and calls `update_preview`, so every active consumer clears
old prepared state and captures the new key even before a listing commits.
Normal listing observers repeat this synchronization on commit. Properties
stores its optional context key independently; changing it invalidates the
current session even when source selection is unchanged. Ctrl-R may invalidate
the same scope twice when both panels show it; each advance synchronously
re-subscribes all active consumers. Deduplicating the batch is an optimization.
No continuous filesystem/index watcher is promised. Editor-save and any reload
path bypassing the common boundary must explicitly invalidate the affected scope.

A consumer captures its key and its own generation. Panel completion accepts only
when folder generation, local query location and current service key still
match. Properties also requires its existing session generation/selection check.
Changing scope, hiding a column, leaving Properties or dropping a controller
releases that consumer's lease and clears obsolete prepared state immediately.
A root/query lookup failure is a contextual result, not an empty `Status`.

## Shared work and cancellation

Identical scope/epoch requests join one pending job and receive the same immutable
`Arc` snapshot. The model assigns independent subscriber IDs. Dropping/cancelling
one lease detaches only that subscriber; work continues while another subscriber
needs it. Last-subscriber cancellation signals the runner, which kills and reaps
its child before releasing its concurrency slot. Scope invalidation detaches all
old-epoch demand; stale completion cannot populate the current cache.

Never hold the service mutex during process execution, blocking waits or reader
callbacks. Start/finish/subscribe/release decisions occur under short model
transitions; acquisition runs outside the lock. Bound concurrent jobs, pending
keys and retained completed snapshots/bytes with named constants. A completed
result larger than the cache budget may serve current subscribers without being
retained. Eviction cannot cancel unrelated active consumers. Failure and absence
may be reused only within the same explicit epoch.

## Native runner and parser

Continue to invoke Git directly with argument vectors, never a shell. Use the
existing scoped porcelain v1 NUL output and `--ignored=matching -- .` behavior.
Preserve modified/conflicted/add/delete/rename/untracked/ignored priorities,
rename's second pathname record and nested directory aggregation. Ignored
children do not mark an otherwise clean parent; a directly ignored directory
still has its own ignored mark.

Read stdout/stderr concurrently with independent named byte caps and a shared
wall deadline covering discovery plus status. Poll cancellation/deadline, kill
and reap on cancellation/timeout/limit, finish reader cleanup, and retain bounded
stderr for useful errors. Reject an oversized/incomplete status snapshot instead
of presenting missing entries as clean. Preserve the Windows no-console flag.

On Unix convert pathname bytes through native `OsString` bytes; lossy conversion
is for display only. Root discovery removes exactly its protocol line ending,
never `trim` or whitespace trimming. Porcelain paths are NUL-delimited and may
contain tabs, newlines, spaces and invalid UTF-8. Remove a directory's trailing
separator without Unicode decoding. Handle malformed/truncated records
explicitly, and maintain existing Windows compilation without extending it.

Implemented limits: one three-second deadline covers discovery and status;
each command allows 8 MiB stdout and 64 KiB stderr. The checked parser rejects
more than 100,000 records or a conservative decoded-index estimate above 16 MiB.
The service admits eight active jobs, including cancelled jobs until their
runner returns, and 64 subscribers per job. Its LRU retains at most 16 snapshots
and an estimated 16 MiB. At 256 explicit scope revisions, the next new scope
revision advances a global fallback and invalidates old demand/cache; controllers
proactively re-capture keys. These are Git-specific limits, not a shared cache API.
Normal exit cancels every active runner and waits for cleanup outside GPUI.
The Linux runner drains nonblocking pipes; the retained non-Linux fallback uses
pipe-reader threads and has not been exercised here.

## Reviewable implementation steps

1. Implement native parser and bounded cancellable injected runner; preserve
   existing mark API while replacing unsafe path conversion/process lifecycle.
2. Implement pure shared request model and provider definition/projections;
   establish explicit scope/epoch and per-subscriber transitions in tests.
3. Thread one service through application construction, panels and Properties.
   Replace panel-only Git task ownership; capture request context independently
   of Properties source selection. Wire listing/editor/operation invalidation.
4. Run deterministic fixtures and concurrency/cancellation tests, then scratch
   app checks for column/Properties agreement and refresh after index changes.
5. Update README, TODO, implementation completion and progress evidence; run
   required fmt/clippy/full tests before the feature commit. No push or PR.

## Required evidence

Automated tests must not require installed Git. Captured porcelain fixtures cover
all marks, rename records, nested directories, ignored behavior and native Unix
non-UTF-8/newline/whitespace paths. Fake runners cover output/error caps, deadline,
kill/reap, stale completion, different scopes, epoch change with unchanged file
metadata, single acquisition for two consumers, cancellation of one subscriber,
last-subscriber cancellation, cache bounds and no-mark versus unavailable/failure.
Properties tests cover local-only authorization and independent baseline facts.

The running app uses a scratch repository and scratch XDG config/data. Check
column/Properties agreement for files and directories, ignored and renamed
entries, explicit refresh after staging without file-content changes, rapid
navigation and Properties tab switching, both panels sharing one scope, and
archive/remote exclusion where locally testable. Record actual results and
limitations in the progress log; do not claim unrun platform checks.


## Completed verification

Final `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`,
`cargo test --quiet` (706 passed, no failures/ignored), and `cargo build` pass.
Automated acquisition tests use controlled executables, not installed Git.
Sol reviewed the model and integration; the [progress log](progress.md) records
resolved review findings and actual live evidence. Final-build UI evidence is
in `/tmp/fyle-git-ui-1ck__zjx`, including an unchanged-file staging/refresh proof,
independent query scopes, archive exclusion, timeout presentation and normal quit
with a reaped active child. Windows and real remote accounts were not exercised.
The next slice compares both provider proofs before deciding broader migration.
