# Decision models

`helio_decision` connects typed external inference to Helios's hypothesis boundary.
A hypothesis emits a `DecisionRequest`; the native service invokes the chosen
provider and journals a `DecisionReceipt`; its answer can return as causal evidence.
This runner has no broker or order interface.

The portable crate defines `DecisionAdapter`, requests, validated answers, and
receipt-to-evidence conversion. Its optional `native` feature supplies HTTP,
bounded concurrency, injected time, SQLite, and the `helios-decision` executable.
Implement `DecisionAdapter` and `DecisionTransport` to add another provider without
changing hypothesis kernels.

## Providers and capabilities

| Provider | Endpoint / default model | Supported boundary |
|---|---|---|
| OpenAI Decisions | `POST https://api.openai.com/v1/decisions`, `gpt-6-luna` | Predicate, typed choice, ordinal score, per-question refusal; native message/image/tool transcripts; cache options |
| TypeSafe Jev | `POST https://api.typesafe.ai/v1/systemone`, `jev-latest` | `noul`, choice, score, image assets and bounding boxes |
| Cloudflare Clef | Workers AI REST account endpoint, `clef` or `clef-flash` | `noul`, choice, score, embedded images; REST envelope handling |

Contracts were checked against the official
[OpenAI reference](https://developers.openai.com/api/reference/resources/decisions/methods/create),
[TypeSafe OpenAPI](https://api.typesafe.ai/openapi.json), and
[Cloudflare Clef documentation](https://developers.cloudflare.com/workers-ai/models/clef/).
Remote access and supported model versions still depend on each account.
`--model` permits an explicit version where the provider supports it.

Portable `state` accepts JSON; OpenAI receives a text representation. For native
OpenAI input, use `{"type":"native","protocol":"open_ai","value":[...]}`.
Native input/options are bound to their protocol and rejected by other adapters.
Options currently allow OpenAI `prompt_cache_key`, `prompt_cache_options`, and
`safety_identifier`; Jev `assets`; and Clef `images`. Jev image requests set its
required image opt-in header. Undocumented extension fields are rejected.

The common contract allows at most 400 questions and 255 distinct choices/levels.
The adapters enforce narrower provider limits: Clef has 64 questions, constrained
question IDs, at least two choices, and 2–10 score levels; Jev has 1–10 score levels.
OpenAI requires at least two choices. Boolean `true` and string `"true"` remain
distinct. System One adapters use explicit ordinal option keys when Boolean values
are present and retain each original typed value in the criterion.

Answers must match the requested IDs, primitives, options, and score indices.
Probabilities must be finite, within `[0,1]`, and distributions must sum to one
within `1e-5`. Score means must agree with their distributions within `1e-5`
times the maximum index (at least `1e-5`); Jev/Clef legends must match the submitted
rubric. Refusals remain refusals. Provider confidence is retained separately
from option probability; neither is treated as a calibrated market forecast.
Media payload details remain provider-owned; invalid media may be rejected remotely.
Clef can truncate long text, so byte limits alone do not establish context retention.
Audio, a separate Clef Omni protocol, and undocumented video payloads are not
implemented by these adapters.

## Local credentials

Create an API key in your own provider account. For OpenAI, use the
[API keys page](https://platform.openai.com/api-keys). ChatGPT and Robinhood sign-in
do not provide an OpenAI API key.

From the repository root, run this yourself in a trusted local or remote terminal:

```bash
python3 scripts/setup-decision-credentials.py --provider openai
```

The prompt hides input and atomically saves an owner-readable file at
`~/.config/helios-alpha/decision-credentials.json`, outside Git. It does not create
an account, change billing, or validate provider access. Do not paste keys in chat.
Use `--provider jev` or `--provider clef` to configure another account.

The runner also accepts `OPENAI_API_KEY`, `TYPESAFE_API_KEY`, or
`CLOUDFLARE_API_TOKEN` with `CLOUDFLARE_ACCOUNT_ID`. Environment variables override
the credential file. `--credentials PATH` chooses another private JSON file.
On Unix, group/world-readable credential files and symlinks are rejected.
The native HTTP transport uses fixed official endpoints and refuses redirects.
Network error messages exclude credentials and provider response bodies.

Build once, then perform a small, billable connectivity check:

```bash
cargo build --locked --manifest-path rust/Cargo.toml -p helio_decision --features native
rust/target/debug/helios-decision --smoke --provider openai
```

A successful receipt validates that this key/account can call Decisions and that
the returned answer passed local checks. A fixture cannot establish this.

### 1Password from a phone

Create the OpenAI key in your personal account using your phone's browser, then
save it in your personal 1Password account's **Personal** vault as an **API
Credential** named **Helios OpenAI**, with the key in its **credential** field.
[1Password sync](https://support.1password.com/sync/) makes the item available
on the Mac signed in to that account.

With the native runner built, supply your personal 1Password account's sign-in
address or account ID explicitly:

```bash
HELIOS_1PASSWORD_ACCOUNT='your-personal-account-address-or-ID' \
  scripts/run-decision-1password.sh --smoke
```

The helper resolves `op://Personal/Helios OpenAI/credential` through
[`op run`](https://www.1password.dev/cli/secrets-environment-variables), directly
into the worker's environment. It does not copy the key to the local credential
file. `HELIOS_OPENAI_SECRET_REF` can select another item/field within Personal.
The account must be your personal account; work credentials are not used.

Mobile sync does not approve the CLI on the Mac.
[Desktop integration](https://www.1password.dev/cli/app-integration) can require
a separate local authentication prompt, including after the vault locks.
The official [1Password Environments MCP server](https://www.1password.dev/environments/mcp-server)
also requires desktop approval and a local client; it is not a mobile approval
relay. This helper does not establish unattended access after a restart.

### Local browser credential entry

When Terminal interaction is unavailable, a browser on the Mac can save an OpenAI
key using the same private credential file:

```bash
python3 scripts/setup-decision-credentials.py --browser
```

Open the printed URL in a browser **on that Mac**, enter the key in its password
field, and select **Save key locally**. An already-unlocked password manager can
supply the key using its normal copy/paste controls. Any password-manager
reauthentication still applies; an agent needs explicit approval for the transfer.
Clear the clipboard after pasting. Never paste the key into a chat or tool argument.

The form binds only to `127.0.0.1`, accepts one submission, and expires after five
minutes. It checks the Host, Origin, and a random setup token; caps input sizes;
disables caching and embedding; and omits request logs and credential values from
responses. It cannot execute commands. The URL is not accessible from the phone.
Do not expose or tunnel this HTTP listener to a network.

Saving makes a persistent local copy with mode `0600`; it does not validate the API
or start inference. Run the explicit smoke check above after saving.

## Local event flow

The runner reads JSONL requests from stdin or `--input PATH`, and emits JSONL
results as calls finish. The shape is `{"Ok":{"replayed":false,"receipt":...}}`
or `{"Err":"error_code"}`. A receipt's `outcome` is also a typed `Ok`/`Err`.
Any failed delivery or outcome produces a nonzero exit status at end of input.

```bash
rust/target/debug/helios-decision \
  --provider openai \
  --input requests.jsonl \
  --journal data/decisions/journal.sqlite3 \
  --concurrency 4 --timeout-ms 10000
```

A request contains:

```json
{
  "schema_version": 1,
  "request_id": "source-revision:question-set-version",
  "effective_at_ms": 100,
  "available_at_ms": 150,
  "deadline_ms": 10150,
  "input": {"type": "state", "value": {"verified": true}},
  "questions": {
    "verified": {"type": "predicate", "instructions": "Is verified true?"}
  }
}
```

These illustrative timestamps are historical and will be rejected by a live call.
All timestamps use Unix milliseconds. An event's effective time may differ from
its availability. Unavailable or expired evidence is rejected before network I/O.

The scientific shadow bridge reads committed observations from the existing
SQLite journal, carries source metadata and quality flags, and emits deterministic
request identities. Availability is the later of publication and local observation.
Its default two-minute deadline derives from that timestamp, including on restart.
Expired observations are skipped; `--include-expired` is for offline inspection.

```bash
# Inspect requests without contacting a provider.
python3 scripts/shadow-decision-requests.py

# Keep the inference worker attached to new scientific observations.
# Requires credentials; eligible observations cause billable calls.
python3 scripts/shadow-decision-requests.py --follow |
  rust/target/debug/helios-decision --provider openai
```

Run `scripts/run-scientific-shadow.sh` separately to collect the seven scientific
feeds and serve the local operator. The bridge's sample questions perform research
triage. They do not establish a crypto trading strategy or expected returns.
Use `--questions PATH` for a versioned question map and `--after-sequence N` to
restrict input. Changing questions or TTL changes the request identity.
The polling bridge adds up to its one-second poll interval; it is not a low-latency
news wire. Faster sources should write requests directly to the service.

## Hypothesis integration

`HypothesisModel` remains pure. Emit a request as a model output, dispatch it to
`DecisionService::execute` outside the transition, and feed the receipt back using
`receipt.into_evidence(next_sequence)`. When a domain uses an evidence enum, wrap
the payload while preserving the returned sequence and both timestamps.
Availability is receipt time, never the earlier source timestamp.

The consumer owns its contiguous sequence, request correlation, duplicate handling,
and durable checkpoint. A replayed delivery is not automatically a new hypothesis
event. Process a receipt at most once per hypothesis and make consumer persistence
idempotent using the request ID. The executable contract test
`tests/hypothesis_boundary.rs` restores a hypothesis snapshot between request and
response. Existing OMS/risk checks remain separate from inference.

## Durability and latency

SQLite uses WAL and `synchronous=FULL`. A transaction reserves identity before HTTP.
The complete request and outcome are stored before delivery. Fingerprints include
canonical request content, adapter version, requested model, encoded body, and
destination. Completed identical requests replay their original receipt; reuse of
an ID with different content/configuration fails.

Normalized answers are validated at the service boundary even for custom
adapters. Replay verifies the saved request, receipt identity, source and arrival
times, question bindings, and answer invariants. Malformed stored data fails with
`Journal` and never causes another provider call. These checks detect semantic
inconsistencies; they do not make a database tamper-proof.

A crash or cancellation after reservation can leave an ambiguous pending row.
Helios refuses to resend it automatically: a remote call may already have completed
and been billed. Inspect/reconcile it before deliberately issuing a new identity.
Authentication, capacity, timeout, and invalid-response failures after reservation
are also durable outcomes. After correcting the cause, a new attempt needs a new
identity and fresh, still-valid evidence. There are no automatic provider retries
or fallback calls.

The journal supports concurrent local workers through SQLite transactions. It is
not a distributed exactly-once system or a transactional hypothesis outbox.
Back up the database with SQLite's backup facility or stop all writers before
copying its database/WAL files. Do not delete pending rows to force a retry.

The runner defaults to four concurrent calls, 2 MiB request/response limits,
100,000 journal records, and a ten-second call timeout bounded by the event
deadline. A full journal fails admission rather than evicting audit history.
`latency_ms` includes reservation, transport, and decode; delivery also waits for
the final journal commit. Connection pooling avoids rebuilding HTTP clients.
No live provider latency or trading profitability is established by local tests.

## Offline checks

```bash
rust/target/debug/helios-decision --smoke \
  --fixture-response rust/fixtures/decisions/openai-smoke-response.json \
  --journal data/decisions/fixture.sqlite3
cd rust
cargo test --locked -p helio_decision --all-features
```

Fixtures identify themselves as `fixture-only`; their transport identity cannot
replay as a live provider. See [Property-based testing](../guide/testing) for the
generated sequence, failure injection, and virtual-time checks.
