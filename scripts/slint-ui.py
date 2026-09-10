#!/usr/bin/env python3
"""Control a live Slint dev window through its loopback-only MCP server."""

import argparse
import base64
import json
import os
import socket
from pathlib import Path
import subprocess
import sys
import urllib.error
import urllib.request

ROOT = Path(__file__).resolve().parent.parent
KEYS = {
    "up": "\uf700", "down": "\uf701", "left": "\uf702", "right": "\uf703",
    "enter": "\n", "return": "\n", "escape": "\x1b", "backspace": "\b",
    "tab": "\t", "space": " ", "pageup": "\uf72c", "pagedown": "\uf72d",
}


class Client:
    def __init__(self, port):
        self.url = f"http://127.0.0.1:{port}/mcp"
        # Never route the local control channel through HTTP_PROXY.
        self.http = urllib.request.build_opener(urllib.request.ProxyHandler({}))
        self.request("initialize", {
            "protocolVersion": "2025-06-18", "capabilities": {},
            "clientInfo": {"name": "zaparoo-slint-ui", "version": "1"},
        })

    def request(self, method, params):
        body = json.dumps({"jsonrpc": "2.0", "id": 1, "method": method,
                           "params": params}).encode()
        request = urllib.request.Request(self.url, data=body,
                                         headers={"Content-Type": "application/json"})
        with self.http.open(request, timeout=30) as response:
            result = json.load(response)
        if "error" in result:
            raise RuntimeError(str(result["error"]))
        return result["result"]

    def call(self, name, arguments):
        result = self.request("tools/call", {"name": name, "arguments": arguments})
        if result.get("isError"):
            raise RuntimeError(str(result.get("content")))
        return result

    def data(self, name, arguments):
        result = self.call(name, arguments)
        return json.loads(next(block["text"] for block in result["content"]
                               if block["type"] == "text"))

    def window(self):
        windows = self.data("list_windows", {}).get("windowHandles", [])
        if len(windows) != 1:
            raise RuntimeError(f"Expected one app window, found {len(windows)}; use call for explicit handles")
        return windows[0]


def run(port, headless, mock):
    with socket.socket() as probe:
        probe.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        probe.bind(("127.0.0.1", port))
    env = os.environ.copy()
    root = ROOT / "output" / "slint-ui"
    # Keep test settings, navigation state, and logs away from normal installs.
    for name, child in [("XDG_CONFIG_HOME", "config"), ("XDG_DATA_HOME", "data"),
                        ("XDG_CACHE_HOME", "cache")]:
        path = root / child
        path.mkdir(parents=True, exist_ok=True)
        env[name] = str(path)
    env["ZAPAROO_STATE_FILE"] = str(root / "state.toml")
    if mock:
        env.pop("ZAPAROO_CORE_ENDPOINT", None)
        core = "mock Core on ws://127.0.0.1:27497/api/v0.1"
    else:
        env["ZAPAROO_CORE_ENDPOINT"] = "ws://127.0.0.1:7497/api/v0.1"
        core = "local Core on ws://127.0.0.1:7497/api/v0.1"
    env["SLINT_MCP_PORT"] = str(port)
    if headless:
        env["SLINT_BACKEND"] = "headless"
    else:
        env.setdefault("SLINT_BACKEND", "winit-software")
    print(f"MCP: http://127.0.0.1:{port}/mcp; {core}; state: {root}", flush=True)
    # Stay in the foreground: Ctrl+C reaches the existing dev script and
    # its mock cleanup. No PID files or orphaned background supervisors.
    return subprocess.call(["just", "slint-run-dev"], cwd=ROOT, env=env)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--port", type=int, default=38080)
    sub = parser.add_subparsers(dest="command", required=True)
    launch = sub.add_parser("run", help="Run isolated dev app; Ctrl+C stops it")
    launch.add_argument("--headless", action="store_true")
    launch.add_argument("--mock", action="store_true", help="Use managed mock Core instead of local Core")
    sub.add_parser("windows", help="List window handles")
    sub.add_parser("tools", help="List full MCP tool schemas")
    tree = sub.add_parser("tree", help="Inspect live UI element tree")
    tree.add_argument("--limit", type=int, default=50)
    shot = sub.add_parser("screenshot", help="Save app-window PNG, not desktop")
    shot.add_argument("path", type=Path)
    key = sub.add_parser("key", help="Send one named key or literal character")
    key.add_argument("key")
    click = sub.add_parser("click", help="Click one qualified element ID")
    click.add_argument("element_id", help="For example App::button; discover via tree")
    call = sub.add_parser("call", help="Call any Slint MCP tool with JSON arguments")
    call.add_argument("tool")
    call.add_argument("arguments", nargs="?", default="{}")
    args = parser.parse_args()
    if not 1 <= args.port <= 65535:
        parser.error("port must be between 1 and 65535")
    if args.command == "run":
        return run(args.port, args.headless, args.mock)
    client = Client(args.port)
    if args.command == "tools":
        result = client.request("tools/list", {})
    elif args.command == "windows":
        result = client.data("list_windows", {})
    elif args.command == "call":
        result = client.call(args.tool, json.loads(args.arguments))
    else:
        window = client.window()
        if args.command == "tree":
            props = client.data("get_window_properties", {"windowHandle": window})
            result = client.data("get_element_tree", {
                "elementHandle": props["rootElementHandle"], "maxElements": args.limit,
            })
        elif args.command == "screenshot":
            result = client.call("take_screenshot", {"windowHandle": window})
            image = next(block for block in result["content"] if block["type"] == "image")
            if image["mimeType"] != "image/png":
                raise RuntimeError("Server did not return a PNG")
            png = base64.b64decode(image["data"], validate=True)
            if not png.startswith(b"\x89PNG\r\n\x1a\n"):
                raise RuntimeError("Invalid PNG response")
            args.path.parent.mkdir(parents=True, exist_ok=True)
            args.path.write_bytes(png)
            print(args.path)
            return 0
        elif args.command == "key":
            text = KEYS.get(args.key.lower(), args.key)
            if len(text) != 1:
                parser.error("unknown key; use arrows, enter, escape, tab, space, pageup, pagedown, or one character")
            result = client.data("dispatch_key_event", {"windowHandle": window,
                                "text": text, "eventType": "PressAndRelease"})
        else:
            elements = client.data("find_elements_by_id", {
                "windowHandle": window, "elementsId": args.element_id,
            }).get("elementHandles", [])
            if len(elements) != 1:
                raise RuntimeError(f"Expected one matching element, found {len(elements)}; use call with an explicit handle")
            result = client.data("click_element", {"elementHandle": elements[0]})
    print(json.dumps(result, indent=2))
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except KeyboardInterrupt:
        sys.exit(130)
    except (OSError, ValueError, RuntimeError, KeyError, StopIteration) as error:
        print(f"slint-ui: {error}", file=sys.stderr)
        sys.exit(1)
