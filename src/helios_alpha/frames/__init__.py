"""Canonical Polars research frame and pandas compatibility accessor."""

# Import for pandas' accessor registration side effect.
from helios_alpha.frames import pandas as _pandas  # noqa: F401
from helios_alpha.frames.core import (
    FRAME_SCHEMA_VERSION,
    HelioFrame,
    HelioFrameError,
    canonical_schema,
)

__all__ = ["FRAME_SCHEMA_VERSION", "HelioFrame", "HelioFrameError", "canonical_schema"]
