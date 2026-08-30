# One source protocol from research to live

A strategy should not have separate, subtly incompatible readers for a notebook backfill, restart
catch-up, and live operation. Helios models all three as positions on one causally ordered source.

## The envelope

Every admitted record carries six facts before its payload:

| Field | Meaning |
|---|---|
| source identity | provider, dataset, and schema contract |
| partition | independently ordered source lane |
| offset | exact durable position inside that lane |
| event time | when the measured event occurred |
| available at | earliest instant this record may affect a decision |
| observed at | when this deployment actually received it |

Event time cannot substitute for availability time. A corrected space-weather bulletin may describe
an old flare while becoming usable only now. Replaying it at the flare time would leak the revision
backward.

## Rewind, backfill, handoff

`HelioSource` exposes four operations with the same checkpoint type:

1. `rewind(available_at)` resolves the source prefix strictly before a causal cut.
2. `backfill(request)` reads a bounded historical range from that prefix.
3. `replay(request)` validates source identity, phase, availability, and contiguous offsets.
4. `stitched(request)` hands the exact validated checkpoint to a resumable live tail.

The Rust contract lives in `helio_scan::source`. The Python research contract lives in
`helios_alpha.sources`. Both reject offset gaps and per-partition availability regression. A
provider that cannot resume strictly after a checkpoint cannot claim gap-free handoff.

Some providers can subscribe first, establish a fence, and backfill up to that fence. Those
adapters may advertise an atomic handoff. A replay-then-live provider must instead prove exact
resume behavior from the final backfill checkpoint.

## Canonical research frame

`HelioFrame` turns source envelopes into one Polars schema:

```python
from helios_alpha.frames import HelioFrame

frame = HelioFrame.from_envelopes(
    records,
    event_type="trade",
    instrument="SPY",
    value_field="price",
    unit="USD",
)
known = frame.as_of(decision_cut)
```

Polars is authoritative. The frame validates identity uniqueness, contiguous offsets, timezone-aware
timestamps, observation after availability, and monotone availability per source partition.

Pandas is a compatibility edge, not a second data model:

```python
pdf = frame.to_pandas()
pdf.helio.validate()
known_pdf = pdf.helio.as_of(decision_cut)
```

The `.helio` accessor converts through the canonical Polars frame before doing work. Notebook code
therefore receives the same point-in-time checks as the native research path.

## What this does not prove

The protocol proves local continuity only when an adapter supplies truthful offsets and
availability. It does not reconstruct revisions that a provider has overwritten, create atomicity
where an API has none, or make a current historical payload point-in-time safe. Preserve every live
revision as observed and document provider-specific gaps explicitly.
