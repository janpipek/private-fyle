# A demand-driven inspection domain with an open presentation protocol

## Provenance and recommendation

This is an independent, feature-only, clean-sheet proposal based exclusively on `/tmp/fyle-format-feature-brief.txt`. No repository code, project documentation, other proposals, or external sources were inspected. Current implementation compatibility and migration cost cannot be verified. Illustrative contracts below are design sketches, not executable changes.

Recommend one inspection domain that owns source identity, recognition evidence, typed facts, bounded representations, and demand scheduling. Properties, columns, and View are separate consumers. None owns format knowledge. Built-in Rust providers and trusted user providers register through the same schema-based contracts. User renderers produce a constrained, interactive UI description interpreted by native GPUI views; they do not dynamically execute arbitrary GPUI code.

User-defined readers, properties, columns, and renderers are core capabilities. Signature recognition, directory facets, related assets, continuous PDF, incremental tables, and directory space views are design provisions, not implementation authorization. Preserve the brief's existing behavior and numerical limits.

## Domain distinctions

An **entry** identifies an object in its source namespace. A **content access** grants access to bytes, possibly through a downloaded or extracted artifact. A **format candidate** records recognition evidence; it is neither an entry kind nor a promise of successful decoding. A **directory facet** describes a detected capability or project context; several facets coexist.

A **fact** is a typed semantic observation with provenance. A **property section** presents facts, possibly in nested groups or tables. A **column** computes a sortable cell from facts. A **representation** describes content for View, such as a sampled table or route. A **renderer** maps a representation to permitted presentation nodes. Related-asset groups have membership and action-policy questions independent of all these categories.

Use enums for closed host guarantees: entry kinds, datum states, scalar values, demand kinds, presentation primitives. Use namespaced IDs and registered schemas for open sets: formats, facts, representations, providers, sections, columns, renderers. Use traits at executable Rust boundaries, not for every noun.

```rust
struct EntryRef {
    id: SourceEntryId,             // local / archive member / remote namespace
    locator: SourceLocator,       // native local path preserves arbitrary bytes
    listing_revision: Revision,
}
struct Subject {
    entry: EntryRef,
    facet: SubjectFacet,           // source object or explicit link target
}
struct ContentAccess {
    source: Subject,
    artifact: ContentArtifactId,   // distinct identity, never replaces source
    source_revision: Revision,
    freshness: FreshnessEvidence,
}
enum Datum<T> {
    NotRequested, Pending,
    Ready { value: T, completeness: Completeness },
    NotApplicable, Unknown, Failed(Failure),
}
struct Fact {
    key: FactId,
    value: Datum<TypedValue>,
    provenance: ObservationOrigin,
}
struct Representation {
    schema: RepresentationId,
    version: SchemaVersion,
    payload: TypedRecord,
    resources: Vec<BlobLease>,
    completeness: Completeness,
}
```

`TypedValue` includes booleans, signed/unsigned numbers, decimal values, timestamps with precision/timezone status, quantities with units, text, byte-preserving names/paths, records, lists, and bounded tables. Schemas validate field meanings and allowed types. Built-ins expose typed Rust adapters over these records; user schemas remain dynamically interpreted. Filesystem/model operations return `io::Result`; observation failures become plain per-datum records, formatted as `path: error` at the UI boundary.

Missing, unknown, failed, pending, and truncated values never become zero. Partial directory size is a lower bound, not a complete sortable size. Columns declare comparisons over their value type and completeness; the host applies a documented bucket order for unavailable states and always keeps directories first. Display formatting cannot affect sorting.

## Recognition, providers, and shared work

Recognition combines bounded independent evidence: native object kind, filename, known basename, declared MIME, optional MIME detector, and signature. Candidates carry explanation and confidence, with conflicting evidence retained. Consumers ask whether a provider can satisfy a demand; they do not switch on extension strings. Remote recognition begins with names/listing and may refine after authorized materialization. Detection alone never downloads content.

