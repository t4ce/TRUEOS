#!/usr/bin/env python3
"""Exercise a real Codex plugin through the public app-server protocol.

Host example:
  python3 tools/testpy/codex_plugin_smoke.py --binary /path/to/codex

The test owns a temporary CODEX_HOME and workspace, installs a local plugin,
discovers its skill and HTTP MCP tool, calls that tool, then uninstalls it.
It never uses a model, API key, user credentials, or the user's configuration.
An optional --output directory preserves the fixture, RPC trace and report.

Native TRUEOS: run --prepare --output DIR --bind 0.0.0.0
--advertise-host HOST_LAN_IP --native-workspace /native/workspace, copy
DIR/workspace and DIR/codex-home to an isolated native filesystem, and launch
codex with CODEX_HOME pointing at that copy and arguments
  app-server --listen ws://0.0.0.0:PORT
Then run --endpoint ws://RIG:PORT --output DIR --workspace /native/workspace
--bind 0.0.0.0 --advertise-host HOST_LAN_IP. This starts the MCP fixture again
at the saved port. --endpoint requires the Python websockets package.
"""

import argparse
import asyncio
from collections import Counter
from contextlib import asynccontextmanager
import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import tempfile
import threading
import time


PLUGIN = "trueos-smoke"
MARKETPLACE = "trueos-smoke-marketplace"
PLUGIN_ID = f"{PLUGIN}@{MARKETPLACE}"
SKILL = "verify-plugin"
TOOL = "echo_plugin"
TOKEN = "codex-trueos-plugin-roundtrip"


def write_json(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2) + "\n")


class McpFixture(ThreadingHTTPServer):
    daemon_threads = True

    def __init__(self, host, port, trace):
        self.calls = Counter()
        self.trace = trace
        self.lock = threading.Lock()
        super().__init__((host, port), McpHandler)


class McpHandler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *args):
        pass

    def reply(self, status, value=None):
        body = b"" if value is None else json.dumps(value).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        # MCP's optional server notification stream is intentionally absent.
        self.reply(405)

    def do_DELETE(self):
        self.reply(200)

    def do_POST(self):
        try:
            request = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
            method = request.get("method")
            with self.server.lock:
                self.server.calls[method] += 1
                with self.server.trace.open("a") as trace:
                    trace.write(json.dumps(request) + "\n")
            if "id" not in request:
                self.reply(202)
                return
            if method == "initialize":
                result = {
                    "protocolVersion": request["params"]["protocolVersion"],
                    "capabilities": {"tools": {}},
                    "serverInfo": {"name": "trueos-plugin-smoke", "version": "1.0.0"},
                }
            elif method == "tools/list":
                result = {"tools": [{
                    "name": TOOL,
                    "description": "Return the smoke-test token unchanged.",
                    "inputSchema": {
                        "type": "object", "properties": {"token": {"type": "string"}},
                        "required": ["token"], "additionalProperties": False,
                    },
                    "annotations": {"readOnlyHint": True, "destructiveHint": False},
                }]}
            elif method == "tools/call":
                assert request["params"]["name"] == TOOL, request
                token = request["params"]["arguments"]["token"]
                assert token == TOKEN, request
                result = {
                    "content": [{"type": "text", "text": token}],
                    "structuredContent": {"token": token, "plugin": PLUGIN_ID},
                    "isError": False,
                }
            elif method == "resources/list":
                result = {"resources": []}
            elif method == "resources/templates/list":
                result = {"resourceTemplates": []}
            elif method == "ping":
                result = {}
            else:
                self.reply(200, {"jsonrpc": "2.0", "id": request["id"],
                                 "error": {"code": -32601, "message": "Unknown method"}})
                return
            self.reply(200, {"jsonrpc": "2.0", "id": request["id"], "result": result})
        except (AssertionError, KeyError, TypeError, ValueError) as error:
            self.reply(400, {"error": str(error)})


