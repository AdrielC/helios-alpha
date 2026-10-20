"""Synthetic subprocesses only: account inspection cannot silently grant trading authority."""

import asyncio
import hashlib
import json
import sys

import pytest

from helios_alpha.control.store import ControlError
from helios_alpha.crypto.account_reader import (
    CONFIG_FILE,
    NativeAccountBackend,
    read_account_availability,
    read_configured_account,
)
from helios_alpha.crypto.order_recovery import RecoveryError

ACCOUNT = "a" * 64


def write_private(path, value):
    path.write_text(json.dumps(value))
    path.chmod(0o600)


def backend(tmp_path, mutation="", *, market=False, batch=False):
    executable = tmp_path / "synthetic-account-reader"
    executable.write_text(
        f"""#!{sys.executable}
import hashlib,json,sys,time
request=json.load(sys.stdin)
assert len(sys.argv)==1
assert set(request)=={{
 "schema_version","account_number","journal_account_key","api_key","private_key_base64"
}} | ({{"markets"}} if {batch!r} else ({{"market"}} if {market!r} else set()))
assert request["journal_account_key"]=={ACCOUNT!r}
assert request["api_key"]=="synthetic-secret"
now=time.time_ns()//1_000_000
s={{
 "schema_version":1,"execution_authority":False,"account_key":{ACCOUNT!r},
 "api_account_fingerprint":"b"*64,"status":"active","api_tradable":True,
 "buying_power_usd":"100.000000000000000007",
 "reported_fee_ratio":"0.0095","reported_fee_bps":95,
 "holdings":{{"BTC":{{"total_quantity":"0.2","available_quantity":"0.000000000000000007"}}}},
 "read_started_ms":now,"read_completed_ms":now
}}
r={{"schema_version":1,"get_requests":2,"availability":{{"snapshot":s}}}}
queries=request["markets"] if {batch!r} else ([request["market"]] if {market!r} else [])
estimates=[]
for query in queries:
 m={{
  "schema_version":1,"execution_authority":False,"account_key":{ACCOUNT!r},
  "api_account_fingerprint":"b"*64,"quantity":query["quantity"],
  "rules":{{"symbol":query["symbol"],"observed_ms":now,
   "asset_increment":"0.000000000000000001","quote_increment":"0.01",
   "max_order_quantity":"10","minimum_order_micros":1000001,
   "status":"tradable","api_tradable":True}},
  "quote":{{"account_key":{ACCOUNT!r},"symbol":query["symbol"],
   "bid":"99999","ask":"100000.000000000000000007","observed_ms":now}},
  "reported_fee_bps":95,"broker_quote_observed_ms":now,
  "read_started_ms":now,"read_completed_ms":now,
 }}
 estimates.append({{"snapshot":m}})
r["get_requests"]=2+2*len(queries)
if {batch!r}:
 r["markets"]=estimates
elif {market!r}:
 r["market"]=estimates[0]
{mutation}
r["availability"]["snapshot_digest"]=hashlib.sha256(
 json.dumps(s,sort_keys=True,separators=(",",":")).encode()).hexdigest()
for entry in estimates:
 entry["snapshot_digest"]=hashlib.sha256(
  json.dumps(entry["snapshot"],sort_keys=True,separators=(",",":")).encode()).hexdigest()
print("synthetic-secret",file=sys.stderr)
print(json.dumps(r))
"""
    )
    executable.chmod(0o700)
    credentials = tmp_path / "credentials.json"
    write_private(
        credentials,
        {
            "schema_version": 1,
            "account_number": "syntheticAccount",
            "journal_account_key": ACCOUNT,
            "api_key": "synthetic-secret",
            "private_key_base64": "synthetic-private-key",
        },
    )
    return NativeAccountBackend(
        executable, hashlib.sha256(executable.read_bytes()).hexdigest(), credentials, ACCOUNT
    )


def test_private_read_preserves_exact_amounts_without_adoption_or_authority(tmp_path, capsys):
    reader = backend(tmp_path)
    result = asyncio.run(reader.read())
    assert result["state"] == "available" and result["execution_authority"] is False
    assert result["broker_requests"] == 2
    snapshot = result["availability"]["snapshot"]
    assert snapshot["buying_power_usd"] == "100.000000000000000007"
    assert snapshot["holdings"]["BTC"]["available_quantity"] == "0.000000000000000007"
    assert not {"cash_micros", "allocation", "order"} & result.keys()
    assert "synthetic" not in json.dumps(result) + capsys.readouterr().out


