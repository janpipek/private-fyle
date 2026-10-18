# Organizing data providers and presentation in fyle

The proposed [implementation plan](implementation-plan.md) turns this
recommendation into ordered slices, decision checkpoints and acceptance checks.

This is a design comparison, not an implementation plan approved for execution. The recommendation incorporates the owner's subsequent preference for fewer abstractions and similar organization across Properties, columns and viewers: organize around data providers, with one recognizable contribution model and typed results for each presentation. Providers can apply through file content, filesystem attributes or surrounding context; formats are one source of data, not the organizing principle for every contribution. Keep bounded reading and host-owned rendering, and prove user extensibility with a small vertical example before building general infrastructure.

## The experiment and its limits

The owner chose four perspectives: one code-informed destination design with migration guidance, one code-informed modest-change design, and two independent clean-sheet designs given only a detailed product brief. User-defined format readers, properties, columns and preview renderers are core requirements. Domain coherence takes priority over migration cost.

Four separate subagents produced these reports:

| Proposal | Information and scope | Main recommendation |
| --- | --- | --- |
| [Code-informed A](code-informed-a.md) | Repository/docs; best destination and migration | Shared recognition and typed facts, independent projections, portable worker extensions and bounded host representations |
| [Code-informed B](code-informed-b.md) | Repository/docs; modest changes | Preserve feature registries, introduce dynamic catalogs and shared contracts, add a scene interpreter for custom renderers |
| [Feature-only A](feature-only-a.md) | Product brief only; clean sheet | Schema-governed capability graph with provenance, shared acquisition and declarative native rendering |
| [Feature-only B](feature-only-b.md) | Product brief only; clean sheet | Demand-driven inspection domain, independent consumers and an open representation/presentation protocol |

The environment admits three subagents beside the parent, so the fourth ran after a slot became available. Each started without inherited conversation history, received its own task, wrote only its assigned report and was forbidden to read the others. The feature-only agents were also forbidden to inspect repository code or project documentation. Their recorded provenance states that they used only the supplied brief. This is instruction-based information isolation, not a filesystem access sandbox.

The feature brief described browsing/selection/operations; local, remote, archive and symlink identity; independent inspector tabs; current Properties sections and columns; all preview families and numerical limits; thumbnails; Open With and contextual tools; cancellation/freshness/partial results; and future detection, directory facets and related assets. It did not disclose existing types, traits, module layout or architecture decisions. It supplied Rust/GPUI and product engineering constraints, and mentioned existing Lua tools as a user capability. This helps explain the proposals' unanimous Lua preference: it is a reasonable inference from common input, not four independent runtime experiments.

There are no prototypes, performance measurements or runtime evaluations here. Agreement supports conceptual fit; it does not establish implementation cost, renderer sufficiency or security guarantees. Clean-sheet reports cannot verify current capabilities beyond the brief. Illustrative Rust snippets in the reports are abbreviated designs, not compilable interfaces.

## Judgment of each proposal

**Code-informed A is the strongest bridge between the requested destination and the actual tree.** Its concrete findings distinguish existing reusable readers from merely similar interfaces. It notices the separate Properties EXIF implementation, text-valued geography quantities, fragmented recognition and process/cancellation gaps. It preserves typed format models while introducing dynamic identifiers where users need extension. I would use this report as the main migration reference. Its broad `Reader` demand/result interface and shared coordinator still need a small vertical experiment: otherwise they could become a dispatcher with every format's special case. Its proposed Lua worker protocol is plausible, not verified.

**Code-informed B is the strongest conservative delivery proposal.** Keeping Preview, Properties and Tools orchestration separate avoids a disruptive framework rewrite; `Content::Extension` and dynamic column descriptors provide concrete compatibility routes. Its custom scene example demonstrates more than choosing a built-in image viewer. However, retaining fixed native paths beside a new extension path can leave two semantic systems unless both share fact definitions and acceptance tests. It also describes shared EXIF modules without emphasizing the separate Properties reader, so its reuse claims need A's qualification. Its incremental extension case is valuable; its minimum-change organization is not the final domain merely because it is cheapest to start.

