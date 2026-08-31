# Run the scientific shadow

The shadow service turns current NOAA SWPC and NASA CCMC documents into an append-only,
point-in-time journal. It has no order authority. Its output is a generic time-series projection
that `helio-operatord` can load and refresh without teaching the Rust stream kernel about space
weather.

## Source contracts

| Source identity | Current product | Projected series |
|---|---|---|
| `noaa-swpc-goes-xray-primary-v1` | primary GOES 0.1–0.8 nm X-ray flux | `goes-xray-flux` |
| `noaa-swpc-goes-protons-primary-v1` | primary GOES integral proton flux at or above 10 MeV | `goes-proton-flux-ge10` |
| `noaa-swpc-planetary-kp-1m-v1` | one-minute planetary K index | `planetary-kp` |
| `noaa-swpc-l1-mag-1m-v1` | active real-time L1 magnetic source | `l1-imf-bz-gsm` |
| `noaa-swpc-l1-wind-1m-v1` | active real-time L1 solar-wind source | `l1-solar-wind-speed` |
| `nasa-ccmc-donki-cme-v1` | CME analyses and latest WSA-ENLIL result | `donki-cme-analysis` |
| `nasa-ccmc-donki-flare-v1` | flare records and revisions | `donki-flare-events` |

The L1 provider is deliberately not named DSCOVR in the series identity. NOAA can switch the
active upstream spacecraft. Every observation retains the provider reported by the document.

NOAA rolling JSON does not expose a trustworthy per-row publication timestamp. The adapter uses
the local HTTPS receipt time as `availableAt`, never the measurement time. DONKI uses the latest
applicable submission or model-completion time and still requires that it not exceed receipt time.

## Commit path

```text
HTTPS document
  -> strict source normalizer
  -> finite canonical payload + provenance
  -> SQLite BEGIN IMMEDIATE
       raw snapshot
       zero or more append-only revisions
       source checkpoint
     COMMIT
  -> atomic mode-0600 operator projection
  -> helio-operatord validates the full projection
  -> one in-memory read-model commit
```

SQLite runs in WAL mode with `synchronous=FULL`. Snapshot storage, observation revisions, source
offsets, and checkpoint advancement commit in one transaction. An unchanged poll stores new poll
evidence and advances the checkpoint while emitting no duplicate observations. A changed natural
key receives the next revision and source offset.

## Run it

```bash
uv sync --extra dev

helios-shadow \
  --journal data/shadow/space-weather.sqlite3 \
  --operator-projection data/shadow/operator-projection.json \
  --interval-seconds 60
```

To certify one source before running the complete set:

```bash
helios-shadow \
  --feed noaa-swpc-goes-xray-primary-v1 \
  --journal data/shadow/space-weather.sqlite3 \
  --operator-projection data/shadow/operator-projection.json
```

Start the operator with the same projection path:

```bash
export HELIOS_TIME_SERIES_PROJECTION_PATH=data/shadow/operator-projection.json
export HELIOS_TIME_SERIES_PROJECTION_POLL_MS=1000
cd rust
cargo run -p helio_operatord
```

Configuration is fail-closed. If a projection path is supplied but cannot be read or validated,
the operator does not start. Later malformed, unknown, stale, non-finite, causally impossible, or
oversized updates are rejected without changing the committed read model. The last valid
projection remains visible.

## Forecast contract

`config/forecasts/space-weather-impact-v1.json` is the checked-in observation contract. It names
the exact ordered series, source identities, required inputs, and freshness budget used by the
forecast. The Python loader and Rust operator compute the SHA-256 of the exact manifest bytes. The
browser validates the version, fingerprint shape, ordered input contract, source lists, and shared
series before presenting it.

Changing a role, source identity, freshness limit, or series order creates a new definition hash.
Change `bundleVersion` when the research meaning changes.

## Evidence and limits

Unit tests prove causal availability, atomic checkpoints, append-only revisions, finite JSON,
projection permissions, latest-revision selection, manifest drift rejection, and atomic Rust
projection replacement. The non-blocking integration job polls the current NOAA X-ray endpoint.
The capital-control job stores the focused shadow proof log as a content-addressed artifact.

This is a single-host shadow path, not distributed source durability. Before this source can admit
capital, place the journal on a monitored persistent volume, back it up and restore it in a drill,
publish committed source observations through an acknowledged durable transport, run the complete
shadow period, and prove required-source freshness gates against real outages and revisions.

Operational references: [NOAA SWPC JSON services](https://services.swpc.noaa.gov/json/),
[NOAA real-time solar wind](https://www.swpc.noaa.gov/products/real-time-solar-wind), and
[NASA CCMC DONKI](https://ccmc.gsfc.nasa.gov/tools/DONKI/).
