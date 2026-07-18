# Semantic TUI roadmap

This document records the implementation sequence used to deliver the Semantic
TUI. Phases 1–5 are implemented; phase 6's release gates are maintained by CI,
browser smoke coverage, and the documentation listed below.

The roadmap is grounded in the frozen handoff and the current implementation constraints in:

- `.orchid/agent-handoffs/semantic-tui-roadmap/00-context.md`
- `.orchid/spec-research/semantic-tui/07-make-specs-handoff.md`
- `Cargo.toml`
- `src/browser/session.rs`
- `src/tools/click.rs`
- `src/tools/navigate.rs`
- `src/tools/go_back.rs`
- `src/tools/go_forward.rs`
- `src/mcp/handler.rs`

The original planning baseline declared Rust 1.88, enabled `mcp-server` by
default, used RMCP 1.5, and had no `tui` feature. The delivered outcome uses
RMCP 2.2 and a default-enabled but compile-time-optional `tui` feature.
`chromewright tui` presents a terminal-native semantic view of the active
Chrome page and shares one browser session and semantic document across
Markdown, Ratatui, the complete flat `tui_*` tool family, and bounded
active/revisioned semantic resources.

## Core invariants

Every phase must preserve these invariants:

- One normalized `SemanticDocument` will be the source for semantic Markdown, semantic and debug projections, Ratatui frames, MCP tools, and semantic MCP resources. Downstream consumers must not duplicate semantic parsing.
- The semantic model will represent real HTML semantic elements and interactive components, including inner text, links, lists, and inputs. It will not render CSS, pixels, or page layout.
- Semantic Markdown will remain distinct from the existing Readability-based `get_markdown` pipeline. This work must not replace, redirect, or change the `get_markdown` contract.
- The terminal and its co-hosted MCP endpoint will use the same Chrome `BrowserSession`, semantic document, page revision, selection, and agent-attention state. Standard stdio MCP mode will remain separate.
- The `tui` feature is enabled by default but remains optional. Builds using
  `--no-default-features --features mcp-server` omit TUI dependencies, the TUI
  command surface, `tui_*` tools, and semantic MCP resources.
- TUI navigation that can change the page will show `Loading`, drive the shared browser session, wait for the page to settle, and publish a fresh semantic render only after capture succeeds. The semantic document, URL, and title must change atomically.
- A stable selected anchor that survives a render transition will retain its viewport-relative position. If it does not survive, the TUI will keep a clamped scroll position and report the anchor change.
- `semantic_ref` values will be opaque and fail closed. Selection, viewport anchoring, copying, and agent attention must never guess, silently retarget, or accept an invalid, stale, or unknown identity.
- Normal and Input modes will remain separate. The TUI will not display key legends; configuration and documentation will be the shortcut-discovery surfaces.
- Active aliases and revision-addressed semantic Markdown, outline, component, semantic JSON, debug, selection, and attention reads will be bounded. State-changing operations will remain MCP tools.
- The implementation will not add a filesystem watcher.

## Phase 1: Dependency updates, RMCP migration, and the optional `tui` feature

### Goal

Establish an MSRV-compatible dependency and transport baseline before adding semantic, terminal, or MCP feature work. Isolate the RMCP migration so later failures can be attributed to the feature that introduced them.

### Dependencies

This phase starts from the current `Cargo.toml` baseline: Rust 1.88, RMCP 1.5, and `mcp-server` as the default feature. Capture a green pre-migration baseline before changing the dependency graph.

### Invariants

- Rust 1.88 remains the MSRV.
- Existing stdio and streamable-HTTP MCP tool behavior remains available.
- `src/mcp/handler.rs` must complete its RMCP major migration while it is still a tool-only handler; semantic resource implementation cannot begin before that checkpoint passes.
- A non-TUI `mcp-server` build does not compile terminal dependencies or advertise `tui_*` tools or semantic resources.

### Deliverables