@pytest.mark.parametrize(
    "mutation",
    [
        's["account_key"]="c"*64',
        's["execution_authority"]=True',
        's["schema_version"]=True',
        's["read_started_ms"]-=5001',
        's["read_completed_ms"]+=100_000',
        's["read_started_ms"]=True',
        's["status"]=[]',
        's["status"]="closed"',
        's["api_tradable"]=1',
        's["reported_fee_ratio"]=None',
        's["reported_fee_bps"]=94',
        's["reported_fee_ratio"]="1.1";s["reported_fee_bps"]=11000',
        's["buying_power_usd"]="NaN"',
        's["buying_power_usd"]="0.0000000000000000001"',
        's["holdings"]["BTC"]["available_quantity"]="0.3"',
        's["holdings"]["BTC"]["extra"]="unknown"',
        's["holdings"]["INVALID-USD"]=s["holdings"].pop("BTC")',
        's["account_number"]="syntheticAccount"',
        'r["get_requests"]=9',
        'r["get_requests"]=True',
        'r["unexpected"]="wrong protocol"',
    ],
)
def test_inconsistent_unbound_or_stale_snapshots_are_unavailable(tmp_path, mutation):
    reader = backend(tmp_path, mutation)
    with pytest.raises(RecoveryError):
        asyncio.run(reader.read())


def test_missing_fees_stay_unknown_instead_of_becoming_zero(tmp_path):
    reader = backend(tmp_path, 's["reported_fee_ratio"]=None;s["reported_fee_bps"]=None')
    result = asyncio.run(reader.read())
    assert result["availability"]["snapshot"]["reported_fee_bps"] is None


def test_digest_is_verified_even_when_the_envelope_is_well_formed(tmp_path):
    reader = backend(tmp_path)
    request = reader.process._request

    async def tamper(arguments):
        response = await request(arguments)
        response["availability"]["snapshot"]["buying_power_usd"] = "1000"
        return response

    reader.process._request = tamper
    with pytest.raises(RecoveryError, match="identity"):
        asyncio.run(reader.read())


def test_missing_credentials_never_launches_the_native_process(tmp_path):
    root = tmp_path / "private"
    root.mkdir(mode=0o700)
    reader = backend(root)
    reader.process.credentials.unlink()
    write_private(
        root / "broker-journal-connection.json",
        {
            "schema_version": 1,
            "account_key": ACCOUNT,
        },
    )
    write_private(
        root / CONFIG_FILE,
        {
            "schema_version": 1,
            "executable": str(reader.process.executable),
            "sha256": hashlib.sha256(reader.process.executable.read_bytes()).hexdigest(),
            "credentials": str(reader.process.credentials),
        },
    )
    assert asyncio.run(read_configured_account(root)) == {
        "schema_version": 1,
        "state": "credentials_required",
        "execution_authority": False,
        "broker_requests": 0,
    }


def test_inspection_rechecks_role_and_does_not_return_cached_results(tmp_path, monkeypatch):
    root = tmp_path / "private"
    root.mkdir(mode=0o700)

    class Store:
        capabilities = ["status:read", "risk:read"]
        calls = 0

        def settings(self, client):
            assert client == "observer"
            self.calls += 1
            return {"client": {"capabilities": self.capabilities}}

    store = Store()
    store.root = root
    assert asyncio.run(read_account_availability(store, "observer"))["state"] == "not_configured"

    async def unavailable(_, **_options):
        raise RecoveryError("synthetic-secret that must not appear")

    monkeypatch.setattr("helios_alpha.crypto.account_reader.read_configured_account", unavailable)
    result = asyncio.run(read_account_availability(store, "observer"))
    assert result == {"schema_version": 1, "state": "unavailable", "execution_authority": False}
    store.capabilities = ["status:read"]
    with pytest.raises(ControlError, match="permissions"):
        asyncio.run(read_account_availability(store, "observer"))
    assert store.calls == 3


def test_quantity_specific_market_read_is_explicit_and_preserves_exact_prices(tmp_path):
    reader = backend(tmp_path, market=True)
    result = asyncio.run(reader.read(symbol="BTC-USD", quantity="0.000000000000000007"))
    assert result["broker_requests"] == 4
    assert result["execution_authority"] is False
    snapshot = result["market"]["snapshot"]
    assert snapshot["quantity"] == "0.000000000000000007"
    assert snapshot["quote"]["ask"] == "100000.000000000000000007"
    assert result["semantics"]["does_not_guarantee_execution_price"]


