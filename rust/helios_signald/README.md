# helios_signald

ZMQ **SUB** daemon: receives `helios.signal.*` JSON from Python `SignalPublisher`.

## Build

**System:** `libzmq` + C++ toolchain (`libzmq3-dev`, `g++` on Debian/Ubuntu).

This crate sets `CXX=g++` in `.cargo/config.toml` so `zmq-sys` does not use `cc` (clang) without libstdc++ dev symlinks.

```bash
cargo build --release
# ./target/release/helios_signald tcp://127.0.0.1:7779
```

Start the **Python PUB first** (it `bind`s); then run this binary (it `connect`s).

## Wire format

Multipart: `[topic_utf8, json_utf8]`. Topic example: `helios.signal.ssi.warning`.
