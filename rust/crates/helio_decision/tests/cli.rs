#![cfg(feature = "native")]
mod common;
use common::*;
use helio_decision::*;
use std::{
    io::{BufRead, BufReader, Write},
    process::{Child, Command, Stdio},
    sync::mpsc,
    time::Duration,
};

struct ChildGuard(Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn open_idle_input_does_not_delay_delivery_and_restart_replays() {
    let dir = tempfile::tempdir().unwrap();
    let fixture = dir.path().join("response.json");
    std::fs::write(
        &fixture,
        predicate_response(Protocol::OpenAi, 0.8).to_string(),
    )
    .unwrap();
    let journal = dir.path().join("j.db");
    let mut r = request();
    let now = SystemClock.now_ms();
    r.available_at_ms = now;
    r.deadline_ms = now + 60_000;
    let bytes = format!("{}\n", serde_json::to_string(&r).unwrap());
    for replayed in [false, true] {
        let mut child = ChildGuard(
            Command::new(env!("CARGO_BIN_EXE_helios-decision"))
                .args([
                    "--fixture-response",
                    fixture.to_str().unwrap(),
                    "--journal",
                    journal.to_str().unwrap(),
                ])
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap(),
        );
        let mut stdin = child.0.stdin.take().unwrap();
        let stdout = child.0.stdout.take().unwrap();
        let (tx, rx) = mpsc::channel();
        let reader = std::thread::spawn(move || {
            let mut output = BufReader::new(stdout);
            let mut line = String::new();
            output.read_line(&mut line).unwrap();
            tx.send(line).unwrap();
        });
        stdin.write_all(bytes.as_bytes()).unwrap();
        stdin.flush().unwrap();
        // Leave stdin open. This is a process/pipe boundary test; virtual Tokio time
        // inside a different process cannot control the OS scheduling deadline.
        let line = rx
            .recv_timeout(Duration::from_secs(5))
            .expect("result must arrive without another event or EOF");
        let result: Result<DecisionDelivery, DecisionError> = serde_json::from_str(&line).unwrap();
        assert_eq!(result.unwrap().replayed, replayed);
        drop(stdin);
        assert!(child.0.wait().unwrap().success());
        reader.join().unwrap();
    }
}