def prepare(directory, mcp_url, workspace):
    local_workspace = directory / "workspace"
    codex_home = directory / "codex-home"
    codex_home.mkdir(parents=True, exist_ok=True)
    (local_workspace / ".git").mkdir(parents=True, exist_ok=True)
    write_json(local_workspace / ".agents/plugins/marketplace.json", {
        "name": MARKETPLACE,
        "plugins": [{"name": PLUGIN, "source": {"source": "local", "path": f"./{PLUGIN}"},
                     "policy": {"authentication": "ON_USE"}}],
    })
    plugin_root = local_workspace / PLUGIN
    write_json(plugin_root / ".codex-plugin/plugin.json", {
        "name": PLUGIN, "version": "1.0.0", "description": "Isolated TRUEOS plugin integration smoke test.",
    })
    write_json(plugin_root / ".mcp.json", {"mcpServers": {
        "smoke-http": {"type": "http", "url": mcp_url},
    }})
    skill_path = plugin_root / "skills" / SKILL / "SKILL.md"
    skill_path.parent.mkdir(parents=True, exist_ok=True)
    skill_path.write_text(
        f"---\nname: {SKILL}\ndescription: Verify the isolated TRUEOS plugin fixture.\n---\n\n"
        f"Call `{TOOL}` with the supplied token and report the exact token returned.\n"
    )
    # The provider is never invoked: thread creation supplies an MCP runtime,
    # and the explicit tool RPC does not submit a model turn.
    (codex_home / "config.toml").write_text(
        'model = "trueos-plugin-smoke"\nmodel_provider = "smoke"\n'
        'check_for_update_on_startup = false\napproval_policy = "never"\n'
        'sandbox_mode = "danger-full-access"\ncli_auth_credentials_store = "file"\n'
        'mcp_oauth_credentials_store = "file"\n'
        f'chatgpt_base_url = {json.dumps(mcp_url.rsplit("/", 1)[0])}\n'
        '[model_providers.smoke]\nname = "Isolated smoke fixture"\n'
        f'base_url = {json.dumps(mcp_url.rsplit("/", 1)[0] + "/v1")}\n'
        'wire_api = "responses"\nrequires_openai_auth = false\n'
        '[features]\nplugins = true\nremote_plugin = false\nshell_snapshot = false\n'
        f'[projects.{json.dumps(workspace)}]\ntrust_level = "trusted"\n'
    )


class Rpc:
    def __init__(self, send, receive, trace, timeout):
        self.send = send
        self.receive = receive
        self.trace = trace
        self.timeout = timeout
        self.next_id = 0

    def record(self, direction, message):
        with self.trace.open("a") as trace:
            trace.write(json.dumps({"direction": direction, "message": message}) + "\n")

    async def notify(self, method, params=None):
        request = {"method": method}
        if params is not None:
            request["params"] = params
        self.record("send", request)
        await self.send(json.dumps(request))

    async def request(self, method, params):
        self.next_id += 1
        request = {"id": self.next_id, "method": method, "params": params}
        self.record("send", request)
        await self.send(json.dumps(request))
        async with asyncio.timeout(self.timeout):
            while True:
                message = json.loads(await self.receive())
                self.record("receive", message)
                if message.get("id") != request["id"]:
                    assert "id" not in message or "method" not in message, message
                    continue
                if "error" in message:
                    raise RuntimeError(f"{method}: {message['error']}")
                return message["result"]


