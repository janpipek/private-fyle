# A coherent format domain, with bounded extensions

This is an independent destination design, informed by the existing code. It does not authorize backlog implementation. User-defined format readers, properties, columns and preview renderers are core requirements. The recommendation prioritizes a coherent domain model over minimizing file movement: share recognition and semantic fact identity; retain separate inspection, column and preview presentation contracts; make built-in and user contributions meet the same execution contracts without forcing every Rust reader through a universal dynamic data model.

## What the current implementation establishes

The following findings are specific reasons for the design, rather than endorsements of every existing seam:

- `src/preview.rs:74` reads an 8 KiB prefix into `Probe`, while `src/properties.rs:178` has a different, extension-based probe. `src/icons.rs:59` chooses icons from names/extensions, and `src/apps.rs:163` obtains MIME associations from filename detection. Recognition is duplicated and consumers can disagree.
- `src/preview.rs:248` orders specific previewers before generic archive/text/binary ones. ZIP-based office documents explicitly precede archives. Recognition must preserve container relationships and ambiguous evidence; a single MIME string cannot express this entire policy.
- `src/properties.rs:214` already permits independent matching providers and failures; `src/preview.rs:230` selects one display producer. These contracts deserve different registries. Their similar shape does not imply identical meaning.
- `src/columns.rs:14` and `:97` close the column catalog over enum variants. Stable settings identifiers already exist at `:121`, but custom columns require a dynamic catalog. `src/panel/metadata/model.rs:12` bounds cache/queue/workers at 256/512/4, and `src/panel/metadata/reader.rs:11` reads only demanded fields. Retain this laziness.
- Shared semantic media and geography data exist in `src/formats/media.rs:54` and `src/formats/geo.rs:25`. Geography Properties converts fractional distances/durations into text (`src/properties/geo.rs:54` onward), losing numeric semantics at presentation time. EXIF is less unified than documentation suggests: columns use `src/formats/exif.rs:86`, while Properties implements its own bounded, cancellable parser (`src/properties/exif_reader.rs:86`). Consolidation must retain the latter's stronger bounds and its rule against image decoding.
- Parquet footer validation is shared (`src/formats/parquet.rs:29`), but row sampling belongs to preview (`src/preview/parquet.rs:38`). PDF mixes recognition, metadata probing and rasterization (`src/preview/pdf.rs:22`, `:26`, `:77`); its `Command::output` calls have no explicit deadline/output cap. Geography preview invokes its cancellable reader with `|| false` (`src/preview/geo.rs:34`). Rejecting stale results alone does not stop expensive work.
- `src/tools/detect.rs:26` already returns multiple project facets, but ancestor depth is effectively unlimited and marker content uses unrestricted `read_to_string`. `src/tools.rs:73` derives tool kinds from `FileIcon`, coupling semantic applicability to icon vocabulary.
- `src/raw_preview.rs:20` checks size/time freshness and publishes atomically; generic preview caching (`src/preview.rs:62`) keys by path/time only. `src/preview/session.rs:25` retains typed source identity beside a local path. These are useful foundations, but not yet a uniform freshness guarantee.

The architecture plan's shared readers and feature-owned state are sound. Departures are intentional: dynamic columns replace the fixed catalog; EXIF becomes a normal fact producer rather than a privileged parsing path; recognition stops being independently defined by each feature; preview selection distinguishes a reading capability from a rendering choice. The plan's first-match preview policy remains a reasonable default, but cannot define the whole extension model. Its historical statements that Properties implementation is pending are stale relative to the source and Properties plan.

## Destination: evidence, facts, projections and resources

Recognition answers “what might this source contain?” A reader validates and obtains data. A semantic fact answers a stable question with provenance, unit and completeness. A property section organizes inspected facts. A column projects a small value for a row. A preview representation describes a bounded thing to show, and a renderer turns that representation into widgets or raster output. None of these should imply that all the others exist.

Three meanings of “group” remain separate. `PropertySectionId` groups fields/tables within one item's inspection. `ColumnGroupId` organizes the column chooser; it schedules no I/O. `RelatedGroup` associates physical entries, such as RAW/JPEG/XMP; it changes neither identities nor operation targets automatically. Use distinct types and modules, not a shared `Group` enum.

