# Property-based testing

Property-based tests are the default for new Helios behavior. Prefer an invariant
that holds across generated inputs and event sequences, then use targeted examples
for protocol contracts and integration boundaries. Existing tests remain useful;
this policy does not require rewriting unrelated suites.

The approach borrows from [ZIO Test's generated inputs and shrinking](https://zio.dev/reference/test/why-zio-test/)
and [replaceable test services](https://zio.dev/reference/test/services/).
Rust uses `proptest`, traits for effectful dependencies, and Tokio's paused clock.
No Scala runtime is involved.

## Test structure

1. State an observable invariant: identical committed requests never cause a second
   provider call; an altered request under the same identity never reuses an answer.
2. Generate valid requests and sequences containing duplicate identities, revisions,
   restarts, and failures. Use an independent model of expected outcomes.
3. Inject time, network responses, and storage failures. Advance virtual time instead
   of sleeping. Keep OS process and socket tests focused on their actual boundaries.
4. Let failures shrink. Commit the generated `proptest-regressions` files with the fix.
   [Failure persistence](https://proptest-rs.github.io/proptest/proptest/failure-persistence.html)
   ensures discovered cases run again.
5. Add documented fixtures where the provider's wire contract matters. A mocked
   response proves local handling, not remote access or model quality.

The first implementation lives in `rust/crates/helio_decision/tests`:

| Boundary | Evidence |
|---|---|
| Typed probabilities and choices | Generated probability ranges, Boolean/string distinction, normalization |
| Ordinal scores | Generated rubrics and probability distributions; mismatched legends and means rejected |
| Causal time | Generated availability/deadline intervals and receipt arrival times |
| Local durability | Generated sequences compared with a reference map, reopening SQLite each step |
| Stored-data integrity | Corrupted request/receipt fields rejected after reopening SQLite without another provider call |
| Failures | Injected reservation/commit failure, virtual-time timeout, rejected authentication |
| Custom providers | Service rejects an adapter's invalid normalized answer and durably replays the failure |
| Concurrency | Independent journal connections cannot reserve the same identity twice |
| Hypothesis consumer | Snapshot restored between external request and delayed evidence |
| Native I/O | Loopback HTTP fixtures; subprocess output while stdin remains open |

## Commands

```bash
cd rust
cargo test --locked -p helio_decision
PROPTEST_CASES=1024 cargo test --locked -p helio_decision --all-features
cargo clippy --locked -p helio_decision --all-targets --all-features -- -D warnings
cargo check --locked --target wasm32-wasip2 -p helio_decision --no-default-features
cargo test --locked
```

CI exercises portable and native paths. No API key is needed. The durability
sequence property uses a smaller configured case count because each generated
step opens a real SQLite database. Increase that property's configuration for a
longer fault campaign.

For a paid provider smoke check, follow [Decision models](../operations/decision-models).
Keep credentials out of fixtures and regression files. Model quality, calibration,
market returns, and production latency need separate measured evaluations.
