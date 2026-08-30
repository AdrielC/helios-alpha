//! OMS command and fill-accounting throughput without transport or persistence latency.

use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};
use helio_execution::{
    ExecutionMode, MoneyMicros, OrderIntent, OrderProposal, PriceMicros, QuantityMicros, Side,
};
use helio_oms::{OmsCommand, OmsCommandPort, ReferenceOms, TimeInForce};

const ORDERS: u64 = 10_000;
const FILLS: u64 = 4_096;

fn intent(index: u64, quantity_micros: u64) -> OrderIntent {
    OrderIntent {
        client_order_id: format!("order-{index}"),
        proposal: OrderProposal {
            proposal_id: format!("proposal-{index}"),
            strategy_id: "benchmark".into(),
            symbol: "SPY".into(),
            venue: "XNAS".into(),
            currency: "USD".into(),
            side: Side::Buy,
            quantity: QuantityMicros(quantity_micros),
            limit_price: PriceMicros(50_000_000),
            mode: ExecutionMode::Paper,
            trading_day: 20260830,
        },
        authorized_notional: MoneyMicros(u64::try_from(u128::from(quantity_micros) * 50).unwrap()),
        risk_policy_version: "risk-v1".into(),
        authorized_at_ns: 1,
    }
}

fn submit_and_acknowledge(c: &mut Criterion) {
    let mut group = c.benchmark_group("oms_submit_ack");
    group.throughput(Throughput::Elements(ORDERS * 2));
    group.bench_function("10000_orders", |b| {
        b.iter(|| {
            let mut oms = ReferenceOms::try_new("benchmark-account").unwrap();
            for index in 0..ORDERS {
                let order_id = format!("order-{index}");
                oms.execute(OmsCommand::Submit {
                    command_id: format!("submit-{index}"),
                    intent: intent(index, 1_000_000),
                    time_in_force: TimeInForce::Day,
                    at_ns: index * 2,
                })
                .unwrap();
                oms.execute(OmsCommand::Acknowledge {
                    command_id: format!("ack-{index}"),
                    client_order_id: order_id,
                    broker_order_id: format!("venue-{index}"),
                    at_ns: index * 2 + 1,
                })
                .unwrap();
            }
            black_box(oms.next_cursor())
        });
    });
    group.finish();
}

fn exact_fill_accounting(c: &mut Criterion) {
    let mut group = c.benchmark_group("oms_fill_accounting");
    group.throughput(Throughput::Elements(FILLS));
    group.bench_function("4096_fills_one_order", |b| {
        b.iter(|| {
            let mut oms = ReferenceOms::try_new("benchmark-account").unwrap();
            oms.execute(OmsCommand::Submit {
                command_id: "submit".into(),
                intent: intent(0, FILLS * 1_000_000),
                time_in_force: TimeInForce::Day,
                at_ns: 1,
            })
            .unwrap();
            oms.execute(OmsCommand::Acknowledge {
                command_id: "ack".into(),
                client_order_id: "order-0".into(),
                broker_order_id: "venue-0".into(),
                at_ns: 2,
            })
            .unwrap();
            for index in 0..FILLS {
                oms.execute(OmsCommand::RecordFill {
                    command_id: format!("fill-command-{index}"),
                    client_order_id: "order-0".into(),
                    broker_order_id: Some("venue-0".into()),
                    execution_id: format!("execution-{index}"),
                    venue_occurred_at: None,
                    quantity: QuantityMicros(1_000_000),
                    price: PriceMicros(50_000_000 + index),
                    at_ns: index + 3,
                })
                .unwrap();
            }
            black_box(oms.next_cursor())
        });
    });
    group.finish();
}

criterion_group!(benches, submit_and_acknowledge, exact_fill_accounting);
criterion_main!(benches);
