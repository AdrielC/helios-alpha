"""Roundtrip through the real MCP STDIO protocol, not a direct function stub."""

import asyncio
import json
import os
import sys
from pathlib import Path

import pytest

pytest.importorskip("mcp")
from mcp import ClientSession, StdioServerParameters
from mcp.client.stdio import stdio_client

from helios_alpha.control.models import ClientGrant, RiskLimits
from helios_alpha.control.store import ControlStore, canonical
from helios_alpha.crypto.snapshots import MAX_BYTES, DecisionSnapshots
from helios_alpha.crypto.strategy import PrivateStrategy


def test_mcp_caps_and_durable_pause(tmp_path):
    root = tmp_path / "private"
    ControlStore.initialize(
        root,
        RiskLimits(
            capital_micros=100,
            order_micros=10,
            daily_loss_micros=5,
            drawdown_micros=10,
            symbol_exposure_micros=50,
            max_daily_orders=2,
            max_spread_bps=10,
            max_quote_age_ms=1000,
            symbols=["BTC-USD"],
        ),
        [
            ClientGrant(client_id="observer", capabilities=["status:read", "risk:read"]),
            ClientGrant(client_id="status-only", capabilities=["status:read"]),
            ClientGrant(
                client_id="operator",
                capabilities=[
                    "status:read",
                    "risk:read",
                    "execution:pause",
                ],
            ),
        ],
        actor="test",
    )

    async def check(client):
        params = StdioServerParameters(
            command=sys.executable,
            args=[
                "-m",
                "helios_alpha.control.mcp_server",
                "--test-sqlite",
                "--root",
                str(root),
                "--client",
                client,
            ],
            env={"PYTHONPATH": str(Path(__file__).parents[1] / "src"), "PATH": os.environ["PATH"]},
        )
        async with stdio_client(params) as streams, ClientSession(*streams) as session:
            await session.initialize()
            tools = {tool.name for tool in (await session.list_tools()).tools}
            assert "update_risk_policy" not in tools
            assert "list_decision_snapshots" in tools
            assert "get_event_radar" in tools
            radar = await session.call_tool("get_event_radar", {})
            assert not radar.isError
            radar = radar.structuredContent or json.loads(radar.content[0].text)
            assert radar == {"schema_version": 1, "online": False, "execution_authority": False}
            resource = await session.read_resource("helios://events/v1")
            assert json.loads(resource.contents[0].text) == radar
            assert "get_news_study" in tools
            study = await session.call_tool("get_news_study", {})
            assert not study.isError
            study = study.structuredContent or json.loads(study.content[0].text)
            assert study["state"] == "not_recording"
            assert study["not_strategy_pnl"]
            assert not study["execution_authority"]
            assert not study["profitability_established"]
            assert "replay_decision" not in tools
            snapshots = await session.call_tool("list_decision_snapshots", {"limit": 1})
            assert not snapshots.isError
            snapshots = snapshots.structuredContent or json.loads(snapshots.content[0].text)
            assert snapshots["state"] == "not_recording"
            if client == "status-only":
                assert "get_decision_input" not in tools
                assert "get_broker_observation" not in tools
                assert "get_broker_journal" not in tools
                assert "get_broker_order_record" not in tools
                assert "get_allocation_balance" not in tools
                assert "get_allocation_risk" not in tools
                assert "get_broker_account_availability" not in tools
                assert "get_broker_market" not in tools
                denied = await session.call_tool("get_broker_observation", {})
                assert denied.isError
                resources = (await session.list_resources()).resources
                assert "helios://broker-observation/v1" not in {str(r.uri) for r in resources}
                denied = await session.call_tool("get_decision_input", {"snapshot_id": "a" * 64})
                assert denied.isError
                return
            assert "get_decision_input" in tools
            assert "get_allocation_risk" in tools
            allocation_risk = await session.call_tool("get_allocation_risk", {})
            assert not allocation_risk.isError
            allocation_risk = allocation_risk.structuredContent or json.loads(
                allocation_risk.content[0].text
            )
            assert allocation_risk == {"state": "not_configured", "execution_authority": False}
            assert "get_risk_policy" in tools
            assert "get_settings" in tools
            assert "get_news_evidence" in tools
            assert "get_broker_observation" in tools
            assert "get_broker_journal" in tools
            assert "get_broker_order_record" in tools
            assert "get_allocation_balance" in tools
            assert "get_broker_account_availability" in tools
            assert "get_broker_market" in tools
            assert not {"prepare_broker_order", "submit_broker_order", "cover_account"} & tools
            journal = await session.call_tool("get_broker_journal", {})
            assert not journal.isError
            journal = journal.structuredContent or json.loads(journal.content[0].text)
            assert journal == {"state": "not_configured", "execution_authority": False}
            allocation = await session.call_tool("get_allocation_balance", {})
            assert not allocation.isError
            allocation = allocation.structuredContent or json.loads(allocation.content[0].text)
            assert allocation == journal
            availability = await session.call_tool("get_broker_account_availability", {})
            assert not availability.isError
            availability = availability.structuredContent or json.loads(
                availability.content[0].text
            )
            assert availability["state"] == "not_configured"
            assert availability["execution_authority"] is False
            assert availability["broker_requests"] == 0
            market = await session.call_tool(
                "get_broker_market", {"symbol": "BTC-USD", "quantity": "0.0001"}
            )
            assert not market.isError
            market = market.structuredContent or json.loads(market.content[0].text)
            assert market["state"] == "not_configured"
            assert market["execution_authority"] is False
            assert market["broker_requests"] == 0
            assert "import_broker_observation" not in tools
            broker = await session.call_tool("get_broker_observation", {})
            assert not broker.isError
            broker = broker.structuredContent or json.loads(broker.content[0].text)
            assert broker == {"state": "unavailable", "execution_authority": False}
            evidence = await session.call_tool("get_news_evidence", {"limit": 1})
            assert not evidence.isError
            evidence = evidence.structuredContent or json.loads(evidence.content[0].text)
            assert not evidence["execution_authority"]
            assert evidence["evidence"]["state"] == "not_recording"
            settings = await session.call_tool("get_settings", {})
            assert not settings.isError
            settings = settings.structuredContent or json.loads(settings.content[0].text)
            assert settings["client"]["id"] == client
            assert settings["money_scale"] == 1_000_000
            assert not settings["execution"]["live_admitted"]
            resources = (await session.list_resources()).resources
            assert "helios://settings/v1" in {str(r.uri) for r in resources}
            assert "helios://news-evidence/v1" in {str(r.uri) for r in resources}
            assert "helios://broker-observation/v1" in {str(r.uri) for r in resources}
            assert "helios://broker-journal/v1" in {str(r.uri) for r in resources}
            assert "helios://allocation/v1" in {str(r.uri) for r in resources}
            allocation_resource = await session.read_resource("helios://allocation/v1")
            assert json.loads(allocation_resource.contents[0].text) == allocation
            journal_resource = await session.read_resource("helios://broker-journal/v1")
            assert json.loads(journal_resource.contents[0].text) == journal
            broker_resource = await session.read_resource("helios://broker-observation/v1")
            assert json.loads(broker_resource.contents[0].text) == broker
            resource = await session.read_resource("helios://settings/v1")
            assert json.loads(resource.contents[0].text) == settings
            result = await session.call_tool("get_risk_policy", {})
            assert not result.isError
            if client == "observer":
                denied = await session.call_tool("pause_trading", {})
                assert denied.isError
            else:
                assert "pause_trading" in tools
                assert not (await session.call_tool("pause_trading", {})).isError

    asyncio.run(check("observer"))
    asyncio.run(check("operator"))
    asyncio.run(check("status-only"))
    assert ControlStore(root).status("observer")["paused"]


