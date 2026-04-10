//! Thin wrapper: supports `replay-event-shock` as the first argv token (per demo contract).

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if args
        .first()
        .map(|s| s == "replay-event-shock")
        .unwrap_or(false)
    {
        args.remove(0);
    }
    helio_event::replay_event_shock_run_from_args(args);
}
