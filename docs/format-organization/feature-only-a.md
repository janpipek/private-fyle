# A capability graph over native sources

This is an independent, feature-only, clean-sheet proposal. I read only the supplied feature brief; I have not inspected repository code, documentation, other proposals, or external sources. The owner’s final answers make user-defined readers, properties, columns, and renderers mandatory. I recommend a schema-governed capability graph, built-in Rust readers, trusted Lua extensions, and a finite declarative preview language rendered by native GPUI views. Coherence takes priority over preserving existing abstractions. Current implementation details, migration cost, dependency availability, and compatibility cannot be verified here.

The confirmed product behaviors and bounds in the brief are requirements. Signature recognition, multifaceted directory detection, asset relationships, continuous PDF pages, incremental table rows, and space views are design allowances, not authorization to implement them. This proposal makes room for them without changing current behavior.

## Sources, identities, and evidence

The central object is a source, not a path or format. A local path is an OS-native path retaining non-UTF-8 bytes and symlink spelling. Archive members and remote objects have provider-native identifiers; extracted/downloaded files are separate local sources linked by a materialization record. Listing facts describe the original source. Cache stat describes the cache. Neither can substitute for the other.

Illustrative contracts, rather than proposed executable edits:

```rust
pub struct Source {
    pub id: SourceId,
    pub locator: SourceLocator,
    pub revision: RevisionEvidence,
}
pub enum SourceLocator {
    Local(PathBuf),
    ArchiveMember { container: SourceId, member: MemberId },
    Remote { provider: ProviderId, object: ObjectId },
    VirtualCollection(CollectionId),
}
pub enum Datum<T> {
    Missing,
    Pending,
    Ready { value: T, completeness: Completeness },
    Failed(FactError),
}
pub enum Completeness {
    Complete,
    LowerBound,
    Truncated { reason: String },
}
pub struct FactRecord {
    pub subject: SubjectId,
    pub key: FactId,
    pub datum: Datum<Value>,
    pub provenance: Provenance,
}
```

`Missing` means unavailable or absent; applicability lives in the fact schema and presentation rules. Unknown size is missing, never zero. A lower-bound directory size remains a number explicitly marked incomplete. Fresh metadata failures produce separate records, leaving usable listing evidence intact. Provenance records producer, acquisition time, revision evidence, and whether information came from listing, native stat, link target, or content. Consumers choose evidence through documented policies, rather than last-writer-wins updates.

A local symlink has separate link-object and target subjects joined by a resolution relation. Broken target resolution is meaningful evidence. Format reads may address the target subject while Properties displays both subjects. Operations retain the original link source. Likewise, materialization never changes the remote or archive identity selected by the user.

## Recognition and readers are different contracts

Recognition returns candidates with explanations and evidence, not one irreversible enum. A format ID is a stable namespaced string such as `builtin:parquet`; matches may come from names, extension, provider MIME, bounded signatures, or a configured MIME detector. Filename-only remote recognition can be refined after an authorized download. Conflicts remain visible and deterministic: stronger evidence wins for default preview selection, user overrides take precedence, and ties use an explicit registry order. Recognition does not grant access to content.

Directory facets use a separate recognizer contract returning a set: Git repository, Cargo project, Python project, and IDE metadata can coexist. Every finding identifies the directory actually inspected and its evidence. An enclosing project root is an additional relation used by tools; it does not become the facts of an explicitly targeted directory. Bounded scans inspect only declared names and limited ancestor depths.

Readers advertise capabilities, rather than requiring every format to implement properties, thumbnails, conversions, and previews. A Rust reader receives narrow host services and emits plain data:

```rust
pub trait Reader: Send + Sync {
    fn descriptor(&self) -> ReaderDescriptor;
    fn read(
        &self,
        request: ReadRequest,
        host: &mut dyn ReadHost,
        emit: &mut dyn ResultSink,
    ) -> io::Result<()>;
}
pub enum ReadRequest {
    Facts(FactSelection),
    Preview(PreviewRequest),
    Thumbnail(ThumbnailRequest),
    Dataset(DatasetRequest),
}
pub trait ReadHost {
    fn read_range(&mut self, range: ByteRange) -> io::Result<Vec<u8>>;
    fn budget(&mut self) -> &mut Budget;
    fn cancelled(&self) -> bool;
}
```

Additional host services cover bounded external processes and explicitly authorized directory enumeration. Complete dataset requests have an independent stream contract, declared fidelity, and output policy. Preview samples cannot satisfy conversion input contracts. External processes run on workers with wall-time/output limits, are terminated and reaped on cancellation, and report unavailable programs distinctly.

Use traits at executable boundaries: readers, recognizers, source access, and MIME detectors. Use enums for closed host concepts such as request class, completeness, and supported preview primitives. Use structs and namespaced IDs for extensible facts, formats, renderers, and schemas. A giant format enum would make user registration artificial; an untyped universal map would lose units, sorting semantics, and validation.

## Semantic facts and consumer presentations

