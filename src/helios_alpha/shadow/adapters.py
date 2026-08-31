from __future__ import annotations

import hashlib
import math
from collections.abc import Callable, Mapping
from dataclasses import dataclass
from datetime import UTC, date, datetime, timedelta
from typing import Any, Protocol

import httpx

from helios_alpha.shadow.models import RetrievedJson, ShadowCandidate, ShadowContractError
from helios_alpha.timekeeping import Clock, SystemClock


class JsonTransport(Protocol):
    def get(
        self, url: str, params: Mapping[str, str] | None = None
    ) -> RetrievedJson: ...


class HttpxJsonTransport:
    def __init__(
        self,
        clock: Clock | None = None,
        timeout_seconds: float = 30.0,
        client: httpx.Client | None = None,
    ) -> None:
        self._clock = clock or SystemClock()
        self._owned_client = client is None
        self._client = client or httpx.Client(timeout=timeout_seconds, follow_redirects=False)

    def close(self) -> None:
        if self._owned_client:
            self._client.close()

    def __enter__(self) -> HttpxJsonTransport:
        return self

    def __exit__(self, *_: object) -> None:
        self.close()

    def get(
        self, url: str, params: Mapping[str, str] | None = None
    ) -> RetrievedJson:
        response = self._client.get(url, params=params)
        response.raise_for_status()
        content = response.content
        observed = self._clock.now_utc()
        observed_at = datetime.fromtimestamp(observed.timestamp(), tz=UTC)
        return RetrievedJson(
            source_url=str(response.url),
            observed_at=observed_at,
            payload=response.json(),
            body_sha256=hashlib.sha256(content).hexdigest(),
            etag=response.headers.get("etag"),
            last_modified=response.headers.get("last-modified"),
        )


Normalizer = Callable[[RetrievedJson], list[ShadowCandidate]]
ParamsFactory = Callable[[date], Mapping[str, str] | None]


@dataclass(frozen=True)
class ShadowFeed:
    source_id: str
    url: str
    normalizer: Normalizer
    initial_lookback: timedelta
    revision_lookback: timedelta = timedelta(minutes=15)
    params_factory: ParamsFactory | None = None

    def fetch(
        self, transport: JsonTransport, today: date
    ) -> tuple[RetrievedJson, list[ShadowCandidate]]:
        params = self.params_factory(today) if self.params_factory else None
        document = transport.get(self.url, params=params)
        return document, self.normalizer(document)


def _parse_time(value: object, field: str) -> datetime:
    if not isinstance(value, str) or not value.strip():
        raise ShadowContractError(f"{field} must be an ISO timestamp")
    normalized = value.strip()
    if normalized.endswith("Z"):
        normalized = normalized[:-1] + "+00:00"
    try:
        parsed = datetime.fromisoformat(normalized)
    except ValueError as error:
        raise ShadowContractError(f"invalid {field}") from error
    if parsed.tzinfo is None:
        parsed = parsed.replace(tzinfo=UTC)
    return parsed.astimezone(UTC)


def _finite_or_none(value: object, field: str) -> float | None:
    if value is None:
        return None
    try:
        number = float(value)
    except (TypeError, ValueError) as error:
        raise ShadowContractError(f"{field} must be numeric") from error
    if not math.isfinite(number):
        raise ShadowContractError(f"{field} must be finite")
    return number


def _rows(document: RetrievedJson) -> list[dict[str, Any]]:
    if not isinstance(document.payload, list):
        raise ShadowContractError("source response must be a JSON array")
    if any(not isinstance(row, dict) for row in document.payload):
        raise ShadowContractError("source response rows must be JSON objects")
    return document.payload


