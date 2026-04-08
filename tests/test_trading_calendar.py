import pandas as pd

from helios_alpha.markets.trading_calendar import TradingCalendar


def test_xnys_jan2_2024_session():
    tc = TradingCalendar("XNYS")
    d = pd.Timestamp("2024-01-02")
    assert tc.is_session(d)
    o = tc.session_open_utc(d)
    c = tc.session_close_utc(d)
    assert o.hour == 14 and o.minute == 30
    assert c.hour == 21


def test_shift_sessions():
    tc = TradingCalendar("XNYS")
    d = pd.Timestamp("2024-01-02")
    n = tc.shift_sessions(d, 1)
    assert n.normalize() == pd.Timestamp("2024-01-03")


def test_session_offset_date():
    tc = TradingCalendar("XNYS")
    from datetime import date

    end = tc.session_offset_date(date(2024, 1, 2), 2)
    assert end == date(2024, 1, 4)
