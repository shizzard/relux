#!/usr/bin/env -S python3 -u
"""Key-value database service backed by flat files."""

import argparse
import json
import os
import sys
import socketserver
from http.server import HTTPServer, BaseHTTPRequestHandler


class FastHTTPServer(HTTPServer):
    """HTTPServer that skips the slow getfqdn() call in server_bind."""

    def server_bind(self):
        socketserver.TCPServer.server_bind(self)
        _host, port = self.server_address[:2]
        self.server_name = "localhost"
        self.server_port = port


class DbHandler(BaseHTTPRequestHandler):
    def log_message(self, format, *args):
        pass  # suppress default logging

    def _send(self, status, body=None):
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.end_headers()
        if body is not None:
            self.wfile.write(json.dumps(body).encode())

    def _read_body(self):
        length = int(self.headers.get("Content-Length", 0))
        return self.rfile.read(length) if length else b""

    def _parse_path(self):
        parts = self.path.strip("/").split("/")
        if len(parts) < 2 or parts[0] != "db":
            return None, None
        name = parts[1]
        key = "/".join(parts[2:]) if len(parts) > 2 else None
        return name, key

    def _db_dir(self, name):
        return os.path.join(self.server.data_dir, name)

    def do_POST(self):
        name, key = self._parse_path()
        if name is None:
            self._send(400, {"error": "bad request"})
            return
        if key is not None:
            self._send(400, {"error": "bad request"})
            return
        db_dir = self._db_dir(name)
        if os.path.exists(db_dir):
            print(f"error: db {name} already exists", flush=True)
            self._send(409, {"error": f"db {name} already exists"})
            return
        os.makedirs(db_dir)
        print(f"created db {name}", flush=True)
        self._send(200, {"created": name})

    def do_GET(self):
        name, key = self._parse_path()
        if name is None or key is None:
            self._send(400, {"error": "bad request"})
            return
        db_dir = self._db_dir(name)
        if not os.path.isdir(db_dir):
            print(f"error: db {name} not found", flush=True)
            self._send(404, {"error": f"db {name} not found"})
            return
        key_path = os.path.join(db_dir, key)
        if not os.path.isfile(key_path):
            print(f"error: key {key} not found in {name}", flush=True)
            self._send(404, {"error": f"key {key} not found in {name}"})
            return
        with open(key_path, "r") as f:
            value = f.read()
        print(f"read {key} from {name}", flush=True)
        self._send(200, {"value": value})

    def do_PUT(self):
        name, key = self._parse_path()
        if name is None or key is None:
            self._send(400, {"error": "bad request"})
            return
        db_dir = self._db_dir(name)
        if not os.path.isdir(db_dir):
            print(f"error: db {name} not found", flush=True)
            self._send(404, {"error": f"db {name} not found"})
            return
        body = json.loads(self._read_body())
        value = body.get("value", "")
        key_path = os.path.join(db_dir, key)
        with open(key_path, "w") as f:
            f.write(value)
        print(f"wrote {key} to {name}", flush=True)
        self._send(200, {"wrote": key})

    def do_DELETE(self):
        name, key = self._parse_path()
        if name is None or key is None:
            self._send(400, {"error": "bad request"})
            return
        db_dir = self._db_dir(name)
        if not os.path.isdir(db_dir):
            print(f"error: db {name} not found", flush=True)
            self._send(404, {"error": f"db {name} not found"})
            return
        key_path = os.path.join(db_dir, key)
        if not os.path.isfile(key_path):
            print(f"error: key {key} not found in {name}", flush=True)
            self._send(404, {"error": f"key {key} not found in {name}"})
            return
        os.remove(key_path)
        print(f"deleted {key} from {name}", flush=True)
        self._send(200, {"deleted": key})


def main():
    parser = argparse.ArgumentParser(description="Key-value database service")
    parser.add_argument("--port", type=int, default=9000)
    parser.add_argument("--data-dir", default="/tmp/database")
    args = parser.parse_args()

    os.makedirs(args.data_dir, exist_ok=True)

    FastHTTPServer.allow_reuse_address = True
    server = FastHTTPServer(("127.0.0.1", args.port), DbHandler)
    server.data_dir = args.data_dir
    print(f"listening on {args.port}", flush=True)
    server.serve_forever()


if __name__ == "__main__":
    main()
