"""Register bounded, data-only research views; publishers own domain interpretation.

Definitions are durable local settings. The browser knows only renderer kinds,
never publisher names, tickers, provider endpoints, or Python expressions.
"""

from __future__ import annotations

from pathlib import Path
from typing import Annotated, Literal

from pydantic import BaseModel, ConfigDict, Field, model_validator

from helios_alpha.control.broker_sync import write_private
from helios_alpha.control.golem_store import read_private_json
from helios_alpha.control.store import ControlError, canonical, private_root

Identifier = Annotated[str, Field(pattern=r"^[a-z][a-z0-9-]{0,63}$")]
Text = Annotated[str, Field(max_length=4096)]
Selector = Annotated[
    str, Field(pattern=r"^(\.|[a-zA-Z_][a-zA-Z0-9_]*(\.[a-zA-Z_][a-zA-Z0-9_]*)*)$", max_length=256)
]


class StrictModel(BaseModel):
    model_config = ConfigDict(extra="forbid", strict=True, allow_inf_nan=False)


class FieldDefinition(StrictModel):
    key: Identifier
    label: Text
    path: Selector
    format: Literal["text", "number", "currency", "percent", "datetime", "duration"] = "text"
    scale: float = Field(default=1, gt=0, le=1_000_000)
    precision: int = Field(default=2, ge=0, le=6)
    unit: Text = ""
    currency: str = Field(default="USD", pattern=r"^[A-Z]{3}$")
    labels: dict[str, Text] = Field(default_factory=dict, max_length=32)


class SectionDefinition(StrictModel):
    id: Identifier
    kind: Literal["metrics", "table", "notes"]
    title: Text
    description: Text = ""
    empty: Text = "No observations yet."
    rows_path: Selector = "."
    fields: list[FieldDefinition] = Field(min_length=1, max_length=12)
    observed_path: Selector | None = None
    max_age_ms: int | None = Field(default=None, ge=1, le=604_800_000)
    max_age_path: Selector | None = None

    @model_validator(mode="after")
    def unique_fields(self):
        if len({f.key for f in self.fields}) != len(self.fields):
            raise ValueError("duplicate view field")
        if self.observed_path is not None and self.max_age_ms is None and self.max_age_path is None:
            raise ValueError("dated observations need a freshness limit")
        return self


class ViewDefinition(StrictModel):
    schema_version: Literal[1] = 1
    enabled: bool = True
    title: Text
    subtitle: Text = ""
    mark: str = Field(default="", max_length=8)
    capabilities: list[Text] = Field(default_factory=list, max_length=8)
    lease_ms: int = Field(default=15_000, ge=1000, le=300_000)
    sections: list[SectionDefinition] = Field(max_length=16)

    @model_validator(mode="after")
    def unique_sections(self):
        if len({s.id for s in self.sections}) != len(self.sections):
            raise ValueError("duplicate view section")
        return self


def select(value, path):
    """Dictionary traversal only. Missing data stays unknown, never zero."""
    if path == ".":
        return value
    for key in path.split("."):
        if not isinstance(value, dict):
            return None
        value = value.get(key)
    return value


def view_settings(root: Path, publisher: str, default: Path) -> ViewDefinition:
    # Use the same filename validation for settings and publication.
    _identifier(publisher)
    directory = private_root(root / "research-view-settings", create=True)
    path = directory / f"{publisher}.json"
    if not path.exists():
        definition = ViewDefinition.model_validate_json(default.read_text())
        write_private(path, definition.model_dump())
    return ViewDefinition.model_validate(read_private_json(path))


def _identifier(value):
    import re

    if not isinstance(value, str) or not re.fullmatch(r"[a-z][a-z0-9-]{0,63}", value):
        raise ValueError("invalid view publisher")


def project(publisher: str, definition: ViewDefinition, snapshot: dict, at: int) -> dict:
    _identifier(publisher)
    if type(at) is not int or not 0 <= at <= 8_640_000_000_000_000:
        raise ValueError("invalid view timestamp")
    if snapshot.get("execution_authority") is not False:
        raise ValueError("views require an explicitly non-executing projection")
    sections = []
    for section in definition.sections:
        source = select(snapshot, section.rows_path)
        sources = source if isinstance(source, list) else [] if source is None else [source]
        if len(sources) > 100:
            raise ValueError("view row limit exceeded")
        max_age = (
            (select(snapshot, section.max_age_path) if section.max_age_path else section.max_age_ms)
            if section.observed_path
            else None
        )
        if section.observed_path and (type(max_age) is not int or not 1 <= max_age <= 604_800_000):
            raise ValueError("invalid observation freshness")
        rows = []
        for index, item in enumerate(sources):
            values = {}
            for field in section.fields:
                value = select(item, field.path)
                if value is not None and type(value) not in (str, bool, int, float):
                    raise ValueError("view cells must be scalar")
                if isinstance(value, str) and len(value) > 4096:
                    raise ValueError("view cell too large")
                if (
                    type(value) in (int, float)
                    and not -9_007_199_254_740_991 <= value <= 9_007_199_254_740_991
                ):
                    raise ValueError("view number outside browser precision bounds")
                # Labels are explicit presentation mappings, not truthiness defaults.
                label_key = str(value).lower() if isinstance(value, bool) else str(value)
                values[field.key] = field.labels.get(label_key, value)
            observed = select(item, section.observed_path) if section.observed_path else None
            if section.observed_path:
                if observed is not None and (type(observed) is not int or not 0 <= observed <= at):
                    raise ValueError("invalid observation time")
            rows.append({"id": str(index), "values": values, "observed_ms": observed})
        sections.append(
            {
                "id": section.id,
                "kind": section.kind,
                "title": section.title,
                "description": section.description,
                "empty": section.empty,
                "fields": [f.model_dump(exclude={"path", "labels"}) for f in section.fields],
                "rows": rows,
                "max_age_ms": max_age,
            }
        )
    result = {
        "schema_version": 1,
        "id": publisher,
        "execution_authority": False,
        "title": definition.title,
        "subtitle": definition.subtitle,
        "mark": definition.mark,
        "capabilities": definition.capabilities,
        "lease_ms": definition.lease_ms,
        "observed_ms": at,
        "sections": sections,
    }
    if len(canonical(result).encode()) > 1_048_576:
        raise ValueError("view exceeds publication limit")
    return result


def publish(root: Path, publisher: str, definition: ViewDefinition, snapshot: dict, at: int):
    _identifier(publisher)
    directory = private_root(root / "research-views", create=True)
    path = directory / f"{publisher}.json"
    if not definition.enabled:
        path.unlink(missing_ok=True)
        return
    write_private(path, project(publisher, definition, snapshot, at))


def publish_configured(root: Path, publisher: str, default: Path, snapshot: dict, at: int) -> bool:
    """Invalid display settings withdraw the view without stopping source capture."""
    _identifier(publisher)
    try:
        publish(root, publisher, view_settings(root, publisher, default), snapshot, at)
    except (ValueError, ControlError):
        directory = private_root(root / "research-views", create=True)
        # The gateway counts this as a rejected registration, never as a book.
        # Exception messages can contain source values and must not be published.
        write_private(
            directory / f"{publisher}.json",
            {
                "schema_version": 1,
                "registration_error": "invalid_definition_or_projection",
            },
        )
        return False
    return True
