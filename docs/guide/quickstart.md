# Start with a research question

Helios Alpha has two working layers. Use Python to define and challenge an event hypothesis. Use Rust when the accepted computation must run causally, incrementally, and with explicit restart state.

If you are new to the repository, start with the research contract below. It prevents a fast implementation of the wrong backtest.

## Choose your working layer

| Your immediate job | Work in | What belongs there |
|---|---|---|
| Explore an event hypothesis | Python | Historical pulls, event studies, controls, and research reports |
| Build a deterministic operator | Rust | Ordering, watermarks, online state, typed outcomes, snapshots, and replay |
| Review the architecture | Documentation | Contracts, evidence standards, production gaps, and crate boundaries |

Python and Rust should implement the same causal definition. Python is not permission to use future information, and Rust is not evidence that a strategy has alpha.

## Write the causal contract first

Before choosing an operator, write down these six facts:

| Question | Example answer for the walkthrough |
|---|---|
| What is the event? | A typed observation satisfying a documented detection rule |
| When did it occur? | `event_time`, used for ordering and bucket assignment |
| When was it knowable? | `available_at`, used to gate the decision cut |
| What state is required? | A bounded reorder buffer and 10-minute online moments |
| What can it emit? | A closed bucket summary, then a research-owned candidate signal |
| What would falsify it? | Pre-registered controls, costs, and out-of-sample failure criteria |

The distinction between `event_time` and `available_at` is essential. An observation may describe an earlier event while becoming usable only later.

## Verify the Rust substrate

The Rust crates currently live in this repository rather than crates.io.

```bash
git clone https://github.com/AdrielC/helios-alpha.git
cd helios-alpha/rust
cargo test
```

This verifies the repository's operator mechanics, including its test suite. It does not validate a research hypothesis, broker integration, live risk controls, or profitable execution.

The default workspace excludes the optional ZMQ daemon and browser WASM target. Build those explicitly when you need them:

```bash
cargo build --release -p helios_signald
cargo check -p helio_backtest_wasm --target wasm32-unknown-unknown
```

## Run the Python research layer

```bash
cd helios-alpha
python -m venv .venv
source .venv/bin/activate
pip install -e ".[dev]"

helios-pipeline \
  pipeline.start_date=2024-01-01 \
  pipeline.end_date=2024-01-31 \
  pipeline.as_of_date=2024-01-31
```

`as_of_date` is the causal cut for the research run. Data published after that cut must not affect a result attributed to it.

## Map the question to a primitive

| If the question requires | Start with |
|---|---|
| A generic stateful transform | `helio_scan::Scan` |
| Zero or many outputs per input | `helio_scan::Emit` |
| A watermark or end-of-input boundary | `helio_scan::FlushableScan` |
| Restartable operator state | `SnapshottingScan` and `FallibleRestoreScan` |
| Stable streaming variance | `helio_stats::OnlineMoments` |
| Bounded disorder followed by time buckets | `helio_window::OrderedBucketPipeline` |
| A rare-event proving ground | `helio_event` |

## Follow the walkthrough

1. [Compose the 10-minute event signal](./compose-a-strategy).
2. [Bound agentic causal inference](./agentic-causal-trading) when semantic evidence is part of the strategy.
3. [Understand event time, availability, and watermarks](../concepts/event-time).
4. [Inspect the scan algebra](../concepts/scan-algebra).
5. [Prove checkpoint and replay behavior](./restart-a-pipeline).
6. [Apply the rare-event evidence standard](../research/evidence-standard).
7. [Audit the production boundary](../operations/production-readiness).
