# helio_robinhood

Official Robinhood Crypto API adapter for the Helios broker ports.

The crate owns signed request construction, exact fixed-point limit-order mapping, bounded
reconciliation by client UUID, lifecycle normalization, cancellation, and an optional native HTTP
transport. It never loads credentials and cannot authorize capital by itself.

```bash
cargo test -p helio_robinhood --all-features
cargo clippy -p helio_robinhood --all-targets --all-features -- -D warnings
cargo check --target wasm32-wasip2 -p helio_robinhood --no-default-features
```

Read the [Robinhood operating boundary](../../../docs/operations/robinhood.md) before integrating an
account. The adapter is implemented but not broker-certified, and Robinhood's official Crypto API
does not document a paper environment.
