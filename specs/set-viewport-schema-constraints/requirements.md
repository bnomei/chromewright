# Requirements - set-viewport-schema-constraints

## Scope

Make the `set_viewport` MCP input schema advertise the same parameter constraints enforced by runtime validation.

This spec covers JSON Schema output, schema tests, and documentation. It does not change CDP behavior, output fields, or viewport metrics plumbing.

## Requirements

- R001: When the MCP tool schema advertises `set_viewport.width`, the schema shall describe it as an integer with minimum `1` and maximum equal to the runtime viewport dimension maximum.
- R002: When the MCP tool schema advertises `set_viewport.height`, the schema shall describe it as an integer with minimum `1` and maximum equal to the runtime viewport dimension maximum.
- R003: When the MCP tool schema advertises `set_viewport.device_scale_factor`, the schema shall describe it as a number greater than `0`.
- R004: When the MCP tool schema advertises `set_viewport.reset`, the schema shall make the reset-only parameter combination clear enough that agents do not infer width, height, or emulation fields are valid alongside `reset = true`.
- R005: When runtime validation bounds change, schema tests shall fail unless the advertised schema is updated to match.
- R006: If schema expressiveness cannot represent a runtime constraint exactly, then the schema description shall document the runtime rule and tests shall assert that description.
- R007: The change shall preserve runtime validation behavior and shall not relax any existing invalid-argument rejection.

