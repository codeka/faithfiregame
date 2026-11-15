#!/usr/bin/env python3
"""Build the client-wasm WebAssembly bundle and copy it into the Angular frontend."""

from __future__ import annotations

import argparse
import shutil
import subprocess
import sys
from pathlib import Path


def resolve_tool(name: str) -> str:
    path = shutil.which(name)
    if not path:
        raise RuntimeError(f"Required tool '{name}' was not found on PATH.")
    return path


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="Build the Faith & Fire client-wasm crate and run wasm-bindgen."
    )
    parser.add_argument(
        "--release",
        action="store_true",
        help="Build optimized release WASM without the editor feature.",
    )
    args = parser.parse_args(argv)

    repo_root = Path(__file__).resolve().parent.parent
    cargo = resolve_tool("cargo")
    wasm_bindgen = resolve_tool("wasm-bindgen")

    if args.release:
        print("Building client-wasm (RELEASE, no editor)...")
        cargo_cmd = [
            cargo,
            "build",
            "-p",
            "client-wasm",
            "--target",
            "wasm32-unknown-unknown",
            "--release",
        ]
        wasm_path = (
            repo_root / "target" / "wasm32-unknown-unknown" / "release" / "client_wasm.wasm"
        )
    else:
        print("Building client-wasm (DEV + editor)...")
        cargo_cmd = [
            cargo,
            "build",
            "-p",
            "client-wasm",
            "--target",
            "wasm32-unknown-unknown",
            "--features",
            "editor",
        ]
        wasm_path = (
            repo_root / "target" / "wasm32-unknown-unknown" / "debug" / "client_wasm.wasm"
        )

    res = subprocess.run(cargo_cmd, cwd=repo_root)
    if res.returncode != 0:
        return res.returncode

    js_out_dir = repo_root / "web" / "src" / "app" / "wasm"
    public_wasm_dir = repo_root / "web" / "public" / "wasm"
    js_out_dir.mkdir(parents=True, exist_ok=True)
    public_wasm_dir.mkdir(parents=True, exist_ok=True)

    print("Running wasm-bindgen...")
    bindgen_cmd = [
        wasm_bindgen,
        "--target",
        "web",
        "--out-dir",
        str(js_out_dir),
        str(wasm_path),
    ]
    res = subprocess.run(bindgen_cmd, cwd=repo_root)
    if res.returncode != 0:
        return res.returncode

    dest_wasm = public_wasm_dir / "client_wasm_bg.wasm"
    shutil.copy2(js_out_dir / "client_wasm_bg.wasm", dest_wasm)
    print(f"WASM build ready -> {dest_wasm}")
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except Exception as exc:
        print(f"Error: {exc}", file=sys.stderr)
        sys.exit(1)
