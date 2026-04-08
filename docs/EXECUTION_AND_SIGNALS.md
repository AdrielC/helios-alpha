# Execution and local signal broadcast

This repo is primarily **research**. Live trading needs a **hard boundary** between signal generation and order placement so you never “accidentally” send risk.

## Design goals

1. **Low latency, local IPC** — same machine or LAN; no cloud hop for the critical path.
2. **One schema, many consumers** — Python emits; **Rust** (and Python) subscribers react (risk, sizing, broker adapters).
3. **Explicit causality** — every signal carries `emitted_at_utc` and optional `causal_ts_utc` (what instant the model treated as “now”).
4. **Kill switch & idempotency** — execution layer dedupes on `signal_id`; global disable outside the research stack.

## Recommended topology

```
┌─────────────────────┐     PUB (tcp ipc inproc)     ┌──────────────────────┐
│ Python: research    │ ─────────────────────────► │ Rust: helios_signald │
│ ingest / SSI / rules│         JSON + topic         │ parse, fan-out, log  │
└─────────────────────┘                              └──────────┬───────────┘
                                                                │
                    ┌───────────────────────────────────────────┼───────────────┐
                    ▼                                           ▼               ▼
            ┌───────────────┐                          ┌─────────────┐   ┌──────────────┐
            │ bar / quote   │                          │ risk engine │   │ broker I/O   │
            │ microstructure│                          │ (limits)    │   │ (separate    │
            │ (future Rust) │                          └─────────────┘   │  process)    │
            └───────────────┘                                             └──────────────┘
```

- **Tier A — signals**: space-weather / regime flips (`helios.signal` topic). Small messages, high fan-out.
- **Tier B — market data**: later, **shared memory** or **nanomsg/iceoryx** for bar/tick firehose; ZMQ is fine for moderate intraday aggregates.
- **Tier C — orders**: **only** behind a dedicated adapter (IBKR, FIX, etc.) that subscribes to **approved** `OrderIntent` messages (separate topic or queue), never directly from research code.

## Why ZMQ here

- **PUB/SUB** matches “broadcast signal, many listeners.”
- **PUSH/PULL** if you need strict load-balanced workers later.
- **inproc://** for same-process Python+Rust via FFI is rare; prefer **tcp://127.0.0.1:port** between processes.

Alternatives you can swap without changing the JSON body: **Redis pub/sub**, **NATS**, **MQTT** (local broker). The **Pydantic schema** in `helios_alpha.signals.schema` stays the contract.

## Python → Rust contract

- **Payload**: UTF-8 JSON, one object per message (ZMQ multipart: `[topic, json_bytes]`).
- **Versioning**: `schema_version` field; bump when fields are removed or semantics change.
- Code: `helios_alpha.signals.publisher.SignalPublisher` and `rust/helios_signald`.

## Operational checklist (before real money)

- [ ] Execution process runs under a **different user** / **capability drop** than research.
- [ ] **Rate limits** and **max notional** per signal type in the Rust (or dedicated) risk layer.
- [ ] **Paper trading** flag in config; broker adapter refuses live when set.
- [ ] **Audit log** append-only (signal_id, timestamp, outcome).
- [ ] **Clock sync** (chrony) if you compare wall time to exchange timestamps.

## Where Rust pays off next

| Component | Role |
|-----------|------|
| `helios_signald` | Sub microsecond parse + route; optional aggregation |
| Bar joiner | Align ticks to **CustomBusinessHour** session flags (bitsets) |
| Feature microbatch | Rolling VWAP, imbalance, spread — feed Python or second ZMQ topic |
| Risk / OMS shim | Last line before broker; keep **deterministic** and tested |

Python keeps: Hydra config, research notebooks, DONKI ingest, event studies.

## Quick test

PUB **binds**, SUB **connects** — start Python first.

Terminal 1:

```bash
pip install -e ".[execution]"
python -c "
import time
from helios_alpha.signals import HeliosSignalV1, SignalPublisher
p = SignalPublisher('tcp://127.0.0.1:7779')
time.sleep(0.15)  # allow SUBs to connect
p.publish(HeliosSignalV1.warning(
    'helios_alpha.ssi', {'ssi': 0.62, 'band': 'warning'}, topic_suffix='ssi',
))
p.close()
"
```

Terminal 2 (Rust subscriber; see `rust/helios_signald/README.md` for build deps):

```bash
cd rust/helios_signald && cargo run --release -- tcp://127.0.0.1:7779
```

(Or run Terminal 2 **before** Terminal 1 and leave the subscriber running, then publish.)