Directory detection returns independent facets and evidence. Explicit directory facts describe that directory; enclosing project roots are separate context references for tools. Bounded scans permit Git, Rust, Python, Node, justfile, and IDE facets simultaneously without one winner overriding others.

```rust
trait Recognizer {
    fn recognize(&self, probe: &mut BoundedProbe) -> io::Result<Vec<Evidence>>;
}
trait Reader {
    fn descriptor(&self) -> &ReaderDescriptor;
    fn read(&self, request: ReadRequest, host: &mut ReadHost)
        -> io::Result<ReadBatch>;
}
struct ReadRequest {
    subject: Subject,
    demands: Vec<Demand>,
    limits: WorkLimits,
    cancellation: Cancellation,
}
enum Demand {
    Facts(Vec<FactId>),
    Representation { kind: RepresentationId, selector: Selector },
}
```

Descriptors declare supported subjects, prerequisites, fact/representation outputs, access costs, and schema versions. `ReadHost` offers bounded reads, sibling listings, approved process adapters, and resource creation. Every real read runs outside the UI thread.

The inspection service plans a finite dependency graph. Cycle detection and fan-out limits apply to user computations. A RAW reader may publish embedded image bytes, EXIF observations, and image dimensions together; later consumers reuse those outputs. A Parquet footer observation can satisfy schema Properties and table planning. Sharing uses source identity, revision, provider/schema version, selector, and effective limits. A small truncated result cannot satisfy a larger request. Avoid a universal parse object: formats publish only useful bounded observations and representations.

View and Properties acquire separate subscriptions and separate materialization policies. Remote/archive Properties requests listing facts only, even if View has an artifact. Cached content facts cannot leak into that policy. Fresh local-stat failure preserves source listing facts with provenance. Link-object and target observations stay separately addressed.

## User extensions and registration

Recommend TOML manifests plus trusted Lua programs, executed in a helper process launched from the application binary. This fits the existing Lua tool-definition product vocabulary while allowing executable readers and computations. An embedded Lua library is justified if needed for the chosen runtime; do not add a second extension language or a generic plugin framework.

A manifest registers IDs such as `user.jan.photo/capture-place`, versions, schemas, supported demand declarations, property sections, column catalog groups, and renderer associations. Declarative properties bind facts into sections; declarative columns support selection, formatting, composition, and conditional fallback. Lua is needed for new decoding, nontrivial derivation, or rendering logic. All four extension categories are available without compiling the application.

Built-ins use reserved namespaces. Duplicate IDs reject the conflicting package with diagnostics; installation order never silently wins. Explicit user preferences choose among compatible providers/renderers. Registry construction validates dependencies, schema compatibility, and column persistence. Missing extensions leave persisted column IDs visible as unavailable settings rather than discarding them. Schema major versions represent incompatible changes; additive optional fields preserve compatibility. Cached results include package fingerprints and schema versions.

Trust is explicit when enabling executable extensions. Native sources are exposed through opaque byte-preserving handles; no script assumes a UTF-8 path. The helper receives brokered source reads and leases, not unrestricted filesystem paths. Network is denied by the host API; process execution uses declared, allowlisted adapters. Runtime memory, instruction count, wall time, IPC size, read bytes, nested containers, and output objects are capped. Safe Lua libraries exclude direct filesystem/process/native-library access. These API restrictions are not a substitute for operating-system sandboxing against native-runtime vulnerabilities; the design assumes trusted user code and still applies isolation for mistakes.

Timeout kills and reaps an unresponsive helper. A crashed package fails its demands without bringing down the GUI; other packages run separately. Cancellation keeps a worker occupied until actual exit. Lua instruction hooks cannot interrupt a blocking native call, so the parent process deadline remains authoritative. Optional command adapters bound stdout/stderr and terminate/reap process groups. Platform-specific process containment needs implementation verification, especially while preserving Windows compilation.

## Rendering and GUI ownership

