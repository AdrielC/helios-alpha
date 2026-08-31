#!/usr/bin/env bash
set -euo pipefail

readonly test_root="$(mktemp -d /tmp/helios-golem-integration.XXXXXX)"
readonly data_dir="$test_root/data"
readonly ports_file="$test_root/ports.json"
readonly server_log="$test_root/golem-server.log"
readonly config_dir="$test_root/config"

golem_server_pid=""

stop_server() {
  if [[ -n "$golem_server_pid" ]] && kill -0 "$golem_server_pid" 2>/dev/null; then
    kill -TERM "$golem_server_pid"
    wait "$golem_server_pid" 2>/dev/null || true
  fi
  golem_server_pid=""
}

cleanup() {
  local -r exit_code=$?
  trap - EXIT
  stop_server
  if (( exit_code != 0 )) && [[ -f "$server_log" ]]; then
    echo "Golem server tail after smoke-test failure:" >&2
    tail -200 "$server_log" >&2
  fi
  case "$test_root" in
    /tmp/helios-golem-integration.*) rm -rf -- "$test_root" ;;
    *) echo "Refusing to remove unexpected test path: $test_root" >&2 ;;
  esac
  exit "$exit_code"
}
trap cleanup EXIT

start_server() {
  rm -f -- "$ports_file"
  # The built-in local profile uses 9881 for management. The per-test data directory keeps this
  # proof isolated from developer state; port collisions fail the test instead of sharing a server.
  golem server run \
    --router-addr 127.0.0.1 \
    --router-port 9881 \
    --custom-request-port 0 \
    --mcp-port 0 \
    --ports-file "$ports_file" \
    --data-dir "$data_dir" \
    >>"$server_log" 2>&1 &
  golem_server_pid=$!

  for _ in $(seq 1 240); do
    if [[ -s "$ports_file" ]]; then
      break
    fi
    if ! kill -0 "$golem_server_pid" 2>/dev/null; then
      echo "Golem server exited before writing its ports file" >&2
      return 1
    fi
    sleep 0.25
  done
  if [[ ! -s "$ports_file" ]]; then
    echo "Timed out waiting for Golem server readiness" >&2
    return 1
  fi

  local -r router_port="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["routerPort"])' "$ports_file")"
  if [[ "$router_port" != "9881" ]]; then
    echo "Golem smoke server reported unexpected router port: $router_port" >&2
    return 1
  fi
}

assert_contains() {
  local -r output=$1
  local -r expected=$2
  if [[ "$output" != *"$expected"* ]]; then
    echo "Expected Golem output to contain: $expected" >&2
    echo "$output" >&2
    return 1
  fi
}

assert_equal() {
  local -r actual=$1
  local -r expected=$2
  if [[ "$actual" != "$expected" ]]; then
    echo "Expected repeated idempotent invocation to return the original result" >&2
    diff <(printf '%s\n' "$expected") <(printf '%s\n' "$actual") >&2 || true
    return 1
  fi
}

