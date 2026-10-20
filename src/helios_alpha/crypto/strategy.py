"""Host an owner-selected private strategy; repository code contains no trading rules."""

from __future__ import annotations

import hashlib
import json
import os
import re
import stat
from pathlib import Path

from helios_alpha.control.golem_store import read_private_json
from helios_alpha.control.store import ControlError, private_root

MAX_NEWS_EVIDENCE = 8


class PrivateStrategy:
    def __init__(self, path: Path, parameters_path: Path, *, expected_fingerprint=None):
        private_root(path.parent)
        private_root(parameters_path.parent)
        info = path.lstat()
        if not stat.S_ISREG(info.st_mode) or info.st_uid != os.getuid() or info.st_mode & 0o077:
            raise ControlError("strategy must be a private owner-controlled file")
        if info.st_size > 64 * 1024:
            raise ControlError("strategy source exceeds bound")
        source = path.read_bytes()
        self.parameters = read_private_json(parameters_path)
        self.source_bytes = source
        self.parameter_bytes = parameters_path.read_bytes()
        if json.loads(self.parameter_bytes) != self.parameters:
            raise ControlError("strategy parameters changed while loading")
        self.fingerprint = hashlib.sha256(source + self.parameter_bytes).hexdigest()
        if expected_fingerprint is not None and self.fingerprint != expected_fingerprint:
            raise ControlError("archived strategy fingerprint mismatch")
        namespace = {"__name__": "helios_private_strategy", "__file__": str(path)}
        # This is trusted local code selected by the owner, not a sandbox for third-party code.
        exec(compile(source, str(path), "exec"), namespace)
        self.function = namespace["decide"]

    def decide(self, context: dict) -> dict:
        result = self.function(context, self.parameters)
        if not isinstance(result, dict) or type(result.get("allocation_bps")) is not int:
            raise ValueError("strategy returned an invalid allocation")
        if not 0 <= result["allocation_bps"] <= 10000:
            raise ValueError("strategy requested leverage or a short")
        if not isinstance(result.get("unusual_event", False), bool):
            raise ValueError("invalid unusual-event flag")
        return {
            "allocation_bps": result["allocation_bps"],
            "unusual_event": result.get("unusual_event", False),
        }

    def propose(self, context: dict) -> dict:
        """Generic event strategy interface; selection rules remain private."""
        result = self.function(context, self.parameters)
        required = {"action", "allocation_bps", "reason", "unusual_event"}
        if (
            not isinstance(result, dict)
            or not required <= set(result)
            or set(result) - required - {"news_id", "evidence_news_ids"}
        ):
            raise ValueError("invalid event proposal")
        action, allocation = result["action"], result["allocation_bps"]
        if action not in ("hold", "enter", "exit") or type(allocation) is not int:
            raise ValueError("invalid event action")
        if not 0 <= allocation <= 10000 or (action != "enter" and allocation != 0):
            raise ValueError("invalid event allocation")
        if type(result["unusual_event"]) is not bool:
            raise ValueError("invalid event review flag")
        if not isinstance(result["reason"], str) or not re.fullmatch(
            r"[a-z0-9_]{1,80}", result["reason"]
        ):
            raise ValueError("invalid event reason")
        if action == "enter" and (
            not isinstance(result.get("news_id"), str)
            or not re.fullmatch(r"[a-f0-9]{64}", result["news_id"])
        ):
            raise ValueError("entry proposal lacks news identity")
        if "evidence_news_ids" in result:
            ids = result["evidence_news_ids"]
            available = {item["news_id"] for item in context["news"]}
            if (
                action != "enter"
                or not isinstance(ids, list)
                or not 1 <= len(ids) <= MAX_NEWS_EVIDENCE
                or any(
                    not isinstance(identity, str)
                    or not re.fullmatch(r"[a-f0-9]{64}", identity)
                    for identity in ids
                )
                or len(set(ids)) != len(ids)
                or result["news_id"] not in ids
                or not set(ids) <= available
            ):
                raise ValueError("invalid proposal evidence identities")
        return result
