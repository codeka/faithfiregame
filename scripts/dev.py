#!/usr/bin/env python3
"""Cross-conversation controller for Faith & Fire local dev services."""

from __future__ import annotations

import argparse
import csv
import json
import os
import shutil
import signal
import socket
import subprocess
import sys
import time
from collections import deque
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
DEV_DIR = REPO_ROOT / ".dev"
PIDS_FILE = DEV_DIR / "pids.json"
SERVER_LOG = DEV_DIR / "server.log"
WEB_LOG = DEV_DIR / "web.log"

SERVER_PORT = 3000
WEB_PORT = 4200
SERVER_PROC_PREFIX = "server"
WEB_PROC_PREFIX = "node"


def server_binary(release: bool) -> Path:
    exe = "server.exe" if os.name == "nt" else "server"
    profile = "release" if release else "debug"
    return REPO_ROOT / "target" / profile / exe


def resolve_tool(name: str) -> str:
    path = shutil.which(name)
    if not path:
        raise RuntimeError(f"Required tool '{name}' was not found on PATH.")
    return path


def load_pids() -> dict[str, dict]:
    if not PIDS_FILE.exists():
        return {}
    try:
        data = json.loads(PIDS_FILE.read_text(encoding="utf-8"))
        return data if isinstance(data, dict) else {}
    except (OSError, json.JSONDecodeError):
        return {}


def save_pids(data: dict[str, dict]) -> None:
    DEV_DIR.mkdir(parents=True, exist_ok=True)
    PIDS_FILE.write_text(json.dumps(data, indent=2) + "\n", encoding="utf-8")


def is_port_open(port: int) -> bool:
    for host in ("127.0.0.1", "::1"):
        try:
            with socket.create_connection((host, port), timeout=0.25):
                return True
        except OSError:
            continue
    return False


def listening_pids_on_port(port: int) -> list[int]:
    pids: list[int] = []
    if os.name == "nt":
        # Omit `-p tcp` so TCPv6 listeners such as `[::1]:4200` are included.
        res = subprocess.run(
            ["netstat", "-ano"],
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
        )
        if res.returncode != 0:
            return []
        suffix = f":{port}"
        for raw in res.stdout.splitlines():
            parts = raw.split()
            if len(parts) < 5:
                continue
            if parts[0].upper() != "TCP" or parts[3].upper() != "LISTENING":
                continue
            if not parts[1].endswith(suffix):
                continue
            try:
                pid = int(parts[4])
            except ValueError:
                continue
            if pid > 0 and pid not in pids:
                pids.append(pid)
        return pids

    lsof = shutil.which("lsof")
    if not lsof:
        return []
    res = subprocess.run(
        [lsof, "-ti", f"tcp:{port}", "-sTCP:LISTEN"],
        capture_output=True,
        text=True,
    )
    if res.returncode != 0:
        return []
    for line in res.stdout.splitlines():
        try:
            pid = int(line.strip())
        except ValueError:
            continue
        if pid > 0 and pid not in pids:
            pids.append(pid)
    return pids


def _windows_image_name(pid: int) -> str | None:
    res = subprocess.run(
        ["tasklist", "/FI", f"PID eq {pid}", "/FO", "CSV", "/NH"],
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
    )
    if res.returncode != 0:
        return None
    out = res.stdout.strip()
    if not out or "no tasks" in out.lower() or not out.startswith('"'):
        return None
    for row in csv.reader([out]):
        if row:
            return row[0].strip().lower()
    return None


def is_pid_alive(pid: int) -> bool:
    if pid <= 0:
        return False
    if os.name == "nt":
        return _windows_image_name(pid) is not None
    try:
        os.kill(pid, 0)
        return True
    except OSError:
        return False


def pid_matches_expected(pid: int, expected_prefix: str) -> bool:
    if pid <= 0:
        return False
    if os.name == "nt":
        image = _windows_image_name(pid)
        return image is not None and image.startswith(expected_prefix.lower())
    return is_pid_alive(pid)