def normalize_goes_xray(document: RetrievedJson) -> list[ShadowCandidate]:
    candidates = []
    for row in _rows(document):
        if row.get("energy") != "0.1-0.8nm":
            continue
        event_time = _parse_time(row.get("time_tag"), "time_tag")
        satellite = int(row["satellite"])
        flux = _finite_or_none(row.get("flux"), "flux")
        flags = ("missing_flux",) if flux is None else ()
        candidates.append(
            ShadowCandidate(
                natural_key=f"{event_time.isoformat()}:{satellite}:0.1-0.8nm",
                event_time=event_time,
                payload={
                    "seriesId": "goes-xray-flux",
                    "satellite": satellite,
                    "energy": "0.1-0.8nm",
                    "fluxWattsPerSquareMeter": flux,
                    "observedFlux": _finite_or_none(row.get("observed_flux"), "observed_flux"),
                    "electronCorrection": _finite_or_none(
                        row.get("electron_correction"), "electron_correction"
                    ),
                    "electronContamination": bool(row.get("electron_contaminaton", False)),
                },
                quality_flags=flags,
            )
        )
    return candidates


def normalize_goes_protons(document: RetrievedJson) -> list[ShadowCandidate]:
    candidates = []
    for row in _rows(document):
        if row.get("energy") != ">=10 MeV":
            continue
        event_time = _parse_time(row.get("time_tag"), "time_tag")
        satellite = int(row["satellite"])
        flux = _finite_or_none(row.get("flux"), "flux")
        candidates.append(
            ShadowCandidate(
                natural_key=f"{event_time.isoformat()}:{satellite}:ge10mev",
                event_time=event_time,
                payload={
                    "seriesId": "goes-proton-flux-ge10",
                    "satellite": satellite,
                    "energy": ">=10 MeV",
                    "fluxPfu": flux,
                },
                quality_flags=("missing_flux",) if flux is None else (),
            )
        )
    return candidates


def normalize_planetary_kp(document: RetrievedJson) -> list[ShadowCandidate]:
    candidates = []
    for row in _rows(document):
        event_time = _parse_time(row.get("time_tag"), "time_tag")
        candidates.append(
            ShadowCandidate(
                natural_key=event_time.isoformat(),
                event_time=event_time,
                payload={
                    "seriesId": "planetary-kp",
                    "kpIndex": int(row["kp_index"]) if row.get("kp_index") is not None else None,
                    "estimatedKp": _finite_or_none(row.get("estimated_kp"), "estimated_kp"),
                    "kpLabel": row.get("kp"),
                },
                quality_flags=("missing_kp",) if row.get("estimated_kp") is None else (),
            )
        )
    return candidates


def normalize_l1_magnetic_field(document: RetrievedJson) -> list[ShadowCandidate]:
    candidates = []
    for row in _rows(document):
        event_time = _parse_time(row.get("time_tag"), "time_tag")
        quality = int(row.get("overall_quality") or 0)
        flags = []
        if quality != 0:
            flags.append("provider_quality_nonzero")
        if not bool(row.get("active", False)):
            flags.append("source_not_primary")
        candidates.append(
            ShadowCandidate(
                natural_key=f"{event_time.isoformat()}:{row.get('source') or 'unknown'}",
                event_time=event_time,
                payload={
                    "seriesId": "l1-imf-bz-gsm",
                    "source": row.get("source"),
                    "active": bool(row.get("active", False)),
                    "btNt": _finite_or_none(row.get("bt"), "bt"),
                    "bzGsmNt": _finite_or_none(row.get("bz_gsm"), "bz_gsm"),
                    "sampleSize": row.get("sample_size"),
                    "quality": quality,
                },
                quality_flags=tuple(flags),
            )
        )
    return candidates


def normalize_l1_solar_wind(document: RetrievedJson) -> list[ShadowCandidate]:
    candidates = []
    for row in _rows(document):
        event_time = _parse_time(row.get("time_tag"), "time_tag")
        quality = int(row.get("overall_quality") or 0)
        flags = []
        if quality != 0:
            flags.append("provider_quality_nonzero")
        if not bool(row.get("active", False)):
            flags.append("source_not_primary")
        candidates.append(
            ShadowCandidate(
                natural_key=f"{event_time.isoformat()}:{row.get('source') or 'unknown'}",
                event_time=event_time,
                payload={
                    "seriesId": "l1-solar-wind-speed",
                    "source": row.get("source"),
                    "active": bool(row.get("active", False)),
                    "speedKms": _finite_or_none(row.get("proton_speed"), "proton_speed"),
                    "densityPerCm3": _finite_or_none(
                        row.get("proton_density"), "proton_density"
                    ),
                    "temperatureK": _finite_or_none(
                        row.get("proton_temperature"), "proton_temperature"
                    ),
                    "sampleSize": row.get("proton_sample_size"),
                    "quality": quality,
                },
                quality_flags=tuple(flags),
            )
        )
    return candidates


