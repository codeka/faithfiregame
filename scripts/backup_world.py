#!/usr/bin/env python3
"""Zip the Faith & Fire world directory, upload it to Google Drive via OAuth 2.0,
and prune older backups beyond the configured retention count.
"""

from __future__ import annotations

import argparse
import base64
import hashlib
import http.server
import json
import os
import re
import secrets
import sys
import tempfile
import time
import urllib.error
import urllib.parse
import urllib.request
import webbrowser
import zipfile
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

DEFAULT_DRIVE_FOLDER = "FaithAndFire/WorldBackups"
DEFAULT_KEEP = 10
DRIVE_SCOPE = "https://www.googleapis.com/auth/drive"
DPAPI_ENTROPY = b"FaithAndFire.GoogleDriveBackup.v1"
BACKUP_NAME_RE = re.compile(r"^faithfire-world-\d{8}-\d{6}Z?\.zip$")
TOKEN_ENDPOINT = "https://oauth2.googleapis.com/token"
AUTH_ENDPOINT = "https://accounts.google.com/o/oauth2/v2/auth"
DRIVE_FILES_ENDPOINT = "https://www.googleapis.com/drive/v3/files"
DRIVE_UPLOAD_ENDPOINT = "https://www.googleapis.com/upload/drive/v3/files"


def get_state_dir() -> Path:
    if os.name == "nt":
        base = Path(os.environ.get("LOCALAPPDATA") or (Path.home() / "AppData" / "Local"))
        return base / "FaithAndFire"
    if sys.platform == "darwin":
        return Path.home() / "Library" / "Application Support" / "FaithAndFire"
    base = Path(os.environ.get("XDG_CONFIG_HOME") or (Path.home() / ".config"))
    return base / "faithfire"


def ensure_state_dir(state_dir: Path | None = None) -> Path:
    target = state_dir or get_state_dir()
    target.mkdir(parents=True, exist_ok=True)
    if os.name != "nt":
        os.chmod(target, 0o700)
    return target


def get_config_path() -> Path:
    return get_state_dir() / "backup-config.json"


def get_token_path() -> Path:
    suffix = "gdrive-oauth.dpapi" if os.name == "nt" else "gdrive-oauth.json"
    return get_state_dir() / suffix


def load_backup_config() -> tuple[str, int]:
    drive_folder = DEFAULT_DRIVE_FOLDER
    keep = DEFAULT_KEEP
    cfg_path = get_config_path()
    if cfg_path.exists():
        raw = cfg_path.read_text(encoding="utf-8").strip()
        if raw:
            data = json.loads(raw)
            saved_folder = str(data.get("drive_folder", "") or "").strip()
            if saved_folder:
                drive_folder = saved_folder
            if data.get("keep") is not None:
                keep = int(data["keep"])
    return drive_folder, keep


def save_backup_config(drive_folder: str, keep: int) -> None:
    ensure_state_dir()
    payload = json.dumps({"drive_folder": drive_folder, "keep": keep}, indent=4) + "\n"
    get_config_path().write_text(payload, encoding="utf-8")


# --- Encrypted / restricted credential storage ---

