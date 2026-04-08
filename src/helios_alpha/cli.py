"""
Hydra CLI entry: does not change working directory (compose-only).

Usage:
  helios-pipeline pipeline.start_date=2024-01-01 pipeline.end_date=2024-01-31
  helios-pipeline pipeline.clock.kind=frozen pipeline.clock.frozen_iso=2024-06-01T00:00:00+00:00
"""

from __future__ import annotations

import sys
from pathlib import Path

from hydra import compose, initialize_config_dir
from hydra.core.global_hydra import GlobalHydra

from helios_alpha.main import run_pipeline


def main(argv: list[str] | None = None) -> None:
    argv = argv if argv is not None else sys.argv[1:]
    pkg_conf = Path(__file__).resolve().parent / "conf"
    if GlobalHydra.instance().is_initialized():
        GlobalHydra.instance().clear()
    with initialize_config_dir(config_dir=str(pkg_conf), version_base=None):
        cfg = compose(config_name="config", overrides=list(argv))
    run_pipeline(cfg)


if __name__ == "__main__":
    main()
