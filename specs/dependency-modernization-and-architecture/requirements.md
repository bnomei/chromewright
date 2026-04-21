# Requirements — dependency-modernization-and-architecture

## Scope

This spec covers the next change wave for `chromewright`:

- establish a trustworthy baseline
- modernize dependencies in bounded waves
- handle the `rmcp` major upgrade intentionally
- improve structural seams that are currently making upgrades and testing brittle

## Requirements

- R001: When starting any upgrade wave, the repository shall provide a repeatable baseline that records which validation commands currently pass and which currently fail.
- R002: When executing this spec, the repository shall land work in bounded baby steps, and each step shall close with one atomic commit after its validation is green.
- R003: When reviewing direct dependencies, the repository shall classify each one as lockfile refresh only, same-major manifest bump, or major-version migration so that low-risk changes land before disruptive ones.
- R004: If browser-launch tests are used as a validation gate, then the repository shall provide deterministic browser launch behavior instead of depending on an implicit debugging-port scan that can fail in local environments.
- R005: When upgrading `rmcp` from `0.8.x` to `1.x`, the repository shall replace or remove any transport and API usage that is no longer supported upstream.
- R006: When exposing MCP tool results, the server shall preserve machine-readable structured data instead of collapsing all tool output into prettified text only.
- R007: When the CLI advertises browser connectivity or profile flags, the binary shall either honor those flags end-to-end or remove them from the public interface.
- R008: When structurally cleaning up the tool layer, the repository shall reduce duplicated element-target resolution logic and avoid stale DOM snapshots after state-changing actions.
