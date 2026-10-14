# Code-informed B: extend the existing seams

This proposal adds a shared classification and semantic-fact seam, dynamic presentation catalogs, and a bounded user-extension runtime. It preserves the binary crate, the existing reader modules, and separate Preview, Properties and Tools orchestration. The smallest coherent change is **not** simply more enum variants: user-defined readers, properties, columns and preview renderers require runtime identities and contracts. Implementation remains subject to owner decisions in TODO.md; this document does not authorize those features.

## Evidence and constraints

The current foundation is already useful:

- `src/location.rs:44` distinguishes local, remote and archive addresses; `src/entry.rs:11` contains listing facts. `properties::Selection` explicitly combines identity and row (`src/properties.rs:24`). `remote::jobs::LocalCopy` keeps source and downloaded path separate (`src/remote/jobs.rs:414`); `PreviewTarget` also separates identity and working path (`src/preview/session.rs:25`).
- Recognition is fragmented: `icons::icon_for` classifies names/extensions (`src/icons.rs:59`), `MimeDb` uses shared-mime-info filename rules without magic (`src/mime.rs:1`), Preview reads an 8 KiB head (`src/preview.rs:57`), and Properties probes local metadata without a head (`src/properties.rs:178`). Directory project detection belongs to Tools and already returns multiple facets with nearest-root ancestor semantics (`src/tools/detect.rs:26`), but its ancestor walk and some text reads lack explicit bounds.
- Preview selects one first-matching `Previewer` (`src/preview.rs:230`, `src/preview.rs:246`); its `Content` is a closed enum (`src/preview.rs:126`) rendered by a match (`src/preview_view.rs:422`). Properties runs all matching providers and retains independent failures (`src/properties.rs:214`, `src/properties.rs:261`). The architecture deliberately rejects a universal registry (`docs/architecture-plan.md:118`).
- Shared EXIF, RAW, media, Parquet and GPX/FIT readers already exist under `formats/` (`src/formats.rs:1`). Columns are a fixed `ColumnKind` enum/catalog (`src/columns.rs:14`, `src/columns.rs:97`); settings store that enum (`src/settings.rs:108`). Panel optional metadata caches formatted strings with fixed fields (`src/panel/metadata/model.rs:21`, `src/panel/metadata/model.rs:72`).
- Lua currently defines Tools only, with a process-wide interpreter behind a mutex (`src/tools/lua.rs:64`). Properties fields have stable IDs but labels are mostly inferred by humanizing them (`src/properties_view.rs:233`). Current extension entry points do not provide user-defined readers or renderers.

Keep all existing previews: text/code, Markdown, raster/RAW, PDF, media, directory summary, binary, notebooks, presentations, archive listings, documents and tabular/workbook samples. Keep listing-only nonlocal Properties and existing column IDs/defaults. This follows README.md's current behavior and docs/properties-plan.md's independent inspection lifecycle. It departs from the architecture plan's fixed column catalog and from presentation groups being described as the semantic storage contract: extensible facts need their own identity and typing.

## Domain separation

A **format** describes an encoding or recognized artifact, not its icon or view. A file can have several candidates, such as TIFF container and camera RAW. A **directory facet** describes a bounded observation about a directory or its enclosing context; Git, Rust, Python and IDE facets coexist. A **reader** produces facts or bounded format data. Recognition does not invoke every reader.

A **fact** is a typed assertion with subject, provenance and completeness. A **property section** arranges facts/tables for inspection; a **column group** arranges column choices in the catalog; a **relationship group** links distinct physical items. These are separate types and namespaces. EXIF can be a property section and a column group without implying a RAW/JPEG/XMP relationship. A relationship group's members retain individual `Location`s; it is never a new filesystem identity.

A **preview representation** is bounded display data: a sample, outline, raster or page handle. A **renderer** presents that representation. Neither is a full document/dataset model suitable for conversion. Keep format-specific Rust structs for meaningful shared data; add a typed fact projection for discovery and plugins rather than translating all native models into a universal JSON tree.

Focused sketches (new types are proposals):