def kill_pid_tree(pid: int) -> None:
    if pid <= 0:
        return
    if os.name == "nt":
        subprocess.run(
            ["taskkill", "/PID", str(pid), "/T", "/F"],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        return
    try:
        os.killpg(pid, signal.SIGTERM)
    except OSError:
        try:
            os.kill(pid, signal.SIGTERM)
        except OSError:
            pass


def wait_file_writable(path: Path, timeout: float = 5.0) -> None:
    if not path.exists():
        return
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        try:
            with open(path, "ab"):
                return
        except OSError:
            time.sleep(0.15)
    raise RuntimeError(f"Timed out waiting for Windows file lock release on {path}")


def tail_lines(path: Path, lines: int) -> list[str]:
    if not path.exists() or lines <= 0:
        return []
    with open(path, "r", encoding="utf-8", errors="replace") as fh:
        return list(deque(fh, maxlen=lines))


def print_tail(title: str, path: Path, lines: int) -> None:
    print(f"--- {title} ({path}) ---")
    recent = tail_lines(path, lines)
    if not recent:
        print("(empty)")
        return
    for line in recent:
        print(line, end="")
    if not recent[-1].endswith("\n"):
        print()


def spawn_detached(
    cmd: list[str],
    cwd: Path,
    log_path: Path,
    env: dict[str, str] | None = None,
) -> subprocess.Popen[bytes]:
    DEV_DIR.mkdir(parents=True, exist_ok=True)
    log_fh = open(log_path, "ab")
    try:
        banner = (
            f"\n=== Starting {' '.join(cmd)} at "
            f"{time.strftime('%Y-%m-%d %H:%M:%S')} ===\n"
        )
        log_fh.write(banner.encode("utf-8"))
        log_fh.flush()

        popen_kwargs: dict = {
            "cwd": str(cwd),
            "env": env,
            "stdin": subprocess.DEVNULL,
            "stdout": log_fh,
            "stderr": subprocess.STDOUT,
            "close_fds": True,
        }
        if os.name == "nt":
            # Never combine DETACHED_PROCESS with CREATE_NO_WINDOW (CreateProcessW WinError 87).
            popen_kwargs["creationflags"] = (
                subprocess.CREATE_NEW_PROCESS_GROUP | subprocess.CREATE_NO_WINDOW
            )
        else:
            popen_kwargs["start_new_session"] = True

        return subprocess.Popen(cmd, **popen_kwargs)
    finally:
        log_fh.close()


def stop_service(name: str) -> int:
    port, prefix = (
        (SERVER_PORT, SERVER_PROC_PREFIX)
        if name == "server"
        else (WEB_PORT, WEB_PROC_PREFIX)
    )
    pids_data = load_pids()
    candidates: list[int] = []

    rec_pid = pids_data.get(name, {}).get("pid")
    if isinstance(rec_pid, int) and pid_matches_expected(rec_pid, prefix):
        candidates.append(rec_pid)

    for pid in listening_pids_on_port(port):
        if pid_matches_expected(pid, prefix) and pid not in candidates:
            candidates.append(pid)

    for pid in candidates:
        kill_pid_tree(pid)

    if candidates:
        deadline = time.monotonic() + 5.0
        while time.monotonic() < deadline and is_port_open(port):
            time.sleep(0.15)

    if name == "server":
        for rel in (False, True):
            wait_file_writable(server_binary(rel))

    pids_data.pop(name, None)
    save_pids(pids_data)

    if is_port_open(port):
        holders = listening_pids_on_port(port)
        print(
            f"[warn] {name} port {port} is still occupied by PID(s) {holders}",
            file=sys.stderr,
        )
        return 1

    if candidates:
        print(f"Stopped {name} (killed PID(s) {', '.join(map(str, candidates))}).")
    else:
        print(f"{name} already stopped.")
    return 0


def build_server(release: bool) -> int:
    stop_rc = stop_service("server")
    if stop_rc != 0:
        return stop_rc

    wait_file_writable(server_binary(release))
    cargo = resolve_tool("cargo")
    cmd = [cargo, "build", "-p", "server"]
    if release:
        cmd.append("--release")
    else:
        cmd.extend(["--features", "editor"])

    print(f"Building server ({'release' if release else 'dev + editor'})...")
    res = subprocess.run(cmd, cwd=REPO_ROOT)
    return res.returncode


def start_server(release: bool) -> int:
    if is_port_open(SERVER_PORT):
        holders = listening_pids_on_port(SERVER_PORT)
        print(
            f"server already running on http://localhost:{SERVER_PORT}"
            f" (PID(s): {', '.join(map(str, holders)) or 'unknown'})"
        )
        return 0

    bin_path = server_binary(release)
    if not bin_path.exists():
        rc = build_server(release)
        if rc != 0:
            return rc

    env = os.environ.copy()
    env.setdefault("RUST_LOG", "info")
    proc = spawn_detached([str(bin_path)], REPO_ROOT, SERVER_LOG, env=env)

    deadline = time.monotonic() + 12.0
    while time.monotonic() < deadline:
        if is_port_open(SERVER_PORT):
            holders = listening_pids_on_port(SERVER_PORT)
            pid = holders[0] if holders else proc.pid
            pids_data = load_pids()
            pids_data["server"] = {
                "pid": pid,
                "mode": "release" if release else "dev",
            }
            save_pids(pids_data)
            print(f"Started server on http://localhost:{SERVER_PORT} (PID {pid}).")
            return 0
        if proc.poll() is not None:
            print(
                f"server exited early with code {proc.returncode}.",
                file=sys.stderr,
            )
            print_tail("server.log", SERVER_LOG, 30)
            return proc.returncode or 1
        time.sleep(0.2)

    kill_pid_tree(proc.pid)
    print(f"Timed out waiting for server on port {SERVER_PORT}.", file=sys.stderr)
    print_tail("server.log", SERVER_LOG, 30)
    return 1


def build_wasm(release: bool) -> int:
    cmd = [sys.executable, str(REPO_ROOT / "scripts" / "build_wasm.py")]
    if release:
        cmd.append("--release")
    res = subprocess.run(cmd, cwd=REPO_ROOT)
    return res.returncode


def start_web(release: bool) -> int:
    if is_port_open(WEB_PORT):
        holders = listening_pids_on_port(WEB_PORT)
        print(
            f"web already running on http://localhost:{WEB_PORT}"
            f" (PID(s): {', '.join(map(str, holders)) or 'unknown'})"
        )
        return 0

    wasm_bg = REPO_ROOT / "web" / "public" / "wasm" / "client_wasm_bg.wasm"
    if not wasm_bg.exists():
        rc = build_wasm(release)
        if rc != 0:
            return rc

    node = resolve_tool("node")
    ng_js = REPO_ROOT / "web" / "node_modules" / "@angular" / "cli" / "bin" / "ng.js"
    if not ng_js.exists():
        raise RuntimeError(
            f"Angular CLI not found at {ng_js}. Run `npm install` inside web/ first."
        )

    proc = spawn_detached(
        [node, str(ng_js), "serve", "--proxy-config", "proxy.conf.json"],
        REPO_ROOT / "web",
        WEB_LOG,
    )

    deadline = time.monotonic() + 30.0
    while time.monotonic() < deadline:
        if is_port_open(WEB_PORT):
            holders = listening_pids_on_port(WEB_PORT)
            pid = holders[0] if holders else proc.pid
            pids_data = load_pids()
            pids_data["web"] = {"pid": pid}
            save_pids(pids_data)
            print(f"Started web on http://localhost:{WEB_PORT} (PID {pid}).")
            return 0
        if proc.poll() is not None:
            print(
                f"web exited early with code {proc.returncode}.",
                file=sys.stderr,
            )
            print_tail("web.log", WEB_LOG, 40)
            return proc.returncode or 1
        time.sleep(0.25)

    kill_pid_tree(proc.pid)
    print(f"Timed out waiting for web on port {WEB_PORT}.", file=sys.stderr)
    print_tail("web.log", WEB_LOG, 40)
    return 1


def print_status(target: str) -> int:
    pids_data = load_pids()

    if target in ("all", "server"):
        running = is_port_open(SERVER_PORT)
        holders = listening_pids_on_port(SERVER_PORT)
        rec = pids_data.get("server", {})
        mode = rec.get("mode", "unknown")
        state = "RUNNING" if running else "STOPPED"
        pid_str = ", ".join(map(str, holders)) if holders else (str(rec.get("pid", "-")))
        print(
            f"server : {state:<7} http://localhost:{SERVER_PORT}  "
            f"pid={pid_str}  mode={mode}  log={SERVER_LOG}"
        )

    if target in ("all", "web"):
        running = is_port_open(WEB_PORT)
        holders = listening_pids_on_port(WEB_PORT)
        rec = pids_data.get("web", {})
        state = "RUNNING" if running else "STOPPED"
        pid_str = ", ".join(map(str, holders)) if holders else (str(rec.get("pid", "-")))
        print(
            f"web    : {state:<7} http://localhost:{WEB_PORT}  "
            f"pid={pid_str}  log={WEB_LOG}"
        )

    if target in ("all", "wasm"):
        wasm_bg = REPO_ROOT / "web" / "public" / "wasm" / "client_wasm_bg.wasm"
        present = wasm_bg.exists()
        detail = f"{wasm_bg.stat().st_size} bytes" if present else "missing"
        print(f"wasm   : {'READY' if present else 'MISSING':<7} {wasm_bg} ({detail})")

    return 0


def cmd_start(target: str, release: bool) -> int:
    if target == "wasm":
        return build_wasm(release)
    if target in ("all", "server"):
        rc = start_server(release)
        if rc != 0:
            return rc
    if target in ("all", "web"):
        rc = start_web(release)
        if rc != 0:
            return rc
    return 0


def cmd_stop(target: str) -> int:
    rc = 0
    if target == "wasm":
        print("wasm is a build artifact; nothing to stop.")
        return 0
    if target in ("all", "web"):
        rc |= stop_service("web")
    if target in ("all", "server"):
        rc |= stop_service("server")
    return 1 if rc else 0


def cmd_restart(target: str, release: bool) -> int:
    if target == "wasm":
        return build_wasm(release)
    rc = cmd_stop(target)
    if rc != 0:
        return rc
    return cmd_start(target, release)


def cmd_rebuild(target: str, release: bool) -> int:
    if target == "server":
        rc = build_server(release)
        return rc if rc != 0 else start_server(release)
    if target == "wasm":
        return build_wasm(release)
    if target == "web":
        rc = stop_service("web")
        return rc if rc != 0 else start_web(release)

    # target == "all": rebuild server + wasm, ensure both server and web are running.
    rc = build_server(release)
    if rc != 0:
        return rc
    rc = build_wasm(release)
    if rc != 0:
        return rc
    rc = start_server(release)
    if rc != 0:
        return rc
    return start_web(release)


def cmd_logs(target: str, lines: int) -> int:
    if target == "wasm":
        print("wasm has no background log; rebuild output is printed directly.")
        return 0
    if target in ("all", "server"):
        print_tail("server.log", SERVER_LOG, lines)
    if target in ("all", "web"):
        print_tail("web.log", WEB_LOG, lines)
    return 0


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="Manage Faith & Fire local dev server (:3000), Angular (:4200), and WASM builds."
    )
    parser.add_argument(
        "command",
        nargs="?",
        default="status",
        choices=["status", "start", "stop", "restart", "rebuild", "update", "logs"],
        help="Action to perform (default: status).",
    )
    parser.add_argument(
        "target",
        nargs="?",
        default="all",
        choices=["all", "server", "web", "wasm"],
        help="Service or artifact target (default: all).",
    )
    parser.add_argument(
        "--release",
        action="store_true",
        help="Build/start release binaries without the editor feature.",
    )
    parser.add_argument(
        "-n",
        "--lines",
        type=int,
        default=40,
        help="Number of log lines to show for the `logs` command (default: 40).",
    )
    args = parser.parse_args(argv)

    if args.command == "status":
        return print_status(args.target)
    if args.command == "start":
        return cmd_start(args.target, args.release)
    if args.command == "stop":
        return cmd_stop(args.target)
    if args.command == "restart":
        return cmd_restart(args.target, args.release)
    if args.command in ("rebuild", "update"):
        return cmd_rebuild(args.target, args.release)
    if args.command == "logs":
        return cmd_logs(args.target, args.lines)
    return 1


if __name__ == "__main__":
    try:
        sys.exit(main())
    except Exception as exc:
        print(f"Error: {exc}", file=sys.stderr)
        sys.exit(1)