Representations are schema-validated plain data. Built-in IDs cover text, rich text, image, hex, directory/archive summary, table sample, paged document, slides, notebook, media details, and segmented route. Extensible IDs allow a user's representation without enlarging a Rust enum per format.

A renderer is selected by representation ID/version compatibility and explicit preference. It consumes data and produces a `PresentationTree`: selectable text, semantic groups, bounded grids/tables, image references, tabs, page controls, route paths, and bounded canvas primitives. Extension renderers can combine and style these nodes within host limits. Canvas operations have finite geometry, clipping, draw-count limits, and no arbitrary shaders or native callbacks. No scripts run inside GPUI render.

```rust
trait Renderer {
    fn present(&self, input: RenderInput) -> io::Result<PresentationTree>;
}
struct RenderInput {
    representation: Representation,
    viewport: Viewport,
    state: RendererState,
}
enum PresentationEvent {
    SelectTab(String), RequestPage(u32), Expand(NodeId),
    Activate(ActionToken),
}
```

The host validates node counts, depth, text size, resource references, and event tokens. Events return to a renderer worker and may produce revised presentation or declared new demands. Actions cannot mutate files or execute processes directly: user-visible commands use the shared command catalog, and tools retain reviewable plans and confirmation. Rendering provides no arbitrary HTML, remote images, JavaScript, notebook kernels, or widgets.

GPU images, text layouts, and GPUI entities belong exclusively to thin native views. Plain representations own byte leases; views create GPU resources only when visible and drop them on eviction, target/tab closure, or representation replacement. Notebook outputs render lazily. Releasing a subscription permits cancellation and cache eviction; another subscriber may retain shared work. Content leases are bounded and reference-counted, with final release scheduling artifact cleanup off-thread.

## Lifecycle and bounds

Every subscription carries target identity, generation, active demand, and revision evidence. Results enter shared caches only with their provenance and are applied to a view only if its subscription still matches. Refresh and editor save advance revisions and invalidate dependent observations, thumbnails, failures, and materializations. Remote ETags or archive-container revisions are used where available; sources without strong revisions have bounded reuse and refresh revalidation, not a claim of perfect freshness.

Use separate bounded interactive and optional-column queues, fair per-panel admission, worker limits, and byte-budgeted caches. Optional facts load only for visible rows. Cancellation removes queued work; running work remains charged until completed/reaped. Directory sizes require an explicit scan demand, retain partial lower bounds, avoid symlink traversal, deduplicate hardlinks, and skip filesystem crossings.

Host limits cap provider-declared budgets; extensions cannot raise them. Preserve table limits of 500 rows, 40 columns, and 1024 characters per cell; Parquet footer 8 MiB and decoded chunks 64 MiB; spreadsheets 32 MiB source/256 MiB declared expansion; activity sources 16 MiB/100,000 points; remote quick View known-size files at most 8 MiB. Enforce ZIP/XML/block/slide/text and process bounds as appropriate. Dense spreadsheet allocation and non-interruptible FIT decoding remain explicit limitations, isolated in workers with parent deadlines and memory containment where possible.

Complete/streaming conversion is a separate capability contract. A preview sample never acquires completeness through reuse. Icons consume recognition and badges; Open With combines opener rules with platform associations, including directories. Tools consume context facts and complete-operation capabilities, without owning format discovery.

## End-to-end examples

**RAW/EXIF:** Recognition selects a RAW provider. View requests bounded embedded JPEG extraction; Properties and photo columns request EXIF facts. Extraction is reused where EXIF lives in the JPEG. Failures are independent. The image renderer displays pixels without a metadata overlay; visible thumbnails share freshness-aware image observations and failure records.

**Parquet:** Properties requests footer schema and counts, including repeated/list/map paths and repetition/definition levels. View requests selected scalar columns and bounded row samples. Unsupported nested values can fail View while schema Properties succeeds. Truncation and sample status remain visible.