```rust
struct FormatId(String);
struct FacetId(String);
struct FactId(String);
struct ReaderId(String);
struct Recognition {
    formats: Vec<Candidate>, // id, MIME, evidence, certainty
    facets: Vec<DirectoryFacet>, // subject, root, scope, evidence
}
enum FactState<T> {
    NotRequested, Present(T), Absent, Unsupported(String),
    Failed(String), Truncated { value: T, reason: String },
}
struct Fact { id: FactId, subject: Location, value: Value, provenance: Provenance }
// Value: text, bool, signed/unsigned, finite decimal, time,
// quantity with unit, native path, typed source reference, bounded list/table.
```

Definitions specify units and meaning: `media.duration` is seconds, not an `mm:ss` string; `photo.dimensions` describes the photographed image, not necessarily the embedded JPEG. Do not merge two providers' conflicting assertions silently. An explicit producer preference supplies a canonical fact; Properties can expose provenance/conflicts. Preserve filesystem-object versus symlink-target subjects.

## Modules and demand contracts

Add small modules beside the existing ones, following `foo.rs` plus `foo/`:

```text
location.rs, entry.rs                  existing identities/listing contracts
classification.rs, classification/   evidence, detector adapters, directory probes
facts.rs, facts/                      definitions, states, demand/cache policy
formats.rs, formats/                 existing native readers and new reader adapters
extensions.rs, extensions/           manifests, validation, worker protocol
columns.rs, columns/                 dynamic descriptors + built-in adapters
properties.rs, properties/           sections, providers, inspection session
preview.rs, preview/                 representation offers, session, data adapters
preview_scene.rs                     validated declarative scene model
preview_scene_view.rs                GPUI scene interpreter
relationships.rs                     future rules over sibling source identities
```

Feature services consume identities, recognition and format readers/facts. Reader adapters do not import Properties, panel or GPUI. Extension workers consume a versioned plain protocol. Views consume presentation descriptors and ready data; Commander only coordinates. Preserve independent registry semantics rather than adding a global feature engine.

```rust
trait Detector: Send + Sync {
    fn detect(&self, input: &DetectionInput, limits: &ReadLimits)
        -> io::Result<Vec<Evidence>>;
}
trait FormatReader: Send + Sync {
    fn descriptor(&self) -> &ReaderDescriptor;
    fn read(&self, input: &ReadInput, demand: &Demand, control: &Control)
        -> io::Result<ReadResult>;
}
struct ColumnDef {
    id: ColumnId, label: String, group: ColumnGroupId,
    demand: FactDemand, format: CellFormat, sort: SortPolicy,
}
struct PropertySectionDef { id: SectionId, label: String, layout: PropertyLayout }
struct RendererDef { id: RendererId, accepts: SchemaId, input_version: u32 }
```

`Demand` distinguishes metadata, bounded row projection/sample, document page, workbook sheet, route geometry and raster size. Descriptors advertise supported demands, required programs and budgets. Missing facts stay blank in cells, with an available diagnostic; failure is not zero. Properties retains failed/truncated sections. A renderer gets validated data and request state, never a GPUI context or unrestricted source path lookup during rendering.

Classification starts with listing names, extensions and MIME database hints. Refine with bounded head/tail/signature evidence only when access exists. Inject detector backends (native reader or read-only external `file` adapter); neither command nor crate is mandatory. MIME supports Open With but does not replace stable format IDs. Recognizing ZIP cannot override stronger workbook/document evidence. Preserve competing ambiguous-text candidates; preview uses explicit preference/specificity and deterministic registration order before text/binary fallback. Evidence and user overrides remain inspectable.

## Actual user extensibility

Use a new `<config dir>/fyle/extensions/` package directory; existing tool Lua files retain their contract. A manifest declares API major/minor, extension/version ID, recognition rules, readers/output schemas, fact definitions, property sections, columns, renderers and program requirements. Built-ins use the same descriptors where practical. IDs are namespaced, e.g. `user.alice.sensor.temperature`; existing IDs such as `exif.iso` and `duration` remain aliases. Duplicate ownership fails validation; replacing a builtin requires an explicit override declaration, not load-order accident.

