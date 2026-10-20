import pytest

from helios_alpha.control.store import ControlError
from helios_alpha.control.strategy_observation import project


def test_projection_uses_runtime_books_and_excludes_private_payloads():
    state = {
        "authority": "golem",
        "mode": "paper",
        "paused": False,
        "live_admitted": False,
        "live_blockers": ["strategy_evidence_not_admitted"],
        "decision_budget": {"reserved_calls": 3, "daily_call_limit": 12},
        "paper": {"positions": {"SOL-USD": {}}, "total_orders": 4},
        "runtime": {
            "custom-worker": {
                "updated_ns": 123_000_000,
                "status": {
                    "engine": "custom_signal",
                    "state": "running",
                    "mode": "paper",
                    "feed_connected": True,
                    "active_news": 2,
                    "last_reasons": {"SOL-USD": "spread_too_wide"},
                    "access_token": "excluded",
                    "strategy_source": "excluded",
                },
            }
        },
    }
    actual = project(state, 456)
    assert actual["books"][0]["engine"] == "custom_signal"
    assert actual["books"][0]["signals"] == [{"symbol": "SOL-USD", "reason": "spread_too_wide"}]
    assert actual["books"][0]["updated_ms"] == 123
    assert actual["paper_orders"] == 4
    assert actual["paper_positions"] == 1
    assert actual["model_calls_reserved"] == 3
    assert "excluded" not in str(actual)
    state["authority"] = "local_cache"
    with pytest.raises(ControlError):
        project(state, 456)


@pytest.mark.parametrize("bad", [None, "yes", 1])
def test_unknown_authority_never_becomes_zero_live_strategies(bad):
    with pytest.raises(ControlError):
        project({"authority": "golem", "live_admitted": bad}, 456)
