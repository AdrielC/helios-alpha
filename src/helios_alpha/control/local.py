"""Start/stop the laptop services without deleting durable state or changing risk policy."""

from __future__ import annotations

import argparse
import asyncio
import fcntl
import json
import logging
import logging.handlers
import os
import signal
import socket
import subprocess
import sys
import time
from pathlib import Path

from .golem_store import read_private_json
from .store import DEFAULT_ROOT, ControlError, private_root


def request(root: Path, action: str) -> dict:
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as conn:
        conn.settimeout(5)
        conn.connect(str(root / "supervisor.sock"))
        conn.sendall(json.dumps({"action": action}).encode() + b"\n")
        data = b""
        while b"\n" not in data and len(data) < 65536:
            chunk = conn.recv(4096)
            if not chunk:
                break
            data += chunk
        return json.loads(data)


async def supervise(root: Path) -> None:
    os.umask(0o077)
    lock = (root / "supervisor.lock").open("a+")
    fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
    config = read_private_json(root / "services.json")
    memory_mib = config.get("golem_memory_mib", 2048)
    if type(memory_mib) is not int or not 256 <= memory_mib <= 65536:
        raise ControlError("golem_memory_mib must be an integer between 256 and 65536")
    socket_path = root / "supervisor.sock"
    if socket_path.exists():
        socket_path.unlink()
    stopping = asyncio.Event()
    processes: dict[str, asyncio.subprocess.Process] = {}
    failures: dict[str, int] = {}
    log_tasks: list[asyncio.Task] = []
    loop = asyncio.get_running_loop()
    for sig in (signal.SIGINT, signal.SIGTERM):
        loop.add_signal_handler(sig, stopping.set)
    for port in (config["golem_port"], config["nats_port"]):
        with socket.socket() as probe:
            if probe.connect_ex(("127.0.0.1", port)) == 0:
                raise ControlError("a configured service port is already in use")
    commands = {
        "golem": [
            config["docker_binary"],
            "run",
            "--rm",
            "--init",
            "--name",
            config["golem_container"],
            "--publish",
            f"127.0.0.1:{config['golem_port']}:9881",
            "--mount",
            f"type=bind,source={root / 'golem-container-data'},target=/data",
            "--cpus",
            "2",
            "--memory",
            f"{memory_mib}m",
            "--cap-drop",
            "ALL",
            "--security-opt",
            "no-new-privileges",
            "--read-only",
            "--tmpfs",
            "/tmp:rw,nosuid,nodev,size=256m",
            "--user",
            f"{os.getuid()}:{os.getgid()}",
            config["golem_image"],
            "server",
            "run",
            "--router-addr",
            "0.0.0.0",
            "--router-port",
            "9881",
            "--custom-request-port",
            "0",
            "--mcp-port",
            "0",
            "--data-dir",
            "/data",
        ],
        "nats": [config["nats_binary"], "--config", str(root / "nats.conf")],
        "crypto": [
            sys.executable,
            "-m",
            "helios_alpha.crypto.runtime",
            "--root",
            str(root),
        ],
    }
    env = {
        k: v
        for k, v in os.environ.items()
        if not any(word in k.upper() for word in ("TOKEN", "SECRET", "API_KEY", "PASSWORD"))
    }
    env["PYTHONPATH"] = str(Path(__file__).resolve().parents[2])

    async def collect_log(name, process):
        logger = logging.getLogger(f"helios-local.{name}")
        handler = logging.handlers.RotatingFileHandler(
            root / f"{name}.log",
            maxBytes=5 * 1024 * 1024,
            backupCount=2,
        )
        logger.addHandler(handler)
        logger.setLevel(logging.INFO)
        try:
            while line := await process.stdout.readline():
                logger.info(line[:4096].decode(errors="replace").rstrip())
        finally:
            logger.removeHandler(handler)
            handler.close()

    async def stop_process(name):
        process = processes.pop(name, None)
        if process is not None and process.returncode is None:
            if name == "golem":
                stopper = await asyncio.create_subprocess_exec(
                    config["docker_binary"],
                    "stop",
                    "--time",
                    "15",
                    config["golem_container"],
                    stdout=asyncio.subprocess.DEVNULL,
                    stderr=asyncio.subprocess.DEVNULL,
                )
                await asyncio.wait_for(stopper.wait(), timeout=20)
            elif process.returncode is None:
                process.terminate()
            try:
                await asyncio.wait_for(process.wait(), timeout=15)
            except TimeoutError:
                process.kill()
                await process.wait()

    async def handle(reader, writer):
        try:
            raw = await asyncio.wait_for(reader.readline(), timeout=2)
            action = json.loads(raw)["action"]
            if action == "run":
                (root / "runtime-enabled").touch(mode=0o600)
            elif action == "stop":
                (root / "runtime-enabled").unlink(missing_ok=True)
                await stop_process("crypto")
            elif action == "down":
                stopping.set()
            elif action != "status":
                raise ValueError("unknown action")
            result = {
                "supervisor": "running",
                "runtime_requested": (root / "runtime-enabled").exists(),
                "services": {
                    name: {
                        "running": proc.returncode is None,
                        "pid": proc.pid,
                        "exit_code": proc.returncode,
                        "restarts": failures.get(name, 0),
                    }
                    for name, proc in processes.items()
                },
            }
            writer.write(json.dumps(result).encode() + b"\n")
            await writer.drain()
        except (ValueError, KeyError, TimeoutError):
            writer.write(b'{"error":"invalid local request"}\n')
        finally:
            writer.close()
            await writer.wait_closed()

    server = await asyncio.start_unix_server(handle, path=str(socket_path), limit=4096)
    socket_path.chmod(0o600)
    try:
        while not stopping.is_set():
            desired = ["golem", "nats"]
            if (root / "runtime-enabled").exists():
                desired.append("crypto")
            for name in desired:
                previous = processes.get(name)
                if previous is None or previous.returncode is not None:
                    if previous is not None:
                        failures[name] = failures.get(name, 0) + 1
                    processes[name] = await asyncio.create_subprocess_exec(
                        *commands[name],
                        stdin=asyncio.subprocess.DEVNULL,
                        stdout=asyncio.subprocess.PIPE,
                        stderr=asyncio.subprocess.STDOUT,
                        cwd=root,
                        env=env,
                        start_new_session=True,
                    )
                    log_tasks.append(asyncio.create_task(collect_log(name, processes[name])))
            log_tasks[:] = [task for task in log_tasks if not task.done()]
            try:
                await asyncio.wait_for(stopping.wait(), timeout=5)
            except TimeoutError:
                pass
    finally:
        server.close()
        await server.wait_closed()
        for name in ("crypto", "golem", "nats"):
            await stop_process(name)
        await asyncio.gather(*log_tasks, return_exceptions=True)
        socket_path.unlink(missing_ok=True)
        lock.close()


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=DEFAULT_ROOT)
    parser.add_argument("action", choices=["up", "run", "stop", "status", "down", "supervise"])
    args = parser.parse_args()
    root = private_root(args.root)
    if args.action == "supervise":
        asyncio.run(supervise(root))
        return
    if args.action == "up":
        try:
            result = request(root, "status")
        except (OSError, ValueError):
            os.umask(0o077)
            with (root / "supervisor.log").open("ab") as log:
                subprocess.Popen(
                    [
                        sys.executable,
                        "-m",
                        __package__ + ".local",
                        "--root",
                        str(root),
                        "supervise",
                    ],
                    stdin=subprocess.DEVNULL,
                    stdout=log,
                    stderr=log,
                    start_new_session=True,
                    close_fds=True,
                )
            for _ in range(100):
                try:
                    result = request(root, "status")
                    break
                except (OSError, ValueError):
                    time.sleep(0.1)
            else:
                raise ControlError("local supervisor did not start; inspect private supervisor.log")
    else:
        result = request(root, args.action)
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