readonly agent_id='HypothesisShardAgent("ci-proof-v1", "normalized-events", 0, 17, 40)'
readonly open_key='v1/11:ci-proof-v1/17:normalized-events/0/17/40-40'
readonly likelihood_key='v1/11:ci-proof-v1/17:normalized-events/0/17/41-41'
readonly open_batch='SourceBatch { records: [SourceRecord { offset: 40, mutation: SourceMutation::Open(Open { key: "solar-flare", evidence: EvidenceEnvelope { sequence: 0, effective_at: 9, available_at: 10, payload: EventShockEvidence::Trigger(Trigger { prior_ppm: 1000, deadline_available_at: 100 }) } }) }] }'
readonly likelihood_batch='SourceBatch { records: [SourceRecord { offset: 41, mutation: SourceMutation::Evidence(Evidence { key: "solar-flare", evidence: EvidenceEnvelope { sequence: 1, effective_at: 19, available_at: 20, payload: EventShockEvidence::LikelihoodAssessment(LikelihoodAssessment { observation_positive: true, sensitivity_ppm: 950000, false_positive_ppm: 1000, deadline_available_at: 200 }) } }) }] }'
readonly oms_agent_id='OmsAccountAgent("ci-paper-account")'
readonly oms_submit_key='oms-ci-paper-account-submit-1'
readonly oms_submit='SubmitOrderInput { command_id: "oms-submit-1", intent: OrderIntentInput { client_order_id: "oms-order-1", proposal_id: "proposal-1", strategy_id: "strategy-1", symbol: "SPY", venue: "XNAS", currency: "USD", side: SideInput::Buy, quantity_micros: 2000000, limit_price_micros: 50000000, execution_mode: ExecutionModeInput::Paper, trading_day: 20260830, authorized_notional_micros: 100000000, risk_policy_version: "risk-v1", authorized_at_ns: 1 }, time_in_force: TimeInForceInput::Day, at_ns: 10 }'
readonly oms_ack='VenueAcknowledgementInput { command_id: "oms-ack-1", client_order_id: "oms-order-1", broker_order_id: "oms-venue-order-1", at_ns: 11 }'
readonly oms_fill='FillInput { command_id: "oms-fill-1", client_order_id: "oms-order-1", broker_order_id: Some("oms-venue-order-1"), execution_id: "oms-execution-1", venue_occurred_at: Some("20260830-15:42:00.000"), quantity_micros: 500000, price_micros: 49500000, at_ns: 12 }'
readonly oms_order_id='"oms-order-1"'
readonly risk_agent_id='RiskAccountAgent("ci-paper-account")'
readonly risk_policy='{"version":"paper-risk-v1","live_enabled":false,"allowed_venues":["XNYS"],"max_market_data_age_ns":1000,"max_portfolio_age_ns":1000,"max_order_notional":100000000,"max_gross_exposure":1000000000,"max_strategy_exposure":500000000,"max_symbol_position_micros":10000000,"max_daily_orders":10}'
readonly risk_policy_literal="$(python3 -c 'import json,sys; print(json.dumps(sys.argv[1]))' "$risk_policy")"
readonly risk_schedule_literal="$(python3 -c 'import json,sys; print(json.dumps(open(sys.argv[1]).read()))' '../rust/crates/helio_time/tests/fixtures/xnys_2026_thanksgiving.json')"
readonly risk_portfolio='PortfolioRiskInput { as_of_ns: 1795617999999999900, trading_day: 20782, gross_exposure_micros: 0, strategy_exposure: [], symbol_positions: [], daily_order_count: 0 }'
readonly risk_config="ConfigureRiskInput { risk_policy_json: $risk_policy_literal, venue_schedule_json: $risk_schedule_literal, initial_portfolio: $risk_portfolio }"
readonly risk_authorize="AuthorizeRiskInput { proposal: RiskProposalInput { proposal_id: \"risk-order-1\", strategy_id: \"manual\", symbol: \"SPY\", venue: \"XNYS\", currency: \"USD\", side: SideInput::Buy, quantity_micros: 1000000, limit_price_micros: 25000000, execution_mode: ExecutionModeInput::Paper, trading_day: 20782 }, context: RiskContextInput { now_ns: 1795618000000000000, market_data_at_ns: 1795617999999999900, venue_time_utc_sec: 1795618000 }, portfolio: $risk_portfolio }"
readonly projection_cursor_agent_id='ProjectionCursorAgent("ci-paper-account", "nats-oms-events")'
readonly projection_cursor_advance='AdvanceProjectionCursorInput { expected_cursor: 0, next_cursor: 1, event_id: "oms:v1:ci-paper-account:oms-order-1:1" }'

start_server

golem --environment test --config-dir "$config_dir" --yes deploy

risk_configured="$(golem --environment test --config-dir "$config_dir" --format text agent invoke --no-stream "$risk_agent_id" configure "$risk_config")"
assert_contains "$risk_configured" 'policy_version: "paper-risk-v1"'
risk_first_decision="$(golem --environment test --config-dir "$config_dir" --format text agent invoke --no-stream --idempotency-key risk-ci-paper-authorize-1 "$risk_agent_id" authorize "$risk_authorize")"
assert_contains "$risk_first_decision" 'RiskDecisionOutput::Approved'
risk_duplicate_decision="$(golem --environment test --config-dir "$config_dir" --format text agent invoke --no-stream --idempotency-key risk-ci-paper-authorize-1 "$risk_agent_id" authorize "$risk_authorize")"
assert_equal "$risk_duplicate_decision" "$risk_first_decision"

cursor_first_receipt="$(golem --environment test --config-dir "$config_dir" --format text agent invoke --no-stream "$projection_cursor_agent_id" advance "$projection_cursor_advance")"
assert_contains "$cursor_first_receipt" 'cursor: 1'
assert_contains "$cursor_first_receipt" 'replayed: false'
cursor_replay_receipt="$(golem --environment test --config-dir "$config_dir" --format text agent invoke --no-stream "$projection_cursor_agent_id" advance "$projection_cursor_advance")"
assert_contains "$cursor_replay_receipt" 'replayed: true'

oms_first_receipt="$(golem --environment test --config-dir "$config_dir" --format text agent invoke --no-stream --idempotency-key "$oms_submit_key" "$oms_agent_id" submit "$oms_submit")"
assert_contains "$oms_first_receipt" 'version: 1'
assert_contains "$oms_first_receipt" 'event_count: 1'

oms_duplicate_receipt="$(golem --environment test --config-dir "$config_dir" --format text agent invoke --no-stream --idempotency-key "$oms_submit_key" "$oms_agent_id" submit "$oms_submit")"
assert_equal "$oms_duplicate_receipt" "$oms_first_receipt"