Keep small Rust enums for closed concepts: evidence level, value kinds, completion states, demand shape and standard representation kinds. Use validated namespaced identifiers for extensible concepts: formats, facets, readers, facts, sections, columns and renderers. Built-in parsers keep their domain types (`GeoData`, `MediaMetadata`, Parquet schema), with projections into portable facts at the extension seam. This preserves compile-time clarity without excluding custom formats.

Illustrative contracts (support types abbreviated):

```rust
pub struct FormatId(pub String); // validated at registration
pub struct FactId(pub String);
pub enum EvidenceKind { Name, Extension, Mime, Signature, Structure }
pub struct Candidate {
    pub format: FormatId,
    pub evidence: Vec<Evidence>,
    pub mime: Option<String>,
}
pub trait MimeDetector: Send + Sync {
    fn detect(&self, probe: &RecognitionProbe, ctl: &ReadControl)
        -> io::Result<Vec<MimeEvidence>>;
}
pub enum Demand {
    Facts(Vec<FactId>),
    Preview(PreviewRequest),
}
pub trait Reader: Send + Sync {
    fn descriptor(&self) -> &ReaderDescriptor;
    fn read(&self, source: &ReadableSource, demand: &Demand,
        ctl: &ReadControl) -> io::Result<ReadBatch>;
}
pub enum FactValue {
    Text(String), Unsigned(u64), Signed(i64), Boolean(bool),
    Decimal { value: f64, unit: Unit },
    Time(SystemTime), Path(PathBuf),
}
pub struct Fact {
    pub id: FactId,
    pub value: FactValue,
    pub origin: FactOrigin,
}
pub struct ColumnSpec {
    pub id: ColumnId,
    pub group: ColumnGroupId,
    pub facts: Vec<FactId>,
    pub projection: ColumnProjection,
    pub sort: Option<SortProjection>,
}
```

Validate finite decimal values, units, field/table shapes and maximum text lengths. A fact ID's schema is immutable: `media.duration_seconds` does not sometimes mean a formatted duration. Distinguish absent, unsupported, failed and truncated results in `ReadBatch`; truncation belongs to the affected dataset, not just the whole inspection. Unknown differs from zero. An error retains reader/source context, uses `io::Result` internally, and becomes `<path>: <error>` only at the UI seam. Interrupted work is obsolete work, not a permanent failed section.

Property definitions explicitly select facts and bounded tables, labels/order and optional nesting. Columns explicitly declare dependencies and formatting/composition; they are never generated automatically from every property. Custom columns can combine camera make/model or activity totals. Sorting uses typed projections where declared, with a documented unknown-value order; do not silently launch full-folder expensive reads for sorting. Preserve existing cheap built-in `SortKey` behavior while deciding whether incomplete metadata sorting is acceptable.

## Recognition and directories

`recognition` produces an immutable evidence snapshot from listing names immediately, and optionally refines it through bounded content access. Reuse `mime::MimeDb` for filename/alias/subclass evidence. Put the swappable MIME detector here; a bounded `file` adapter or Rust adapter can complement it. MIME remains association vocabulary; `FormatId` identifies reader capabilities. XML is a container candidate, GPX/XMP are structural candidates; ZIP does not override an evidenced XLSX. Valid UTF-8 does not prove Python, YAML or shader syntax. Keep conflicting candidates with reasons; a deterministic specificity rule chooses an icon/default preview, while Open With can offer compatible associations.

Bound head/tail probes, nested-container checks and every detector invocation. No whole-file parser runs just to paint a row. A confident signature contradicting a suffix disqualifies the suffix's default specialized preview, but explicit user choice can still request that reader and receive validation errors. Ambiguous text remains generic text unless names, declarations or bounded structural checks support a subtype.

Directory recognition returns facets with evidence and scope: “this directory” and “nearest enclosing root” are distinct. A directory containing `.git`, `Cargo.toml`, `pyproject.toml`, and `.idea` can simultaneously be Git, Cargo, Python and IDE-associated. `.idea` alone does not prove which JetBrains product should open it. Bound ancestor count, marker bytes, child count and deadline; expose incompleteness. Direct marker checks need no recursive tree scan. Keep Git origin parsing as a demanded enrichment, not a prerequisite for displaying the Git facet. Tools project their current `Facts` from these facets, preserving nearest-per-kind behavior; icon choice remains presentation.

## User contributions and renderers