def _latest_enlil(analysis: dict[str, Any]) -> dict[str, Any] | None:
    models = analysis.get("enlilList") or []
    if not isinstance(models, list):
        raise ShadowContractError("enlilList must be an array")
    valid = [model for model in models if isinstance(model, dict)]
    return max(
        valid,
        key=lambda model: _parse_time(model.get("modelCompletionTime"), "modelCompletionTime"),
        default=None,
    )


def normalize_donki_cme(document: RetrievedJson) -> list[ShadowCandidate]:
    candidates = []
    for cme in _rows(document):
        activity_id = cme.get("activityID")
        if not isinstance(activity_id, str) or not activity_id:
            raise ShadowContractError("CME activityID must not be empty")
        event_time = _parse_time(cme.get("startTime"), "startTime")
        analyses = cme.get("cmeAnalyses") or []
        if not isinstance(analyses, list):
            raise ShadowContractError("cmeAnalyses must be an array")
        accurate = [row for row in analyses if isinstance(row, dict) and row.get("isMostAccurate")]
        analysis = accurate[0] if accurate else next(
            (row for row in analyses if isinstance(row, dict)), None
        )
        if analysis is None:
            candidates.append(
                ShadowCandidate(
                    natural_key=f"{activity_id}:analysis",
                    event_time=event_time,
                    available_at=(
                        _parse_time(cme["submissionTime"], "submissionTime")
                        if cme.get("submissionTime")
                        else None
                    ),
                    payload={
                        "seriesId": "donki-cme-analysis",
                        "activityId": activity_id,
                        "analysis": None,
                        "linkedEvents": cme.get("linkedEvents") or [],
                    },
                    quality_flags=("analysis_unavailable",),
                )
            )
            continue
        model = _latest_enlil(analysis)
        completion = (
            _parse_time(model["modelCompletionTime"], "modelCompletionTime")
            if model and model.get("modelCompletionTime")
            else None
        )
        analysis_available = (
            _parse_time(analysis["submissionTime"], "submissionTime")
            if analysis.get("submissionTime")
            else None
        )
        cme_available = (
            _parse_time(cme["submissionTime"], "submissionTime")
            if cme.get("submissionTime")
            else None
        )
        available_candidates = [
            instant for instant in (analysis_available, cme_available, completion) if instant
        ]
        available_at = max(available_candidates) if available_candidates else None
        earth_impacts = []
        if model:
            earth_impacts = [
                impact
                for impact in (model.get("impactList") or [])
                if isinstance(impact, dict)
                and str(impact.get("location") or "").casefold() == "earth"
            ]
        flags = []
        if available_at is None:
            flags.append("publication_clock_unavailable")
        if model is None:
            flags.append("propagation_model_unavailable")
        candidates.append(
            ShadowCandidate(
                natural_key=f"{activity_id}:analysis",
                event_time=event_time,
                available_at=available_at,
                payload={
                    "seriesId": "donki-cme-analysis",
                    "activityId": activity_id,
                    "catalog": analysis.get("catalog"),
                    "sourceVersion": cme.get("versionId"),
                    "speedKms": _finite_or_none(analysis.get("speed"), "speed"),
                    "halfAngleDeg": _finite_or_none(analysis.get("halfAngle"), "halfAngle"),
                    "longitudeDeg": _finite_or_none(analysis.get("longitude"), "longitude"),
                    "latitudeDeg": _finite_or_none(analysis.get("latitude"), "latitude"),
                    "cmeType": analysis.get("type"),
                    "isMostAccurate": bool(analysis.get("isMostAccurate", False)),
                    "modelCompletionTime": completion.isoformat() if completion else None,
                    "estimatedShockArrivalTime": (
                        model.get("estimatedShockArrivalTime") if model else None
                    ),
                    "isEarthGlancingBlow": bool(model.get("isEarthGB", False)) if model else False,
                    "isEarthMinorImpact": (
                        bool(model.get("isEarthMinorImpact", False)) if model else False
                    ),
                    "earthImpacts": earth_impacts,
                    "kpForecast": (
                        {
                            "kp18": model.get("kp_18"),
                            "kp90": model.get("kp_90"),
                            "kp135": model.get("kp_135"),
                            "kp180": model.get("kp_180"),
                        }
                        if model
                        else None
                    ),
                    "linkedEvents": cme.get("linkedEvents") or [],
                },
                quality_flags=tuple(flags),
            )
        )
    return candidates


