# Quickstart

Helios Alpha has two working layers. Use Python to form and test event hypotheses. Use Rust when the same computation must run causally, incrementally, and with explicit restart state.

## Run the Rust workspace

The Rust crates currently live in this repository rather than crates.io.

```bash
git clone https://github.com/AdrielC/helios-alpha.git
cd helios-alpha/rust
cargo test
```

The default workspace excludes the optional ZMQ daemon and browser WASM target. Build those explicitly when you need them:

```bash
cargo build --release -p helios_signald
cargo check -p helio_backtest_wasm --target wasm32-unknown-unknown
```

## Run the research pipeline

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

`as_of_date` is the causal cut. Data published after that instant must not affect a result attributed to that instant.

## Choose the right first primitive

| If you need | Start with |
|---|---|
| A generic stateful transform | `helio_scan::Scan` |
| Zero or many outputs per input | `helio_scan::Emit` |
| Watermark or session controls | `helio_scan::FlushableScan` |
| Restartable state | `SnapshottingScan` and `FallibleRestoreScan` |
| Stable streaming variance | `helio_stats::OnlineMoments` |
| Out-of-order events and time buckets | `helio_window::OrderedBucketPipeline` |
| A rare-event strategy proving ground | `helio_event` |

## Next

- [Compose a strategy](./compose-a-strategy)
- [Understand the scan algebra](../concepts/scan-algebra)
- [Audit the production boundary](../operations/production-readiness)