- Upgrade RMCP first as a distinct migration, preserving the required macro, server, stdio transport, and streamable-HTTP server capabilities represented by the existing feature wiring.
- Update direct dependencies to the newest releases that remain compatible with the declared Rust 1.88 MSRV.
- Regenerate `Cargo.lock` from the accepted manifest constraints.
- Treat every other direct major-version change as a separate migration with its own compile/test checkpoint and recorded MSRV result.
- Add a `tui` Cargo feature and make terminal-specific dependencies optional. It is enabled in the default product binary but can be omitted from server-only builds.
- Gate the planned TUI command, semantic modules, `tui_*` tools, resource capability, and semantic resources behind `tui`.
- Preserve `mcp-server` as the default feature and preserve the existing binary's `mcp-server` requirement.
- Define an isolated `mcp-server`-versus-`tui` feature matrix that later phases can use for surface checks.

### Acceptance checks

Acceptance requires:

- the RMCP migration compiles and passes the existing default-feature tests before semantic TUI code is introduced;
- the isolated `mcp-server` build retains its non-TUI MCP surface and does not compile or expose TUI dependencies, TUI entry points, `tui_*` tools, resource capability, or semantic resources;
- a build with `--features tui` resolves and compiles its complete optional dependency closure; and
- the feature tree demonstrates that terminal-only dependencies are reachable only through `tui`.

### Risks

Mixing a major RMCP migration with semantic, terminal, and MCP additions would obscure the source of API and transport regressions. If the feature gate leaks at this stage, every later phase could accidentally change the default public surface.

### Completion gate

Phase 1 is complete only when the isolated RMCP migration, direct dependency update, MSRV check, and isolated feature-surface checks are green. MCP resource work remains blocked until this gate passes.

## Phase 2: Semantic capture, component model, and fail-closed identity

### Goal

Create one normalized semantic document and component model that every renderer and coordination surface can consume without reparsing the page.

### Dependencies

Phase 1 must pass first. Renderer, anchor, selection, attention, active Markdown resource, and input contracts depend on this phase's normalized component and identity rules.

### Invariants

- `BrowserSession::extract_dom` in `src/browser/session.rs` continues to return the actionability-focused `DomTree`; semantic capture is a separate path.
- Capture is read-only with respect to Chrome and does not depend on CSS layout or pixels.
- Published semantic state contains matching document metadata, revision, model, and reference index.
- `semantic_ref` resolution never silently rebinds to a similar component.

### Deliverables

- Add the planned semantic module, document model, component model, and browser-side semantic extraction.
- Normalize `main`, `aside`, `header`, `nav`, `section`, and `footer` landmarks plus headings/text, lists/list items, links, images, inputs, textareas, selects, buttons, and generic semantic groups.
- Retain meaningful inner text and interaction data. Ignore generic layout wrappers unless they carry included semantics or are required to preserve a semantic child.
- Exclude CSS, pixel, and layout rendering from capture and from the normalized model.
- Assign a document revision to each successfully captured semantic document.
- Define opaque `semantic_ref` identities for components and anchors used by viewport restoration, copying, human selection, and agent attention. Prefer a durable unique author identity when present; otherwise use a normalized semantic-ancestry fingerprint scoped to document origin/path.
- Resolve the same exact identity across consecutive captures when it survives. If it does not survive, report it stale or changed rather than retargeting by text similarity.
- Define fail-closed handling for invalid, stale, unknown, or revision-incompatible semantic references.
- Add representative semantic fixtures for content, links, lists, form controls, and mixed interactive documents.

### Acceptance checks

Acceptance requires deterministic normalized fixtures and component relationships, rejection of every unsupported identity instead of guessed targeting, and a model that later renderers can consume without inspecting raw page HTML.

### Risks

If renderers, terminal state, or MCP contracts precede the semantic and identity contracts, they will tend to duplicate parsing or embed presentation-specific identities. That would make cross-renderer selection and revision safety unreliable.

### Completion gate

Phase 2 is complete only when one bounded, revision-identified `SemanticDocument` can be captured through the shared session and stale, malformed, ambiguous, or wrong-document references cannot retarget silently.

## Phase 3: Shared Markdown, semantic, debug, and Ratatui renderers

### Goal

Render all semantic views from the normalized model while preserving the independent Readability-based `get_markdown` behavior.