def normalize_donki_flare(document: RetrievedJson) -> list[ShadowCandidate]:
    candidates = []
    for flare in _rows(document):
        flare_id = flare.get("flrID")
        if not isinstance(flare_id, str) or not flare_id:
            raise ShadowContractError("flare ID must not be empty")
        event_time = _parse_time(flare.get("peakTime"), "peakTime")
        available_at = (
            _parse_time(flare["submissionTime"], "submissionTime")
            if flare.get("submissionTime")
            else None
        )
        candidates.append(
            ShadowCandidate(
                natural_key=flare_id,
                event_time=event_time,
                available_at=available_at,
                payload={
                    "seriesId": "donki-flare-events",
                    "flareId": flare_id,
                    "classType": flare.get("classType"),
                    "beginTime": flare.get("beginTime"),
                    "peakTime": flare.get("peakTime"),
                    "endTime": flare.get("endTime"),
                    "activeRegion": flare.get("activeRegionNum"),
                    "sourceVersion": flare.get("versionId"),
                    "linkedEvents": flare.get("linkedEvents") or [],
                },
                quality_flags=("publication_clock_unavailable",) if available_at is None else (),
            )
        )
    return candidates


def _donki_params(lookback_days: int) -> ParamsFactory:
    def params(today: date) -> Mapping[str, str]:
        return {
            "startDate": (today - timedelta(days=lookback_days)).isoformat(),
            "endDate": today.isoformat(),
        }

    return params


DEFAULT_SHADOW_FEEDS: tuple[ShadowFeed, ...] = (
    ShadowFeed(
        "noaa-swpc-goes-xray-primary-v1",
        "https://services.swpc.noaa.gov/json/goes/primary/xrays-1-day.json",
        normalize_goes_xray,
        timedelta(hours=6),
    ),
    ShadowFeed(
        "noaa-swpc-goes-protons-primary-v1",
        "https://services.swpc.noaa.gov/json/goes/primary/integral-protons-1-day.json",
        normalize_goes_protons,
        timedelta(hours=6),
    ),
    ShadowFeed(
        "noaa-swpc-planetary-kp-1m-v1",
        "https://services.swpc.noaa.gov/json/planetary_k_index_1m.json",
        normalize_planetary_kp,
        timedelta(hours=6),
    ),
    ShadowFeed(
        "noaa-swpc-l1-mag-1m-v1",
        "https://services.swpc.noaa.gov/json/rtsw/rtsw_mag_1m.json",
        normalize_l1_magnetic_field,
        timedelta(hours=6),
    ),
    ShadowFeed(
        "noaa-swpc-l1-wind-1m-v1",
        "https://services.swpc.noaa.gov/json/rtsw/rtsw_wind_1m.json",
        normalize_l1_solar_wind,
        timedelta(hours=6),
    ),
    ShadowFeed(
        "nasa-ccmc-donki-cme-v1",
        "https://kauai.ccmc.gsfc.nasa.gov/DONKI/WS/get/CME",
        normalize_donki_cme,
        timedelta(days=7),
        revision_lookback=timedelta(days=7),
        params_factory=_donki_params(7),
    ),
    ShadowFeed(
        "nasa-ccmc-donki-flare-v1",
        "https://kauai.ccmc.gsfc.nasa.gov/DONKI/WS/get/FLR",
        normalize_donki_flare,
        timedelta(days=7),
        revision_lookback=timedelta(days=7),
        params_factory=_donki_params(7),
    ),
)
