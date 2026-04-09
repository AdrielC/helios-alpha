//! Subscribe to `helios.signal.*` JSON envelopes from Python `SignalPublisher`.
//! Run: `helios_signald tcp://127.0.0.1:7779`
//!
//! Requires libzmq (e.g. `apt install libzmq3-dev` or brew `zeromq`).

use std::env;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let addr = env::args()
        .nth(1)
        .unwrap_or_else(|| "tcp://127.0.0.1:7779".to_string());

    let ctx = zmq::Context::new();
    let sock = ctx.socket(zmq::SUB)?;
    sock.connect(&addr)?;
    sock.set_subscribe(b"helios.signal")?;

    eprintln!("helios_signald SUB connected to {addr} (prefix helios.signal)");

    loop {
        let parts = sock.recv_multipart(0)?;
        if parts.len() < 2 {
            continue;
        }
        let topic = String::from_utf8_lossy(&parts[0]);
        let body = String::from_utf8_lossy(&parts[1]);
        match serde_json::from_str::<serde_json::Value>(&body) {
            Ok(v) => println!("{topic}\t{}", serde_json::to_string(&v)?),
            Err(e) => eprintln!("BAD_JSON topic={topic} err={e} raw={body}"),
        }
    }
}
