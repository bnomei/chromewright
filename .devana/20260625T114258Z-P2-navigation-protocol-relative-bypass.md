DEVANA-FINDING: v1
DEVANA-STATE: fixed | P2 | medium | security=yes
DEVANA-KEY: src/tools/utils.rs:52 | navigation-protocol-relative-bypass

# Protocol-relative URLs bypass navigation scheme safety gate

## Finding

`validate_navigation_url` blocks non-http(s) absolute schemes unless `allow_unsafe=true`, but treats any URL without a `:` scheme prefix as safe. `normalize_url` passes protocol-relative URLs like `//evil.example/path` through unchanged because they start with `/`. Browsers resolve them against the current page's scheme, enabling cross-site navigation without the explicit `allow_unsafe` opt-in documented for dangerous schemes.

## Violated Invariant Or Contract

With `allow_unsafe=false`, `navigate` and `new_tab` should not enable navigation patterns that smuggle an external origin without the same opt-in required for absolute `data:`, `file:`, or `chrome:` URLs.

## Oracle

README: "`navigate` and `new_tab` reject unsafe schemes such as `data:` and `file:` unless the caller passes `allow_unsafe=true`." Tests cover absolute `data:` blocking (`utils.rs` tests) but not `//host` forms. `test_normalize_url_relative_paths` expects `/path` passthrough by design.

## Counterexample

Active tab is `https://trusted.example/app`. MCP calls `navigate` with `{ "url": "//evil.example/phish", "allow_unsafe": false }`. `normalize_url` returns the string unchanged; `validate_navigation_url` returns `Ok` because `has_absolute_scheme("//evil.example/phish")` is false. Chrome navigates to `https://evil.example/phish`.

## Why It Might Matter

Agents and policies that rely on `allow_unsafe=false` as a hard boundary for non-web navigation can still be driven to arbitrary https origins via protocol-relative URLs while attached to an authenticated session.

## Proof

Counterexample value `//evil.example/phish` with control-flow through `normalize_url` (relative branch) → `validate_navigation_url` early `Ok` at `allow_unsafe || !has_absolute_scheme` → `BrowserSession::navigate` → CDP navigation sink.

## Counterevidence Checked

Same-origin relative paths (`/settings`) are intentionally supported. Absolute `//` only inherits scheme when a page context exists; `about:blank` behavior may differ. This does not block same-site relative navigation, which is desirable—but protocol-relative is not same-site relative.

## Suggested Next Step

Reject URLs whose trimmed form starts with `//`, or resolve and re-validate the absolute URL before navigation when a base URL is known.

## Agent Handoff

After working this report, preserve the original finding body. Update line 2 `DEVANA-STATE: ...` and the final `DEVANA-SUMMARY:` status/priority/confidence prefix. Use one of: `open`, `fixed`, `invalid`, `stale`, `duplicate`, `wontfix`. Keep `DEVANA-KEY:` stable unless the same finding moved. Add dated notes below with evidence checked.

## Status Notes

- 2026-06-25: open by Devana. Initial report written from static source inspection.
- 2026-06-26: fixed. `validate_navigation_url` now rejects protocol-relative
  targets unless `allow_unsafe=true`. Added `is_protocol_relative`, which flags
  inputs whose first two significant chars are slash-like (`//`, `/\`, `\/`,
  `\\`) — covering the browser's backslash normalization. Checked against both
  the raw input and the normalized form, since `normalize_url` leaves `//host`
  and `/\host` unchanged in its relative branch but mangles leading-backslash
  variants. Same-origin relative paths (`/settings`, `./`, `../`) still pass.
  Chose rejection (the report's primary suggestion) over silently promoting
  `//host` to `https://host`, since the gate cannot know the page's base scheme
  and failing closed is the safe posture; callers pass a full http(s) URL or
  opt in. Added block/opt-in/relative-path tests.

DEVANA-KEY: src/tools/utils.rs:52 | navigation-protocol-relative-bypass
DEVANA-SUMMARY: fixed | P2 | medium | validate_navigation_url now blocks protocol-relative //host (and backslash variants) unless allow_unsafe=true.