Recommend a versioned package manifest plus a bounded portable execution protocol. A package declares recognizers, fact schemas, readers, property definitions, column definitions and renderer offers; it need not supply all six. Definitions are declarative wherever possible. Rust built-ins implement the same descriptors and reader/renderer interfaces directly. Portable readers accept demand/source context and return validated fact or representation batches. User renderers transform representations into standard text/table/property/image/page outputs or a restricted display tree; they never return GPUI objects.

The strongest initial runtime recommendation is a worker process running restricted Lua through the already-used `mlua`, with host-mediated source reads, bounded allocation/instructions, and no unrestricted `io`, `os` or `require`. It avoids a new language dependency and separates inspection extensions from existing tools Lua state (`src/tools/lua.rs:64` uses a shared mutex/VM without inspection budgets). A typed process protocol also permits user executables when explicitly enabled. Their execution is trusted local code, not sandboxed merely by an argv interface. A worker process enables termination; OS memory/CPU limits are still necessary for hard bounds. Validate actual platform support before promising equivalent hard isolation on Windows.

WASM offers stronger capability isolation but adds dependencies/tooling; defer that choice pending a small runtime comparison. Native shared-library plugins have unstable Rust ABI/crash ownership and are rejected. An unrestricted HTML/web renderer introduces network, scripts and layout complexity unrelated to file-manager needs; reject it for this destination. Standard outputs plus bounded raster/vector primitives satisfy custom visual rendering without letting plugins own focus, windows or app commands.

Renderer requests include viewport/page/row range, pixel budget, style and resource version. Host widgets handle scrolling, text selection and navigation; custom renderers can generate charts, maps and document pages within these contracts. Nested display trees have depth/node/text/table caps. Images are owned resource handles, not arbitrary path strings; verify size/dimensions and import only files produced in the request's private output directory. Disable automatic URI fetching and interactive actions in generated Markdown. The finite vocabulary limits exotic interactivity, a deliberate tradeoff; add capabilities only for a concrete user need.

IDs follow `package.local_name`; reserve `fyle.*`, retain aliases for existing column IDs. Reject duplicate IDs/schema mismatches at registration with actionable diagnostics. Replacement of a built-in offer requires an explicit owner configuration entry, never load order. Pin protocol major version and representation schema version; reject incompatible packages, ignore documented optional fields, and include package revision in caches. Uninstalled custom columns remain preserved as unresolved settings IDs so reinstall restores them. Load only the owner's configuration packages; never automatically execute project-local definitions. Automatic readers differ from confirmed Tools commands: configuration enablement authorizes background execution under declared capabilities. Processes/network access require separate explicit grants, not repeated per-file prompts.

## Ownership, lifetime and bounds

Use `source.rs` and `source/` for readable leases and materialization, consuming `Location`, listing/provider/archive modules. A `ReadableSource` contains original identity, local access, original listing snapshot, revision token and lease. Readers receive access; they do not download, extract or decide writeback. Properties keeps listing-only remote/archive behavior until explicitly enabled materialized inspection exists. A downloaded file's permissions/mtime never become remote filesystem facts. Symlink object facts and target facts carry distinct provenance; broken links retain useful inspection.

Source revisions use provider tokens where available; local open-handle metadata plus size/time/identity gives best-effort freshness, with validation before publishing. Same-size same-time rewrites remain a documented limitation; refresh/editor-save epochs invalidate regardless. Check symlink target changes too. Archive revision includes outer archive state and member identity. No canonicalization should erase source spelling or alias identities.

Caches key source revision, producer/version, demand and limits; raster caches also key page/viewport/style. Results are owned bounded values or `Arc` datasets; asynchronous resources hold leases, and eviction/inspector close releases them. Keep generated RAW previews, remote materialized originals and PDF page rasters as distinct cache products. Byte-bounded LRU and atomic publication replace unbounded temp-directory accumulation.

Controllers preserve request generations. A small UI-independent read coordinator coalesces compatible same-source demands, retains only relevant pending work, and enforces total and expensive-process concurrency. Preserve existing metadata 4-worker and Properties 1-worker limits initially; cancelled workers retain slots until exit. A dropped GPUI task does not count as cancellation. Pass `ReadControl` deadlines/cancellation into every reader/render worker. Drain capped stdout/stderr concurrently, kill and reap timed-out processes, and handle descendants. Missing optional programs yield capability-specific availability reasons; Properties metadata should not depend on a page renderer being installed.