### Dependencies

Phase 2 must pass first. Ratatui frame work and active Markdown resource serialization may start only after the renderer input contract and reference propagation are fixed.

### Invariants

- Markdown, Ratatui, outline/component, semantic JSON, and debug projections traverse the same captured model.
- Renderers copy `semantic_ref` values from the model rather than deriving identity from display text.
- Rendering cannot navigate, capture, mutate selection, or advance a revision.
- Existing Readability `get_markdown` retains its name, cache, and public behavior.

### Deliverables

- Add a semantic Markdown renderer that consumes `SemanticDocument`.
- Add semantic JSON, outline, component, and debug projections from the same model.
- Add the Ratatui renderer as another consumer of the same document and component identities.
- Preserve semantic order, content, interaction metadata, and opaque references across representations where each representation can express them.
- Add shared fixtures or golden outputs that make representation drift visible.
- Keep the semantic Markdown path independent of the existing Readability and HTML-to-Markdown `get_markdown` pipeline.

### Acceptance checks

Acceptance requires shared fixtures to demonstrate consistent document, component order, reference meaning, and revision across semantic Markdown, semantic/debug projections, and Ratatui frames. Regression coverage must show that `get_markdown` still uses its independent pipeline in `src/tools/services/markdown.rs`.

### Risks

Renderer-specific extraction would create contradictory views of the same page and duplicate identity logic. Reusing or replacing `get_markdown` would also turn an additive feature into a regression for the existing Readability surface.

### Completion gate

Phase 3 is complete only when all projections use one fixture-backed model, preserve exact references and output bounds, and leave `get_markdown` unchanged.

## Phase 4: Terminal browser lifecycle, navigation, inputs, and keymap

### Goal

Provide the planned `chromewright tui` terminal browser over the shared Chrome session, with explicit lifecycle states, stable semantic navigation, input handling, and Vimari-compatible controls.

### Dependencies

Phases 1–3 must pass first. The lifecycle coordinator must account for current browser behavior: `src/tools/navigate.rs` can wait for navigation; `src/tools/go_back.rs` and `src/tools/go_forward.rs` already settle history actions; `src/tools/click.rs` performs a real click but still requires an explicit wait and fresh read. The TUI must normalize those paths instead of treating their current handoffs as a fresh semantic render.

### Invariants

- The command lifecycle is `Ready -> Loading -> Ready | Error`.
- `Ready` is impossible until wait/settle, fresh metadata, semantic capture, reconciliation, and atomic publication complete.
- `Error` preserves the last valid semantic document and never presents a partial update as ready.
- Normal-mode actions cannot fire during URL or page-input editing.
- TUI frames contain no shortcut legend.

### Deliverables

- Add persistent terminal browser chrome that presents the interaction mode, active URL, title, lifecycle/error status, and back/forward availability.
- Model navigation status as `Loading`, `Ready`, or `Error`. While ready, keep Normal, Input, and Hint interaction modes distinct.
- Route TUI navigation through the shared `BrowserSession`: enter `Loading`, perform the browser action, wait for the page to settle, capture a fresh semantic document, and atomically publish the document with its URL and title.
- Enter `Error` when navigation or fresh capture fails rather than presenting a partial update as ready.
- Support semantic navigation through anchors, links and hints, scrolling, URL entry, browser history and reload actions, input focus, and input editing.
- Use `Tab` and `Shift-Tab` to traverse focusable semantic inputs in document order. Bridge input, select, button, and submission actions to existing browser tools, then recapture after successful page-changing interactions.
- Include forward search, collapse, inspection metadata, deterministic two-key link hints, and clipboard copy. `f` follows in the current tab, `F` opens in a new tab, and hint selection remains available for chained follows until Escape.
- Retain search, selection, collapse, and attention only by exact `semantic_ref`. `y` copies rendered block text and `Y` copies the opaque reference through OSC 52; terminal refusal uses a visible non-destructive fallback.
- Restore a surviving selected anchor to its viewport-relative position after a render transition; otherwise retain clamped scroll and report the identity change.
- Use the decided Vimari-compatible default key set: `f/F`, `j/k`, `h/l`, `u/d`, `gg`, `G`, `gi`, `H/L`, `r`, `w/q`, `x`, and `t`, plus `o` for URL entry.
- Load a TOML action-to-binding overlay from `--config <PATH>` or the XDG default configuration path.
- Keep shortcut discovery in configuration and documentation rather than rendering key legends in the TUI.
- Do not add filesystem watching for configuration or page content.

