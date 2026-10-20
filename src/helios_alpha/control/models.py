from __future__ import annotations

from typing import Annotated, Literal

from pydantic import BaseModel, ConfigDict, Field, model_validator

PositiveMicros = Annotated[int, Field(strict=True, gt=0, le=1_000_000_000_000)]
Name = Annotated[str, Field(min_length=1, max_length=80, pattern=r"^[a-zA-Z0-9_-]+$")]


class RiskLimits(BaseModel):
    """Money is integer millionths of USD, never binary floating point."""

    model_config = ConfigDict(extra="forbid", strict=True, frozen=True)
    capital_micros: PositiveMicros
    order_micros: PositiveMicros
    daily_loss_micros: PositiveMicros
    drawdown_micros: PositiveMicros
    symbol_exposure_micros: PositiveMicros
    max_daily_orders: Annotated[int, Field(strict=True, ge=1, le=1000)]
    max_spread_bps: Annotated[int, Field(strict=True, ge=1, le=500)]
    max_quote_age_ms: Annotated[int, Field(strict=True, ge=100, le=60_000)]
    symbols: list[Literal["BTC-USD", "ETH-USD"]]
    leverage: Literal[False] = False

    @model_validator(mode="after")
    def check_limits(self) -> RiskLimits:
        if not (
            self.order_micros <= self.symbol_exposure_micros <= self.capital_micros
            and self.daily_loss_micros <= self.drawdown_micros <= self.capital_micros
        ):
            raise ValueError(
                "order/symbol/capital and daily/drawdown/capital limits must be ordered"
            )
        if not self.symbols or len(set(self.symbols)) != len(self.symbols):
            raise ValueError("symbols must be nonempty and unique")
        return self


class PolicyUpdate(BaseModel):
    model_config = ConfigDict(extra="forbid", strict=True)
    expected_revision: Annotated[int, Field(ge=1)]
    request_id: Name
    limits: RiskLimits
    reason: Annotated[str, Field(min_length=3, max_length=500)]


CAPABILITIES = frozenset(
    {
        "status:read",
        "risk:read",
        "audit:read",
        "execution:pause",
        "risk:write",
        "runtime:write",
        "decisions:reserve",
        "execution:resume-paper",
    }
)


class ClientGrant(BaseModel):
    model_config = ConfigDict(extra="forbid", strict=True)
    client_id: Name
    capabilities: list[str]

    @model_validator(mode="after")
    def check_capabilities(self) -> ClientGrant:
        if len(set(self.capabilities)) != len(self.capabilities):
            raise ValueError("duplicate capabilities")
        if not set(self.capabilities) <= CAPABILITIES:
            raise ValueError("unknown capability")
        return self