Validate manifests before registration: referenced IDs, typed values/units, source permissions, supported demands, bounded layouts, schema compatibility and renderer input schemas. Unsupported API major disables the package with a useful diagnostic; additive minor fields have declared optionality. Schema major changes require a new identity or migration. Preserve unknown column IDs in config and show them unavailable, so temporarily disabling a package does not destroy settings. Readers' versions/schema revisions enter cache keys. Hot reload replaces a validated registry snapshot atomically and invalidates dependent work.

Two authoring levels are sufficient: TOML declarative recognition/properties/columns plus argv JSON readers; Lua for parsing and custom preview construction. **User-defined renderers are Lua callbacks or argv workers returning a scene**, not merely selecting a builtin image/table view. A scene supports bounded rows/columns, selectable text, images by host-issued artifact handle, typed tables, a path canvas, badges and named event controls. A GPUI host interpreter renders those nodes. Events such as page/series selection send validated state to the worker for another bounded scene; they cannot directly run tool actions. A sensor plugin can draw its own graph using canvas paths, choose its layout and produce interaction controls without rebuilding fyle. Raster rendering is an additional route, not the only extension mechanism.

```lua
-- Manifest binds input schema, facts and demands; illustrative callback.
function render(data, state, host)
  return { type = "column", children = {
    { type = "text", text = data.title },
    { type = "canvas", paths = graph(data.samples), width = 800, height = 240 },
    { type = "button", label = "Next", event = "next_series" }
  }}
end
```

This requires a new interpreter and schema/event validation; current `Content` cannot deliver it. Add `Content::Extension` carrying renderer/schema identity and validated scene/session data, retaining existing variants. Arbitrary dynamically loaded native GPUI widgets are rejected: unstable Rust ABI, unsafe code and GPUI lifecycle ownership make that a different plugin system. The scene vocabulary is intentionally finite; new primitives require host work.

Run inspection Lua in a separate worker process using fyle's existing vendored Lua capability, with memory/instruction limits and a parent wall deadline. Host-provided bounded read/range/artifact calls support Lua readers; block ambient `io`, `os`, `debug`, module loading and network APIs. Process isolation permits termination if cancellation cannot interrupt parsing. Declared external argv readers/renderers receive validated input and have stdout/stderr, runtime and artifact limits. Such executables are trusted local code, not sandboxed merely by this protocol. Only builtin/user-config packages load; never auto-load project-local extensions. Explicitly enabling program-backed inspection authorizes automatic read-time execution, unlike Tools' per-action confirmation; that policy must be documented and decided before shipping. No dependency is added by this proposal; implementation must separately justify any isolation or scene dependency.

## Source identity and operational limits

`ReadInput` keeps typed source, source revision, original display name, link context and a separate readable local artifact/range handle. Materialization records origin, fetched revision and a lease; local-cache timestamps are not remote metadata. Properties continues listing-only for nonlocal sources unless an explicit new content-inspection policy is approved. Preview may download/extract under existing behavior, then refine recognition. Do not pass archive/remote compatibility path strings into native readers. A lease never grants writeback rights.

For symlinks, `lstat` facts describe the selected object; content reads follow a regular target with explicit provenance and target freshness. Broken links retain listing/link facts. Refresh, saves, target changes, package reload and source revision changes invalidate relevant demands.

Cache by typed identity, revision, link-target revision, reader/schema version, demand parameters and renderer/style version where appropriate. Share completed metadata and coalesce identical pending demands across Properties/columns/Preview, with separate subscribers: leaving View cancels its interest, not another consumer's work. Preserve bounded queue/cache/concurrency policies from `panel/metadata/model.rs:13` and generation acceptance from Preview/Properties sessions. Never free a worker slot before it exits. Use byte-weighted caches and artifact leases, not only entry counts. Negative results have bounded lifetimes; installing an optional program invalidates its unavailable status.