### Acceptance checks

Acceptance requires integration scenarios to prove:

- every page-changing TUI navigation follows `Loading` to fresh capture to atomic `Ready`, or terminates in `Error`;
- URL, title, document, and revision cannot expose a mixed transition;
- back, forward, reload, URL entry, link or hint navigation, scrolling, and input flows preserve the Normal/Input/Hint boundaries;
- `f` and `F` have distinct current-tab/new-tab behavior, chained hints end on Escape, and search/collapse/inspection/copy actions preserve exact references;
- surviving anchors restore viewport-relative position and missing anchors follow the clamped-scroll fallback; and
- Vimari defaults, explicit config precedence, XDG fallback, configuration parsing, and action rebinding are deterministic.

### Risks

Building lifecycle behavior before capture, identity, and rendering contracts would allow URL, title, and document state to diverge. It would also make focus, input modes, and viewport restoration depend on unstable component identities.

### Completion gate

Phase 4 is complete only when every required command uses the shared session, shows the complete loading lifecycle, publishes only fresh atomic state, restores or explicitly loses anchors, supports configured Vimari-compatible actions and semantic inputs, and renders no shortcut legend.

## Phase 5: Co-hosted HTTP MCP, `tui_*` tools, and bounded revisioned resources

### Goal

Let the terminal and MCP clients coordinate against the same in-process browser session, semantic document, selection, attention state, navigation state, and page revision.

### Dependencies

The RMCP gate in Phase 1 is mandatory. Resource serialization depends on Phase 3, while shared loading, selection, and attention coordination depends on the Phase 4 state contract.

### Invariants

- The companion binds to loopback and shares the TUI's in-process `BrowserSession` and semantic state.
- Standard stdio MCP remains separate.
- Revision-addressed resources never return content from a different revision.
- Human selection and agent attention remain separately owned.
- Resource reads are bounded and non-mutating; state changes remain tools.
- Standard MCP transports omit companion wiring, `tui_*` descriptors, resource capability, and all semantic resources; isolated non-TUI builds omit their code entirely.

### Deliverables

- Co-host an in-process loopback streamable-HTTP MCP endpoint with the TUI, with explicit port and HTTP-path options.
- Keep the co-hosted endpoint distinct from the standard stdio MCP mode.
- Add the frozen flat tool family in `src/tools/tui.rs`: `tui_render`, `tui_refresh`, `tui_inspect`, `tui_query`, `tui_selection_read`, `tui_selection_update`, `tui_attention_read`, `tui_attention_set`, and `tui_attention_clear`.
- Make tools consume the shared semantic model and state rather than introducing a second parser or document cache.
- Expose the planned resource catalog through `src/mcp/resources.rs` and `src/mcp/handler.rs`:
  - `chromewright://active/semantic.md`;
  - `chromewright://page/{document_id}/{revision}/semantic.md`;
  - `chromewright://page/{document_id}/{revision}/outline.md`;
  - `chromewright://page/{document_id}/{revision}/semantic.json`;
  - `chromewright://page/{document_id}/{revision}/debug.json`;
  - `chromewright://page/{document_id}/{revision}/component/{semantic_ref}.md`;
  - `chromewright://tui/selection.json`;
  - `chromewright://tui/attention.json`.
- Treat the active and collaboration URIs as dynamic orientation aliases. Treat document/revision URIs as immutable views of the named revision.
- Bound every read and paginate semantic Markdown. Define and test finite revision retention; unavailable revisions fail explicitly and never fall through to the active document.
- Expose loading, navigation, selection, attention, and current-revision coordination consistently to the terminal and MCP clients.
- Keep state-changing actions as tools and resource access as bounded reads.
- Apply the `tui` feature gate to loopback co-hosting, every `tui_*` tool, resource capability, and every semantic resource.

