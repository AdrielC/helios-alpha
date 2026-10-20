#!/usr/bin/env python3
"""Crash-test the exact crypto journal on a disposable Golem server. No broker requests."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shutil
import subprocess
import tempfile
import time
import uuid
from datetime import UTC, datetime, timedelta
from pathlib import Path

REPO = Path(__file__).resolve().parents[1]
ARTIFACT = REPO / "golem/golem-temp/agents/helios_crypto_broker_journal_debug.wasm"
PROOF = Path.home() / ".local/share/helios-alpha/private/broker-journal-golem-proof.json"
# Public development-server credential documented by Golem, never a user/cloud credential:
# https://learn.golem.cloud/v1.5/how-to-guides/scala/golem-call-from-external-scala
LOCAL_DEVELOPMENT_TOKEN = "5c832d93-ff85-4a8f-9803-513950fdfdb1"


def millis() -> int:
    return time.time_ns() // 1_000_000


def canonical(value: object) -> str:
    return json.dumps(value, sort_keys=True, separators=(",", ":"))


def private_write(path: Path, value: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
    with tempfile.NamedTemporaryFile(mode="w", dir=path.parent, delete=False) as handle:
        temporary = Path(handle.name)
        os.chmod(temporary, 0o600)
        json.dump(value, handle, indent=2)
        handle.write("\n")
        handle.flush()
        os.fsync(handle.fileno())
    os.replace(temporary, path)


class IsolatedGolem:
    def __init__(
        self,
        executable: str,
        root: Path,
        artifact: Path,
        *,
        app: str = "helios-journal-proof",
        component: str = "helios:crypto-broker-journal",
    ):
        self.executable = executable
        self.root = root
        self.artifact = artifact
        self.app = app
        self.component = component
        self.process: subprocess.Popen | None = None
        self.profile = ""
        self.starts = 0
        self.hard_crashes = 0
        self.log = (root / "server.log").open("ab")
        self.env = {
            key: value
            for key, value in os.environ.items()
            if not any(word in key.upper() for word in ("TOKEN", "SECRET", "API_KEY", "PASSWORD"))
        }

    def run(self, arguments: list[str], *, timeout: int = 60) -> str:
        result = subprocess.run(
            [self.executable, "--config-dir", str(self.root / "cli"), *arguments],
            capture_output=True,
            text=True,
            cwd=self.root,
            env=self.env,
            timeout=timeout,
            check=False,
        )
        if result.returncode:
            raise RuntimeError(f"Golem command failed: {result.stderr[-4000:]}")
        return result.stdout

    def start(self) -> None:
        if self.process is not None and self.process.poll() is None:
            raise RuntimeError("isolated Golem is already running")
        ports_path = self.root / "ports.json"
        ports_path.unlink(missing_ok=True)
        self.process = subprocess.Popen(
            [
                self.executable,
                "--disable-app-manifest-discovery",
                "server",
                "run",
                "--router-addr",
                "127.0.0.1",
                "--router-port",
                "0",
                "--custom-request-port",
                "0",
                "--mcp-port",
                "0",
                "--ports-file",
                str(ports_path),
                "--data-dir",
                str(self.root / "data"),
            ],
            stdout=self.log,
            stderr=self.log,
            cwd=self.root,
            env=self.env,
            start_new_session=True,
        )
        deadline = time.monotonic() + 60
        while not ports_path.exists():
            if self.process.poll() is not None:
                raise RuntimeError("isolated Golem exited before becoming ready")
            if time.monotonic() >= deadline:
                raise TimeoutError("isolated Golem did not publish its bound ports")
            time.sleep(0.1)
        ports = json.loads(ports_path.read_text())
        port = ports["routerPort"]
        if not isinstance(port, int) or not 1 <= port <= 65535:
            raise RuntimeError("invalid discovered Golem port")
        url = f"http://127.0.0.1:{port}"
        self.starts += 1
        self.profile = f"journal-proof-{self.starts}"
        self.run(
            [
                "--disable-app-manifest-discovery",
                "profile",
                "new",
                self.profile,
                "--url",
                url,
                "--static-token",
                LOCAL_DEVELOPMENT_TOKEN,
                "--default-format",
                "json",
            ]
        )
        manifest = {
            "manifestVersion": "1.5.0",
            "app": self.app,
            "environments": {
                "test": {
                    "server": {
                        "url": url,
                        "auth": {"staticToken": LOCAL_DEVELOPMENT_TOKEN},
                    },
                    "cli": {"autoConfirm": True},
                    "deployment": {"compatibilityCheck": False, "versionCheck": False},
                }
            },
            "components": {
                self.component: {
                    "dir": ".",
                    "componentWasm": str(self.artifact),
                    "outputWasm": str(self.root / "journal.wasm"),
                }
            },
        }
        # JSON is valid YAML; keep this isolated verifier free of third-party Python dependencies.
        (self.root / "golem.yaml").write_text(json.dumps(manifest, indent=2))

    def deploy(self, *, force_build: bool = False) -> None:
        self.run(
            [
                "--app-manifest-path",
                str(self.root / "golem.yaml"),
                "--environment",
                "test",
                "--yes",
                "deploy",
                *(["--force-build"] if force_build else []),
            ],
            timeout=180,
        )

    def connected(self, arguments: list[str], *, output_format: str = "json") -> str:
        return self.run(
            [
                "--disable-app-manifest-discovery",
                "--profile",
                self.profile,
                "--format",
                output_format,
                *arguments,
            ]
        )

    def invoke(
        self,
        agent: str,
        method: str,
        arguments: list[str],
        *,
        key: str | None = None,
        denied: bool = False,
    ):
        command = ["agent", "invoke", "--no-stream"]
        if key is not None:
            command.extend(["--idempotency-key", key])
        command.extend([agent, method, *[json.dumps(argument) for argument in arguments]])
        envelope = json.loads(self.connected(command))
        typed = envelope["result_json"]
        assert typed["typ"] == {
            "type": "Result",
            "ok": {"type": "Str"},
            "err": {"type": "Str"},
        }, typed
        value = typed["value"]
        if denied:
            assert set(value) == {"err"}, value
            return value["err"]
        assert set(value) == {"ok"}, value
        response = json.loads(value["ok"])
        assert response["schema_version"] == 1 and response["execution_authority"] is False
        return response["result"]

    def hard_crash(self) -> None:
        assert self.process is not None and self.process.poll() is None
        self.process.kill()
        self.process.wait(timeout=15)
        self.hard_crashes += 1

    def close(self) -> None:
        if self.process is not None and self.process.poll() is None:
            self.process.terminate()
            try:
                self.process.wait(timeout=15)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait(timeout=15)
        self.log.close()


def run_adapter_fixture(executable: Path, fixture: dict) -> dict:
    result = subprocess.run(
        [str(executable)],
        input=canonical(fixture),
        text=True,
        capture_output=True,
        check=True,
        timeout=15,
    )
    decoded = json.loads(result.stdout)
    assert decoded["schema_version"] == 1 and decoded["execution_authority"] is False
    return decoded


def adapter_preparation(executable: Path, planned: dict) -> dict:
    now = planned["created_ms"]
    # Field order is the Rust CryptoRiskLimits serialization used by Golem's policy hash.
    limits = {
        "capital_micros": 1_000_000_000,
        "order_micros": 20_000_000,
        "daily_loss_micros": 25_000_000,
        "drawdown_micros": 100_000_000,
        "symbol_exposure_micros": 500_000_000,
        "max_daily_orders": 4,
        "max_spread_bps": 50,
        "max_quote_age_ms": 10_000,
        "symbols": ["BTC-USD", "ETH-USD"],
        "leverage": False,
    }
    context = {
        "policy": {
            "schema_version": 1,
            "revision": planned["policy_revision"],
            "updated_ns": now * 1_000_000,
            "limits": limits,
            "sha256": hashlib.sha256(
                json.dumps(limits, separators=(",", ":")).encode()
            ).hexdigest(),
        },
        "account": {
            "account_key": planned["account_key"],
            "symbol": planned["symbol"],
            "snapshot_digest": "9" * 64,
            "observed_ms": now,
            "day_utc": now // 86_400_000,
            "buying_power_micros": 1_000_000_000,
            "equity_micros": 1_000_000_000,
            "day_open_equity_micros": 1_000_000_000,
            "peak_equity_micros": 1_000_000_000,
            "gross_exposure_micros": 0,
            "symbol_exposure_micros": 0,
            "available_quantity": "0",
            "daily_order_count": 0,
            "unresolved_orders": False,
            "paused": False,
            "loss_stop_latched": False,
            "fee_reserve_bps": 37,
            "settlement_quantum_micros": 10_000,
        },
        "quote": {
            "account_key": planned["account_key"],
            "symbol": planned["symbol"],
            "bid": planned["limit_price"],
            "ask": planned["limit_price"],
            "observed_ms": now,
        },
        "rules": {
            "symbol": planned["symbol"],
            "observed_ms": now,
            "asset_increment": "0.00000001",
            "quote_increment": "0.01",
            "max_order_quantity": "10",
            "minimum_order_micros": 1_000_000,
            "status": "tradable",
            "api_tradable": True,
        },
        "signal": {
            "input_digest": planned["input_digest"],
            "available_ms": now,
            "expires_ms": planned["expires_ms"],
        },
    }
    decoded = run_adapter_fixture(
        executable, {"operation": "prepare", "intent": planned, "context": context, "now_ms": now}
    )
    assert decoded["synthetic_get_requests"] == 0
    receipt = decoded["preparation"]
    assert receipt["execution_authority"] is False and receipt["intent"] == planned
    assert (
        receipt["notional_micros"] + receipt["fee_reserve_micros"]
        <= planned["reserved_cash_micros"]
    )
    assert hashlib.sha256(decoded["request_body"].encode()).hexdigest() == decoded["body_sha256"]
    body = json.loads(decoded["request_body"])
    assert body["client_order_id"] == planned["client_order_id"]
    assert body["limit_order_config"] == {
        "asset_quantity": planned["quantity"],
        "limit_price": planned["limit_price"],
        "time_in_force": "gtc",
    }
    return receipt["intent"]


def adapter_observation(executable: Path, planned: dict, report: dict) -> dict:
    """Normalize a synthetic API reply using the production Rust adapter, without networking."""

    def timestamp(ms: int) -> str:
        # Integer arithmetic preserves the exact millisecond across Python and Rust.
        return (datetime(1970, 1, 1, tzinfo=UTC) + timedelta(milliseconds=ms)).isoformat(
            timespec="milliseconds"
        )

    order = {
        "id": report["broker_order_id"],
        "account_number": "syntheticJournalApiAccount",
        "client_order_id": report["client_order_id"],
        "symbol": report["symbol"],
        "side": report["side"],
        "type": "limit",
        "state": {"Open": "open", "Canceled": "canceled", "Filled": "filled"}[report["state"]],
        "limit_order_config": {
            "asset_quantity": report["ordered_quantity"],
            "limit_price": report["limit_price"],
        },
        "fee_charged": report["fee_usd"],
        "filled_asset_quantity": report["filled_quantity"],
        "executions": [
            {
                "effective_price": fill["price"],
                "quantity": fill["quantity"],
                "timestamp": timestamp(fill["occurred_ms"]),
            }
            for fill in report["executions"]
        ],
        "created_at": timestamp(planned["created_ms"]),
        "updated_at": timestamp(report["updated_ms"]),
    }
    fixture = {
        "operation": "recover",
        "intent": planned,
        "response_body": canonical({"next": None, "results": [order]}),
        "received_ms": report["received_ms"],
    }
    decoded = run_adapter_fixture(executable, fixture)
    assert decoded["synthetic_get_requests"] == 1
    normalized = decoded["observation"]
    assert normalized is not None
    for name in ("account_key", "client_order_id", "broker_order_id", "filled_quantity", "fee_usd"):
        assert normalized[name] == report[name], name
    return normalized


def verify(server: IsolatedGolem, adapter_fixture: Path | None = None) -> dict:
    server.start()
    print("Isolated Golem ready; deploying journal.", flush=True)
    server.deploy()
    account = hashlib.sha256(f"synthetic-journal-{uuid.uuid4()}".encode()).hexdigest()
    agent = f"helios-journal-proof/test/CryptoBrokerJournalAgent({json.dumps(account)})"
    bootstrap = {
        "schema_version": 1,
        "clients": {
            "observer": ["journal:read"],
            "executor": [
                "journal:read",
                "journal:prepare",
                "journal:dispatch",
                "journal:cancel",
            ],
            "reconciler": ["journal:read", "journal:reconcile"],
        },
    }
    server.invoke(agent, "initialize", [canonical(bootstrap)])

    adapter_reads = 0

    def command(client: str, action: str, payload: dict | None = None, **options):
        nonlocal adapter_reads
        if action == "observe" and adapter_fixture is not None:
            payload = adapter_observation(adapter_fixture, planned, payload)
            adapter_reads += 1
        return server.invoke(
            agent, "command", [client, action, canonical(payload or {})], **options
        )

    def status() -> dict:
        return command("observer", "status")

    def simulate_crash() -> None:
        before = status()
        server.connected(["agent", "simulate-crash", agent])
        assert status() == before, "agent crash changed journal state"

    checks = []
    now = millis()
    client_id = str(uuid.uuid4())
    order_id = {"client_order_id": client_id}
    planned = {
        "account_key": account,
        "client_order_id": client_id,
        "symbol": "BTC-USD",
        "side": "buy",
        "quantity": "0.00012345",
        "limit_price": "100000",
        "reserved_cash_micros": 20_000_000,
        "policy_revision": 7,
        "strategy_fingerprint": "2" * 64,
        "input_digest": "3" * 64,
        "risk_reservation_id": "4" * 64,
        "created_ms": now,
        "expires_ms": now + 120_000,
    }
    if adapter_fixture is not None:
        planned = adapter_preparation(adapter_fixture, planned)
        checks.append(
            "exact unsigned request passes fee, policy and news-deadline checks before journaling"
        )
    assert command("executor", "prepare", planned)["changed"]
    before_denied = status()
    command("observer", "prepare", planned, denied=True)
    command("reconciler", "prepare", planned, denied=True)
    command("unknown-client", "status", denied=True)
    assert status() == before_denied
    checks.append("permission checks leave state unchanged")

    dispatch_key = str(uuid.uuid4())
    first = command("executor", "mark_dispatch_started", order_id, key=dispatch_key)
    assert first["changed"] and first["record"]["phase"] == "submission_uncertain"
    assert first["execution_authority"] is False
    simulate_crash()
    # A cached RPC reply repeats the *original* receipt, even when it says changed=true.
    # Therefore a marker receipt must never be treated as standalone permission to submit.
    assert command("executor", "mark_dispatch_started", order_id, key=dispatch_key) == first
    fresh = command("executor", "mark_dispatch_started", order_id)
    assert fresh["changed"] is False
    assert fresh["record"]["intent"] == planned
    assert status()["reservations"]["cash_micros"] == 20_000_000
    checks.append("lost marker reply retains identity and reservation without a second transition")
    checks.append("cached RPC receipt is distinct from a fresh journal command")
    print("Agent crash recovery and permission checks passed.", flush=True)

    now = millis()
    report = {
        "account_key": account,
        "client_order_id": client_id,
        "broker_order_id": str(uuid.uuid4()),
        "symbol": planned["symbol"],
        "side": planned["side"],
        "ordered_quantity": planned["quantity"],
        "limit_price": planned["limit_price"],
        "state": "Open",
        "filled_quantity": "0",
        "fee_usd": "0",
        "executions": [],
        "updated_ms": now,
        "received_ms": now,
    }
    command("executor", "observe", report, denied=True)
    command("reconciler", "observe", report)
    cancel = command("executor", "request_cancel", order_id)
    assert cancel["record"]["phase"] == "open"
    assert status()["reservations"]["cash_micros"] == 20_000_000

    now = millis()
    first_fill = {
        "execution_id": "synthetic-fill-a",
        "quantity": "0.000000000000000007",
        "price": "99999.123456789",
        "occurred_ms": now,
    }
    report.update(
        state="Canceled",
        filled_quantity=first_fill["quantity"],
        fee_usd="0.0000001",
        executions=[first_fill],
        updated_ms=now,
        received_ms=now,
    )
    terminal = command("reconciler", "observe", report)
    assert status()["reservations"]["cash_micros"] == 20_000_000
    assert terminal["record"]["observation"]["filled_quantity"] == first_fill["quantity"]
    coverage = {
        "account_key": account,
        "observation_digest": terminal["record"]["observation_digest"],
        "account_snapshot_digest": "5" * 64,
        "snapshot_observed_ms": millis(),
    }
    command("executor", "cover_account", {**order_id, "coverage": coverage}, denied=True)
    command("reconciler", "cover_account", {**order_id, "coverage": coverage})
    assert status()["reservations"]["cash_micros"] == 0
    checks.append("cancel request keeps reservation until exact terminal account coverage")

    before_restart = status()
    record_before_restart = command("observer", "record", order_id)
    server.hard_crash()
    server.start()
    assert status() == before_restart, "server termination changed journal state"
    assert command("observer", "record", order_id) == record_before_restart
    command("observer", "mark_dispatch_started", order_id, denied=True)
    assert command("executor", "mark_dispatch_started", order_id)["changed"] is False
    checks.append("SIGKILL and server restart preserve exact fills, coverage and permissions")
    print("Hard server restart preserved exact fills and account coverage.", flush=True)

    now = millis()
    report.update(
        state="Filled",
        filled_quantity=planned["quantity"],
        fee_usd="0.01",
        executions=[
            first_fill,
            {
                "execution_id": "synthetic-fill-b",
                "quantity": "0.000123449999999993",
                "price": "99998",
                "occurred_ms": now,
            },
        ],
        updated_ms=now,
        received_ms=now,
    )
    final_fill = command("reconciler", "observe", report)
    assert status()["reservations"]["cash_micros"] == 20_000_000
    assert final_fill["record"]["account_coverage"] is None
    before_stale = status()
    command("reconciler", "cover_account", {**order_id, "coverage": coverage}, denied=True)
    assert status() == before_stale
    simulate_crash()
    coverage.update(
        observation_digest=final_fill["record"]["observation_digest"],
        account_snapshot_digest="6" * 64,
        snapshot_observed_ms=millis(),
    )
    command("reconciler", "cover_account", {**order_id, "coverage": coverage})
    assert status()["reservations"]["cash_micros"] == 0
    checks.append("late fill invalidates old coverage across a second agent crash")

    changed_bootstrap = json.loads(canonical(bootstrap))
    changed_bootstrap["clients"]["observer"].append("journal:dispatch")
    server.invoke(agent, "initialize", [canonical(changed_bootstrap)], denied=True)
    command("observer", "mark_dispatch_started", order_id, denied=True)
    checks.append("bootstrap replay cannot escalate an existing reader")

    # Ensure the configured every(10) policy has multiple chances to create a real snapshot.
    for _ in range(10):
        status()
    oplog = server.connected(["agent", "oplog", agent], output_format="text")
    snapshot_entries = re.findall(r"(?m)^#\d+:\s+SNAPSHOT\b", oplog)
    if not snapshot_entries:
        raise AssertionError("no completed snapshot found in oplog: " + oplog[-8000:])
    simulate_crash()
    final = status()
    assert final["record_count"] == 1
    assert not final["unresolved"]
    assert command("observer", "record", order_id)["intent"] == planned
    checks.append("completed Golem snapshots present and final state survives recovery")
    if adapter_fixture is not None:
        assert adapter_reads == 4
        checks.append(
            "real Robinhood recovery adapter preserves exact synthetic fills and fees into Golem"
        )
    return {
        "checks": checks,
        "completed_snapshots": len(snapshot_entries),
        "agent_crashes": 3,
        "hard_server_crashes": server.hard_crashes,
        "server_starts": server.starts,
        "final_record_count": final["record_count"],
        "final_snapshot_digest": final["snapshot_digest"],
        "final_reserved_cash_micros": final["reservations"]["cash_micros"],
        "broker_http_requests": 0,
        "synthetic_adapter_get_requests": adapter_reads,
        "synthetic_adapter_preparations": int(adapter_fixture is not None),
        "broker_account_certified": False,
        "execution_authority": False,
        "production_deployment": False,
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--golem", default=shutil.which("golem"))
    parser.add_argument("--artifact", type=Path, default=ARTIFACT)
    parser.add_argument("--proof", type=Path, default=PROOF)
    parser.add_argument(
        "--adapter-fixture",
        type=Path,
        help="built recover_journal_fixture example; synthetic API responses, no broker HTTP",
    )
    args = parser.parse_args()
    if not args.golem:
        parser.error("golem executable is required")
    if not args.artifact.is_file():
        parser.error("build the Golem journal component before running this proof")
    if args.adapter_fixture is not None:
        args.adapter_fixture = args.adapter_fixture.resolve()
        if not args.adapter_fixture.is_file():
            parser.error("build the Rust recovery adapter fixture before running this proof")
    os.umask(0o077)
    proof = {
        "schema_version": 1,
        "scope": "isolated Golem crypto order journal with synthetic broker observations",
        "started_ms": millis(),
        "artifact_sha256": hashlib.sha256(args.artifact.read_bytes()).hexdigest(),
        "passed": False,
    }
    if args.adapter_fixture is not None:
        proof["scope"] = (
            "Exact order preparation and Robinhood recovery to isolated Golem with synthetic inputs"
        )
        proof["adapter_fixture_sha256"] = hashlib.sha256(
            args.adapter_fixture.read_bytes()
        ).hexdigest()
    with tempfile.TemporaryDirectory(prefix="helios-journal-recovery-") as directory:
        root = Path(directory)
        server = IsolatedGolem(args.golem, root, args.artifact.resolve())
        try:
            proof.update(verify(server, args.adapter_fixture))
            proof["passed"] = True
        except Exception as exc:
            proof["error"] = str(exc)
            raise
        finally:
            server.close()
            proof["finished_ms"] = millis()
            if not proof["passed"]:
                log_text = (root / "server.log").read_text(errors="replace")
                proof["server_log_tail"] = log_text[-16000:]
            private_write(args.proof, proof)
    print(json.dumps({"passed": True, "proof": str(args.proof), "checks": proof["checks"]}))


if __name__ == "__main__":
    main()