The schema registry defines each fact’s type, unit, cardinality, applicability, and compatibility version. `Value` supports booleans, signed/unsigned numbers, measured values, timestamps, text, lossless source references, records, sequences, and typed tables. Built-ins get typed Rust wrappers/codecs over these same schemas. User facts use the validated dynamic representation. For example, duration is a measured time, not a localized string; camera composition uses structured make/model facts.

Properties is a presentation definition over facts. A section has a stable ID, label, applicability expression, ordered fields, nested sections, and table definitions. Independent field failures remain independent. Empty/inapplicable sections disappear; pending, failed, and truncated sections have visible states. Labels and displayed values are selectable/copyable. Format readers never construct GPUI property widgets.

Columns similarly define ID, catalog group, dependencies, value expression, display format, and sort semantics. They may compose multiple facts. Persist IDs, not labels or positions. Sorting uses typed values and an explicit state ordering; directory-first ordering is a separate primary rule. Partial sizes do not acquire a false exact ordering: expose completeness and apply a consistent lower-bound ordering. Remote/archive unsupported fields remain blank. Catalog groups, Properties groups, and asset relationships are three different types.

Icons and Open With consume recognition and source facts without triggering expensive reads. Tools consume directory facets and selection facts to construct reviewable plans through their existing product contract. Missing programs become disabled actions. Related assets would form explicitly evidenced relationships with member roles; they would not change operation targets until the owner defines membership and action policies.

## Preview representations and renderers

A reader produces a representation, not a view. Built-in representations include bounded text, rich-text blocks, raster, hex, listing, typed table sample, paged document, slide outline, notebook, and segmented route. Each carries completeness, dimensions/counts when known, and named lazy requests. A paged document describes page count and requests; it is not an already allocated sequence of GUI images. The renderer chooses layout and interactions for that representation.

Native renderers own all GPUI entities and image registrations. A preview session owns their resource arena; releasing the session drops image handles, textures, and any renderer state. Data caches may retain bounded encoded rasters, but never retain GPUI handles. Render calls access only prepared data and dispatch requests; they do not read files or launch work.

User renderers are real executable extensions with a constrained output language: Lua receives a validated representation and returns a `ViewTree` composed of host primitives—text, rich text, raster reference, table, tabs, stack, list, route canvas, status, and actions. Actions are host-recognized intents such as requesting another page or sheet. Keys support incremental reuse; limits bound depth, node count, text, and image resources. Native adapters implement keyboard focus, selection, copying, clipping, and cleanup. Lua cannot instantiate arbitrary GPUI widgets, retain GUI entities, or execute arbitrary callbacks on the UI thread. This supports user-defined layouts and representation transformations while stating the boundary honestly. A genuinely new interactive primitive needs a reviewed built-in adapter.

Renderer selection is capability based, using representation schema, version, configured preference, and deterministic fallback. File recognition does not choose a renderer directly: PDF pages and embedded image previews may both render raster content while retaining different session controls.

## Implementable user extension runtime

Use trusted, explicitly enabled Lua packages, matching the language already specified for user tools. Each package declares a namespace, version, host API version, schemas, recognizers/readers, property definitions, column definitions, renderer registrations, and requested capabilities. Simple presentations are declarative; readers, computed facts, and render transformations are Lua functions. Built-in Rust implementations and Lua adapters register in the same catalog.

Registration is transactional and validates IDs, schemas, dependency cycles, renderer compatibility, and limits before activation. Duplicate IDs fail activation; replacing a built-in requires an explicit override declaration and user configuration. Never silently overwrite. Persisted column IDs remain resolvable through declared aliases; absent packages leave settings intact and show unavailable columns. Schema evolution allows compatible additive fields within a version, but incompatible meaning/type changes require a new major schema identifier and an explicit adapter. Labels can change independently.

Host APIs provide bounded byte ranges, authorized enumeration, emitted records, lazy requests, and explicitly permitted process calls. Disable unrestricted `io`, `os`, native module loading, and arbitrary package loading. Use an isolated VM per task with instruction hooks, memory limits, wall-time supervision, and validated output size. No extension work runs in render. This is capability discipline for trusted code, not a security promise for hostile native dependencies or every uninterruptible host call. Truly untrusted packages would require a separate constrained process runtime and OS sandbox policy; that is an unresolved addition, not claimed here.

A permission manifest distinguishes content reads, directories/ancestors, subprocess executable allowlists, writes, and network. Default format plugins need only scoped reads. Properties demand never grants materialization. Lua execution errors fail that producer only, with bounded diagnostic text and continued built-in fallback. Native crashes cannot be isolated by catching Rust errors; process isolation is the alternative if required. No new dependency is prescribed: verify the existing Lua runtime’s hook/memory facilities first; a missing facility needs an explicit justification before adding one.

## Work ownership, reuse, and freshness