**Feature-only A provides the richest source/fact model.** Explicit subjects, provenance, completeness and producer selection address cache/source confusion and competing metadata better than a flat map. A schema-governed capability graph also gives user computations a place to declare dependencies. The risk is turning schemas and dependency planning into the main product: heterogeneous records, sequences, datasets and many host operations create a large interface before concrete consumers demand them. Its `Missing` state merges absence and unavailability more than I would accept; its in-process per-task Lua VM cannot offer the same hard termination guarantee as a supervised helper. Keep its provenance discipline, but narrow its graph and prefer isolation for executable extensions.

**Feature-only B gives the clearest destination vocabulary for this task.** Its distinctions among entry, content access, evidence, fact, representation and renderer map directly to the different format needs. It also says that cached content must not leak through listing-only Properties policy, even when View has already downloaded the file. This is an important correction to naive sharing. Its extension vertical slice should happen early. I would avoid putting source identity, all schemas, recognition, scheduling and caches under one large `inspection` owner: shared foundations serve icons, tools and Open With too. The name and directory are less important than keeping their interfaces small and ownership explicit.

My overall choice is **Feature-only B's domain distinctions, Code-informed A's evidence and migration discipline, Code-informed B's compatibility techniques, and Feature-only A's provenance model**. The follow-up discussion sharpens this choice: retain those distinctions without turning each into a separate framework, and make the organization of Properties, columns and viewers deliberately similar. The four original reports remain independent records; this synthesis contains the revised recommendation.

## The recommended domain

The core relationships should read as ordinary statements: an item has a source identity and context; a provider decides where its contributions apply and acquires bounded data; Properties, columns and viewers present that data through similarly organized contributions; native views own GUI resources. File recognition helps select some providers, while repository membership, native attributes or directory markers select others.

For example, a Git provider can apply to an ordinary file because it is inside a repository, regardless of its format. The existing Git status column and a proposed Git Properties section can project the same repository-relative status data. A media provider obtains data through a format reader; a filesystem provider obtains native attributes. An item can match all three. A provider need not contribute to every presentation.

| Concept | Owns | Must not imply |
| --- | --- | --- |
| Source identity and subject | Native spelling, source namespace, link-object/target distinction | A temporary local path or permission to write back |
| Content access | Revision-bound readable handle and materialization lease | Authorization for every consumer to inspect those bytes |
| Recognition evidence | Name/MIME/signature/structure candidates, directory facets and scope | Successful parsing or one exclusive directory kind |
| Semantic observation | Typed value, unit, subject, producer, completeness and provenance | A label, displayed string or unique uncontested truth |
| Data provider | Applicability, bounded acquisition and its presentation contributions | A particular file format, or a remote storage connection |
| Format reader | Bounded format decoding and meaningful format-specific data | Properties layout, column catalog or GPUI widgets |
| Property section | Fields/tables/nesting and display policy | A related-file group or automatic column definition |
| Column definition | Dependencies, cell projection, formatting and typed comparison | Full-directory eager parsing when selected or sorted |
| Preview representation | Sample/page/route/notebook data and lazy requests | A complete conversion dataset or a GUI entity |
| Renderer | Mapping compatible representation to prepared presentation | File mutation, arbitrary UI-thread callbacks or unrestricted GPUI access |

These are concepts to keep clear, not a requirement for a trait, registry or top-level module per concept. Reuse `Location` and `Entry` for identity/listing data; a materialized-copy value adds original source, readable access and freshness without replacing them. Keep useful typed provider results such as repository status, `GeoData` and media metadata. Use closed enums for host guarantees such as read states and presentation primitives, and namespaced IDs where users need to register contributions. Introduce traits for genuinely varying execution, including detector implementations and extension adapters, rather than for every domain noun.

Composition lets several providers apply to one item. A provider adapter supplies the same contribution contract for Rust built-ins and user workers. This offers a concrete reason for shared interfaces: adding or understanding a contribution should follow the same steps across all three presentations.

A fact identity must include more than its schema key. Its subject, source revision and producer matter. Two assertions named `photo.rating` from embedded metadata and an XMP sibling may disagree. Store their origins and select through an explicit policy; do not resolve them by whichever request finishes last. Keep availability/request state distinct from completeness: a ready size can be a lower bound, and a successfully decoded table can still be a sample. Preserve finite numbers, units and type-correct comparisons. Rational EXIF quantities and timestamp precision should survive where consumers need them; avoid requiring every possible value type immediately.

