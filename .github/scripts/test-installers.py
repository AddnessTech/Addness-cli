#!/usr/bin/env python3
"""Exercise the real installer with a native CLI and an isolated release server."""

import hashlib
import http.server
import io
import os
from pathlib import Path
import platform
import shutil
import subprocess
import sys
import tarfile
import tempfile
import threading
import zipfile


REPO = Path(__file__).resolve().parents[2]
WINDOWS = sys.platform == "win32"
BINARY_NAME = "addness.exe" if WINDOWS else "addness"


def main():
    binary = Path(sys.argv[1]).resolve()
    version_output = subprocess.check_output([str(binary), "--version"], text=True).strip()
    version = "v" + version_output.removeprefix("addness ")
    if WINDOWS:
        target = "x86_64-pc-windows-msvc"
    elif sys.platform == "darwin":
        arch = "aarch64" if platform.machine() == "arm64" else "x86_64"
        target = arch + "-apple-darwin"
    else:
        target = "x86_64-unknown-linux-gnu"
    archive_name = f"addness-{target}." + ("zip" if WINDOWS else "tar.gz")
    archive_data = io.BytesIO()
    if WINDOWS:
        with zipfile.ZipFile(archive_data, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=1) as archive:
            archive.write(binary, BINARY_NAME)
    else:
        with tarfile.open(fileobj=archive_data, mode="w:gz", compresslevel=1) as archive:
            archive.add(binary, arcname=BINARY_NAME)
    payload = archive_data.getvalue()
    digest = hashlib.sha256(payload).hexdigest()
    requests = []
    mode = "success"
    api_started = threading.Event()
    api_release = threading.Event()

    class Handler(http.server.BaseHTTPRequestHandler):
        def log_message(self, *_args):
            pass

        def do_GET(self):
            requests.append(self.path)
            status = 200
            if not self.path.startswith("/releases/"):
                # Keep the installed executable running while the installer replaces it.
                api_started.set()
                api_release.wait(30)
                body = b'{}'
            elif self.path == "/releases/latest/version.txt":
                status = 503 if mode == "missing-version" else 200
                metadata = "<html>unavailable</html>" if mode == "invalid-version" else version
                if mode == "wrong-version":
                    metadata = "v0.0.0"
                body = (metadata + "\n").encode()
            elif self.path.endswith(".sha256"):
                name = "unrelated-file" if mode == "bad-checksum-name" else archive_name
                body = f"{digest}  {name}\n".encode()
            elif self.path.endswith(archive_name):
                status = 503 if mode == "missing-archive" else 200
                body = b"corrupt" if mode == "corrupt-archive" else payload
            else:
                status, body = 404, b"not found"
            self.send_response(status)
            self.send_header("Content-Type", "text/plain" if self.path.endswith(".txt") else "application/octet-stream")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            try:
                self.wfile.write(body)
            except (BrokenPipeError, ConnectionResetError):
                pass

    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    worker = threading.Thread(target=server.serve_forever, daemon=True)
    worker.start()
    base_url = f"http://127.0.0.1:{server.server_port}"
    try:
        with tempfile.TemporaryDirectory(prefix="addness-install-tests-") as directory:
            root = Path(directory)
            cases = ["success", "pinned", "missing-version", "invalid-version", "missing-archive",
                     "corrupt-archive", "bad-checksum-name", "wrong-version", "running"]
            for case in cases:
                mode = case
                requests.clear()
                install_dir = root / case / "custom bin"
                install_dir.mkdir(parents=True)
                installed = install_dir / BINARY_NAME
                sentinel = b"previous installation must survive failed updates"
                installed.write_bytes(sentinel)
                environment = os.environ.copy()
                environment.update({
                    "ADDNESS_CDN_BASE": base_url,
                    "ADDNESS_INSTALL_DIR": str(install_dir),
                    "ADDNESS_NO_MODIFY_PATH": "1",
                    "ADDNESS_VERSION": version if case == "pinned" else "latest",
                })
                running = None
                if case == "running":
                    shutil.copy2(binary, installed)
                    environment.update({
                        "ADDNESS_API_URL": base_url,
                        "ADDNESS_API_TOKEN": "installer-test-token",
                        "ADDNESS_ORG_ID": "11111111-2222-4333-8444-555555555555",
                        "ADDNESS_CACHE_DIR": str(root / "api-cache"),
                    })
                    environment.pop("ADDNESS_API_RESOLVE", None)
                    running = subprocess.Popen([str(installed), "user", "me", "--json"],
                                               env=environment, stdout=subprocess.DEVNULL,
                                               stderr=subprocess.DEVNULL)
                    if not api_started.wait(10):
                        running.kill()
                        running.wait()
                        raise AssertionError("CLI did not reach the isolated API server")
                command = (["powershell", "-NoProfile", "-Command", (REPO / "install.ps1").read_text(encoding="utf-8")]
                           if WINDOWS else ["sh", str(REPO / "install.sh")])
                try:
                    result = subprocess.run(command, env=environment, text=True, encoding="utf-8", errors="replace",
                                            stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=60)
                    succeeds = case in ("success", "pinned", "running")
                    assert (result.returncode == 0) == succeeds, f"{case}: {result.stdout}"
                    if succeeds:
                        actual = subprocess.check_output([str(installed), "--version"], text=True).strip()
                        assert actual == version_output, (case, actual)
                        assert f"/releases/{version}/{archive_name}" in requests, requests
                        assert not any(f"/latest/{archive_name}" in request for request in requests), requests
                        if case == "pinned":
                            assert "/releases/latest/version.txt" not in requests
                    else:
                        assert installed.read_bytes() == sentinel, case
                        assert "installed successfully" not in result.stdout.lower(), result.stdout
                    if running:
                        assert running.poll() is None, "The old process must survive replacement"
                    print(f"installer: {case} passed", flush=True)
                finally:
                    if running:
                        running.terminate()
                        running.wait(timeout=10)
                        api_release.set()
    finally:
        api_release.set()
        server.shutdown()
        server.server_close()
        worker.join(timeout=5)


if __name__ == "__main__":
    main()