A shared acquisition broker owns scheduling, actual worker occupancy, cancellation, caches, and dependency requests. Demand is indexed by source subject/revision, producer/version, request parameters, and budget class. Multiple consumers may subscribe to the same compatible work. Dropping a subscriber removes demand; cancellation asks execution to stop but does not release its worker slot until it exits. Bound queue size, concurrent workers, cache bytes, and per-source work.

Properties and View use separate demands. Optional column work is limited to visible rows. Directory-size scans require an explicit user command, skip symlinks and filesystem crossings, count hardlinks once, and publish cancellable lower bounds. Expensive results are not scheduled merely because a schema exists.

Every inspector/tab session has a generation token; delivery requires matching source revision and generation. Closing, switching, refreshing, and editor saves invalidate appropriate subscriptions and GUI arenas. Thumbnail failures are cached against source freshness. Revision evidence may be imperfect remotely; refresh must invalidate explicitly, and evidence strength should be recorded rather than assuming a timestamp proves identity. Sharing parser summaries is optional and bounded; a shared large decoder handle is not the default.

## Concrete flows

**RAW/EXIF:** recognition identifies a candidate; Properties requests EXIF and View requests an image separately. A bounded embedded-JPEG extraction can supply both EXIF fallback and raster bytes through shared demand. Failure to extract still leaves other facts. Icon thumbnails use a distinct size/budget request. No full sensor decode or metadata overlay appears.

**Parquet:** Properties reads a capped 8 MiB footer and publishes schema paths, levels, types, counts, and row-group sizes. View requests a sample limited to 500 rows, 40 columns, 1024 characters per cell, and 64 MiB selected decoded chunks. Repeated/list/map fields stay inspectable in Properties while View reports their unsupported sampling. Neither output claims complete conversion data.

**PDF:** a paged-document representation owns page navigation; each page request runs bounded optional poppler work and emits raster data. Previous/next transitions replace session image resources. Ordinary raster preview reuses the raster primitive without acquiring PDF controls. Continuous pages would add bounded viewport demand, not eager rendering.

**GPX/FIT:** one read can publish independent metadata and segmented route data under the 16 MiB/100,000-point bounds. Session totals outrank inferred distances. No-GPS activities still produce useful Properties. An uninterruptible decoder retains its worker slot after cancellation; generation checks discard late output.

**Multifaceted directory:** bounded recognition can return Git, Cargo, and Python facets together, each with evidence. Tools can ask for enclosing roots; direct-child summary asks only for that directory. Recognition uncertainty remains explainable, and an optional future space view would request its own explicit scan.

**Remote/archive/symlink:** remote Properties uses listing evidence only. View materializes only known-size remote files at most 8 MiB; archive View may extract a member. Materialization is revision-bound and managed as a separate local source. A symlink’s content requests resolve to its target subject while link-object facts and native spelling remain visible.

## Organization, adoption, and checks

Suggested modules are `source.rs`/`source/`, `schema.rs`, `recognition.rs`/`recognition/`, `facts.rs`, `acquisition.rs`/`acquisition/`, `formats.rs`/`formats/`, `properties.rs`, `columns.rs`, `preview.rs`/`preview/`, `extensions.rs`/`extensions/`, and `inspector_view.rs`/`inspector_view/`. Split format adapters and native renderer adapters by responsibility below roughly 1000 lines. Source/schema models form the bottom; recognition/readers depend on them; the broker orchestrates readers; consumer models depend on catalog/broker data; GPUI views depend on consumer models. Extensions adapt to those interfaces, never become a second domain model.

For each format addition: reserve namespaced schemas and capabilities, define recognition evidence and ambiguity, specify budgets/applicability/provenance, implement readers, declare Properties/columns, choose or register representations/renderers, add resource invalidation, and test degraded inputs. Commands enter the shared command catalog. Preserve Windows compilation without adding platform scope.

Conceptual adoption starts with identity, evidence, and schema contracts; then broker lifecycle; then a vertical local-stat/text example; then independent Properties/columns and lazy raster/PDF; then complex readers; finally extension registration and declarative renderer validation. Start the Lua adapter early with a small user reader to prove the shared contracts before expanding built-ins. This order is conceptual, not a claim about today’s code.

Unit tests cover lossless paths, link/target separation, listing preservation, schema evolution, sort states, catalog conflicts, budget exhaustion, and demand deduplication. Deterministic fake workers test late results, cancellation without premature slot release, refresh, and tab independence. Fixtures test each bounded reader without network or optional programs; fake process services test missing programs/timeouts. Renderer tests verify tree bounds and resource release, followed by running-app checks for focus, copying, and lazy navigation.

The chief risks are schema complexity, restricted renderer expressiveness, library allocations beyond declared budgets, and unreliable remote revision evidence. Keep schemas consumer-driven and host primitives finite. Remaining owner decisions include related-asset action policies, detector precedence defaults, extension process sandboxing, and incomplete-value sort presentation. These do not weaken the core recommendation: preserve source identity, separate evidence from presentation, and make every executable extension use the same bounded acquisition and native resource lifecycle as built-ins.