@asynccontextmanager
async def connection(args, directory):
    trace = directory / "app-server-rpc.jsonl"
    if args.endpoint:
        from websockets.asyncio.client import connect
        async with connect(args.endpoint, max_size=16 * 1024 * 1024, proxy=None) as socket:
            yield Rpc(socket.send, socket.recv, trace, args.timeout)
        return
    # Explicit env overrides apply only to this child and prevent credentials or
    # proxy settings inherited from the IDE from entering the isolated fixture.
    env = {key: value for key, value in os.environ.items()
           if not key.startswith(("OPENAI_", "CODEX_")) and key.upper() not in
           {"HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY", "NO_PROXY"}}
    env.update(CODEX_HOME=str(directory / "codex-home"), NO_PROXY="*",
               XDG_CONFIG_HOME=str(directory / "xdg"),
               XDG_CACHE_HOME=str(directory / "cache"))
    with (directory / "app-server.stderr").open("wb") as stderr:
        process = await asyncio.create_subprocess_exec(
            str(Path(args.binary).resolve()), "app-server", "--listen", "stdio://",
            cwd=directory / "workspace", env=env,
            stdin=asyncio.subprocess.PIPE, stdout=asyncio.subprocess.PIPE, stderr=stderr,
        )

        async def send(message):
            process.stdin.write((message + "\n").encode())
            await process.stdin.drain()

        async def receive():
            line = await process.stdout.readline()
            if not line:
                raise RuntimeError(f"app-server exited; see {directory / 'app-server.stderr'}")
            return line

        try:
            yield Rpc(send, receive, trace, args.timeout)
        finally:
            process.stdin.close()
            try:
                await asyncio.wait_for(process.wait(), timeout=10)
            except TimeoutError:
                process.terminate()
                await process.wait()


def find_skill(response):
    return [skill for entry in response["data"] for skill in entry["skills"]
            if skill.get("pluginId") == PLUGIN_ID and skill["name"].split(":")[-1] == SKILL]