Validation belongs at registration and the extension wire seam. Start with field definitions, types/units and representation versions needed by actual extensions, rather than a general schema framework. Built-in results need not become dynamic records throughout their implementations. Conversely, user outputs need more than untyped strings. Stable descriptors and validated results form the common contract; they do not mandate serialization of every Rust function result or a global fact database.

Recognition and capability selection remain separate. Keep multiple explainable candidates; MIME serves desktop associations, while stable format IDs serve reader capabilities. ZIP plus office-document structure is valid layered evidence. Ambiguous text can remain text. Directory recognition returns independent facets with direct-directory versus enclosing-root scope, bounded marker reads and incomplete-result status. A Git/Rust/Python directory does not need one winning kind.

Three group types remain distinct: property sections, column catalog groups and related-item associations. Related groups keep each physical member's identity and role. They do not expand rename/delete/copy targets until operation policy explicitly chooses members. Sidecar precedence is similarly a product decision, not a parser convenience.

## One contribution model for three presentations

Similarity is an explicit readability goal. Properties, columns and viewers should follow **definition → applicability → data request → prepared result → rendering**, with consistent names, registration, availability/result states and freshness rules.

| Common stage | Properties | Column | Viewer |
| --- | --- | --- | --- |
| Definition | Section descriptor | Column descriptor | Viewer descriptor |
| Applicability | Supported items/context | Supported rows/context | Supported items/context |
| Data request | Required metadata | Required cell facts | Required content/page/sample |
| Prepared result | Structured section | Typed cell value | Preview representation |
| Rendering | Section view | Cell view | Content view |

A small generic contribution interface can express this pattern with typed requests and outputs. Descriptors carry stable identity, applicability and data requirements. Provider execution prepares the requested data off the UI thread; contribution projection and rendering consume ready results. A cheap cell projection does not need to launch a separate worker merely to follow the same pattern. Applicability uses captured evidence/context; any filesystem or repository probes needed to establish it run in background work.

The shared structure does not require identical selection policies. Properties combines applicable sections, rows display the chosen columns, and Preview selects an applicable viewer. Keep those policies explicit and small. Property sections and columns can compose fields without being generated automatically from each other. Shared registration can collect a provider's contributions together while each presentation retains its own selection rule.

For Git, background context discovery identifies the repository root and the selected item's path relative to it. A bounded status request can prepare one repository snapshot for several visible rows and the inspector. The column projects a status cell; a proposed Properties contribution projects a section. Neither defines applicability by file extension. Cache/request identity includes repository context and its invalidation policy: Git status can change when the index or repository state changes even if the selected file's size and modification time do not. Preserve the distinction between an explicitly inspected directory and its enclosing project context.

The same organization works for user-defined providers. A package can contribute a contextual column or Properties section without declaring a new format or supplying a viewer. Providers acquire data; contributions present it. Here, “provider” means an inspection data provider, distinct from the existing remote storage providers that list and transfer files.

## Abstractions to keep small or defer

- **Universal fact database:** keep typed provider results and field definitions at the extension interface. Retain provenance/completeness without building a general storage engine.
- **Capability/dependency graph:** declare the data each contribution needs and request it directly. Add general dependency planning only if real extensions require it.
- **Shared acquisition broker:** preserve feature scheduling initially and selectively share costly work, such as repository status or a media probe. Extract more coordination when repeated policy demonstrates its value.
- **Universal format-reader trait:** keep concrete bounded functions for Parquet footers, PDF pages and other distinct operations. The common contribution contract can adapt them without forcing identical reader requests.
- **Registry for every noun:** register provider contributions together, retaining typed catalogs or indexes only where selection requires them.
- **General schema framework:** begin with stable IDs, concrete field definitions and validation/versioning of extension outputs.

Reducing abstractions should increase similarity, not scatter ad hoc implementations. Consistent provider/contribution organization is worth sharing even when Properties, columns and viewers have different result types.

## Reading, presenting and sharing

