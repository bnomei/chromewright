# Changelog

## 0.5.1 - 2026-06-10

### Fixed

- Advertise `wait` tool parameters as a flat object schema without top-level schema composition, while preserving strict runtime validation.
- Ensure registered tool input schemas avoid top-level `oneOf`, `anyOf`, `allOf`, `enum`, and `not` keywords for broader client compatibility.

## 0.5.0 - 2026-06-01

### Security

- Fail closed for stale cursor mutations while preserving read-only `inspect_node` selector rebinding.
- Add bounded wait, screenshot, DOM extraction, reader, markdown, and inspection resource limits.
- Harden screenshot artifact storage with private per-session directories and exclusive artifact file creation.

### Changed

- Add `inspect_node.truncated_fields` metadata when compact fields are shortened by inspection output limits.