if os.name == "nt":
    import ctypes
    from ctypes import wintypes

    class _DataBlob(ctypes.Structure):
        _fields_ = [
            ("cbData", wintypes.DWORD),
            ("pbData", ctypes.POINTER(ctypes.c_ubyte)),
        ]

    _crypt32 = ctypes.WinDLL("crypt32", use_last_error=True)
    _kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)

    _crypt32.CryptProtectData.argtypes = [
        ctypes.POINTER(_DataBlob),
        wintypes.LPCWSTR,
        ctypes.POINTER(_DataBlob),
        ctypes.c_void_p,
        ctypes.c_void_p,
        wintypes.DWORD,
        ctypes.POINTER(_DataBlob),
    ]
    _crypt32.CryptProtectData.restype = wintypes.BOOL

    _crypt32.CryptUnprotectData.argtypes = [
        ctypes.POINTER(_DataBlob),
        ctypes.c_void_p,
        ctypes.POINTER(_DataBlob),
        ctypes.c_void_p,
        ctypes.c_void_p,
        wintypes.DWORD,
        ctypes.POINTER(_DataBlob),
    ]
    _crypt32.CryptUnprotectData.restype = wintypes.BOOL

    _kernel32.LocalFree.argtypes = [ctypes.c_void_p]
    _kernel32.LocalFree.restype = ctypes.c_void_p

    _CRYPTPROTECT_UI_FORBIDDEN = 0x01

    def _make_blob(data: bytes) -> tuple[_DataBlob, Any]:
        if not data:
            return _DataBlob(0, None), None
        buf = ctypes.create_string_buffer(data, len(data))
        ptr = ctypes.cast(buf, ctypes.POINTER(ctypes.c_ubyte))
        return _DataBlob(len(data), ptr), buf

    def _dpapi_protect(plain: bytes) -> bytes:
        in_blob, _in_buf = _make_blob(plain)
        ent_blob, _ent_buf = _make_blob(DPAPI_ENTROPY)
        out_blob = _DataBlob()
        ok = _crypt32.CryptProtectData(
            ctypes.byref(in_blob),
            None,
            ctypes.byref(ent_blob),
            None,
            None,
            _CRYPTPROTECT_UI_FORBIDDEN,
            ctypes.byref(out_blob),
        )
        if not ok:
            raise ctypes.WinError(ctypes.get_last_error())
        try:
            return ctypes.string_at(out_blob.pbData, out_blob.cbData)
        finally:
            if out_blob.pbData:
                _kernel32.LocalFree(out_blob.pbData)

    def _dpapi_unprotect(cipher: bytes) -> bytes:
        in_blob, _in_buf = _make_blob(cipher)
        ent_blob, _ent_buf = _make_blob(DPAPI_ENTROPY)
        out_blob = _DataBlob()
        ok = _crypt32.CryptUnprotectData(
            ctypes.byref(in_blob),
            None,
            ctypes.byref(ent_blob),
            None,
            None,
            _CRYPTPROTECT_UI_FORBIDDEN,
            ctypes.byref(out_blob),
        )
        if not ok:
            raise ctypes.WinError(ctypes.get_last_error())
        try:
            return ctypes.string_at(out_blob.pbData, out_blob.cbData)
        finally:
            if out_blob.pbData:
                _kernel32.LocalFree(out_blob.pbData)


