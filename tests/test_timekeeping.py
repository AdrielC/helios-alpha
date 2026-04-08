from datetime import UTC, datetime

from helios_alpha.timekeeping import FrozenClock, SystemClock, utc_today_offset


def test_frozen_clock_stable():
    c = FrozenClock(datetime(2024, 3, 15, 12, 0, 0, tzinfo=UTC))
    assert c.today_utc().isoformat() == "2024-03-15"
    assert c.now_utc() == datetime(2024, 3, 15, 12, 0, 0, tzinfo=UTC)


def test_utc_today_offset():
    c = FrozenClock(datetime(2024, 1, 10, 0, 0, 0, tzinfo=UTC))
    assert utc_today_offset(c, 5).isoformat() == "2024-01-15"


def test_system_clock_returns_aware():
    c = SystemClock()
    assert c.now_utc().tzinfo is not None
