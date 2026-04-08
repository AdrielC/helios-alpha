from pathlib import Path

from pydantic import Field
from pydantic_settings import BaseSettings, SettingsConfigDict


class Settings(BaseSettings):
    model_config = SettingsConfigDict(
        env_prefix="HELIOS_",
        env_file=".env",
        env_file_encoding="utf-8",
        extra="ignore",
    )

    repo_root: Path = Field(default_factory=lambda: Path(__file__).resolve().parents[2])
    nasa_api_key: str = Field(default="DEMO_KEY", description="NASA api.nasa.gov key for DONKI")
    polygon_api_key: str = Field(default="", description="Polygon.io API key (market data)")

    @property
    def data_raw(self) -> Path:
        return self.repo_root / "data" / "raw"

    @property
    def data_processed(self) -> Path:
        return self.repo_root / "data" / "processed"


def load_settings() -> Settings:
    return Settings()