def test_large_decision_inputs_survive_mcp_stdio_without_truncation(tmp_path):
    root = tmp_path / "private"
    ControlStore.initialize(
        root,
        RiskLimits(
            capital_micros=100,
            order_micros=10,
            daily_loss_micros=5,
            drawdown_micros=10,
            symbol_exposure_micros=50,
            max_daily_orders=2,
            max_spread_bps=10,
            max_quote_age_ms=1000,
            symbols=["BTC-USD"],
        ),
        [ClientGrant(client_id="observer", capabilities=["status:read", "risk:read"])],
        actor="test",
    )
    source, parameters = root / "strategy.py", root / "parameters.json"
    source.write_text(
        'def decide(c,p): return dict(action="hold",allocation_bps=0,'
        'reason="synthetic",unusual_event=False)\n'
    )
    parameters.write_text("{}")
    source.chmod(0o600)
    parameters.chmod(0o600)
    strategy = PrivateStrategy(source, parameters)
    context = {
        "symbol": "BTC-USD",
        "now_ms": 1_800_000_000_000,
        "news": [
            {
                "news_id": f"{i:064x}",
                "synthetic": True,
                "title": "\U0001f680" * 500,
                "url": "https://example.com/" + "\U0001f680" * 2000,
            }
            for i in range(200)
        ],
    }
    assert 6_000_000 < len(canonical(context).encode()) < MAX_BYTES
    recorder = DecisionSnapshots(root, strategy)
    try:
        identity = recorder.capture(
            canonical(context), strategy.propose(context), "evaluation", "synthetic"
        )
    finally:
        recorder.close()

    async def check():
        params = StdioServerParameters(
            command=sys.executable,
            args=[
                "-m",
                "helios_alpha.control.mcp_server",
                "--test-sqlite",
                "--root",
                str(root),
                "--client",
                "observer",
            ],
            env={"PYTHONPATH": str(Path(__file__).parents[1] / "src"), "PATH": os.environ["PATH"]},
        )
        async with stdio_client(params) as streams, ClientSession(*streams) as session:
            await session.initialize()
            listing = await session.call_tool("list_decision_snapshots", {"limit": 1})
            assert not listing.isError
            metadata = listing.structuredContent or json.loads(listing.content[0].text)
            assert metadata["snapshots"][0]["snapshot_id"] == identity
            assert metadata["retention"]["maximum_input_bytes"] == MAX_BYTES
            assert len(canonical(metadata)) < 4096
            result = await session.call_tool("get_decision_input", {"snapshot_id": identity})
            assert not result.isError
            snapshot = result.structuredContent or json.loads(result.content[0].text)
            assert snapshot["context"] == context
            assert snapshot["snapshot_id"] == identity
            assert snapshot["execution_authority"] is False

    asyncio.run(asyncio.wait_for(check(), 20))
