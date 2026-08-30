# Add an agentic semantic layer without surrendering replay

Helios can use a language model to investigate a nuanced event without letting probabilistic text
generation become an unbounded trading policy. The design rule is:

> Be agentic above the decision boundary and deterministic below it.

The semantic layer may classify evidence, identify affected entities, expose competing causal paths,
request another read-only source, and report contradictions. It does not calculate authoritative
probabilities, size positions, authorize capital, or call a broker.

This page describes a target architecture. The repository currently implements the deterministic
hypothesis, statistical, replay, risk, and capital-control substrate. It does not yet implement the
semantic inference adapter or establish that one produces alpha.

## Put the language model in one bounded role

```text
timestamped source artifacts
          │
          ▼
read-only semantic inference
event type · entities · claims · citations · missing evidence
          │
          ▼
verified typed evidence
          │
          ▼
deterministic hypothesis machine
          │
          ▼
calibrated posterior + expected utility after costs
          │
          ▼
ENTER · EXIT · REDUCE · HOLD · NO ACTION
          │
          ▼
independent risk authority + capital admission
          │
          ▼
idempotent broker gateway
```

The current [`HypothesisModel`](../concepts/hypothesis-machines) boundary already supports this
shape. External inference is a typed output from one transition. Its response returns later as
`CausalEvidence`, with its own `effective_at`, `available_at`, and exact sequence. No network call is
hidden inside a state transition and no inference process receives order authority.

A semantic result should resemble a source-grounded evidence proposal:

```json
{
  "schema_version": 1,
  "event_family": "gulf_tropical_cyclone",
  "affected_regions": ["offshore_gulf", "louisiana_refining"],
  "claims": [
    {
      "claim": "forecast track intersects the production region",
      "source_artifact_id": "nhc-advisory-17",
      "status": "verified"
    }
  ],
  "contradictions": [],
  "missing_evidence": ["operator evacuation reports"],
  "proposed_next_action": "request_infrastructure_impact_model"
}
```

The adapter must reject unknown fields where the schema requires closure, invalid source identities,
unsupported claims, non-finite values, and actions outside its allowlist. A model may propose another
read-only evidence request. It may not emit `PlaceOrder`.

## Choose events where synthesis can matter

Helios should not try to beat specialized headline systems at millisecond keyword reactions. Its
candidate niche is medium-horizon causal synthesis:

- the market response can unfold over roughly minutes to days rather than microseconds;
- three to six observable stages connect the event to an economic effect;
- relevant evidence is public but fragmented across structured and textual sources;
- the event family repeats often enough to estimate and challenge its response distribution;
- one or two liquid instruments have a defensible exposure mapping; and
- abstaining is the ordinary outcome.

Promising research shapes include hurricane revisions into energy infrastructure exposure,
temperature forecasts into grid conditions, port closures into industrial supply chains,
semiconductor fabrication disruptions into sector capacity, and staged sanctions into energy or
shipping effects.

Singular attacks, surprise explosions, and other instantaneous shocks still matter to Helios. Their
first use is risk control: cancel stale intents, close capital admission, reassess open exposure, and
wait for a tradable venue. A large opening gap is not evidence of a realizable strategy return.

## Model scenarios, not one fragile conjunction

Do not treat a causal story as one long product that must reach its final node:

```text
P(A) × P(B | A) × P(C | A,B) × P(D | A,B,C)
```

Maintain competing scenarios and integrate their consequences instead:

```text
expected utility(action | evidence)
  = sum over scenarios(
      P(scenario | evidence)
      × expected payoff(action | scenario, evidence)
    )
  - execution costs
  - risk penalty
```

For a Gulf storm, production shut-in, refinery damage, rapid weakening, an already-priced physical
effect, and a broader risk-off move can all remain live branches. The policy acts on the distribution
across them. It does not ask a language model to select one persuasive story.

The [Bayesian event portfolio](../research/bayesian-event-portfolios) starts with a hurdle model:

```text
P(nonzero response | event, context)
×
effect magnitude | nonzero response, event, context
```

The language model may map source text into typed claims or suggest which calibrated model to
request. Authoritative probabilities and return distributions come from versioned statistical or
physical models with measured calibration.

## Define determinism at the system boundary