golem --environment test --config-dir "$config_dir" --format text agent invoke --no-stream "$oms_agent_id" acknowledge "$oms_ack" >/dev/null
golem --environment test --config-dir "$config_dir" --format text agent invoke --no-stream "$oms_agent_id" record_fill "$oms_fill" >/dev/null

oms_before_crash="$(golem --environment test --config-dir "$config_dir" --format text agent invoke --no-stream "$oms_agent_id" order "$oms_order_id")"
assert_contains "$oms_before_crash" 'state: OrderStateOutput::PartiallyFilled'
assert_contains "$oms_before_crash" 'filled_notional_micros: 24750000'

first_receipt="$(golem --environment test --config-dir "$config_dir" --format text agent invoke --no-stream --idempotency-key "$open_key" "$agent_id" process_batch "$open_batch")"
assert_contains "$first_receipt" 'next_offset: 41'
assert_contains "$first_receipt" 'RequestLikelihoodAssessment'

duplicate_receipt="$(golem --environment test --config-dir "$config_dir" --format text agent invoke --no-stream --idempotency-key "$open_key" "$agent_id" process_batch "$open_batch")"
assert_equal "$duplicate_receipt" "$first_receipt"

golem --environment test --config-dir "$config_dir" agent simulate-crash "$agent_id"
golem --environment test --config-dir "$config_dir" agent simulate-crash "$oms_agent_id"
golem --environment test --config-dir "$config_dir" agent simulate-crash "$risk_agent_id"
golem --environment test --config-dir "$config_dir" agent simulate-crash "$projection_cursor_agent_id"
after_crash="$(golem --environment test --config-dir "$config_dir" --format text agent invoke --no-stream "$agent_id" status)"
assert_contains "$after_crash" 'next_offset: 41'
assert_contains "$after_crash" 'next_deadline_available_at: Some(100)'

oms_after_crash="$(golem --environment test --config-dir "$config_dir" --format text agent invoke --no-stream "$oms_agent_id" order "$oms_order_id")"
assert_contains "$oms_after_crash" 'state: OrderStateOutput::PartiallyFilled'
risk_after_crash="$(golem --environment test --config-dir "$config_dir" --format text agent invoke --no-stream "$risk_agent_id" status)"
assert_contains "$risk_after_crash" 'outstanding_reservations: 1'
assert_contains "$risk_after_crash" 'reserved_gross_micros: 25000000'
cursor_after_crash="$(golem --environment test --config-dir "$config_dir" --format text agent invoke --no-stream "$projection_cursor_agent_id" status)"
assert_contains "$cursor_after_crash" 'cursor: 1'

stop_server
start_server

after_restart="$(golem --environment test --config-dir "$config_dir" --format text agent invoke --no-stream "$agent_id" status)"
assert_contains "$after_restart" 'next_offset: 41'
assert_contains "$after_restart" 'active_hypotheses: 1'

oms_after_restart="$(golem --environment test --config-dir "$config_dir" --format text agent invoke --no-stream "$oms_agent_id" order "$oms_order_id")"
assert_contains "$oms_after_restart" 'state: OrderStateOutput::PartiallyFilled'
oms_events="$(golem --environment test --config-dir "$config_dir" --format text agent invoke --no-stream "$oms_agent_id" events_after 0 16)"
assert_contains "$oms_events" 'next_cursor: 3'
risk_after_restart="$(golem --environment test --config-dir "$config_dir" --format text agent invoke --no-stream "$risk_agent_id" status)"
assert_contains "$risk_after_restart" 'outstanding_reservations: 1'
assert_contains "$risk_after_restart" 'reserved_gross_micros: 25000000'
cursor_after_restart="$(golem --environment test --config-dir "$config_dir" --format text agent invoke --no-stream "$projection_cursor_agent_id" status)"
assert_contains "$cursor_after_restart" 'cursor: 1'
assert_contains "$cursor_after_restart" 'last_event_id: Some("oms:v1:ci-paper-account:oms-order-1:1")'

replayed_receipt="$(golem --environment test --config-dir "$config_dir" --format text agent invoke --no-stream --idempotency-key "$open_key" "$agent_id" process_batch "$open_batch")"
assert_equal "$replayed_receipt" "$first_receipt"

likelihood_receipt="$(golem --environment test --config-dir "$config_dir" --format text agent invoke --no-stream --idempotency-key "$likelihood_key" "$agent_id" process_batch "$likelihood_batch")"
assert_contains "$likelihood_receipt" 'next_offset: 42'
assert_contains "$likelihood_receipt" 'posterior_ppm: 487429'

final_status="$(golem --environment test --config-dir "$config_dir" --format text agent invoke --no-stream "$agent_id" status)"
assert_contains "$final_status" 'next_offset: 42'
assert_contains "$final_status" 'next_deadline_available_at: Some(200)'

echo "Golem durability smoke passed: hypothesis, OMS, risk, and projection cursor duplicate suppression, reservations, simulated crash, full server restart, event cursor, and contiguous resume."
