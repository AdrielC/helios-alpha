from helios_alpha.crypto.atlas import AtlasHistory, descriptors
from helios_alpha.crypto.streaming import QuoteBook


def quote(at=1000):
    return dict(
        symbol="BTC-USD",
        bid_micros=100_000_000,
        ask_micros=102_000_000,
        observed_ms=at,
        received_ms=at + 1,
        sequence=at,
    )


def test_history_preserves_clocks_restarts_and_does_not_repeat_stale_quotes(tmp_path):
    path = tmp_path / "history.sqlite3"
    history = AtlasHistory(path)
    book = QuoteBook()
    book.connected = True
    book.accept(quote())
    history.sample(book, ["BTC-USD"], 1002)
    first = history.projection(["BTC-USD"], 1002)
    assert first["series"][0]["points"][0] == dict(
        kind="scalar",
        timestamp="1970-01-01T00:00:01.000Z",
        availableAt="1970-01-01T00:00:01.001Z",
        value=101.0,
    )
    assert first["series"][1]["points"][0]["value"] == 20_000 / 101
    history.sample(book, ["BTC-USD"], 1003)  # cached quote is deduplicated
    book.reset()
    history.sample(book, ["BTC-USD"], 1004)  # disconnection adds nothing
    book.connected = True
    book.accept(quote(2000))
    history.sample(book, ["BTC-USD"], 10_000)  # stale quote adds nothing
    history.close()
    reopened = AtlasHistory(path)
    result = reopened.projection(["BTC-USD"], 10_000)
    assert result["sequence"] > first["sequence"]
    assert result["series"] == first["series"]
    reopened.sample(book, ["BTC-USD"], 100_000_000)
    assert not reopened.projection(["BTC-USD"], 100_000_000)["series"][0]["points"]
    reopened.close()


def test_projection_is_bounded_and_catalog_tracks_configured_symbols(tmp_path):
    history = AtlasHistory(tmp_path / "history.sqlite3")
    with history.db:
        history.db.executemany(
            "INSERT INTO samples VALUES(?,?,?,?,?)",
            [("BTC-USD", at, at, 100, 101) for at in range(12_000)],
        )
    result = history.projection(["BTC-USD"], 12_000)
    assert len(result["series"][0]["points"]) <= 5000
    assert result["series"][0]["points"][-1]["timestamp"] == "1970-01-01T00:00:11.999Z"
    assert all("ETH-USD" in d["sourceNames"] for d in descriptors(["ETH-USD"]))
    assert not any("BTC" in d["id"] for d in descriptors(["ETH-USD"]))
    history.close()
