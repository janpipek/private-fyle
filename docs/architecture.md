# Architecture

This is the current module map. The
[`architecture refactoring record`](architecture-plan.md) explains the
ownership boundaries and completed migration slices.
The [data-provider implementation plan](format-organization/implementation-plan.md)
and [progress log](format-organization/progress.md) track the contribution
and extension migration. Its contracts live in `src/data_providers.rs` and
`src/data_providers/`: provider-owned presentation definitions, validated IDs
and quantities, captured applicability, result completeness and provenance.
They perform no I/O; source freshness and stale-result checks stay with the
consumer that owns the request. The slice 7
[proof review](format-organization/proof-review.md) found no need for a
universal observation or cache identity. `data_providers/geo.rs` owns the
GPX/FIT contribution bundle and bounded native acquisition used by the existing
Properties and View adapters. Its typed
quantities retain recorded/inferred provenance and independent completeness;
format readers remain under `formats/geo`. Each feature keeps its own registry,
scheduling and rendering. `columns::Catalog` resolves persisted IDs to built-in,
registered native-provider or unavailable descriptors; its native provider
readers run through the existing metadata worker queue. `extensions` validates
versioned manifests and loads bounded, unevaluated scripts for explicitly named
packages, preserving per-package filesystem diagnostics. Its standalone Linux
supervisor runs one Lua request at a time in the same executable's headless
helper mode, with bounded source brokerage, protocol output, Lua allocation,
instructions and wall time. Cancellation kills and reaps the helper before
releasing its slot. Static startup preparation builds one immutable snapshot
of feature registries from enabled packages; callbacks execute only through background
feature demand. Ready values retain completeness through the cache and pure
cell projection. Main retains the shared supervisor and shuts it down after
GPUI exits. Existing Tools plugins retain their separate behavior.
`extensions/results.rs` prepares separate typed column, Properties field and
scene results. The internal protocol is version 2; existing column Lua source
remains compatible. Both the worker and host validate scenes, and Properties
retains per-field completeness and independent failures. Application Properties
and preview adapters use the same immutable enabled package snapshot and supervisor, while retaining separate selection, cancellation
and stale-result policies. `preview_scene` defines validated stacks, selectable
text, typed tables and bounded paths, with private construction after schema and
resource checks; `preview_scene_view` draws them with native theme and selection
behavior. Preview requests retain their original source and explicitly cancel
package work on replacement, reload or suspension.

`data_providers/git.rs` owns one contextual Git contribution definition, scoped
refresh epochs, independent subscriber leases, the tested column request model
and bounded shared snapshots. The column and `properties/git.rs` consume that
same provider/service. `git_status.rs` owns native-path porcelain parsing and
background process acquisition: Linux drains bounded nonblocking pipes under
one three-second deadline, kills/reaps cancelled children and rejects incomplete
or oversized output. Main waits for Git service shutdown after the GUI exits.
See the [Git boundary](format-organization/git-plan.md) for limits and policy.

Model first: `src/model.rs` holds each panel's state (`Folder`: typed
`PanelContent` with `Directory(Location)`, `Search { root, title }` and
`Places` modes, rows, cursor, marks, sort, hidden files, tree expansion) with
no UI code and its own unit tests. Navigation destinations use `PanelTarget`,
which keeps the places overview separate from file addresses.
`src/panel.rs` manages the panel view, input and focus on top of a `Folder`;
`panel/table_view.rs` renders the `DataTable` and forwards its events. The
UI-independent `src/listing.rs` service owns local, archive and remote listing
I/O; `src/listing/contracts.rs` owns the request and result values shared with
`Folder`. Pure remote and archive address semantics live in
`src/location/remote.rs` and `src/location/archive.rs`; provider and archive I/O
remain in their execution modules. Commander runs requests in cancellable
background tasks and applies only the current result; `Folder` sorts and
filters the cached rows without I/O.
Each panel keeps a bounded cache of its last 32 remote listings, while explicit
refreshes bypass that cache.
`src/commander.rs` owns the two models and their views and implements the
commands. `src/entry.rs` owns shared `Entry`/`Attrs` values, sort settings and
pure ordering; `src/fs_ops.rs` owns file system operations plus native metadata
and account-name adapters. Owner names are resolved by the existing background
metadata worker. `Entry.size` is optional, so a provider's missing size stays
distinct from a known zero.
`src/location.rs` identifies local paths, configured remote paths and archive
members without a live connection. Local paths retain native bytes and symlink
spelling; pure modules own remote parsing and archive member validation.
`Folder` derives its path spelling from `PanelContent`; rows, marks, history,
activation and remote transfer
selections carry typed identities. `Folder::path()` and `Entry::path` remain
compatibility spellings for panel and native-operation boundaries; virtual
spellings must not be passed to native filesystem calls.

