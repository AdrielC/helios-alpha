"""Official public releases, preserving publication dates and observation time separately."""

from __future__ import annotations

import json
import re
from datetime import UTC, datetime

EIA = "https://ir.eia.gov/ngs/wngsr.json"
CPC = "https://ftp.cpc.ncep.noaa.gov/htdocs/degree_days/weighted/daily_forecasts_7day/latest/"
SOURCES = {
    "eia-storage": EIA,
    "noaa-hdd": CPC + "Population.Heating.txt",
    "noaa-cdd": CPC + "Population.Cooling.txt",
}


def parse_source(source: str, text: str) -> dict:
    if source == "eia-storage":
        raw = json.loads(text.lstrip("\ufeff"))
        row = next(s for s in raw["series"] if s["series_id"] == "png.nw2_epg0_swo_r48_bcf.w")
        values = dict(row["data"])
        # EIA's JSON uses midnight for the release DATE, not the release instant.
        # Never manufacture a 10:30 availability timestamp from this field.
        return {
            "kind": "storage",
            "period": raw["current_week"],
            "release_date": raw["release_date"],
            "published_ms": None,
            "storage_bcf": values[raw["current_week"]],
            "change_bcf": row["calculated"]["net_change"],
            "seasonal_average_bcf": row["calculated"]["5yr-avg"],
            "revision": row["calculated"]["revision_flag_net_change"] == "true",
        }
    if source not in {"noaa-hdd", "noaa-cdd"}:
        raise ValueError("unknown natural-gas source")
    lines = text.strip().splitlines()
    match = re.search(r"Issued (\d{4})UTC (\d{8})", lines[0])
    if not match or "Weights: Population" not in lines:
        raise ValueError("unrecognized NOAA vintage or weighting")
    issue = datetime.strptime(match[2] + match[1], "%Y%m%d%H%M").replace(tzinfo=UTC)
    header = next(line for line in lines if line.startswith("Region|")).split("|")[1:-1]
    row = next(line for line in lines if line.startswith("CONUS|")).split("|")[1:-1]
    if len(header) != 7 or len(row) != 7 or len(set(header)) != 7:
        raise ValueError("incomplete NOAA forecast")
    dates = [datetime.strptime(day, "%Y%m%d").date().isoformat() for day in header]
    values = [int(value) for value in row]
    if any(not 0 <= value <= 200 for value in values):
        raise ValueError("invalid degree days")
    return {
        "kind": "hdd" if source == "noaa-hdd" else "cdd",
        "period": issue.date().isoformat(),
        "published_ms": int(issue.timestamp() * 1000),
        "daily": dict(zip(dates, values)),
        "total": sum(values),
        "weighting": "population",
    }