**PDF:** A document representation describes page navigation; a page selector requests optional Poppler rasterization. The result is an image representation usable by the ordinary image renderer, while page ownership/navigation remains with the document controller. Missing Poppler produces a clear demand failure. Continuous pages would add viewport-driven page demands, not a different format reader.

**GPX/FIT:** Metadata and route geometry are separately requestable outputs. Route segments remain separate; recorded session totals outrank inferred distance. A no-GPS activity still renders useful metadata. The route renderer uses offline geometry only.

**Multifaceted directory:** A directory with Cargo and Python files inside a Git root has several explainable facets. Explicit Properties reflects the selected directory; a tool may use the enclosing Git root. Directory summary reads direct children; recursive size requires its own command. Related RAW/JPEG/XMP assets can later form memberships without silently expanding file actions.

**Remote, archive, symlink:** Remote View first validates known size and policy, then obtains an artifact lease; Properties stays listing-only. An extracted archive member remains identified by container/member, never temporary path. A local symlink keeps its spelling and link facts while supported target format facts carry target provenance. Broken links retain useful object facts. Unknown sizes stay unknown throughout.

## Modules, verification, and adoption

Suggested ownership, using `foo.rs` plus `foo/` and small responsibility-based files:

```text
source.rs / source/{identity,access,materialize,freshness}.rs
inspection.rs / inspection/{recognition,facets,facts,schema,registry,planner,scheduler,cache}.rs
readers.rs / readers/{native,image,parquet,pdf,media,office,notebook,activity}.rs
properties.rs / properties/{sections,bindings}.rs
columns.rs / columns/{catalog,evaluate,sort}.rs
representations.rs / representations/{table,document,route,notebook}.rs
extensions.rs / extensions/{manifest,wire,worker,broker}.rs
presentation.rs / presentation/{tree,selection,events}.rs
inspector_view.rs / inspector_view/{properties,content,resources}.rs
```

Source and schema primitives have no GUI dependency. Readers depend on those primitives. The inspection service orchestrates readers and source access. Properties/columns/presentation consume plain inspection outputs. Views depend on those models and GPUI. The extension broker adapts the same contracts; it does not become a second application model.

Adding a format means registering recognition evidence, schemas, bounded readers, applicable facts, representations, property bindings, optional columns, and compatible renderers; declaring permissions and limits; documenting completeness; and testing failure paths. Most formats reuse existing renderers. A custom renderer is justified only by a new presentation need.

Unit-test provenance and link identity; unavailable-state sorting; schema validation; provider conflicts/cycles; demand sharing and limits; stale-generation suppression; cancellation slot accounting; listing-only policy; container/process bounds; resource release; and helper crash recovery. Use temporary files and fake brokers/process adapters; optional programs and network are unnecessary. Test extension contracts with malformed trees, excessive output, incompatible schemas, missing packages, and non-UTF-8 sources. Native-view integration tests should verify selectable properties, lazy images, page/tab interaction, and cleanup in a running app using scratch configuration.

Conceptual adoption starts with identity/state/schema contracts, then scheduler/broker, then one complete built-in and user-extension vertical slice covering reader, property, column, and renderer. Add remaining formats and consumer catalogs only after that validates the protocol. This is ordering advice without guesses about today's code.

Reject a monolithic format trait owning UI, columns, and metadata: different consumers have different access policies and lifetimes. Reject untyped JSON everywhere: absent/completeness semantics and sortable quantities become conventions. Reject arbitrary dynamic native GPUI plugins: unstable ABI, resource ownership, and failure isolation overwhelm the benefit. Reject per-feature independent parsers: bounds, recognition, freshness, and duplicated work drift.

Open decisions include exact recognition tie-breaking, extension permission UX, sort bucket placement, renderer fallback UX, directory scan scope, related-asset action membership, and future streaming/space presentations. Main risks are schema verbosity, helper overhead, library allocations escaping accounting, process containment portability, and renderer grammar limits. Validate the extension vertical slice early: extensibility is a defining requirement, not a promise appended after built-ins.
