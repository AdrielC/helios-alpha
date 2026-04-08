import pendulum

from helios_alpha.timekeeping import FrozenClock, SystemClock, utc_today_offset


def test_frozen_clock_stable():
    c = FrozenClock(pendulum.datetime(2024, 3, 15, 12, 0, 0, tz="UTC"))
    assert c.today_utc().isoformat() == "2024-03-15"
    assert c.now_utc().year == 2024 and c.now_utc().month == 3 and c.now_utc().day == 15
    assert c.now_utc().hour == 12 and c.now_utc().minute == 0


def test_utc_today_offset():
    c = FrozenClock(pendulum.datetime(2024, 1, 10, 0, 0, 0, tz="UTC"))
    assert utc_today_offset(c, 5).isoformat() == "2024-01-15"


def test_system_clock_returns_aware():
    c = SystemClock()
    assert c.now_utc().timezone_name == "UTC"