`src/formats/exif.rs` owns ordered semantic EXIF values and bounded,
metadata-only Properties reads. `src/data_providers/exif.rs` binds those tags
to the existing Properties fields and built-in EXIF column IDs; the Properties
view only labels semantic tags. Properties and column acquisition keep their
own limits and request lifecycles. `src/formats/raw.rs`
owns the bounded embedded-JPEG selector; `src/raw_preview.rs` owns its cache
identity and atomic materialization.
`src/tools/` is the model for context-dependent tools (run ruff on Python
files, uv commands in uv projects, convert images, …); see
[`docs/tools.md`](tools.md). F2 opens them; your own go in
`<config dir>/fyle/tools.toml`, or in Lua plugins in `<config dir>/fyle/plugins/*.lua`.
Folder tests use `model/test_support.rs::TestFolder`, a synchronous driver for
`ListingService` and the production listing/history commits, with an in-memory
provider for remote fixtures. The Folder model has no separate test-only listing
implementation, remote cache or archive index.
`src/navigation.rs` owns per-panel navigation request freshness and pending typed
destinations. Commander keeps task scheduling and cancellation signals, with
`breadcrumbs::NavigationGuard` separately checking the source folder. The boundary
and regression coverage are recorded in
[`docs/navigation-lifecycle-plan.md`](navigation-lifecycle-plan.md). Pending
listings keep the current rows visible; obsolete archive/remote loading
placeholders and the unused archive controller have been removed.
`src/settings.rs` owns the config schema, defaults, validation and path expansion;
`src/settings/persistence.rs` owns config-path discovery, file loading/creation
and comment-preserving saves through `toml_edit`.
`src/editor.rs` loads/saves edited files (UTF-8 only, atomic save keeping
permissions); `src/editor_view.rs` renders each editor in its own window.
`src/dirsize.rs` measures directories recursively (on demand, cancellable, UI-independent); `Folder` caches the results per path until navigation or refresh. A hard-linked file counts once per measured folder, so each size stands alone. Other reloads (after file operations, saves) keep the sizes.
`src/search.rs` finds files and searches contents with ripgrep's libraries
(`ignore`, `grep-searcher`); results are a mode of the `Folder` model.
`src/transfer.rs` copies and moves with conflict resolution (merging
directories, unique names, never writing through aliases of the source);
`src/conflict_view.rs` formats the conflict details.
`src/icons.rs` maps names/extensions to semantic icon kinds; the table view
picks the glyph for each kind. `src/links.rs` holds the UI-independent
symlink state (`LinkState`: not a link / link with target / broken with
reason), probed in `fs_ops::read_dir` from the metadata already fetched (one
`read_link` per shown link; the resolved path is computed for tooltips only);
`src/link_badge.rs` draws the badge over an icon.
`src/breadcrumbs.rs` derives panel path segments, parses edited paths and guards
background navigation results. `src/places.rs` discovers grouped desktop places,
Linux mounts, bookmarks and frequent paths; `src/location_view.rs` renders
them as the locations menu and the searchable picker. Path loading and place discovery run in the background.