Temperature zero can improve consistency, but it is not an exact replay contract. Hosted model
weights, serving infrastructure, decoding implementations, tools, and retrieved content can change.
Even provider features intended to improve reproducibility are commonly described as best effort.
For one example, see OpenAI's
[reproducible outputs guidance](https://cookbook.openai.com/examples/reproducible_outputs_with_the_seed_parameter).

Helios should require a stronger property:

> Given the same immutable event log and recorded inference artifacts, the system emits the same
> candidate and control trace.

Persist each external inference as an immutable artifact containing at least:

| Field | Why it belongs in the replay contract |
|---|---|
| provider and model identity | distinguishes one inference implementation from another |
| prompt, schema, and policy hashes | captures every instruction that can change interpretation |
| ordered source artifact hashes | proves exactly what evidence was supplied |
| tool allowlist and complete tool trace | makes agentic investigation inspectable |
| request parameters and provider fingerprint | records reproducibility controls and backend identity when available |
| raw response hash and validated typed result | separates generated bytes from admitted evidence |
| request and response availability times | prevents a slow response from appearing earlier in replay |
| validation result and rejection reason | preserves fail-closed behavior |

An exact replay consumes the recorded typed result. It does not call the model again. Rerunning the
same sources through a changed model, prompt, schema, tool, or policy creates a new pipeline
fingerprint and a separate experiment.

This complements Helios's keyed Thompson draws. Their strategy fingerprint, decision identity, arm
identity, and sampler version derive a reproducible random stream. Randomized allocation can be
replayable even when external semantic inference is not reproducible by regeneration.

## Backtest without lending the model the answer

An agentic historical backtest has additional leakage paths beyond ordinary look-ahead bias:

- a current model may already know how a famous historical event ended;
- a web page may contain a later correction under its original URL;
- a news database may expose revised text or normalized timestamps unavailable at the decision cut;
- a tool may return today's entity graph, ranking, or market interpretation; and
- a slow historical response may be treated as if it existed at the source publication time.

Use language models primarily for source-grounded extraction in historical tests. Require claims to
cite supplied contemporaneous artifacts, retain every revision, and assign the inference response
its real `available_at`. Do not ask a current model for a free-form prediction of a historical event
whose outcome may be in its training data.

Run three different evaluations and do not combine their claims:

1. **Exact replay:** consume recorded inference artifacts and require an identical trace.
2. **Inference stability audit:** regenerate structured evidence and measure field-level disagreement,
   unsupported claims, latency, and abstention changes.
3. **Prospective shadow:** collect events after the strategy and inference contract are frozen. This is
   the strongest evidence against training-data hindsight.

The [evidence standard](../research/evidence-standard) still requires a predeclared causal cut,
controls, costs, holdout, uncertainty, and a replayable fingerprint. Agentic inference adds artifacts;
it does not lower that standard.

## Isolate untrusted content from tools and capital

News, filings, social posts, and arbitrary web pages are untrusted input. Treat their contents as
quoted data, never as instructions. The semantic process should have:

- a read-only tool allowlist;
- no broker, credential, capital, or deployment tool;
- bounded requests, bytes, tool calls, and wall time;
- source-specific freshness and identity checks;
- schema validation after generation;
- contradiction and unsupported-claim rejection; and
- a durable record of every request, response, and tool result.

A prompt injection or malformed document must be able to cause only a rejected evidence proposal,
not a new external capability.

## Introduce authority in stages

Use one inference contract through progressively stronger operating modes:

| Stage | Permitted effect | Required evidence before promotion |
|---|---|---|
| shadow observer | produce an audit-only evidence proposal | source coverage, schema validity, latency, and stability |
| evidence requester | request another approved read-only model or source | bounded tools, deadlines, and restart proof |
| entry veto | block a new candidate pending verification | false-veto rate and incident procedure |
| risk advisory | recommend hold, reduce, or exit | prospective calibration and cost-aware outcome study |
| bounded risk automation | reduce exposure within deterministic limits | paper fault matrix, reconciliation, and shadow operation |
| alpha source | propose a small predeclared event trade | out-of-sample net edge plus every capital-admission artifact |

An inference result never weakens a hard constraint. Liquidity, freshness, venue state, position,
gross exposure, daily-count, cost, kill-switch, and capital-admission checks execute after the
research proposal and independently of it.

FINRA's [algorithmic trading guidance](https://www.finra.org/rules-guidance/key-topics/algorithmic-trading)
is written for regulated member firms, not as a claim about Helios's legal status. Its emphasis on
software testing, system validation, post-deployment monitoring, alerts, and reconciliation is still
a useful operational floor for automated trading research.

## What this architecture proves

This boundary can make an agentic investigation inspectable, bounded, and exactly replayable from
recorded artifacts. It can keep probabilistic semantic interpretation outside order authority.

It does not prove that a language model understands an event, that a causal mapping predicts market
returns, that the response survives costs, or that live capital is admitted. Those remain separate
empirical and operational claims.

Next, follow one external response through the [keyed hypothesis machine](../concepts/hypothesis-machines),
build a [constrained Thompson portfolio](./build-a-thompson-portfolio), and audit
[capital admission](../operations/capital-admission).
