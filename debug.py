#!/usr/bin/env python3
"""调试代理：自动启动 llm_proxy，夹在 client 和 llm_proxy 之间打印全部请求响应。

用法：python3 debug.py [代理端口，默认 4000]
按 Ctrl+C 同时停止 llm_proxy 和本代理。
"""

import http.client
import http.server
import json
import sys
import os
import re
import subprocess
import atexit
import threading
from urllib.parse import urlparse

sys.stdout.reconfigure(line_buffering=True)

root = os.path.dirname(os.path.abspath(__file__))
binary = os.path.join(root, "target", "release", "llm_proxy")

if not os.path.isfile(binary):
    print("compiling llm_proxy ...")
    subprocess.run(["cargo", "build", "--release"], cwd=root, check=True)

proc = subprocess.Popen(
    [binary],
    cwd=root,
    stdout=subprocess.PIPE,
    stderr=subprocess.STDOUT,
    text=True,
)

atexit.register(lambda: proc.poll() is None and (proc.terminate(), proc.wait()))

addr = None
for line in proc.stdout:
    m = re.search(r"listening on ([\d.:]+)", line)
    print(f"  [llm] {line}", end="" if not m else "")
    if m:
        addr = m.group(1)
        print()
        break

if not addr:
    print("failed to detect llm_proxy listen address")
    sys.exit(1)

UPSTREAM_URL = f"http://{addr}"
_upstream = urlparse(UPSTREAM_URL)
UPSTREAM_HOST = _upstream.hostname
UPSTREAM_PORT = _upstream.port or 80


def follow_llm_log():
    for line in proc.stdout:
        print(f"  [llm] {line}", end="")
    proc.wait()


threading.Thread(target=follow_llm_log, daemon=True).start()

HOP_HEADERS = {"host", "content-length", "transfer-encoding", "connection", "accept-encoding"}


class DebugHandler(http.server.BaseHTTPRequestHandler):
    def do_POST(self):
        length = int(self.headers.get("Content-Length", 0))
        body = self.rfile.read(length)

        print(f"\n{'=' * 60}")
        print(f"POST {self.path}")
        print(f"  Headers: {dict(self.headers)}")
        try:
            print(f"  Body: {json.dumps(json.loads(body), indent=2, ensure_ascii=False)}")
        except Exception:
            print(f"  Body (raw): {body[:1024]}")

        forward_headers = {
            k: v for k, v in self.headers.items() if k.lower() not in HOP_HEADERS
        }

        conn = http.client.HTTPConnection(UPSTREAM_HOST, UPSTREAM_PORT, timeout=30)
        try:
            conn.request("POST", self.path, body=body, headers=forward_headers)
            resp = conn.getresponse()
            resp_body = resp.read()
            print(f"  {resp.status}")
            try:
                text = json.dumps(json.loads(resp_body), indent=2, ensure_ascii=False)
                print(f"  Body: {text[:2048]}")
            except Exception:
                print(f"  Body (raw): {resp_body[:1024]}")

            self.send_response(resp.status)
            for k, v in resp.getheaders():
                if k.lower() not in ("transfer-encoding", "content-encoding", "content-length"):
                    self.send_header(k, v)
            self.end_headers()
            self.wfile.write(resp_body)
        except Exception as e:
            print(f"  upstream error: {e}")
            self.send_response(502)
            self.end_headers()
        finally:
            conn.close()

    def do_GET(self):
        conn = http.client.HTTPConnection(UPSTREAM_HOST, UPSTREAM_PORT, timeout=30)
        try:
            headers = {
                k: v for k, v in self.headers.items() if k.lower() not in HOP_HEADERS
            }
            conn.request("GET", self.path, headers=headers)
            resp = conn.getresponse()
            body = resp.read()
            self.send_response(resp.status)
            self.end_headers()
            self.wfile.write(body)
        finally:
            conn.close()


if __name__ == "__main__":
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 4000
    server = http.server.HTTPServer(("0.0.0.0", port), DebugHandler)
    print(f"\ndebug proxy: http://localhost:{port} -> {UPSTREAM_URL}")
    print(f"set base_url to http://localhost:{port}")
    print(f"(redirect all output: python3 debug.py > app.log 2>&1)\n")
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        print()