`src/archive.rs` reads zip, tar and tar.gz archives into an `Index` (a tree of
nodes). `src/archive/extraction.rs` safely extracts selected entries. Inside
an archive a panel uses a typed archive location with its inner member (shown
as `file.zip › inner/dir`); `locate` only converts native path spellings at
input boundaries. The listing
service reads the index in the background, supports cancellation, and returns
typed rows for navigation and sorting. Preview loads an entry by extracting it
to the preview cache. The shared index cache is keyed by path, size and
modification time. Archives are
read-only. Extraction (`jobs::run_extract`) writes to a
staging folder in the destination, then moves the result with the usual
conflict resolution (the dialog names the archive entry, `file.zip › inner/name`,
as the source; the status counts extracted files); names with `..` or absolute paths are skipped, links
must stay inside the destination and nothing is written through a link.

Where things live (`src/`):

| Module | Role |
|---|---|
| `bookmarks.rs` | Local and remote bookmark toggling in memory and in `config.toml` |
| `main.rs` | Startup, settings, start directories, window |
| `window_chrome_view.rs` | Applies the immutable startup title bar choice to window options and rendering; Commander keeps the saved next-start preference separate |
| `commander.rs`, `commander/` | Main window: the two panels, actions, dialogs, jobs bar, tools orchestration. Split by area into `commander/` (keys, navigation, search, files, jobs, view, archives, remotes, tools, menus, appearance); new logic goes into its own module, only wiring here. |
| `menus.rs` | `Cmd`: every user-visible command with labels, availability (with an explanation) and toggle state, plus the menu bar and context menu layouts. Menus, palette and F1 help are built from it; `commander::keys::action_for` maps each `Cmd` to its action. |
| `entry.rs`, `model.rs`, `location.rs`, `location/`, `location_codec.rs` | Shared `Entry`/`Attrs` and pure sort rules; `Folder`: typed location, cached rows, cursor, marks, filter, sort, tree, search results, archive mount and directory size cache; pure local, remote and archive address semantics; versioned lossless location persistence |
| `formats.rs`, `formats/` | Semantic EXIF fields and bounded metadata parsing, bounded RAW embedded-JPEG selection, Parquet footer validation/decoding, RPM/Debian package metadata, and shared bounded GPX/FIT route/activity readers used by Preview and Properties |
| `data_providers.rs`, `data_providers/` | Pure contribution definitions and typed result vocabulary; `geo.rs` owns the GPX/FIT Properties/viewer bundle and typed quantities; `media.rs` binds Duration, Properties and Preview to one typed definition with independent scheduling; `parquet.rs` binds metadata Properties and row-sample Preview to the checked native footer; `exif.rs` binds bounded Properties facts and existing EXIF columns to shared tags; `git.rs` owns contextual Git contributions, shared bounded snapshots, scope epochs and subscriber/request lifecycles |
| `extensions.rs`, `extensions/` | Static manifest/package validation, enabled-package Columns/Properties/View registration, typed protocol/result validation and retained-file brokerage; restricted Lua worker and shared Linux supervisor with bounded queue, cancellation/deadline and child reaping. Reserved helper dispatch precedes config/GUI startup; main owns normal-exit cleanup. |
| `copy_path.rs` | UI-independent selection path policy: local native paths, provider supplied standard URLs and no archive pseudo-paths |
| `listing.rs`, `listing/contracts.rs` | Cancellable background listing service for local directories, Places, archives, search refresh and remote providers; shared request/result values and bounded per-panel remote listing cache |
| `panel.rs`, `panel/header_view.rs`, `panel/table_view.rs`, `panel/metadata.rs`, `panel/metadata/`, `panel/column_filter.rs`, `panel/column_filter_view.rs` | `FolderView` owns input/menu lifecycle, focus and list/icon coordination; `header_view.rs` renders breadcrumbs, location editing and header buttons; `table_view.rs` owns `FileTable` rendering and forwarding to `Folder`; `column_filter.rs` runs one cancellable full-listing filter/facet scan per panel; `column_filter_view.rs` edits text or categorical rules; `metadata/model.rs` owns bounded render-cache request policy; `metadata/reader.rs` reads optional local values; `metadata.rs` schedules GPUI workers shared by list and icon views; `metadata/exif.rs` adapts provider-owned EXIF columns to the metadata queue |
| `column_filter.rs`, `model/column_filter.rs`, `columns/display.rs` | Pure AND/OR matching, distinct value outcomes, bounded category aggregation and scan identity; Folder owns filter rules and row projection; shared listing-column text formatting keeps size, extension, permissions and time matches aligned with table cells |
| `thumbnails.rs`, `thumb_queue.rs`, `raw_preview.rs` | Icon view thumbnails: the freedesktop cache (URI and MD5 names, validity by `Thumb::MTime`, atomic writes, failure records), RAW preview cache identity and atomic JPEG materialization, and the load scheduler with its LRU, all UI-independent and tested |
| `grid.rs`, `icon_view.rs`, `glyphs.rs`, `context_menu.rs` | Icon view: `ViewMode`, icon sizes, `Layout` (columns for a width) and cursor moves (UI-independent, tested); the lazy `IconGrid` view; the `FileIcon` → glyph table and the right-click menu shared with the list |
| `workspace.rs`, `inspector_view.rs` | Workspace layout and active panel roles; the inspector tab controller retains View/Properties selection while the inspector is closed or dock layout changes. |
| `columns.rs`, `columns/` | Persisted `ColumnId`s, mandatory Name/selection normalization, shared descriptor `Catalog` and native provider reader seam; `ColumnKind` retains specialized built-in rendering/sorting. Unknown IDs survive settings changes, provider cells use typed validated values, and provider/unavailable columns remain unsortable. |
| `fs_ops.rs`, `transfer.rs`, `jobs.rs` | Native metadata/account adapters and file operations; copy/move with conflict resolution; background jobs with progress and cancel |
| `remote.rs`, `remote/` | Remote providers: `Provider` trait; compatibility `Location` alias to `location::RemoteLocation`; Proton Drive (`proton.rs`), rclone (`rclone.rs`), shared CLI runner (`process.rs`), `remotes.toml` parsing (`connections.rs`) and management persistence (`manage.rs`, `manage/credentials.rs`), native saved-password operations (`credentials.rs`), SFTP host key check and typed unknown-key confirmation/persistence (`hostkey.rs`), SFTP/FTP/WebDAV/S3 (`remotefs.rs`); `remote/jobs.rs` plans and runs transfers, removals and the download cache; `LocalCopy` keeps a typed source address separate from the cache path used at native I/O boundaries; `model/remote.rs` holds cached listings |
| `archive.rs`, `archive/extraction.rs`, `dirsize.rs`, `links.rs` | Archive index; safe extraction; recursive sizes; symlink state |
| `preview/`, `preview_view.rs`, `notebook_view.rs`, `presentation_view.rs` | `Previewer`s and their `Registry` (including bounded nbformat 4 notebook parsing, PowerPoint parsing and GPX/FIT outline generation); one render arm per `Content`; notebooks and presentations render through variable-height lazy lists; `PreviewSession` owns target identity, reloads, navigation and stale-result acceptance; GUI resources stay in the pane |
| `xattr.rs`, `xattr/inspection.rs` | Bounded no-follow native name/value reads shared with columns; UI-independent Properties snapshots, typed text/hex prefixes, budgets and per-attribute failures |
| `properties.rs`, `properties/`, `properties_view.rs` | `properties.rs` owns shared result contracts, section/group policy and cancellable `PropertiesSession`; `properties/registry.rs` orchestrates native providers and enabled typed extension sections; `properties/filesystem.rs` reads local metadata and symlink facts; format modules map shared EXIF, Parquet, media, GPX/FIT and package readers into groups; `properties/xattrs.rs` projects bounded `xattr/inspection.rs` snapshots, while `properties/sqlite.rs` reads database structure; the GPUI controller limits inspection to one active worker with the latest selection queued; selectable text runs support cross-field copying and Ctrl-C takes priority over panel file yank while a text selection exists |
| `preview_scene.rs`, `preview_scene/`, `preview_scene_view.rs` | Pure strict scene DTOs and bounded validation; native selectable stacks/tables and clipped path painting from validated data only |
| `editor.rs`, `editor_view.rs` | Text document model (UTF-8, ≤ 8 MiB, atomic save) and separate editor windows |
| `tools/`, `tools_view.rs` | Tool, Action and Registry model; `defaults.toml`; built-ins; Lua plugins; `exec.rs` runs them |
| `search.rs`, `places.rs`, `breadcrumbs.rs`, `remote_labels.rs`, `rename.rs`, `openers.rs`, `terminal.rs`, `palette.rs`, `themes.rs`, `icons.rs`, `settings.rs`, `zoom.rs` | Further UI-independent models, each with tests |
| `navigation.rs` | Per-panel request tokens, pending destinations and cancellation/completion decisions; no GUI tasks or listing I/O |
| `settings/persistence.rs` | Config-path discovery, loading with defaults and path-qualified errors, example-file creation and comment-preserving setting saves |
| `recognition.rs`, `recognition/` | Shared bounded filename/content recognition, ranked MIME candidates and native detector; `properties/recognition.rs` exposes candidates and evidence for local regular files; caller-owned reads and freshness |
| `apps.rs`, `windows_apps.rs`, `mime.rs`, `desktop.rs`, `mimeapps.rs`, `xdg.rs`, `ini.rs` | Open With model: `apps::Catalog` (Linux and Windows discovery) and `merge` with the openers; Windows registry associations; MIME type from shared-mime-info `globs2`/`subclasses`/`aliases`; `.desktop` parsing and `Exec` expansion; `mimeapps.list`; XDG directories |
| `*_view.rs`, `menu_bar.rs`, `link_badge.rs`, `theme_picker.rs` | Thin views |
| `commander/remotes_view.rs`, `commander/remotes_view/` | Remote management controller, separate form rendering and dialog setup, background connection testing and saved-password status |