async def exercise(args, directory, workspace):
    manifest = workspace.rstrip("/") + "/.agents/plugins/marketplace.json"
    checks = []
    async with connection(args, directory) as rpc:
        initialized = await rpc.request("initialize", {
            "clientInfo": {"name": "trueos_plugin_smoke", "version": "1.0.0"},
            "capabilities": {"experimentalApi": True},
        })
        await rpc.notify("initialized")
        listed = await rpc.request("plugin/list", {"cwds": [workspace], "marketplaceKinds": ["local"]})
        plugins = [plugin for market in listed["marketplaces"] for plugin in market["plugins"]
                   if plugin["id"] == PLUGIN_ID]
        assert len(plugins) == 1 and not plugins[0]["installed"], listed
        checks.append("local marketplace discovery")
        await rpc.request("plugin/install", {"marketplacePath": manifest, "pluginName": PLUGIN})
        detail = (await rpc.request("plugin/read", {"marketplacePath": manifest, "pluginName": PLUGIN}))["plugin"]
        assert detail["summary"]["installed"] and detail["summary"]["enabled"], detail
        assert detail["mcpServers"], detail
        checks.append("plugin installation and enabled manifest")
        skills = await rpc.request("skills/list", {"cwds": [workspace], "forceReload": True})
        assert len(find_skill(skills)) == 1, skills
        checks.append("installed plugin skill discovery")
        thread = await rpc.request("thread/start", {
            "cwd": workspace, "model": "trueos-plugin-smoke", "modelProvider": "smoke",
            "approvalPolicy": "never", "sandbox": "danger-full-access", "ephemeral": True,
        })
        thread_id = thread["thread"]["id"]
        status = await rpc.request("mcpServerStatus/list", {"threadId": thread_id})
        servers = [server for server in status["data"] if server.get("pluginId") == PLUGIN_ID]
        assert len(servers) == 1 and TOOL in servers[0]["tools"], status
        checks.append("plugin MCP initialization and tool discovery")
        result = await rpc.request("mcpServer/tool/call", {
            "threadId": thread_id, "server": servers[0]["name"], "tool": TOOL,
            "arguments": {"token": TOKEN},
        })
        assert not result.get("isError"), result
        assert any(block.get("text") == TOKEN for block in result["content"]), result
        assert result.get("structuredContent") == {"token": TOKEN, "plugin": PLUGIN_ID}, result
        checks.append("plugin MCP tool call and exact response")
        await rpc.request("plugin/uninstall", {"pluginId": PLUGIN_ID})
        skills = await rpc.request("skills/list", {"cwds": [workspace], "forceReload": True})
        assert not find_skill(skills), skills
        detail = (await rpc.request("plugin/read", {"marketplacePath": manifest, "pluginName": PLUGIN}))["plugin"]
        assert not detail["summary"]["installed"], detail
        checks.append("plugin uninstall removes installed skill")
    return {"status": "PASS", "target": args.endpoint or str(Path(args.binary).resolve()),
            "initialize": initialized, "checks": checks}


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--binary", help="Host codex executable built from the source under test")
    mode.add_argument("--endpoint", help="Existing isolated app-server WebSocket endpoint")
    mode.add_argument("--prepare", action="store_true", help="Only create fixtures for a native run")
    parser.add_argument("--output", type=Path, help="Preserve fixtures, traces and report here")
    parser.add_argument("--workspace", help="Native workspace absolute path (endpoint mode)")
    parser.add_argument("--native-workspace", help="Native workspace path to trust in prepared config")
    parser.add_argument("--bind", default="127.0.0.1", help="HTTP fixture bind address")
    parser.add_argument("--advertise-host", default="127.0.0.1", help="HTTP fixture host reachable by Codex")
    parser.add_argument("--port", type=int, default=0, help="HTTP fixture port (default: auto/saved)")
    parser.add_argument("--timeout", type=float, default=60, help="Per-RPC deadline in seconds")
    args = parser.parse_args()
    if (args.prepare or args.endpoint) and args.output is None:
        parser.error("--prepare and --endpoint require --output")
    if args.endpoint and not args.workspace:
        parser.error("--endpoint requires --workspace")
    temporary = tempfile.TemporaryDirectory(prefix="codex-plugin-smoke-") if args.output is None else None
    directory = args.output.resolve() if args.output else Path(temporary.name)
    directory.mkdir(parents=True, exist_ok=True)
    fixture_path = directory / "fixture.json"
    saved = json.loads(fixture_path.read_text()) if args.endpoint else {}
    port = args.port or saved.get("port", 0)
    server = McpFixture(args.bind, port, directory / "mcp-rpc.jsonl")
    fixture_url = f"http://{args.advertise_host}:{server.server_port}/mcp"
    workspace = args.workspace or args.native_workspace or str(directory / "workspace")
    if not args.endpoint:
        prepare(directory, fixture_url, workspace)
        write_json(fixture_path, {"port": server.server_port, "url": fixture_url, "workspace": workspace})
    if args.prepare:
        server.server_close()
        print(json.dumps({"status": "PREPARED", "directory": str(directory), "fixtureUrl": fixture_url}))
        return
    threading.Thread(target=server.serve_forever, daemon=True).start()
    started = time.monotonic()
    try:
        report = asyncio.run(exercise(args, directory, workspace))
        assert server.calls["initialize"] >= 1, server.calls
        assert server.calls["tools/list"] >= 1, server.calls
        assert server.calls["tools/call"] == 1, server.calls
        if args.binary:
            with open(args.binary, "rb") as binary:
                report["binarySha256"] = hashlib.file_digest(binary, "sha256").hexdigest()
    except Exception as error:
        report = {"status": "FAIL", "error": str(error), "errorType": type(error).__name__}
    finally:
        server.shutdown()
        server.server_close()
    report.update(elapsedSeconds=round(time.monotonic() - started, 3),
                  mcpCalls=dict(server.calls), artifacts=str(directory) if args.output else None)
    write_json(directory / "report.json", report)
    print(json.dumps(report, indent=2))
    if temporary:
        temporary.cleanup()
    raise SystemExit(0 if report["status"] == "PASS" else 1)


if __name__ == "__main__":
    main()
