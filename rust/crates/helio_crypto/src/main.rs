use std::io::{self, Read};

fn main() {
    if run().is_err() {
        eprintln!("crypto replay rejected input; no order was submitted");
        std::process::exit(2);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut input = Vec::new();
    io::stdin()
        .take(64 * 1024 * 1024 + 1)
        .read_to_end(&mut input)?;
    if input.len() > 64 * 1024 * 1024 {
        return Err("input exceeds bound".into());
    }
    let request: helio_crypto::ReplayInput = serde_json::from_slice(&input)?;
    let report = helio_crypto::replay(&request)?;
    serde_json::to_writer(io::stdout(), &report)?;
    Ok(())
}
