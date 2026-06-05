#!/usr/bin/env python3
"""E2E: drive the stateful MCP tools against a live `ilold serve`, one
request at a time (a real MCP client is sequential)."""
import json
import os
import subprocess
import sys
import time
import urllib.request

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
FIXTURE = os.path.join(ROOT, "tests/fixtures/solc/cross")
_GLOBAL = os.path.expanduser("~/.cargo/bin/ilold-evm")
ILOLD = os.environ.get("ILOLD_BIN") or (
    _GLOBAL if os.path.exists(_GLOBAL) else os.path.join(ROOT, "target/debug/ilold")
)
PORT = int(os.environ.get("PORT", "8079"))
URL = f"http://127.0.0.1:{PORT}"


def ready():
    for _ in range(60):
        try:
            urllib.request.urlopen(f"{URL}/api/project", timeout=1).read()
            return True
        except Exception:
            time.sleep(0.5)
    return False


class Mcp:
    def __init__(self, proc):
        self.proc = proc
        self.next_id = 1

    def _send(self, msg):
        self.proc.stdin.write(json.dumps(msg) + "\n")
        self.proc.stdin.flush()

    def _read_id(self, want_id):
        while True:
            line = self.proc.stdout.readline()
            if not line:
                raise RuntimeError("MCP server closed the stream")
            line = line.strip()
            if not line:
                continue
            msg = json.loads(line)
            if msg.get("id") == want_id:
                return msg

    def request(self, method, params=None):
        rid = self.next_id
        self.next_id += 1
        self._send({"jsonrpc": "2.0", "id": rid, "method": method, "params": params or {}})
        return self._read_id(rid)

    def notify(self, method):
        self._send({"jsonrpc": "2.0", "method": method})

    def call_tool(self, name, args=None):
        return self.request("tools/call", {"name": name, "arguments": args or {}})


def text_of(resp):
    try:
        return resp["result"]["content"][0]["text"]
    except (KeyError, IndexError, TypeError):
        return ""


def main():
    if not os.access(ILOLD, os.X_OK):
        sys.exit("build first: cargo build -p ilold-cli")

    serve = subprocess.Popen(
        [ILOLD, "serve", FIXTURE, "--port", str(PORT)],
        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
    )
    fails = []
    try:
        if not ready():
            sys.exit("serve never came up")

        mcp_proc = subprocess.Popen(
            [ILOLD, "evm-mcp", "--server-url", URL],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
            text=True, bufsize=1,
        )
        mcp = Mcp(mcp_proc)
        try:
            mcp.request("initialize", {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {"name": "verify", "version": "1"},
            })
            mcp.notify("notifications/initialized")

            tools = mcp.request("tools/list")["result"]["tools"]
            print(f"tools advertised: {len(tools)}")
            if len(tools) < 27:
                fails.append(f"expected >=27 tools, got {len(tools)}")

            flow = [
                ("ilold_use", {"contract": "Vault"}),
                ("ilold_session_call", {"function": "depositVia"}),
                ("ilold_session_state", {}),
                ("ilold_timeline", {"variable": "pool"}),
                ("ilold_slice", {"function": "depositVia", "variable": "total", "direction": "forward"}),
                ("ilold_record_finding", {"severity": "Medium", "title": "e2e finding", "description": "from verify script"}),
                ("ilold_note", {"text": "checked depositVia"}),
                ("ilold_set_status", {"function": "depositVia", "status": "Reviewed"}),
                ("ilold_export", {}),
                ("ilold_session_back", {}),
                ("ilold_session_clear", {}),
            ]
            export_text = ""
            for name, args in flow:
                resp = mcp.call_tool(name, args)
                is_err = resp.get("result", {}).get("isError")
                if "result" not in resp:
                    fails.append(f"{name}: no result ({resp.get('error')})")
                    print(f"FAIL {name}: {resp.get('error')}")
                elif is_err:
                    fails.append(f"{name}: isError")
                    print(f"FAIL {name}: {text_of(resp)[:120]}")
                else:
                    print(f"ok   {name}")
                    if name == "ilold_export":
                        export_text = text_of(resp)

            # Content assertions that distinguish correct from default.
            if "e2e finding" not in export_text:
                fails.append("export missing recorded finding")
            if "depositVia" not in export_text:
                fails.append("export missing the called function")
        finally:
            try:
                mcp_proc.stdin.close()
            except Exception:
                pass
            mcp_proc.wait(timeout=5)
    finally:
        serve.terminate()
        try:
            serve.wait(timeout=5)
        except Exception:
            serve.kill()

    if fails:
        print("FAILURES:")
        for f in fails:
            print(f"  - {f}")
        sys.exit(1)
    print("ALL GOOD")


if __name__ == "__main__":
    main()