def save_encrypted_tokens(tokens: dict[str, Any], path: Path | None = None) -> Path:
    target = path or get_token_path()
    ensure_state_dir(target.parent)
    plain = json.dumps(tokens, indent=2).encode("utf-8")

    if os.name == "nt":
        target.write_bytes(_dpapi_protect(plain))
    else:
        fd = os.open(str(target), os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
        try:
            os.write(fd, plain)
        finally:
            os.close(fd)
        os.chmod(target, 0o600)
    return target


def load_encrypted_tokens(path: Path | None = None) -> dict[str, Any] | None:
    target = path or get_token_path()
    if not target.exists():
        return None
    raw = target.read_bytes()
    if not raw:
        return None
    try:
        if os.name == "nt":
            plain = _dpapi_unprotect(raw)
        else:
            plain = raw
        return json.loads(plain.decode("utf-8"))
    except Exception as exc:
        raise RuntimeError(
            f"Could not decrypt/parse stored OAuth credentials at '{target}'. "
            f"Re-run with --clear-auth and authorize again. Details: {exc}"
        ) from exc


# --- HTTP helpers ---

def _http_request(
    method: str,
    url: str,
    *,
    headers: dict[str, str] | None = None,
    data: Any = None,
    timeout: int = 120,
) -> tuple[int, dict[str, str], bytes]:
    req = urllib.request.Request(url, data=data, method=method)
    if headers:
        for key, value in headers.items():
            req.add_header(key, value)
    try:
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            resp_headers = {k.lower(): v for k, v in resp.headers.items()}
            return resp.status, resp_headers, resp.read()
    except urllib.error.HTTPError as err:
        err_body = err.read().decode("utf-8", errors="replace")
        raise RuntimeError(f"HTTP {err.code} ({err.reason}) from {method} {url}: {err_body}") from err


def invoke_http_json(
    method: str,
    url: str,
    *,
    access_token: str = "",
    json_body: Any = None,
    form_body: dict[str, str] | None = None,
) -> Any:
    headers: dict[str, str] = {}
    if access_token:
        headers["Authorization"] = f"Bearer {access_token}"

    data: bytes | None = None
    if json_body is not None:
        headers["Content-Type"] = "application/json; charset=utf-8"
        data = json.dumps(json_body).encode("utf-8")
    elif form_body is not None:
        headers["Content-Type"] = "application/x-www-form-urlencoded"
        data = urllib.parse.urlencode(form_body).encode("utf-8")

    _, _, raw = _http_request(method, url, headers=headers, data=data)
    if not raw or not raw.strip():
        return None
    return json.loads(raw.decode("utf-8"))


# --- OAuth 2.0 PKCE + Loopback ---

def _b64url(data: bytes) -> str:
    return base64.urlsafe_b64encode(data).rstrip(b"=").decode("ascii")


def resolve_oauth_client_credentials(
    secret_file: str,
    explicit_client_id: str,
    explicit_client_secret: str,
    example_drive_folder: str,
    stored_creds: dict[str, Any] | None,
) -> tuple[str, str]:
    if secret_file:
        path = Path(secret_file)
        if not path.exists():
            raise FileNotFoundError(f"Client secret file not found: '{secret_file}'")
        raw = json.loads(path.read_text(encoding="utf-8"))
        client_obj = raw.get("installed") or raw.get("web")
        if not isinstance(client_obj, dict):
            raise ValueError(
                f"Invalid Google OAuth JSON '{secret_file}': expected an 'installed' (Desktop app) object."
            )
        cid = str(client_obj.get("client_id", "")).strip()
        csec = str(client_obj.get("client_secret", "")).strip()
        if not cid or not csec:
            raise ValueError(f"Google OAuth JSON '{secret_file}' is missing client_id or client_secret.")
        return cid, csec

    if explicit_client_id and explicit_client_secret:
        return explicit_client_id.strip(), explicit_client_secret.strip()

    env_id = os.environ.get("FAITHFIRE_GDRIVE_CLIENT_ID", "").strip()
    env_sec = os.environ.get("FAITHFIRE_GDRIVE_CLIENT_SECRET", "").strip()
    if env_id and env_sec:
        return env_id, env_sec

    if stored_creds:
        saved_id = str(stored_creds.get("client_id", "")).strip()
        saved_sec = str(stored_creds.get("client_secret", "")).strip()
        if saved_id and saved_sec:
            return saved_id, saved_sec

    storage_note = (
        "encrypted under your Windows user profile via DPAPI"
        if os.name == "nt"
        else f"with 0600 permissions under '{get_token_path()}'"
    )
    raise RuntimeError(
        "No Google Drive OAuth client credentials are configured yet.\n\n"
        f"One-time setup (stored {storage_note}):\n"
        "  1. In Google Cloud Console, enable the \"Google Drive API\".\n"
        "  2. Configure the OAuth consent screen (External + add your Google account as a Test User, or Internal).\n"
        "  3. Create Credentials -> OAuth client ID -> Application type: \"Desktop app\", then download the JSON file.\n"
        "  4. Run once:\n"
        f"     python scripts/backup_world.py --client-secret-file \"path/to/client_secret.json\" --drive-folder \"{example_drive_folder}\"\n\n"
        "Subsequent runs only need:\n"
        "  python scripts/backup_world.py"
    )


def invoke_oauth_browser_flow(client_id: str, client_secret: str) -> dict[str, Any]:
    code_verifier = _b64url(secrets.token_bytes(32))
    code_challenge = _b64url(hashlib.sha256(code_verifier.encode("ascii")).digest())
    expected_state = _b64url(secrets.token_bytes(16))

    result: dict[str, str] = {}

    class _CallbackHandler(http.server.BaseHTTPRequestHandler):
        def do_GET(self) -> None:  # noqa: N802
            parsed = urllib.parse.urlsplit(self.path)
            if parsed.path != "/callback":
                self._write_html(404, "<html><body>Not found</body></html>")
                return

            q = urllib.parse.parse_qs(parsed.query, keep_blank_values=True)
            if "error" in q:
                result["error"] = q["error"][0]
                self._write_html(
                    400,
                    "<html><body><h2>Authorization failed</h2>"
                    "<p>You can close this tab and check the terminal.</p></body></html>",
                )
                return

            state = q.get("state", [""])[0]
            if not secrets.compare_digest(state, expected_state):
                result["error"] = "CSRF state mismatch on OAuth callback."
                self._write_html(400, "<html><body><h2>Invalid OAuth state</h2></body></html>")
                return

            code = q.get("code", [""])[0]
            if not code:
                result["error"] = "Google OAuth callback did not include an authorization code."
                self._write_html(400, "<html><body><h2>Missing authorization code</h2></body></html>")
                return

            result["code"] = code
            self._write_html(
                200,
                "<!doctype html><html><body style='font-family:system-ui,sans-serif;"
                "background:#11161f;color:#e8eef7;padding:40px'>"
                "<h2>Faith &amp; Fire World Backup Authorized</h2>"
                "<p>Google Drive credentials have been saved securely for this user account. "
                "You may close this tab.</p></body></html>",
            )

        def _write_html(self, status: int, body: str) -> None:
            encoded = body.encode("utf-8")
            self.send_response(status)
            self.send_header("Content-Type", "text/html; charset=utf-8")
            self.send_header("Content-Length", str(len(encoded)))
            self.send_header("Connection", "close")
            self.end_headers()
            self.wfile.write(encoded)

        def log_message(self, format: str, *args: Any) -> None:  # noqa: A002
            return

    server = http.server.HTTPServer(("127.0.0.1", 0), _CallbackHandler)
    server.timeout = 0.5
    try:
        port = server.server_address[1]
        redirect_uri = f"http://127.0.0.1:{port}/callback"
        params = urllib.parse.urlencode(
            {
                "client_id": client_id,
                "redirect_uri": redirect_uri,
                "response_type": "code",
                "scope": DRIVE_SCOPE,
                "access_type": "offline",
                "prompt": "consent",
                "state": expected_state,
                "code_challenge": code_challenge,
                "code_challenge_method": "S256",
            }
        )
        auth_url = f"{AUTH_ENDPOINT}?{params}"

        print("\nOpening browser for Google Drive OAuth authorization...")
        print("If the browser does not open automatically, visit:")
        print(f"  {auth_url}\n")
        webbrowser.open(auth_url)

        deadline = time.monotonic() + 180.0
        while "code" not in result and "error" not in result:
            if time.monotonic() > deadline:
                raise TimeoutError("Timed out after 3 minutes waiting for Google OAuth callback.")
            server.handle_request()
    finally:
        server.server_close()

    if "error" in result:
        raise RuntimeError(f"Google OAuth authorization failed: {result['error']}")

    token_resp = invoke_http_json(
        "POST",
        TOKEN_ENDPOINT,
        form_body={
            "client_id": client_id,
            "client_secret": client_secret,
            "code": result["code"],
            "code_verifier": code_verifier,
            "grant_type": "authorization_code",
            "redirect_uri": redirect_uri,
        },
    )
    refresh_token = str(token_resp.get("refresh_token", "")).strip()
    if not refresh_token:
        raise RuntimeError(
            "Google OAuth token response did not contain a refresh_token. "
            "Re-run with --reauth and approve offline access."
        )

    now_unix = int(time.time())
    expires_in = int(token_resp.get("expires_in", 3600))
    return {
        "client_id": client_id,
        "client_secret": client_secret,
        "access_token": str(token_resp.get("access_token", "")),
        "refresh_token": refresh_token,
        "expires_at_unix": now_unix + expires_in,
    }


def refresh_oauth_token(creds: dict[str, Any]) -> dict[str, Any]:
    token_resp = invoke_http_json(
        "POST",
        TOKEN_ENDPOINT,
        form_body={
            "client_id": str(creds["client_id"]),
            "client_secret": str(creds["client_secret"]),
            "refresh_token": str(creds["refresh_token"]),
            "grant_type": "refresh_token",
        },
    )
    now_unix = int(time.time())
    expires_in = int(token_resp.get("expires_in", 3600))
    new_refresh = str(token_resp.get("refresh_token", "")).strip() or str(creds["refresh_token"])
    return {
        "client_id": str(creds["client_id"]),
        "client_secret": str(creds["client_secret"]),
        "access_token": str(token_resp.get("access_token", "")),
        "refresh_token": new_refresh,
        "expires_at_unix": now_unix + expires_in,
    }


def get_drive_access_token(
    secret_file: str,
    explicit_client_id: str,
    explicit_client_secret: str,
    example_drive_folder: str,
    force_reauth: bool,
) -> str:
    stored = load_encrypted_tokens()
    has_custom_client = bool(secret_file) or bool(explicit_client_id and explicit_client_secret)

    if not force_reauth and not has_custom_client and stored:
        access_token = str(stored.get("access_token", "")).strip()
        refresh_token = str(stored.get("refresh_token", "")).strip()
        expires_at = int(stored.get("expires_at_unix", 0))
        if access_token and expires_at > int(time.time()) + 120:
            return access_token
        if refresh_token:
            try:
                refreshed = refresh_oauth_token(stored)
                save_encrypted_tokens(refreshed)
                return str(refreshed["access_token"])
            except Exception as exc:
                print(f"Warning: stored refresh token failed ({exc}). Re-authorizing...", file=sys.stderr)

    cid, csec = resolve_oauth_client_credentials(
        secret_file,
        explicit_client_id,
        explicit_client_secret,
        example_drive_folder,
        stored,
    )
    fresh = invoke_oauth_browser_flow(cid, csec)
    saved_path = save_encrypted_tokens(fresh)
    print(f"Saved Google Drive OAuth credentials -> {saved_path}")
    return str(fresh["access_token"])


# --- Google Drive folders, upload, listing & pruning ---

def _escape_drive_literal(value: str) -> str:
    return value.replace("\\", "\\\\").replace("'", "\\'")


def resolve_drive_folder_path(
    access_token: str,
    folder_path: str,
    *,
    create_missing: bool = False,
) -> str | None:
    segments: list[str] = []
    for raw in re.split(r"[/\\]+", folder_path):
        seg = raw.strip()
        if not seg or seg == ".":
            continue
        if seg == "..":
            raise ValueError(f"Invalid Google Drive folder segment '..' in path '{folder_path}'.")
        segments.append(seg)

    parent_id = "root"
    for seg in segments:
        escaped_name = _escape_drive_literal(seg)
        escaped_parent = _escape_drive_literal(parent_id)
        query = (
            f"name = '{escaped_name}' and '{escaped_parent}' in parents "
            "and mimeType = 'application/vnd.google-apps.folder' and trashed = false"
        )
        qs = urllib.parse.urlencode({"q": query, "fields": "files(id,name)", "pageSize": "10"})
        res = invoke_http_json("GET", f"{DRIVE_FILES_ENDPOINT}?{qs}", access_token=access_token)
        files = res.get("files", [])
        if files:
            parent_id = str(files[0]["id"])
            continue
        if not create_missing:
            return None

        created = invoke_http_json(
            "POST",
            f"{DRIVE_FILES_ENDPOINT}?fields=id,name",
            access_token=access_token,
            json_body={
                "name": seg,
                "mimeType": "application/vnd.google-apps.folder",
                "parents": [parent_id],
            },
        )
        parent_id = str(created["id"])
        print(f"Created Google Drive folder '{seg}' ({parent_id})")

    return parent_id


@dataclass
class ArchiveInfo:
    path: Path
    file_count: int
    size_bytes: int


def create_world_archive(source_dir: Path, zip_path: Path) -> ArchiveInfo:
    if not source_dir.exists():
        raise FileNotFoundError(f"World directory not found: '{source_dir}'")

    root = source_dir.resolve()
    files = sorted(p for p in root.rglob("*") if p.is_file())
    if not files:
        raise RuntimeError(f"World directory '{root}' contains no files to back up.")

    zip_path.parent.mkdir(parents=True, exist_ok=True)
    if zip_path.exists():
        zip_path.unlink()

    with zipfile.ZipFile(zip_path, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=6) as zf:
        for file_path in files:
            arcname = file_path.relative_to(root).as_posix()
            zf.write(file_path, arcname=arcname)

    return ArchiveInfo(path=zip_path, file_count=len(files), size_bytes=zip_path.stat().st_size)


def upload_backup_archive(
    access_token: str,
    folder_id: str,
    zip_path: Path,
    archive_name: str,
) -> dict[str, Any]:
    zip_size = zip_path.stat().st_size
    init_meta = json.dumps(
        {
            "name": archive_name,
            "parents": [folder_id],
            "appProperties": {"faithfireBackup": "true"},
        }
    ).encode("utf-8")

    init_url = f"{DRIVE_UPLOAD_ENDPOINT}?uploadType=resumable&fields=id,name,size,createdTime"
    _, init_headers, _ = _http_request(
        "POST",
        init_url,
        headers={
            "Authorization": f"Bearer {access_token}",
            "Content-Type": "application/json; charset=utf-8",
            "X-Upload-Content-Type": "application/zip",
            "X-Upload-Content-Length": str(zip_size),
        },
        data=init_meta,
    )

    session_uri = init_headers.get("location")
    if not session_uri:
        raise RuntimeError("Google Drive resumable upload response did not include a Location header.")

    with zip_path.open("rb") as f:
        _, _, put_body = _http_request(
            "PUT",
            session_uri,
            headers={
                "Content-Type": "application/zip",
                "Content-Length": str(zip_size),
            },
            data=f,
            timeout=900,
        )
    return json.loads(put_body.decode("utf-8"))


def list_drive_world_backups(access_token: str, folder_id: str) -> list[dict[str, Any]]:
    escaped_parent = _escape_drive_literal(folder_id)
    query = (
        f"'{escaped_parent}' in parents and trashed = false "
        "and appProperties has { key='faithfireBackup' and value='true' }"
    )
    results: list[dict[str, Any]] = []
    page_token = ""

    while True:
        params: dict[str, str] = {
            "q": query,
            "orderBy": "createdTime desc",
            "pageSize": "100",
            "fields": "nextPageToken,files(id,name,size,createdTime)",
        }
        if page_token:
            params["pageToken"] = page_token
        qs = urllib.parse.urlencode(params)
        res = invoke_http_json("GET", f"{DRIVE_FILES_ENDPOINT}?{qs}", access_token=access_token)
        for item in res.get("files", []):
            if BACKUP_NAME_RE.match(str(item.get("name", ""))):
                results.append(item)
        page_token = str(res.get("nextPageToken", "") or "")
        if not page_token:
            break

    return results


def prune_old_backups(access_token: str, folder_id: str, keep_count: int) -> int:
    if keep_count <= 0:
        return 0
    backups = list_drive_world_backups(access_token, folder_id)
    if len(backups) <= keep_count:
        return 0

    deleted = 0
    for old in backups[keep_count:]:
        file_id = str(old["id"])
        url = f"{DRIVE_FILES_ENDPOINT}/{urllib.parse.quote(file_id, safe='')}"
        invoke_http_json("DELETE", url, access_token=access_token)
        print(f"Pruned old backup: {old.get('name')} (id: {file_id})")
        deleted += 1
    return deleted


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Zip world/ and upload versioned backups to Google Drive via OAuth 2.0."
    )
    parser.add_argument(
        "--drive-folder",
        default="",
        help=(
            "Target folder path inside Google Drive 'My Drive' (e.g. 'FaithAndFire/WorldBackups'). "
            "Saved as your default when provided."
        ),
    )
    parser.add_argument(
        "--keep",
        type=int,
        default=-1,
        help="Number of newest backup archives to retain in Google Drive (0 disables pruning; default 10).",
    )
    parser.add_argument(
        "--world-dir",
        default="",
        help="Local world directory to archive (defaults to <repo>/world).",
    )
    parser.add_argument(
        "--client-secret-file",
        default="",
        help="Path to a Google Cloud OAuth 2.0 'Desktop app' client_secret JSON file.",
    )
    parser.add_argument("--client-id", default="", help="Google OAuth 2.0 Client ID.")
    parser.add_argument("--client-secret", default="", help="Google OAuth 2.0 Client Secret.")
    parser.add_argument(
        "--local-out",
        default="",
        help="Optional path to save/keep a local copy of the .zip archive.",
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="Create and inspect the local .zip archive without contacting Google Drive.",
    )
    parser.add_argument(
        "--list",
        action="store_true",
        help="List existing Faith & Fire world backups in the configured Google Drive folder.",
    )
    parser.add_argument(
        "--reauth",
        action="store_true",
        help="Force a fresh browser OAuth consent flow.",
    )
    parser.add_argument(
        "--clear-auth",
        action="store_true",
        help="Remove stored Google Drive OAuth credentials and exit.",
    )
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv)

    if args.clear_auth:
        state_dir = get_state_dir()
        removed = False
        for fname in ("gdrive-oauth.dpapi", "gdrive-oauth.json"):
            candidate = state_dir / fname
            if candidate.exists():
                candidate.unlink()
                print(f"Removed stored Google Drive OAuth credentials: {candidate}")
                removed = True
        if not removed:
            print(f"No stored Google Drive OAuth credentials found in: {state_dir}")
        return 0

    saved_folder, saved_keep = load_backup_config()
    effective_folder = args.drive_folder.strip() or saved_folder
    effective_keep = args.keep if args.keep >= 0 else saved_keep

    repo_root = Path(__file__).resolve().parent.parent
    world_dir = Path(args.world_dir) if args.world_dir else (repo_root / "world")

    if args.drive_folder.strip() or args.keep >= 0 or not get_config_path().exists():
        save_backup_config(effective_folder, effective_keep)

    if args.list:
        token = get_drive_access_token(
            args.client_secret_file,
            args.client_id,
            args.client_secret,
            effective_folder,
            args.reauth,
        )
        folder_id = resolve_drive_folder_path(token, effective_folder)
        if not folder_id:
            print(f"Google Drive folder '{effective_folder}' does not exist yet (0 backups).")
            return 0
        existing = list_drive_world_backups(token, folder_id)
        print(
            f"Google Drive backups in '{effective_folder}' "
            f"({len(existing)} total, keeping latest {effective_keep}):"
        )
        for item in existing:
            size_val = int(item.get("size", 0) or 0)
            print(
                f"  {item.get('name')}  |  {size_val:10d} bytes  |  "
                f"{item.get('createdTime')}  |  id={item.get('id')}"
            )
        return 0

    timestamp = datetime.now(timezone.utc).strftime("%Y%m%d-%H%M%SZ")
    archive_name = f"faithfire-world-{timestamp}.zip"
    keep_local = bool(args.local_out)
    zip_path = Path(args.local_out) if keep_local else (Path(tempfile.gettempdir()) / archive_name)

    try:
        if args.dry_run:
            info = create_world_archive(world_dir, zip_path)
            print(
                f"Archived {info.file_count} files from '{world_dir}' -> '{zip_path}' "
                f"({info.size_bytes / 1024.0:.2f} KB)"
            )
            print(
                f"[DryRun] Target Google Drive folder: '{effective_folder}' "
                f"| Retain latest: {effective_keep}"
            )
            print("[DryRun] Skipping Google Drive authentication and upload.")
            return 0

        token = get_drive_access_token(
            args.client_secret_file,
            args.client_id,
            args.client_secret,
            effective_folder,
            args.reauth,
        )
        info = create_world_archive(world_dir, zip_path)
        print(
            f"Archived {info.file_count} files from '{world_dir}' -> '{zip_path}' "
            f"({info.size_bytes / 1024.0:.2f} KB)"
        )

        folder_id = resolve_drive_folder_path(token, effective_folder, create_missing=True)
        assert folder_id is not None
        uploaded = upload_backup_archive(token, folder_id, zip_path, archive_name)
        uploaded_id = str(uploaded.get("id", "")).strip()
        if not uploaded_id:
            raise RuntimeError(
                "Upload completed without returning a Google Drive file ID; skipping retention pruning."
            )

        print(f"Uploaded '{uploaded.get('name')}' to Google Drive '{effective_folder}' (id: {uploaded_id})")
        pruned = prune_old_backups(token, folder_id, effective_keep)
        print(f"Backup complete (retention limit: {effective_keep}, pruned: {pruned}).")
        return 0
    finally:
        if not keep_local and zip_path.exists():
            zip_path.unlink(missing_ok=True)


if __name__ == "__main__":
    try:
        sys.exit(main())
    except Exception as exc:
        print(f"Error: {exc}", file=sys.stderr)
        sys.exit(1)