### Acceptance checks

Acceptance requires:

- the TUI, `tui_*` tools, and resources observe the same session, document revision, selection, attention, and lifecycle state;
- selection updates do not overwrite agent attention, and attention updates do not overwrite selection or mutate Chrome;
- active reads resolve to the current revision and revision-addressed reads either return the requested retained revision or fail explicitly;
- Markdown, outline, semantic JSON, debug, component, selection, and attention reads obey their bounds and revision contracts;
- invalid, stale, malformed, wrong-document, and evicted references or revisions fail closed;
- state changes occur through tools rather than resource reads; and
- standard MCP transports expose none of the co-hosted TUI tools, resource capability, or semantic resources, and non-TUI builds omit them entirely.

### Risks

Introducing transport and coordination before lifecycle and revision semantics would create split-brain state, stale resource reads, or unbounded revision retention. Poor feature isolation could also change the standard stdio server or default compiled surface.

### Completion gate

Phase 5 is complete only when the loopback companion, terminal, complete tool family, and bounded active/revisioned resources observe one shared revision and coordination state, fail closed for stale identity, remain absent from standard MCP transports, and are omitted from non-TUI builds.

## Phase 6: Integration validation, feature-surface checks, and release/documentation gates

### Goal

Validate the full semantic TUI path and prevent release until the default and isolated feature surfaces satisfy their contracts and the documentation matches delivered behavior.

### Dependencies

Phases 1–5 must pass. This phase integrates their contracts and cannot waive a failed phase gate.

### Invariants

- Default and all-feature validation are independent release surfaces.
- Fixture tests do not replace headless browser and loopback integration coverage.
- Release documentation describes only behavior that has passed the final gates.

### Deliverables

- Add headless integration coverage for navigation, `Loading`/`Ready`/`Error`, wait-and-capture behavior, atomic chrome and document updates, anchor restoration, inputs, modes, and keymap configuration.
- Add end-to-end coverage proving that Ratatui, semantic Markdown/debug/JSON projections, the complete `tui_*` tool family, and active/revision-addressed resources share one document and revision.
- Add failure-path coverage for navigation errors, capture errors, configuration parsing, unknown or evicted revisions, truncation, and invalid or stale semantic references.
- Add isolated `mcp-server`-versus-`tui` regression checks for dependencies, command surface, tools, resource capability, and resources.
- Document the default-enabled optional feature and command, lifecycle states, Vimari-compatible defaults, TOML action bindings, `--config` and XDG configuration lookup, shortcut-discovery policy, and error behavior.
- Document the distinction between the co-hosted loopback HTTP endpoint and standard stdio MCP mode.
- Document the `tui_*` tool and semantic resource contracts, bounds, revisions, and fail-closed reference behavior.
- Preserve the explicit distinction between semantic Markdown and Readability `get_markdown` in user and release documentation.
- Add release notes only when the planned surfaces are delivered; do not publish planned behavior as current behavior.

### Acceptance checks

Acceptance requires the integration and documentation gates to pass and all of these commands to succeed:

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
cargo test
cargo tree -e features
cargo tree -e features --features tui
cargo +1.88 check --all-targets --all-features --locked
cargo package --locked
```

The final review must also confirm that:

- the isolated `mcp-server` build contains no TUI dependencies, command surface, `tui_*` tools, resource capability, or semantic resources;
- the default and isolated `tui` builds contain the complete planned surface;
- the existing Readability `get_markdown` behavior remains independent;
- the documented keymap and configuration precedence match the delivered implementation; and
- release documentation describes only behavior that has passed these gates.

### Risks

Releasing before the isolated feature matrix and end-to-end revision checks pass could leak optional dependencies or APIs into non-TUI builds, publish divergent semantic views, or document planned behavior as if it were available.

### Completion gate

Phase 6 is complete, and the semantic TUI is release-ready, only when every prior gate and the default/all-feature, MSRV, integration, documentation, and packaging checks pass. No required user-facing semantic-TUI behavior is deferred beyond this gate.