Saved remote passwords use the UI-independent `remote::credentials::CredentialStore`
seam. Consumers use the platform-neutral `SystemCredentialStore` facade; its
Linux adapter implements the interface through oo7 and the freedesktop Secret
Service API; the remote manager and connection workers perform store
operations off the UI thread. `remote::connections` persists only an opaque
credential ID in `remotes.toml`, with account identity based on protocol and
endpoint rather than the display name or root path. The interface leaves room
for native Keychain and Credential Manager adapters when those platforms are
implemented; those adapters do not exist yet, and credential portability is
not guaranteed.

Previews follow the same split. `src/preview/` is the model: a `Registry`
of `Previewer`s (directory, embedded-JPEG RAW images, image with EXIF, PDF via poppler, audio/video via ffmpeg, archive, DOCX/ODT documents (to Markdown), PPTX/PPTM slide text, Markdown, read-only IPYNB,
Parquet and CSV tables, text, binary fallback) turns a path into
plain `Content` data; the first previewer whose `matches` accepts the file's
`Probe` (name, extension, size, first 8 KiB) wins. PDF pages render on demand
in the background and can be browsed with Previous/Next controls; PDF metadata
stays in the model for future file properties. Overflowing binary, Markdown,
archive, notebook, table and detail previews show scrollbars. `src/preview_view.rs`
keeps debounce, background tasks, focus and editor/notebook image resources,
with one render arm per `Content` variant. `src/preview/session.rs` owns typed
target identity, line-only changes, reload resets, PDF page and workbook sheet
selection (with sheet tabs in the view), and generation checks for full loads,
page renders and sheet reads. A new file
type is a new `Previewer` registered before the fallbacks; only a new kind of
display needs a new `Content` variant and render arm.

`commander/hostkey_view.rs` presents typed unknown-key errors from navigation and
remote connection tests. Acceptance writes only the confirmed key on a background
worker, rechecks known_hosts for conflicts, and retries through the original
controller with its freshness checks. SSH callbacks never wait for the UI.
