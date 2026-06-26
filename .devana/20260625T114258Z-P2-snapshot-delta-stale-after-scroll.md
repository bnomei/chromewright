DEVANA-FINDING: v1
DEVANA-STATE: fixed | P2 | high | security=no
DEVANA-KEY: src/tools/scroll.rs:141 | snapshot-delta-stale-after-scroll

# snapshot delta reuses pre-scroll viewport cache base

## Finding

`snapshot` delta mode reads the session snapshot cache keyed by `document_id` only, intentionally reusing a prior viewport projection as the diff base. `scroll` invalidates `ToolContext.dom_tree` but does not call `invalidate_snapshot_cache`. Scrolling changes which nodes are viewport-local without necessarily bumping document `revision` (scroll is not a DOM mutation in `document_metadata.js`).

A `snapshot(mode=delta)` after `scroll` can therefore diff the current DOM against a pre-scroll viewport base, yielding incomplete or misleading delta nodes and YAML.

## Violated Invariant Or Contract

After a scroll that changes viewport locality, a delta snapshot should reflect what entered or left the visible surface relative to the current scroll position, not a pre-scroll viewport base.

## Oracle

README describes delta as session-local changed surface when a compatible prior base exists. Tests invalidate snapshot cache on navigate, tab switch, history, and viewport emulation—but not on `scroll`. `snapshot_cache_reuses_prior_revision_for_matching_document_identity` documents intentional revision lag for delta bandwidth, not scroll skew.

## Counterexample

1. `snapshot` default viewport at `scrollY=0` caches viewport-biased nodes `{A,B}` for `document_id=D`.
2. Agent calls `scroll` down so node `C` enters the viewport; `revision` remains `main:N`.
3. Agent calls `snapshot` with `mode: "delta"`. Cache hits on `document_id=D` with base from step 1.
4. Delta output omits newly visible `C` (and may omit removals) because `delta_snapshot_text` only emits lines not consumed from the old multiset and node diff keys on selector.

## Why It Might Matter

Agents using scroll-then-delta as a bandwidth-saving reread pattern get silently stale actionable nodes, causing missed clicks or wrong follow-up cursors after scrolling.

## Proof

State transition trace: viewport snapshot stores cache → scroll (no `invalidate_snapshot_cache`, revision unchanged) → delta snapshot `snapshot_cache_entry` HIT → `project_snapshot` with stale `base` → delta projection sink.

## Counterevidence Checked

`set_viewport` and navigation invalidate snapshot cache and are tested. Fresh `snapshot(mode=viewport)` after scroll rebuilds from current DOM but replaces cache only after that call completes; the first delta after scroll still uses the stale base. `snapshot(mode=full)` does not update cache projection (`cache_projection: None`), leaving an older viewport base for subsequent delta.

## Suggested Next Step

Invalidate snapshot cache on successful `scroll`, or include scroll generation / viewport scroll offset in cache scope, or document that delta must not follow scroll without an intervening viewport snapshot.

## Agent Handoff

After working this report, preserve the original finding body. Update line 2 `DEVANA-STATE: ...` and the final `DEVANA-SUMMARY:` status/priority/confidence prefix. Use one of: `open`, `fixed`, `invalid`, `stale`, `duplicate`, `wontfix`. Keep `DEVANA-KEY:` stable unless the same finding moved. Add dated notes below with evidence checked.

## Status Notes

- 2026-06-25: open by Devana. Initial report written from static source inspection.
- 2026-06-26: fixed. The `scroll` tool now calls
  `context.session.invalidate_snapshot_cache()` after a successful scroll
  (option a), matching the navigate/history/set_viewport seams. The next
  `snapshot(mode=delta)` therefore rebuilds from the current scroll position
  instead of diffing against a pre-scroll viewport base. Chose cache
  invalidation over encoding the scroll offset into the cache scope because it
  reuses the established invalidation pattern and the post-scroll delta has no
  meaningful base to diff against anyway. Added
  `test_scroll_invalidates_snapshot_cache`.
  Residual (out of scope for scroll.rs:141): `press_key` scroll keys (PageDown/
  End) and interaction `scroll_target_into_view` change viewport locality
  without a revision bump and do not invalidate the cache; if confirmed
  problematic they warrant their own report.

DEVANA-KEY: src/tools/scroll.rs:141 | snapshot-delta-stale-after-scroll
DEVANA-SUMMARY: fixed | P2 | high | scroll now invalidates the snapshot cache so the next delta snapshot rebuilds from the current scroll position.