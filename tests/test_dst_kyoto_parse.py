from helios_alpha.ingest.dst_kyoto import _parse_dst_month_lines


def test_parse_dst_month_sample():
    text = (
        "DST2401*01RRX020   0   4   5   7   8   7   7   9   8   6   7   4  -2  -2   1   8   4  "
        "-3 -12 -14  -9  -5  -1  -3  -1   1\n"
    )
    rows = _parse_dst_month_lines(text, 2024, 1)
    assert len(rows) == 24
    assert rows[0]["dst_nT"] == 0.0
    assert rows[23]["dst_nT"] == -3.0