Visible rows request selected cheap column facts; the inspector requests its active perspective; navigation requests pages/sheets on demand. Enforce input, decoded-memory, output, collection, recursion, pixel, time and disk budgets. Existing noninterruptible Parquet/FIT decoding remains honestly reported, not made cancellable by adding a token. Process workers can be terminated; in-process libraries need allocation defenses. PDF currently uses unbounded `Command::output` (`src/preview/pdf.rs:24`, `src/preview/pdf.rs:71`); bringing it into these contracts requires real process-bound improvements.

## Worked cases

**RAW/EXIF:** recognize camera RAW plus its container; EXIF reader yields photo facts, embedded-JPEG reader yields an image artifact. Keep `formats/raw.rs:12`'s 64 MiB scan and decode/candidate limits. Reuse extraction for thumbnails/Preview, but EXIF can read JPEG bytes without materialization (`src/formats/exif.rs:95`). Photo dimensions and preview dimensions stay distinct. Future XMP/JPEG companions create relationships, not automatic metadata overrides.

**Parquet:** metadata demand reads the bounded footer (`src/formats/parquet.rs:10`); Properties projects schema/row-group sections and a user column can request row count. Preview separately requests its 500-row/40-column sample with 64 MiB selected-chunk budget (`src/preview/parquet.rs:13`). Cache footer metadata across both, never imply sampled rows constitute a conversion source.

**PDF:** split metadata demand from page raster demand. Poppler requirements attach independently; readable metadata can survive raster failure. Page requests carry index, resolution and source revision. Missing `pdftoppm` remains explanatory. Continuous scrolling would request visible pages under a separate approved feature, not eagerly rasterize the document.

**GPX/FIT:** retain shared `GeoData` (`src/formats/geo.rs:25`), 16 MiB/100,000-point bounds and FIT session-total precedence. Project quantities into property sections/columns; Preview renders theme-dependent geometry. User renderers may draw altitude/route scenes from declared data demands. No GPS means absent geometry, not failed duration metadata or an invented route.

**Git + Rust + Python + IDE directory:** bounded sibling-marker probes yield all facets with subject/root/scope. `.idea` is IDE evidence, not proof of a particular installed application. Properties displays project sections; Tools retains nearest enclosing-root resolution; Open With intersects facets with installed IDE adapters. Directory summary and eventual space view remain independent preview offers. Remote/archive marker inspection requires their source capabilities, never probing the cached file's parent directory as the source project.

**Remote/archive/link:** filename recognition works before materialization; downloaded/extracted bytes refine it with original identity intact. Remote revision may be unknown: freshness is explicitly uncertain and refresh bypasses reuse. Archive member revision includes enclosing archive revision. Symlink content facts identify the target while operations still target the selected link.

## Delivery and choices remaining

Add a builtin format by registering recognition, a bounded shared reader and typed output, then only the required fact/presentation/renderer adapters. Add a user extension by writing its manifest and reader/render callback, validating it offline, enabling it in user config, and selecting its columns/preview. Neither workflow needs edits to every consumer.

Migrate incrementally: identity/recognition contracts and adapters; dynamic column IDs preserving defaults/settings; fact definitions and EXIF/media reuse; extension validation/workers; scene renderer; remaining formats when needed. Keep separate registry selection semantics and existing Properties baseline sections throughout. Test malformed manifests, duplicates, incompatible schemas, unknown saved IDs, ambiguous detection, multi-facet roots, partial failures, coalescing, stale results, link target changes, worker termination, output bombs, and artifact cleanup using TempDir fixtures and fake processes. Required fmt/clippy/tests precede implementation commits; GUI checks use scratch config/data. Optional-program tests never require Poppler/ffmpeg/network.

Reject a monolithic `FileType` enum, automatic property-to-column generation, eagerly parsing every candidate, universal reader results based on preview samples, and filesystem groups that silently expand operations. Relationship matching/actions, ambiguous-text preferences, plugin execution trust, initial scene primitives, column sorting costs and nonlocal content-inspection policy remain owner decisions. This is the minimum coherent extension of current seams; it is still a real feature program, not a documentation-only refactor.
