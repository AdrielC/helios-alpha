from pathlib import Path

import pytest

from helios_alpha.instruments.registry import (
    load_instrument_registry,
    provider_symbol,
)


@pytest.fixture
def registry_path(tmp_path: Path) -> Path:
    p = tmp_path / "instruments.yaml"
    p.write_text(
        """
version: 1
instruments:
  - id: VIX
    yfinance: ^VIX
    polygon: I:VIX
  - id: SPY
    polygon: SPY
""",
        encoding="utf-8",
    )
    return p


def test_provider_symbol_maps(registry_path: Path):
    reg = load_instrument_registry(registry_path)
    assert provider_symbol(reg, "VIX", "yfinance") == "^VIX"
    assert provider_symbol(reg, "VIX", "polygon") == "I:VIX"
    assert provider_symbol(reg, "SPY", "polygon") == "SPY"
    assert provider_symbol(reg, "SPY", "yfinance") == "SPY"


def test_unknown_id_raises(registry_path: Path):
    reg = load_instrument_registry(registry_path)
    with pytest.raises(KeyError):
        provider_symbol(reg, "FAKE", "polygon")
