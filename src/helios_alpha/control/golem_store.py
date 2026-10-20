"""Local MCP delegates every authoritative operation to Golem, with no SQLite failover."""

from __future__ import annotations

import json
import os
import re
import stat
import subprocess
import uuid
from pathlib import Path

from .models import PolicyUpdate
from .store import DEFAULT_ROOT, ControlError, canonical, private_root


def read_private_json(path: Path) -> dict:
    info = path.lstat()
    if not stat.S_ISREG(info.st_mode) or info.st_uid != os.getuid() or info.st_mode & 0o077:
        raise ControlError("configuration must be a private, user-owned regular file")
    if info.st_size > 1024 * 1024:
        raise ControlError("configuration exceeds size bound")
    result = json.loads(path.read_bytes())
    if not isinstance(result, dict):
        raise ControlError("configuration must be a JSON object")
    return result


class GolemControlStore:
    def __init__(self, root: Path = DEFAULT_ROOT):
        self.root = private_root(root)
        self.config = read_private_json(self.root / "golem-connection.json")
        if self.config.get("schema_version") != 1:
            raise ControlError("unsupported Golem connection version")
        self.profile = self.config.get("profile", "local")
        if not isinstance(self.profile, str) or not re.fullmatch(
            r"[A-Za-z0-9][A-Za-z0-9_-]{0,79}", self.profile
        ):
            raise ControlError("invalid Golem profile")
        self.account = self.config["account_id"]
        if not isinstance(self.account, str) or not 1 <= len(self.account) <= 80:
            raise ControlError("invalid control account identity")
        self.phantom_id = self.config.get("phantom_id")
        if self.phantom_id is not None:
            try:
                if str(uuid.UUID(self.phantom_id)) != self.phantom_id:
                    raise ValueError("noncanonical phantom identity")
            except (ValueError, TypeError, AttributeError) as exc:
                raise ControlError("invalid control recovery identity") from exc

    @property
    def agent_address(self) -> str:
        suffix = f"[{self.phantom_id}]" if self.phantom_id is not None else ""
        return f"helios-alpha/local/CryptoControlAgent({json.dumps(self.account)}){suffix}"

    def invoke(self, method: str, arguments: list[str], *, key: str | None = None):
        return self._invoke_json_at(self.agent_address, method, arguments, key=key)

    def _invoke_json_at(
        self, agent_address: str, method: str, arguments: list[str], *, key: str | None = None
    ):
        # No manifest discovery: runtime calls never build or auto-deploy.
        # Parameters contain policy data, never credentials or strategy code.
        command = [
            self.config["executable"],
            "--disable-app-manifest-discovery",
            "--profile",
            self.profile,
            "--config-dir",
            str(self.root / "golem-cli"),
            "--format",
            "json",
            "agent",
            "invoke",
            "--no-stream",
        ]
        if key is not None:
            command.extend(["--idempotency-key", key])
        command.extend(
            [
                agent_address,
                method,
                *[json.dumps(arg) for arg in arguments],
            ]
        )
        env = {
            k: v
            for k, v in os.environ.items()
            if not any(word in k.upper() for word in ("TOKEN", "SECRET", "API_KEY", "PASSWORD"))
        }
        try:
            result = subprocess.run(
                command,
                capture_output=True,
                text=True,
                timeout=45,
                check=False,
                cwd=self.root,
                env=env,
            )
        except (OSError, subprocess.TimeoutExpired) as exc:
            raise ControlError("Golem unavailable; outcome must be reconciled") from exc
        if result.returncode:
            raise ControlError("Golem invocation failed; outcome must be reconciled")
        if len(result.stdout) > 4 * 1024 * 1024:
            raise ControlError("Golem response exceeds bound")
        try:
            envelope = json.loads(result.stdout)
            typed = envelope["result_json"]
            if typed.get("typ") != {
                "type": "Result",
                "ok": {"type": "Str"},
                "err": {"type": "Str"},
            }:
                raise ControlError("unexpected Golem return type")
            returned = typed["value"]
            if isinstance(returned, dict) and "err" in returned:
                raise ControlError("Golem rejected the request: " + str(returned["err"])[:200])
            if not isinstance(returned, dict) or set(returned) != {"ok"}:
                raise ControlError("unexpected Golem result shape")
            return json.loads(returned["ok"])
        except (ValueError, KeyError, TypeError) as exc:
            raise ControlError("invalid Golem response") from exc

    def command(self, actor: str, action: str, payload: dict | None = None):
        return self.invoke("command", [actor, action, canonical(payload or {})])

    def initialize(self, config: dict) -> dict:
        return self.invoke("initialize", [canonical(config)])

    def capabilities(self, actor: str) -> list[str]:
        return self.command(actor, "capabilities")

    def status(self, actor: str) -> dict:
        return self.command(actor, "status")

    def policy(self, actor: str) -> dict:
        return self.command(actor, "policy")

    def settings(self, actor: str) -> dict:
        return self.command(actor, "settings")

    def pause(self, actor: str) -> dict:
        return self.command(actor, "pause")

    def resume_paper(self, actor: str) -> dict:
        return self.command(actor, "resume_paper")

    def update_policy(self, actor: str, update: PolicyUpdate) -> dict:
        return self.command(actor, "update_policy", update.model_dump())

    def audit(self, actor: str, after: int = 0, limit: int = 100) -> list[dict]:
        return self.command(actor, "audit", {"after": after, "limit": limit})

    def heartbeat(self, actor: str, component: str, status: dict) -> dict:
        return self.command(actor, "heartbeat", {"component": component, "status": status})
