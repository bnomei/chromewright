# Design - set-viewport-schema-constraints

## Goal

Expose `set_viewport` constraints in the MCP schema so agents see the real contract before calling the tool.

## Architecture Alignment

This spec remains active as a contract-stabilization prerequisite. It should land before `contract-domain-extraction` moves viewport request or output DTOs, so the eventual contract module carries the schema-visible constraints instead of preserving a known gap.

## Distilled Discovery

- `SetViewportParams` currently derives `JsonSchema`.
- Runtime validation requires `width` and `height` when `reset` is false.
- Runtime validation rejects zero dimensions and dimensions above `VIEWPORT_DIMENSION_MAX`, currently `10_000_000`.
- Runtime validation rejects non-finite or non-positive `device_scale_factor`.
- Runtime validation rejects `reset = true` when width, height, DPR, mobile, touch, or orientation are also supplied.
- MCP tests currently assert that `width`, `height`, `reset`, and `viewport_after` exist, but they do not assert numeric schema bounds or reset combination semantics.

## Schema Strategy

Implement a custom schema for `SetViewportParams` rather than relying only on derive output.

The schema should advertise:

```text
width:
  type: integer
  minimum: 1
  maximum: VIEWPORT_DIMENSION_MAX

height:
  type: integer
  minimum: 1
  maximum: VIEWPORT_DIMENSION_MAX

device_scale_factor:
  type: number
  exclusiveMinimum: 0

reset:
  type: boolean
  default: false
```

For reset combinations, prefer a JSON Schema `oneOf` if schemars version and MCP conversion preserve it clearly:

```text
oneOf:
  - required: [width, height]
    properties:
      reset:
        const: false
  - properties:
      reset:
        const: true
    not:
      anyOf:
        - required: [width]
        - required: [height]
        - required: [device_scale_factor]
        - required: [mobile]
        - required: [touch]
        - required: [orientation]
```

If this is too awkward with the current schemars API, use field descriptions plus runtime tests. The runtime validator remains the source of enforcement either way.

## Reuse Points

- `ViewportEmulationRequest::validate` owns the runtime numeric rules.
- `VIEWPORT_DIMENSION_MAX` is currently private to `src/browser/backend.rs`; expose it as `pub(crate)` if schema code needs to reference it.
- `src/mcp/mod.rs` already has schema tests for `set_viewport`.
- `src/tools/mod.rs` already has schema tests for the tool registry.

## Validation Plan

Required checks:

```text
cargo test --lib test_set_viewport_schema_exposes_breakpoint_contract
cargo test --lib test_set_viewport_schema_is_exported_via_mcp
cargo test --lib set_viewport
cargo check --locked
```

The schema tests should inspect concrete JSON properties such as `minimum`, `maximum`, `exclusiveMinimum`, `oneOf`, `not`, or fallback descriptions.

## Non-Goals

- Changing `SetViewportOutput`
- Renaming `device_scale_factor`
- Relaxing runtime validation
- Adding viewport constraints to unrelated tools