Share knowledge first: definitions, applicability evidence and bounded acquisition. Share completed work or pending requests only when source/context revision, provider, demand, limits and access policy make reuse valid. A footer read or repository status snapshot is a good shared product; an expensive decoder session held indefinitely is not automatically one. A smaller truncated result cannot satisfy a larger request. Neither data availability nor a cache hit authorizes nonlocal content inspection.

Properties collects independent contributions and failures. Columns request selected visible-row data and project it into cells. Preview chooses a compatible viewer and renderer, with documented default/fallback ordering. Tools discovers all applicable offers and plans a chosen action; its execution contract remains separate. Properties, columns and viewers share the contribution pattern, while their selection and presentation policies remain explicit. Any later shared execution module should hide repeated worker/freshness/resource policy behind a small interface.

Examples establish the necessary seams:

- RAW metadata inspection must remain metadata-only; embedded-image extraction/decode for F3 or thumbnails is a different demand. Share bounded byte knowledge where suitable without forcing image decoding into Properties.
- Parquet schema inspection can succeed when sampled row preview does not support nested columns. Footer work can be shared; samples retain their limits and cannot feed a complete conversion implicitly.
- PDF metadata, page rasterization and displaying raster pixels have different requirements. The page renderer creates an artifact; the GUI raster adapter displays it; document session state owns navigation. Missing a rasterizer need not erase other facts.
- GPX/FIT summaries and segmented geometry are related outputs, with recorded-versus-inferred provenance and independent truncation. Style-dependent outlines are presentation products, not canonical geographic facts.
- Git status is contextual data for files and directories in a repository, independent of format. Repository-scoped acquisition can serve both visible status cells and a proposed Properties section without repeating the probe for each contribution.
- A downloaded remote file supplies content access for View, while Properties retains its listing-only policy. A symlink's object facts remain distinct from target content, and archive revision includes the container. Native cache paths never replace source identities.

A supervised worker continues occupying a slot until it exits. Request generations reject obsolete deliveries; source revisions govern data reuse; neither replaces the other. Caches need byte/disk limits, producer/package versions, demand parameters and resource leases; style/pixel/page parameters affect rendered products. Library allocation expansion and noninterruptible calls remain limitations until actually contained. A deadline field in a struct does not make those calls interruptible.

## User-defined readers, properties, columns and renderers

Use one versioned package contract declaring providers, their field/output definitions and contributions, with reserved built-in namespaces, duplicate validation, explicit overrides, compatible representation versions and retained unresolved settings IDs. Applicability can depend on format evidence, item attributes or surrounding context. Declarative sections and column bindings cover straightforward presentation. Executable acquisition and renderer transformations use the same validation and lifecycle contract through adapters.

My provisional runtime choice is a supervised Lua helper using the existing Lua dependency, with brokered source reads, instruction/allocation limits and parent deadlines. Keep existing Tools Lua semantics separate. A bounded argv protocol can support an explicitly enabled external decoder. This remains a recommendation to validate, not a claim that the current code already provides isolation or a reason to add a new dependency now. Native Rust shared libraries and unrestricted HTML are poor first choices for the required scope.

A user renderer must be able to create its own layout or visual output, not merely request a built-in preview by name. Recommend validated display trees with selectable text, layout, tables, leased rasters, bounded canvas geometry and named interaction intents. Renderer execution happens in workers; native GPUI adapters interpret already prepared results and own focus, selection, images and cleanup. Arbitrary new native widgets remain outside that contract. Accept this grammar only after a real custom graph/route example proves sufficient; otherwise reconsider its expressiveness before promising universal rendering.

Enabling a trusted package can authorize automatic read-time execution within its declared access policy. That is distinct from Tools' confirmation of each action. Host API restrictions and process separation are useful for mistakes, but are not a complete hostile-code OS sandbox. Exact executable grants and platform containment need an implementation decision. Avoid inventing repeated prompts or claiming that a manifest enforces permissions by itself.

## Modules and adoption

A destination sketch places data-provider acquisition and contributions together, with low-level format parsing separate:

```text
providers.rs / providers/
  filesystem.rs
  git.rs
  media.rs
  exif.rs
  parquet.rs
  projects.rs
formats.rs / formats/          bounded parsing and typed format data
properties.rs / properties/    catalog, request model, prepared results, view
columns.rs / columns/          catalog, request model, prepared results, view
preview.rs / preview/          catalog, request model, prepared results, view
extensions.rs / extensions/    registration, validation, execution adapters
location.rs, entry.rs          existing source/listing values
```

The three presentation areas use the same organization where responsibilities exist; this sketch does not require empty files or a separate module for every stage. A small provider can keep acquisition and its contribution definitions in one file. A larger provider can split them using a predictable internal organization. Pure contribution contracts belong below their catalogs/views so providers can declare outputs without depending on GUI controllers. Providers call format readers or system/context adapters; format readers do not import presentation or GPUI. Native rendering and scheduling remain in their existing focused owners until a concrete change justifies moving them. Related-item associations remain separate when approved.

First, define the smallest common contribution descriptor, typed request/result states and dynamic-ID contracts needed by one vertical slice, reusing existing source identities. Then build a custom sensor or activity provider contributing a bounded reader, typed duration/distance data, a Properties section, a sortable column and a custom canvas renderer. Also adapt Git as the contextual example so the interface is not designed exclusively around file formats; a Properties contribution here is proposed work, not a claim about current behavior. Exercise collisions, version mismatches, bad output, unknown saved IDs, cancellation, crashes and resource cleanup. Include non-UTF-8 source access, repository invalidation and listing-only nonlocal policy. Prove the contribution and extension contracts before migrating all providers.

Next adapt one existing shared reader and its consumers, preserving presentation and resource limits. Consolidate EXIF only after comparing Properties' metadata-only bounded reader against the shared reader; fewer functions are not an improvement if resource behavior regresses. Add shared recognition and dynamic column catalogs with compatibility aliases. Move PDF/process and materialization responsibilities as concrete consumers require. Grow coalescing from identical-demand reuse rather than building a generalized dependency engine first. Each implementation slice needs model tests and the repo's normal checks/UI validation.

The research does not choose related-member action policy, sidecar precedence, ambiguous detection defaults, metadata sorting costs, arbitrary renderer interactivity or nonlocal content-inspection opt-in. Those choices remain explicit future work, not blockers for completing this comparison.

## What I learned

The owner requirement for user-defined columns/renderers changes the destination materially: fixed enums remain useful implementation tools, but cannot be the extensible catalog. Properties groups are presentation, not the canonical fact database. A filename classification is evidence, not a format model. “Preview renderer” also names two different jobs—producing a page/image and turning prepared content into GUI—which must be distinguished.

The feature-only designs exposed stronger access-policy and provenance distinctions than a source-only refactor would. The code-informed designs showed that fyle already has useful identities, sessions, queues and shared readers, so a coherent destination does not require replacing all of them. They also showed that documentation can overstate sharing: the actual EXIF implementations differ for meaningful resource reasons.

I therefore changed the recommendation from merely extracting more format readers to defining a small semantic and extension contract, proving it across all four required contributions, and preserving feature-specific consumption. The goal is fewer places that need format knowledge, with explicit behavior when facts, access, completeness or renderer support differ.

The owner's follow-up adds two corrections. First, similarity across Properties, columns and viewers is itself a reason to share an abstraction; minimizing each interface independently can make the whole code harder to learn. Second, format-centered ownership misses Git and other contextual contributions. The revised recommendation is therefore **data providers with one recognizable contribution model, typed acquisition/results, and small presentation-specific selection policies**. Low-level format readers remain useful implementations beneath that organization, while universal fact stores, dependency graphs and brokers are deferred.

## Validation of this documentation change

For the original comparison, all five reports were checked for balanced Markdown fences, valid local links and existing referenced source paths; the staged diff passed whitespace checks. `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` and `cargo test` passed (548 tests). Those checks validated the existing application baseline, not the proposed designs.

The provider/contribution revision changes only this document. Markdown fences, local links and diff whitespace were checked. Formatting passed; clippy/test builds were started under the repository's pre-commit instruction and then stopped following the owner's clarification that a documentation-only edit does not need a build. No application code or dependencies changed, and no UI behavior was changed or live-tested.