Do not pretend library decoding is interruptible or heap-bounded by serialized byte limits. Parquet's existing 8 MiB footer can expand in memory; XLS/ODS dense ranges remain a known risk. Smaller output caps help but cannot prove decoder allocation bounds. Expensive untrusted inputs need isolated workers or a streaming decoder contract before claiming hard limits.

## Modules and examples

Use `recognition.rs` plus `recognition/{file,directory,mime}.rs`; `facts.rs` for IDs/schema/results; `formats.rs` plus format modules for typed readers; `source.rs` plus access/materialization modules; and `extensions.rs` plus manifest/protocol/runtime modules. `properties/`, `columns/`, and `preview/` own projections, catalogs and sessions. `preview/renderers/` owns raster generation; GPUI widgets remain in view modules. `related.rs` owns association evidence. GUI/feature execution depends on these modules; formats/facts/recognition never import GPUI or feature presentation. Rust-only format functions need no trait wrapper unless being registered or replaced.

RAW recognition enables EXIF facts and an embedded-image representation independently. Consolidate the bounded EXIF implementation; Properties/columns share tag identity, while RAW thumbnail/F3 share JPEG selection and cache ownership. Reading capture metadata never decodes an image just to find EXIF.

Parquet requests metadata/schema without decoding rows; View separately asks for a 500-row/40-column sample with its 64 MiB chunk budget. Both use footer validation. Conversions must request complete/streaming data and cannot mistake a sample for a dataset. PDF facts come from a bounded metadata reader even without `pdftoppm`; page renderer requests are lazy, cancellable and pixel-bounded. Future continuous scrolling changes scheduling rather than the PDF format model.

GPX/FIT share typed activity/route facts and bounded segments. Properties may request summary only; outline rendering consumes segments, preserving FIT session-total precedence. A facts-only result must report that inferred totals are sampled when input limits prevent complete inference. A remote GPX initially gets filename evidence, then refined evidence only after materialization. A symlink's activity facts refer to its target while link facts retain the original object.

Future RAW/JPEG/XMP associations are scoped sibling-evidence groups, with ambiguous same-stem matches preserved. XMP metadata remains a separately originated fact; rating precedence and propagation are product decisions. Copy/move/rename/delete target expansion is an explicit operation plan, never an automatic side effect of matching a group.

## Migration and verification

1. Introduce extensible identifiers/descriptors and source revision/result contracts; retain compatibility aliases and existing behavior. Add package validation tests before executing extensions.
2. Consolidate EXIF behind one bounded fact reader; add numeric geography/media projections. Port existing parser fixtures/limit tests, verifying Properties and columns preserve their presentations.
3. Introduce shared evidence snapshots and directory facets; adapt icons, MIME associations, tools and matching predicates. Test wrong suffixes, XML/ZIP subtypes, extensionless files, multi-facet directories and truncated probes.
4. Move PDF metadata/page work into separate interfaces and add actual cancellation, process bounds and leased caches. Test with injected process adapters and fixtures, not installed Poppler/ffmpeg.
5. Replace column enum persistence/catalog with IDs and explicit projections; adapt metadata demands. Preserve defaults/comments and test missing packages, type-correct sorting and bounded queues.
6. Deliver one custom format package end-to-end: recognition, reader, facts, Properties, custom column and custom renderer. Use the same contract tests as Rust built-ins. Test collisions/version rejection, infinite loops, output overflow, invalid images, process exit and cancellation-slot retention.
7. Consolidate the coordinator/materialization seams as consumers migrate; remove duplicate recognition/parser/cache paths. Add related groups only after action semantics are decided.

Every implementation commit runs fmt, clippy and tests; affected inspector, column and preview flows require scratch-config UI checks. Test source changes during reads, rapid cursor/tab changes, obsolete pages, remote identity/cache provenance and broken/retargeted symlinks. No runtime-dependent test requires network or optional programs.

Open decisions: exact runtime and capability-grant UX; dynamic metadata sorting; default preview ranking and explicit alternate-view selection; directory marker rules/IDE choice; remote content-inspection opt-in; related-member operation/rating policy. Uncertainty remains around hard decoder/process-tree bounds across platforms and how much custom interactivity owners require. Those uncertainties should shape acceptance tests, rather than justify a universal format object or silently unbounded execution.
