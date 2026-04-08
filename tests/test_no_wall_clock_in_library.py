"""
Ensure library modules do not call datetime.now / date.today (except timekeeping.SystemClock).
"""

from __future__ import annotations

import ast
from pathlib import Path


def _offenders(root: Path) -> list[str]:
    bad: list[str] = []
    pkg = root / "src" / "helios_alpha"
    for path in pkg.rglob("*.py"):
        if path.name == "timekeeping.py":
            continue
        tree = ast.parse(path.read_text(encoding="utf-8"))
        for node in ast.walk(tree):
            if isinstance(node, ast.Call):
                if isinstance(node.func, ast.Attribute):
                    if node.func.attr in ("now", "today", "utcnow"):
                        if isinstance(node.func.value, ast.Name):
                            if node.func.value.id in ("datetime", "date"):
                                bad.append(f"{path.relative_to(root)}:{node.lineno}")
    return bad


def test_library_has_no_datetime_now_or_today():
    root = Path(__file__).resolve().parents[1]
    bad = _offenders(root)
    assert not bad, "Wall-clock calls in library: " + ", ".join(bad)
