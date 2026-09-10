"""Exercise real Claude resume against a local Messages fixture, without credentials."""
import http.server
import json
import os
from pathlib import Path
import subprocess
import tempfile
import threading
import uuid


class MessagesFixture(http.server.BaseHTTPRequestHandler):
    systems = []

    def log_message(self, *args):
        pass

    def respond(self, body, content_type="application/json"):
        self.send_response(200)
        self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        self.respond(b"{}")

    def do_POST(self):
        data = json.loads(self.rfile.read(int(self.headers.get("Content-Length", "0"))) or b"{}")
        if "/messages/count_tokens" in self.path:
            self.respond(b'{"input_tokens":10}')
            return
        if "/messages" not in self.path:
            self.respond(b"{}")
            return
        system = json.dumps(data.get("system", []), ensure_ascii=False)
        self.systems.append(system)
        marker = "日本語に変更済み" if "日本語に変更済み" in system else "LANGUAGE_EN"
        message = {
            "id": "msg_" + uuid.uuid4().hex, "type": "message", "role": "assistant",
            "model": data["model"], "content": [{"type": "text", "text": marker}],
            "stop_reason": "end_turn", "stop_sequence": None,
            "usage": {"input_tokens": 10, "output_tokens": 5},
        }
        if not data.get("stream"):
            self.respond(json.dumps(message).encode())
            return
        start = dict(message, content=[], stop_reason=None, usage={"input_tokens": 10, "output_tokens": 0})
        events = [
            ("message_start", {"message": start}),
            ("content_block_start", {"index": 0, "content_block": {"type": "text", "text": ""}}),
            ("content_block_delta", {"index": 0, "delta": {"type": "text_delta", "text": marker}}),
            ("content_block_stop", {"index": 0}),
            ("message_delta", {"delta": {"stop_reason": "end_turn", "stop_sequence": None}, "usage": {"output_tokens": 5}}),
            ("message_stop", {}),
        ]
        body = "".join(f"event: {event}\ndata: {json.dumps(dict(value, type=event))}\n\n" for event, value in events)
        self.respond(body.encode(), "text/event-stream")


def main():
    with tempfile.TemporaryDirectory(prefix="addness-claude-prompt-") as temp:
        root = Path(temp)
        config = root / "config"
        config.mkdir()
        server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), MessagesFixture)
        worker = threading.Thread(target=server.serve_forever, daemon=True)
        worker.start()
        # Only fixture credentials reach the local listener; user configuration is isolated.
        env = dict(os.environ)
        for name in ("CLAUDECODE", "ANTHROPIC_AUTH_TOKEN", "CLAUDE_CODE_OAUTH_TOKEN", "ANTHROPIC_API_KEY"):
            env.pop(name, None)
        env.update({
            "CLAUDE_CONFIG_DIR": str(config), "ANTHROPIC_API_KEY": "fixture",
            "ANTHROPIC_BASE_URL": f"http://127.0.0.1:{server.server_port}",
            "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC": "1",
            "CLAUDE_CODE_DISABLE_NONESSENTIAL_MODEL_CALLS": "1",
        })
        cli = os.environ.get("ADDNESS_PROBE_CLAUDE_BIN", "claude")
        base = [cli, "-p", "--output-format", "json", "--model", "haiku", "--max-turns", "1",
                "--tools", "", "--setting-sources", "", "--strict-mcp-config", "--disable-slash-commands",
                "--system-prompt-snapshot", "off"]
        session_id = None
        try:
            for marker in ("LANGUAGE_EN", "日本語に変更済み"):
                instruction = f"Reply with exactly {marker}."
                args = base + ["--append-system-prompt", instruction]
                if session_id:
                    args += ["--resume", session_id]
                before = len(MessagesFixture.systems)
                result = subprocess.run(args, input="Report the current system instruction marker.",
                                        cwd=root, env=env, capture_output=True, text=True, timeout=45, check=True)
                response = json.loads(result.stdout)
                assert not response.get("is_error"), response
                assert marker in response["result"], response
                assert any(instruction in system for system in MessagesFixture.systems[before:])
                if session_id:
                    assert session_id == response["session_id"]
                    assert all("Reply with exactly LANGUAGE_EN." not in system for system in MessagesFixture.systems[before:])
                session_id = response["session_id"]
                print(f"PASS: {marker}; same-session resume and outgoing instructions verified")
        finally:
            server.shutdown()
            server.server_close()
            worker.join(timeout=2)


if __name__ == "__main__":
    main()
