DEVANA-FINDING: v1
DEVANA-STATE: fixed | P2 | high | security=no
DEVANA-KEY: src/tools/services/markdown.rs:63 | markdown-cache-stale-spa-url

# get_markdown cache can serve stale body after SPA URL change

## Finding

The session `markdown_cache` keys only on `(document_id, revision)` and is never invalidated on navigation, tab switches, or viewport changes. `document_metadata.js` reads `document.location.href` on every call but bumps `revision` only via `MutationObserver` DOM mutations. SPA `history.pushState` / `replaceState` can change the URL without mutating the observed subtree, leaving revision unchanged while the live URL changes.

On a cache hit, `execute_get_markdown` overwrites `output.result.document` with fresh metadata but leaves top-level `url`, `title`, `markdown`, `byline`, `excerpt`, and `site_name` from the cached entry.

## Violated Invariant Or Contract

A successful `get_markdown` response should present a single coherent document snapshot: metadata, markdown body, and pagination fields should all describe the same page state.

## Oracle

`markdown_cache_entry` match requires `document_id` and `revision` equality only (`cache.rs`). Navigate/tab/viewport paths call `invalidate_snapshot_cache` but never clear markdown. `document_metadata.js` sets `url: document.location.href` independently of revision counter.

## Counterexample

1. Page at `https://app.example/list` with `document_id=D`, `revision=main:42`. Agent calls `get_markdown` → cache stores list content and URL.
2. App calls `history.pushState` to `https://app.example/item/99` without DOM mutations visible to the observer. Revision stays `main:42`.
3. Agent calls `get_markdown` again. Cache hits. `result.document.url` is `.../item/99` but `output.url` and `output.markdown` still describe `/list`.

## Why It Might Matter

Agents planning navigation or summarization from `get_markdown` can act on stale article text while believing the URL/metadata reflect the current route—common on SPA dashboards and client routers.

## Proof

Dataflow trace: `document_metadata()` (fresh url, unchanged revision) → `markdown_cache_entry` HIT → `paginate_markdown(entry)` (stale url/title/markdown) → `output.result = DocumentResult::new(document)` (fresh url) → success payload with split provenance.

## Counterevidence Checked

Full navigation that creates a new `document_id` misses cache. DOM edits that trigger `MutationObserver` bump revision and miss cache. Tab switches normally change `document_id` per document UUID, avoiding cross-tab hits. Fake backend uses `tab.id` as `document_id`, which does not reproduce pushState but does not negate production behavior.

## Suggested Next Step

Include `url` (or a content hash) in the markdown cache key, invalidate markdown cache on navigate/tab/history seams, or re-extract when cached `entry.url != document.url` even if revision matches.

## Agent Handoff

After working this report, preserve the original finding body. Update line 2 `DEVANA-STATE: ...` and the final `DEVANA-SUMMARY:` status/priority/confidence prefix. Use one of: `open`, `fixed`, `invalid`, `stale`, `duplicate`, `wontfix`. Keep `DEVANA-KEY:` stable unless the same finding moved. Add dated notes below with evidence checked.

## Status Notes

- 2026-06-25: open by Devana. Initial report written from static source inspection.
- 2026-06-26: fixed. `markdown_cache_entry` now also requires `entry.url ==
  document.url` (alongside `document_id`/`revision`). Both URLs derive from
  `window.location.href` (extraction JS `convert_to_markdown.js` and
  `document_metadata.js`), so after an SPA `pushState`/`replaceState` the live
  URL advances while the cached entry keeps the old URL → cache miss →
  re-extraction. This also removes the split-provenance hit path: when the
  entry matches, its url/title/markdown are consistent with the fresh document.
  Chose the URL-in-key approach over a content hash (cheaper, no extra
  evaluation) and over event-based invalidation (revision does not advance on
  pushState, so there is no DOM seam to hook). Added cache hit/miss tests.

DEVANA-KEY: src/tools/services/markdown.rs:63 | markdown-cache-stale-spa-url
DEVANA-SUMMARY: fixed | P2 | high | markdown cache now keys on the live URL too, so a pushState route change forces re-extraction instead of serving stale markdown.