import json
import sqlite3
from concurrent.futures import ThreadPoolExecutor

import pytest
from pydantic import ValidationError

from helios_alpha.control.models import ClientGrant, PolicyUpdate, RiskLimits
from helios_alpha.control.store import ControlError, ControlStore


@pytest.fixture
def limits():
    # Deliberately synthetic policy. Personal deployment values are never fixtures.
    return RiskLimits(
        capital_micros=1_000_000,
        order_micros=100_000,
        daily_loss_micros=10_000,
        drawdown_micros=50_000,
        symbol_exposure_micros=500_000,
        max_daily_orders=3,
        max_spread_bps=10,
        max_quote_age_ms=1000,
        symbols=["BTC-USD"],
    )


@pytest.fixture
def store(tmp_path, limits):
    return ControlStore.initialize(
        tmp_path / "state",
        limits,
        [
            ClientGrant(client_id="observer", capabilities=["status:read", "risk:read"]),
            ClientGrant(
                client_id="operator",
                capabilities=[
                    "status:read",
                    "risk:read",
                    "audit:read",
                    "execution:pause",
                    "risk:write",
                ],
            ),
        ],
        actor="test-owner",
    )


def change(limits, request_id="change-1", expected_revision=1):
    values = limits.model_dump()
    values["order_micros"] -= 1
    return PolicyUpdate(
        expected_revision=expected_revision,
        request_id=request_id,
        limits=RiskLimits(**values),
        reason="test reduction",
    )


def test_restart_replay_and_conflict(store, limits):
    update = change(limits)
    receipt = store.update_policy("operator", update)
    reopened = ControlStore(store.root)
    assert reopened.policy("observer") == receipt
    assert reopened.update_policy("operator", update) == receipt
    assert len(reopened.audit("operator")) == 2
    with pytest.raises(ControlError, match="different contents"):
        reopened.update_policy("operator", update.model_copy(update={"reason": "changed request"}))
    assert reopened.path.stat().st_mode & 0o777 == 0o600
    assert reopened.root.stat().st_mode & 0o777 == 0o700


def test_concurrent_writers_cannot_lose_updates(store, limits):
    def submit(identifier):
        try:
            return store.update_policy("operator", change(limits, identifier))["revision"]
        except ControlError as error:
            return str(error)

    with ThreadPoolExecutor(max_workers=2) as pool:
        results = list(pool.map(submit, ["change-a", "change-b"]))
    assert results.count(2) == 1
    assert sum("revision changed" in str(result) for result in results) == 1


def test_read_only_and_revoked_clients_cannot_write(store, limits):
    with pytest.raises(ControlError, match="capability"):
        store.update_policy("observer", change(limits))
    with pytest.raises(ControlError, match="capability"):
        store.pause("unknown")
    with store.connection(write=True) as conn:
        conn.execute("DELETE FROM clients WHERE id='operator'")
    with pytest.raises(ControlError, match="capability"):
        store.update_policy("operator", change(limits))


def test_risk_change_never_unpauses_or_arms_live(store, limits):
    store.pause("operator")
    store.update_policy("operator", change(limits))
    status = ControlStore(store.root).status("operator")
    assert status["paused"]
    assert status["mode"] == "shadow"
    assert not status["live_admitted"]
    assert len(status["live_blockers"]) > 0


def test_tampered_policy_rejected(store):
    with sqlite3.connect(store.path) as conn:
        conn.execute("UPDATE policy SET limits='{}'")
    with pytest.raises(ControlError, match="integrity"):
        store.policy("observer")


def test_reinitialize_does_not_overwrite(store, limits):
    with pytest.raises(FileExistsError):
        ControlStore.initialize(store.root, limits, [], actor="test")
    assert store.policy("observer")["revision"] == 1


def test_git_tree_and_symlink_state_refused(tmp_path, limits):
    repo = tmp_path / "repo"
    repo.mkdir()
    (repo / ".git").mkdir()
    with pytest.raises(ControlError, match="outside"):
        ControlStore.initialize(repo / "state", limits, [], actor="test")
    target = tmp_path / "target"
    target.mkdir(mode=0o700)
    link = tmp_path / "link"
    link.symlink_to(target)
    with pytest.raises(ControlError, match="symlink"):
        ControlStore.initialize(link / "state", limits, [], actor="test")


@pytest.mark.parametrize(
    "field,value",
    [
        ("order_micros", -1),
        ("capital_micros", 0),
        ("order_micros", True),
        ("order_micros", 1.1),
        ("daily_loss_micros", 100_000),
        ("symbols", []),
        ("symbols", ["BTC-USD", "BTC-USD"]),
        ("symbols", ["NOT-A-PAIR"]),
        ("leverage", True),
    ],
)
def test_invalid_risk_values_fail_closed(limits, field, value):
    data = limits.model_dump()
    data[field] = value
    with pytest.raises(ValidationError):
        RiskLimits(**data)


def test_policy_json_contract(store):
    policy = store.policy("observer")
    assert RiskLimits.model_validate_json(json.dumps(policy["limits"]))
    assert policy["schema_version"] == 1