@pytest.mark.parametrize(
    "mutation",
    [
        'm["account_key"]="c"*64',
        'm["api_account_fingerprint"]="c"*64',
        'm["quantity"]="0.0002"',
        'm["execution_authority"]=True',
        'm["reported_fee_bps"]=10001',
        'm["broker_quote_observed_ms"]-=5000',
        'm["broker_quote_observed_ms"]+=1',
        'm["read_started_ms"]-=1',
        'm["read_completed_ms"]+=100000',
        'm["quote"]["observed_ms"]+=1',
        'm["quote"]["account_key"]="c"*64',
        'm["quote"]["symbol"]="ETH-USD"',
        'm["quote"]["bid"]="0"',
        'm["quote"]["ask"]="1"',
        'm["rules"]["symbol"]="ETH-USD"',
        'm["rules"]["observed_ms"]-=1',
        'm["rules"]["asset_increment"]="0"',
        'm["rules"]["asset_increment"]="1"',
        'm["rules"]["max_order_quantity"]="0.00001"',
        'm["rules"]["max_order_quantity"]="0.0001"',
        'm["rules"]["minimum_order_micros"]=True',
        'm["rules"]["status"]="unavailable"',
        'm["rules"]["api_tradable"]=False',
        'm["rules"]["account_number"]="synthetic"',
        'r["get_requests"]=2',
        'r["get_requests"]=14',
    ],
)
def test_market_identity_freshness_and_rules_must_match_the_request(tmp_path, mutation):
    reader = backend(tmp_path, mutation, market=True)
    with pytest.raises(RecoveryError):
        asyncio.run(reader.read(symbol="BTC-USD", quantity="0.0001"))


@pytest.mark.parametrize(
    ("symbol", "quantity"),
    [
        ("BTC-USD", None),
        (None, "0.1"),
        ("BTC", "0.1"),
        ("BTC-USD", "0"),
        ("BTC-USD", "NaN"),
        ("BTC-USD", "-0.1"),
    ],
)
def test_invalid_market_request_is_rejected_before_subprocess(tmp_path, symbol, quantity):
    reader = backend(tmp_path)

    async def forbidden(_):
        raise AssertionError("must not launch")

    reader.process._request = forbidden
    with pytest.raises(RecoveryError):
        asyncio.run(reader.read(symbol=symbol, quantity=quantity))


def test_batch_reads_every_holding_with_one_account_snapshot(tmp_path):
    reader = backend(tmp_path, batch=True)
    requested = [
        {"symbol": "BTC-USD", "quantity": "0.000000000000000007"},
        {"symbol": "ETH-USD", "quantity": "0.3"},
    ]
    result = asyncio.run(reader.read(markets=requested))
    assert result["broker_requests"] == 6
    assert [m["snapshot"]["quote"]["symbol"] for m in result["markets"]] == ["BTC-USD", "ETH-USD"]
    assert result["markets"][0]["snapshot"]["quantity"] == "0.000000000000000007"
    assert "market" not in result
    cash_only = asyncio.run(reader.read(markets=[]))
    assert cash_only["broker_requests"] == 2 and cash_only["markets"] == []


@pytest.mark.parametrize(
    "mutation",
    [
        'r["markets"]=r["markets"][:1]',
        'r["markets"]=r["markets"][::-1]',
        'r["markets"][1]["snapshot"]["api_account_fingerprint"]="e"*64',
        'r["markets"][1]["snapshot"]["quote"]["observed_ms"]-=5000',
        'r["markets"][1]["snapshot"]["quantity"]="0.4"',
        'r["get_requests"]=19',
        'r["get_requests"]=4',
        'r["market"]=r.pop("markets")',
    ],
)
def test_bad_second_holding_or_partial_batch_is_rejected_as_a_whole(tmp_path, mutation):
    reader = backend(tmp_path, mutation, batch=True)
    with pytest.raises(RecoveryError):
        asyncio.run(
            reader.read(
                markets=[
                    {"symbol": "BTC-USD", "quantity": "0.1"},
                    {"symbol": "ETH-USD", "quantity": "0.3"},
                ]
            )
        )


@pytest.mark.parametrize(
    "markets",
    [
        {},
        [{"symbol": "BTC-USD", "quantity": "0"}],
        [{"symbol": "BTC-USD", "quantity": "1"}, {"symbol": "BTC-USD", "quantity": "2"}],
        [{"symbol": "BTC-USD", "quantity": "1", "extra": True}],
        [{"symbol": "BTC-USD", "quantity": "1"}] * 3,
    ],
)
def test_invalid_batch_never_launches_the_process(tmp_path, markets):
    reader = backend(tmp_path, batch=True)

    async def forbidden(_):
        raise AssertionError("must not launch")

    reader.process._request = forbidden
    with pytest.raises(RecoveryError):
        asyncio.run(reader.read(markets=markets))
    with pytest.raises(RecoveryError):
        asyncio.run(reader.read(markets=[], symbol="BTC-USD", quantity="0.1"))
